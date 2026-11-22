// App commands (docs/design/qx-app.md §5): calls a `public` function of an app's command module
// with JSON-compatible arguments and returns a typed result as JSON, instead of the rendered
// stdout of `qxide run`. Shared by the desktop runtime and its tests; no JSON library needed.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::ast::{Program, Ty};
use super::diag::{Code, Diagnostic, SourceMap};
use super::interp::{OutPiece, Value};
use super::stdlib::Host;
use super::{front_input, msg, render_output, sample, sorted_groups, Input, Limits};

/// Most shots one app call may request.
pub const MAX_SHOTS: u64 = 1_000_000;

/// Largest integer JavaScript represents exactly (`Number.MAX_SAFE_INTEGER`).
const JS_SAFE: i64 = (1 << 53) - 1;

/// A command the page may call: `(name, type)` per parameter, and the return type.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSig {
    pub name: String,
    pub params: Vec<(String, &'static str)>,
    pub ret: &'static str,
}

/// A JSON argument value as the runtime received it from the page.
#[derive(Debug, Clone, PartialEq)]
pub enum JsArg {
    Null,
    Bool(bool),
    /// A JSON number written without a fraction that fits in `i64`.
    Int(i64),
    /// Any other JSON number.
    Float(f64),
    Str(String),
    Array,
    Object,
}

impl JsArg {
    fn describe(&self) -> &'static str {
        match self {
            JsArg::Null => "null",
            JsArg::Bool(_) => "a boolean",
            JsArg::Int(_) | JsArg::Float(_) => "a number",
            JsArg::Str(_) => "a string",
            JsArg::Array => "an array",
            JsArg::Object => "an object",
        }
    }
}

/// A failed call, as the page's `QxError` sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallError {
    pub code: &'static str,
    pub message: String,
    pub diagnostics: Vec<DiagInfo>,
    pub rendered: String,
}

/// One diagnostic with 1-based positions, for editors.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagInfo {
    pub code: &'static str,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub help: Option<String>,
}

impl CallError {
    fn app(code: Code, message: String) -> Self {
        let help = msg::help(code);
        let mut rendered = format!("error[{}]: {message}\n", code.as_str());
        if let Some(h) = &help {
            let _ = writeln!(rendered, "help: {h}");
        }
        CallError {
            code: code.as_str(),
            message,
            diagnostics: Vec::new(),
            rendered,
        }
    }

    pub(super) fn from_diags(map: &SourceMap, diags: &[Diagnostic]) -> Self {
        let first = diags.first();
        CallError {
            code: first.map_or("Q0412", |d| d.code.as_str()),
            message: first.map_or_else(String::new, |d| d.message.clone()),
            // As many as are rendered; the rendered report ends with the Q0418 summary.
            diagnostics: diags
                .iter()
                .take(Limits::APP.max_diagnostics)
                .map(|d| diag_info(map, d))
                .collect(),
            // Every caller serves a desktop app, so the app's report limits apply.
            rendered: super::diag::render_all(diags, &Limits::APP, |d| map.render(d, &Limits::APP)),
        }
    }

    pub fn to_json(&self) -> String {
        let mut s = String::from("{\"code\":");
        json_str(&mut s, self.code);
        s.push_str(",\"message\":");
        json_str(&mut s, &self.message);
        s.push_str(",\"diagnostics\":[");
        for (i, d) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"code\":");
            json_str(&mut s, d.code);
            s.push_str(",\"message\":");
            json_str(&mut s, &d.message);
            s.push_str(",\"file\":");
            json_str(&mut s, &d.file);
            let _ = write!(
                s,
                ",\"line\":{},\"column\":{},\"endLine\":{},\"endColumn\":{},\"help\":",
                d.line, d.column, d.end_line, d.end_column
            );
            match &d.help {
                Some(h) => json_str(&mut s, h),
                None => s.push_str("null"),
            }
            s.push('}');
        }
        s.push_str("],\"rendered\":");
        json_str(&mut s, &self.rendered);
        s.push('}');
        s
    }
}

fn diag_info(map: &SourceMap, d: &Diagnostic) -> DiagInfo {
    let (line, column) = map.line_col(d.span.start);
    let (end_line, end_column) = map.line_col(d.span.end.max(d.span.start));
    DiagInfo {
        code: d.code.as_str(),
        message: d.message.clone(),
        file: map.file_name(d.span.start).to_string(),
        line,
        column,
        end_line,
        end_column,
        help: d.help.clone(),
    }
}

/// Structured diagnostics of `qxide check`/`run` failures, for `std.qxide` (Qxide Studio).
pub fn diagnostics_json(map: &SourceMap, diags: &[Diagnostic]) -> String {
    CallError::from_diags(map, diags).to_json()
}

/// A command's return value.
#[derive(Debug, Clone, PartialEq)]
pub enum Ret {
    Null,
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(String),
    Bit(bool),
    Bits(Vec<bool>),
}

impl Ret {
    fn write(&self, s: &mut String) {
        match self {
            Ret::Null => s.push_str("null"),
            Ret::Int(n) if (-JS_SAFE..=JS_SAFE).contains(n) => {
                let _ = write!(s, "{n}");
            }
            // Outside JavaScript's exact range: a decimal string, turned into a BigInt by the bridge.
            Ret::Int(n) => {
                let _ = write!(s, "\"{n}\"");
            }
            Ret::Double(v) => json_f64(s, *v),
            Ret::Bool(b) => {
                let _ = write!(s, "{b}");
            }
            Ret::Str(t) => json_str(s, t),
            Ret::Bit(b) => s.push(if *b { '1' } else { '0' }),
            Ret::Bits(v) => {
                s.push('[');
                for (i, b) in v.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    s.push(if *b { '1' } else { '0' });
                }
                s.push(']');
            }
        }
    }
}

/// One row of a multi-shot distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct CountRow {
    pub value: Ret,
    pub count: u64,
    pub output: String,
}

/// A successful call.
#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    pub command: String,
    /// The `.qx` return type, so the page can decode `"Infinity"` or large `int`s.
    pub returns: &'static str,
    pub seed: u64,
    pub shots: u64,
    pub value: Ret,
    pub output: String,
    pub counts: Option<Vec<CountRow>>,
}

impl CallResult {
    pub fn to_json(&self) -> String {
        let mut s = String::from("{\"command\":");
        json_str(&mut s, &self.command);
        s.push_str(",\"returns\":");
        json_str(&mut s, self.returns);
        // Seeds are u64; above 2^53 they travel as strings like large ints.
        if self.seed <= JS_SAFE as u64 {
            let _ = write!(s, ",\"seed\":{}", self.seed);
        } else {
            let _ = write!(s, ",\"seed\":\"{}\"", self.seed);
        }
        let _ = write!(s, ",\"shots\":{},\"value\":", self.shots);
        self.value.write(&mut s);
        s.push_str(",\"output\":");
        json_str(&mut s, &self.output);
        s.push_str(",\"counts\":");
        match &self.counts {
            None => s.push_str("null"),
            Some(rows) => {
                s.push('[');
                for (i, r) in rows.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    s.push_str("{\"value\":");
                    r.value.write(&mut s);
                    let _ = write!(s, ",\"count\":{},\"output\":", r.count);
                    json_str(&mut s, &r.output);
                    s.push('}');
                }
                s.push(']');
            }
        }
        s.push('}');
        s
    }
}

pub fn json_str(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // U+2028/2029 end lines in JavaScript source; the runtime evaluates results as script.
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_f64(out: &mut String, v: f64) {
    if v.is_nan() {
        out.push_str("\"NaN\"");
    } else if v.is_infinite() {
        out.push_str(if v > 0.0 {
            "\"Infinity\""
        } else {
            "\"-Infinity\""
        });
    } else {
        let _ = write!(out, "{v:?}");
    }
}

fn ty_allowed(t: Ty) -> bool {
    matches!(t, Ty::Int | Ty::Double | Ty::Boolean | Ty::String)
}

/// Links and checks the command module (a library: no `main` needed).
fn load(input: &Input) -> Result<(SourceMap, Program), CallError> {
    let (map, program) = front_input(input, false);
    match program {
        Ok(p) => Ok((map, p)),
        Err(d) => Err(CallError::from_diags(&map, &d)),
    }
}

// The command module's own functions keep their bare names after linking.
fn find_command<'p>(
    map: &SourceMap,
    program: &'p Program,
    name: &str,
    module: &str,
) -> Result<&'p super::ast::FnDecl, CallError> {
    let f = program
        .fns
        .iter()
        .find(|f| f.name.name == name)
        .ok_or_else(|| CallError::app(Code::BadCommand, msg::command_not_found(name, module)))?;
    let fail = |d: Diagnostic| CallError::from_diags(map, &[d]);
    if f.public.is_none() {
        return Err(fail(Diagnostic::new(
            Code::BadCommand,
            f.name.span,
            msg::command_not_public(name),
        )));
    }
    if let Some(p) = f.params.iter().find(|p| !ty_allowed(p.ty)) {
        return Err(fail(Diagnostic::new(
            Code::BadCommand,
            p.ty_span,
            msg::command_bad_param(name, &p.name.name, p.ty.name()),
        )));
    }
    Ok(f)
}

/// The signatures of `commands` (the `[app] commands` list), checking that each one exists in
/// the command module, is `public`, and takes only build-time parameters.
pub fn signatures(
    input: &Input,
    module: &str,
    commands: &[String],
) -> Result<Vec<CommandSig>, CallError> {
    let (map, program) = load(input)?;
    commands
        .iter()
        .map(|c| {
            let f = find_command(&map, &program, c, module)?;
            Ok(CommandSig {
                name: c.clone(),
                params: f
                    .params
                    .iter()
                    .map(|p| (p.name.name.clone(), p.ty.name()))
                    .collect(),
                ret: f.ret.name(),
            })
        })
        .collect()
}

fn convert(param: &str, cmd: &str, ty: Ty, arg: &JsArg) -> Result<Value, CallError> {
    let bad = || {
        CallError::app(
            Code::BadArguments,
            msg::arg_type(param, cmd, ty.name(), arg.describe()),
        )
    };
    Ok(match (ty, arg) {
        (Ty::Int, JsArg::Int(n)) if (-JS_SAFE..=JS_SAFE).contains(n) => Value::Int(*n),
        // A BigInt or a large integer, sent as a decimal string.
        (Ty::Int, JsArg::Str(s)) => Value::Int(s.trim().parse().map_err(|_| bad())?),
        (Ty::Double, JsArg::Int(n)) => Value::Double(*n as f64),
        (Ty::Double, JsArg::Float(v)) => Value::Double(*v),
        (Ty::Double, JsArg::Str(s)) => Value::Double(match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => return Err(bad()),
        }),
        (Ty::Boolean, JsArg::Bool(b)) => Value::Bool(*b),
        (Ty::String, JsArg::Str(s)) => Value::Str(s.as_str().into()),
        _ => return Err(bad()),
    })
}

/// What a call needs besides the command module.
pub struct Call<'a> {
    pub module: &'a str,
    pub commands: &'a [String],
    pub name: &'a str,
    pub args: &'a [(String, JsArg)],
    pub shots: u64,
    pub seed: u64,
    pub limits: Limits,
}

/// Runs one command: links and checks the module, elaborates the function with the arguments
/// (std effects run here, once), then simulates `shots` shots if the result depends on them.
pub fn call(input: &Input, c: &Call, host: &mut dyn Host) -> Result<CallResult, CallError> {
    if !c.commands.iter().any(|n| n == c.name) {
        return Err(CallError::app(
            Code::UnknownCommand,
            msg::unknown_command(c.name),
        ));
    }
    if c.shots == 0 || c.shots > MAX_SHOTS {
        return Err(CallError::app(
            Code::BadArguments,
            msg::too_many_shots(c.shots, MAX_SHOTS),
        ));
    }
    let (map, program) = load(input)?;
    let f = find_command(&map, &program, c.name, c.module)?;
    if let Some((extra, _)) = c
        .args
        .iter()
        .find(|(n, _)| !f.params.iter().any(|p| &p.name.name == n))
    {
        return Err(CallError::app(
            Code::BadArguments,
            msg::arg_unknown(extra, c.name),
        ));
    }
    let mut values = Vec::with_capacity(f.params.len());
    for p in &f.params {
        let arg = c
            .args
            .iter()
            .find(|(n, _)| *n == p.name.name)
            .map(|(_, a)| a)
            .ok_or_else(|| {
                CallError::app(Code::BadArguments, msg::arg_missing(&p.name.name, c.name))
            })?;
        values.push(convert(&p.name.name, c.name, p.ty, arg)?);
    }
    let at = f.name.span;
    let fail = |d: Diagnostic| CallError::from_diags(&map, &[d]);
    let (e, ret) =
        super::engine::elaborate_call(&program, c.name, values, host, &c.limits).map_err(fail)?;
    let returned: Option<Vec<qxide::Bit>> = match &ret {
        Value::Bit(b) => Some(vec![*b]),
        Value::Bits(v) => Some(v.to_vec()),
        _ => None,
    };
    let single_bit = matches!(ret, Value::Bit(_));
    let classical = match ret {
        Value::Int(n) => Ret::Int(n),
        Value::Double(v) => Ret::Double(v),
        Value::Bool(b) => Ret::Bool(b),
        Value::Str(s) => Ret::Str(s.to_string()),
        _ => Ret::Null,
    };
    let as_ret = |bits: Vec<bool>| {
        if single_bit {
            Ret::Bit(bits[0])
        } else {
            Ret::Bits(bits)
        }
    };
    let prints_bits = e
        .output
        .iter()
        .any(|p| matches!(p, OutPiece::Bit(_) | OutPiece::Bits(..)));
    let mut result = CallResult {
        command: c.name.to_string(),
        returns: f.ret.name(),
        seed: c.seed,
        shots: c.shots,
        value: classical.clone(),
        output: String::new(),
        counts: None,
    };
    let needs_sim = if c.shots == 1 {
        prints_bits || returned.is_some()
    } else {
        !e.measured.is_empty()
    };
    if !needs_sim {
        result.output = render_output(&e.output, None);
        return Ok(result);
    }
    let sims = sample(&e, at, c.shots, c.seed, c.limits).map_err(fail)?;
    let mut sims = sims.map(|r| {
        if host.cancelled() {
            return Err(Diagnostic::new(Code::Cancelled, at, msg::cancelled()));
        }
        r
    });
    if c.shots == 1 {
        let shot = sims.next().expect("one shot").map_err(fail)?;
        if let Some(bits) = &returned {
            result.value = as_ret(bits.iter().map(|&b| shot.get(b)).collect());
        }
        result.output = render_output(&e.output, Some(&shot));
        return Ok(result);
    }
    // Group by the returned bits, or by the whole measurement record for classical returns.
    let key_bits: &[qxide::Bit] = returned.as_deref().unwrap_or(&e.measured);
    // Each new record adds its JSON row to the output budget, as in `qxide run` (B-03).
    let mut groups: HashMap<Vec<bool>, (u64, String)> = HashMap::new();
    let mut bytes: u64 = 0;
    for shot in sims {
        let shot = shot.map_err(fail)?;
        let key: Vec<bool> = key_bits.iter().map(|&b| shot.get(b)).collect();
        if let Some(group) = groups.get_mut(&key) {
            group.0 += 1;
            continue;
        }
        let text = render_output(&e.output, Some(&shot));
        // `{"value":[false,..],"count":N,"output":".."},`: at most 6 bytes per bit plus 64.
        bytes = bytes.saturating_add(6 * key.len() as u64 + text.len() as u64 + 64);
        if bytes > c.limits.max_output_bytes {
            return Err(fail(Diagnostic::new(
                Code::OutputLimit,
                at,
                msg::shots_output_limit(c.shots, c.limits.max_output_bytes),
            )));
        }
        groups.insert(key, (1, text));
    }
    let rows = sorted_groups(groups)
        .into_iter()
        .map(|(bits, (count, output))| CountRow {
            value: if returned.is_some() {
                as_ret(bits)
            } else {
                Ret::Bits(bits)
            },
            count,
            output,
        })
        .collect();
    if returned.is_some() {
        result.value = Ret::Null;
    }
    if !prints_bits {
        result.output = render_output(&e.output, None);
    }
    result.counts = Some(rows);
    Ok(result)
}

/// A call cancelled before it started (Q0713).
pub fn cancelled_error() -> CallError {
    CallError::app(Code::Cancelled, msg::cancelled())
}

/// The JSON array `qxide.commands()` resolves to.
pub fn signatures_json(sigs: &[CommandSig]) -> String {
    let mut s = String::from("[");
    for (i, c) in sigs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"name\":");
        json_str(&mut s, &c.name);
        s.push_str(",\"params\":[");
        for (j, (n, t)) in c.params.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push_str("{\"name\":");
            json_str(&mut s, n);
            s.push_str(",\"type\":");
            json_str(&mut s, t);
            s.push('}');
        }
        s.push_str("],\"returns\":");
        json_str(&mut s, c.ret);
        s.push('}');
    }
    s.push(']');
    s
}
