// JSON values: the built-in type `Json` and the pure module `std.json` (parse, stringify and
// constructors). A `Json` value is immutable: arrays and objects are built from values that
// already exist, so no value can contain itself and nesting is bounded by `MAX_DEPTH`, which also
// bounds every recursive walk below (printing, dropping). Numbers keep the form they were written
// in: an integer literal that fits in 64 bits is an `int`, anything else a `double`, so
// `parse(stringify(v))` gives back `v`. Every engine runs this code through `Machine`, so results
// and step charges agree everywhere.

use std::collections::HashMap;
use std::rc::Rc;

use super::ast::Ty;
use super::diag::{Code, Diagnostic, Span};
use super::interp::Value;
use super::machine::{checked_index, internal, Machine, R, STR_BYTE_STEPS};
use super::msg;
use super::native::{self, NativeVal, Obj};

/// Deepest nesting of arrays and objects a `Json` value may have.
pub const MAX_DEPTH: usize = 256;

/// The contents of one `Json` value.
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    Str(Rc<str>),
    Array(Vec<Rc<NativeVal>>),
    // Members in insertion order and an index from name to member.
    Object(Vec<(Rc<str>, Rc<NativeVal>)>, HashMap<Rc<str>, usize>),
}

pub struct JsonNode {
    pub v: Json,
    // 0 for scalars, one more than the deepest member for arrays and objects.
    pub depth: usize,
}

impl std::fmt::Debug for JsonNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Shared members can make the text far larger than the value: show small values only.
        if text_len(self, None, 0, 4096) > 4096 {
            return write!(f, "Json({})", self.kind());
        }
        let mut out = String::new();
        write_value(self, &mut out, None, 0);
        f.write_str(&out)
    }
}

impl JsonNode {
    /// `null`, `boolean`, `number`, `string`, `array` or `object`.
    pub fn kind(&self) -> &'static str {
        match self.v {
            Json::Null => "null",
            Json::Bool(_) => "boolean",
            Json::Int(_) | Json::Double(_) => "number",
            Json::Str(_) => "string",
            Json::Array(_) => "array",
            Json::Object(..) => "object",
        }
    }
}

/// The `Json` type.
pub fn json_ty() -> Ty {
    Ty::native("Json")
}

// Memory charged per value and per object member, like `List` elements.
const NODE_BYTES: u64 =
    (std::mem::size_of::<NativeVal>() + 2 * std::mem::size_of::<usize>()) as u64;
const MEMBER_BYTES: u64 = (std::mem::size_of::<(Rc<str>, Rc<NativeVal>)>()
    + std::mem::size_of::<(Rc<str>, usize)>()) as u64;
const ITEM_BYTES: u64 = std::mem::size_of::<Rc<NativeVal>>() as u64;

fn node(v: &Value) -> Option<&JsonNode> {
    match v {
        Value::Native(n) => match &n.obj {
            Obj::Json(j) => Some(j),
            _ => None,
        },
        _ => None,
    }
}

fn node_rc(v: Value, span: Span) -> R<Rc<NativeVal>> {
    match v {
        Value::Native(n) if matches!(n.obj, Obj::Json(_)) => Ok(n),
        _ => Err(internal(span)),
    }
}

fn kind_error(j: &JsonNode, want: &str, span: Span) -> Diagnostic {
    Diagnostic::new(Code::JsonKind, span, msg::json_kind(j.kind(), want))
}

// Builds objects member by member: a repeated name keeps its first position and takes the last
// value, like `Map.put`.
#[derive(Default)]
struct Members {
    list: Vec<(Rc<str>, Rc<NativeVal>)>,
    index: HashMap<Rc<str>, usize>,
}

impl Members {
    // Whether the name is new.
    fn put(&mut self, k: Rc<str>, v: Rc<NativeVal>) -> bool {
        match self.index.get(&k) {
            Some(&i) => {
                self.list[i].1 = v;
                false
            }
            None => {
                self.index.insert(k.clone(), self.list.len());
                self.list.push((k, v));
                true
            }
        }
    }
}

// A syntax error at a byte offset, or a diagnostic (step limit, cancellation) that ends the run.
enum Fail {
    Syntax(usize, String),
    Stop(Diagnostic),
}

impl From<Diagnostic> for Fail {
    fn from(d: Diagnostic) -> Self {
        Fail::Stop(d)
    }
}

struct Parser<'s> {
    s: &'s [u8],
    i: usize,
    span: Span,
    opt: Ty,
}

impl Parser<'_> {
    fn fail<T>(&self, what: String) -> Result<T, Fail> {
        Err(Fail::Syntax(self.i, what))
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.s.get(self.i) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn unexpected<T>(&self) -> Result<T, Fail> {
        match self.text_at(self.i) {
            Some(c) => self.fail(msg::json_unexpected(c)),
            None => self.fail(msg::json_end()),
        }
    }

    // The character starting at byte `i` (the input is UTF-8 text).
    fn text_at(&self, i: usize) -> Option<char> {
        std::str::from_utf8(&self.s[i..]).ok()?.chars().next()
    }

    fn expect(&mut self, b: u8) -> Result<(), Fail> {
        if self.peek() == Some(b) {
            self.i += 1;
            Ok(())
        } else {
            self.unexpected()
        }
    }

    fn make(&self, m: &mut Machine<'_>, v: Json, depth: usize) -> Result<Rc<NativeVal>, Fail> {
        m.charge_work(NODE_BYTES * STR_BYTE_STEPS, self.span)?;
        Ok(native::json_val(JsonNode { v, depth }, self.opt))
    }

    fn value(&mut self, m: &mut Machine<'_>, level: usize) -> Result<Rc<NativeVal>, Fail> {
        self.ws();
        match self.peek() {
            Some(b'{') => {
                if level >= MAX_DEPTH {
                    return self.fail(msg::json_too_deep(MAX_DEPTH));
                }
                self.i += 1;
                let mut members = Members::default();
                let mut depth = 0;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.peek() != Some(b'"') {
                            return self.unexpected();
                        }
                        let k = self.string()?;
                        self.ws();
                        self.expect(b':')?;
                        let v = self.value(m, level + 1)?;
                        depth = depth.max(depth_of(&v) + 1);
                        if members.put(k, v) {
                            m.charge_work(MEMBER_BYTES * STR_BYTE_STEPS, self.span)?;
                        }
                        self.ws();
                        match self.peek() {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.unexpected(),
                        }
                    }
                }
                self.make(m, Json::Object(members.list, members.index), depth.max(1))
            }
            Some(b'[') => {
                if level >= MAX_DEPTH {
                    return self.fail(msg::json_too_deep(MAX_DEPTH));
                }
                self.i += 1;
                let mut items = Vec::new();
                let mut depth = 0;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                } else {
                    loop {
                        let v = self.value(m, level + 1)?;
                        depth = depth.max(depth_of(&v) + 1);
                        m.charge_work(ITEM_BYTES * STR_BYTE_STEPS, self.span)?;
                        items.push(v);
                        self.ws();
                        match self.peek() {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.unexpected(),
                        }
                    }
                }
                self.make(m, Json::Array(items), depth.max(1))
            }
            Some(b'"') => {
                let s = self.string()?;
                self.make(m, Json::Str(s), 0)
            }
            Some(b't') => self.word(m, "true", Json::Bool(true)),
            Some(b'f') => self.word(m, "false", Json::Bool(false)),
            Some(b'n') => self.word(m, "null", Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let v = self.number()?;
                self.make(m, v, 0)
            }
            _ => self.unexpected(),
        }
    }

    fn word(&mut self, m: &mut Machine<'_>, w: &str, v: Json) -> Result<Rc<NativeVal>, Fail> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            self.make(m, v, 0)
        } else {
            self.unexpected()
        }
    }

    // RFC 8259 numbers: an optional minus, an integer part without leading zeros, then an
    // optional fraction and exponent.
    fn number(&mut self) -> Result<Json, Fail> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let from = p.i;
            while let Some(b'0'..=b'9') = p.peek() {
                p.i += 1;
            }
            p.i - from
        };
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let int_start = self.i;
        if digits(self) == 0 || (self.s[int_start] == b'0' && self.i - int_start > 1) {
            self.i = start;
            return self.fail(msg::json_bad_number());
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            self.i += 1;
            integer = false;
            if digits(self) == 0 {
                self.i = start;
                return self.fail(msg::json_bad_number());
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.i += 1;
            integer = false;
            if let Some(b'+' | b'-') = self.peek() {
                self.i += 1;
            }
            if digits(self) == 0 {
                self.i = start;
                return self.fail(msg::json_bad_number());
            }
        }
        // Only ASCII bytes were consumed.
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        if integer {
            if let Ok(n) = text.parse::<i64>() {
                return Ok(Json::Int(n));
            }
        }
        match text.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(Json::Double(x)),
            _ => {
                self.i = start;
                self.fail(msg::json_bad_number())
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Fail> {
        let h = self.s.get(self.i..self.i + 4).and_then(|h| {
            std::str::from_utf8(h)
                .ok()
                .filter(|t| t.bytes().all(|b| b.is_ascii_hexdigit()))
                .and_then(|t| u32::from_str_radix(t, 16).ok())
        });
        match h {
            Some(v) => {
                self.i += 4;
                Ok(v)
            }
            None => self.fail(msg::json_bad_escape()),
        }
    }

    // A string literal starting at the opening quote.
    fn string(&mut self) -> Result<Rc<str>, Fail> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let run = self.i;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.i += 1;
            }
            // The run is a slice of UTF-8 text cut at ASCII bytes.
            out.push_str(std::str::from_utf8(&self.s[run..self.i]).unwrap_or(""));
            match self.peek() {
                None => return self.fail(msg::json_end()),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(Rc::from(out));
                }
                Some(b'\\') => {
                    let at = self.i;
                    self.i += 1;
                    let c = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.i += 1;
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                // A high surrogate must be followed by an escaped low one.
                                if !self.s[self.i..].starts_with(b"\\u") {
                                    self.i = at;
                                    return self.fail(msg::json_bad_escape());
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    self.i = at;
                                    return self.fail(msg::json_bad_escape());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            match char::from_u32(code) {
                                Some(c) => {
                                    out.push(c);
                                    continue;
                                }
                                None => {
                                    self.i = at;
                                    return self.fail(msg::json_bad_escape());
                                }
                            }
                        }
                        _ => {
                            self.i = at;
                            return self.fail(msg::json_bad_escape());
                        }
                    };
                    self.i += 1;
                    out.push(c);
                }
                Some(_) => return self.fail(msg::json_control_char()),
            }
        }
    }
}

fn depth_of(v: &NativeVal) -> usize {
    match &v.obj {
        Obj::Json(j) => j.depth,
        _ => 0,
    }
}

// `"line 2, column 5"` of a byte offset, counting characters.
fn line_col(s: &str, at: usize) -> (usize, usize) {
    let before = &s[..at.min(s.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, col)
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// A `double` always shows a fraction or an exponent (`1.0`, `1e300`), so it reads back as a
// `double`; `{:?}` is the shortest text that reads back as the same number.
fn write_double(x: f64, out: &mut String) {
    out.push_str(&format!("{x:?}"));
}

// The text of `j`: compact, or indented by `indent` spaces per level.
fn write_value(j: &JsonNode, out: &mut String, indent: Option<usize>, level: usize) {
    let newline = |out: &mut String, level: usize| {
        if let Some(n) = indent {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', n * level));
        }
    };
    match &j.v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(n) => out.push_str(&n.to_string()),
        Json::Double(x) => write_double(*x, out),
        Json::Str(s) => write_str(s, out),
        Json::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                if let Obj::Json(c) = &v.obj {
                    write_value(c, out, indent, level + 1);
                }
            }
            if !items.is_empty() {
                newline(out, level);
            }
            out.push(']');
        }
        Json::Object(members, _) => {
            out.push('{');
            for (i, (k, v)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                write_str(k, out);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                if let Obj::Json(c) = &v.obj {
                    write_value(c, out, indent, level + 1);
                }
            }
            if !members.is_empty() {
                newline(out, level);
            }
            out.push('}');
        }
    }
}

// Length of `write_value`'s text, computed without building it, or some length above `cap`.
// Shared members count once per occurrence, so the text can be far larger than the value; the
// walk stops once it passes `cap`, so it visits at most `cap` values.
fn text_len(j: &JsonNode, indent: Option<usize>, level: usize, cap: u64) -> u64 {
    let nl = |level: usize| indent.map_or(0, |n| 1 + (n * level) as u64);
    let child = |v: &NativeVal, level: usize, cap: u64| match &v.obj {
        Obj::Json(c) => text_len(c, indent, level, cap),
        _ => 0,
    };
    match &j.v {
        Json::Null => 4,
        Json::Bool(b) => 4 + u64::from(!*b),
        Json::Int(n) => n.to_string().len() as u64,
        Json::Double(x) => format!("{x:?}").len() as u64,
        Json::Str(s) => {
            let mut out = String::new();
            write_str(s, &mut out);
            out.len() as u64
        }
        Json::Array(items) => {
            let mut n: u64 = 2;
            for (i, v) in items.iter().enumerate() {
                if n > cap {
                    return n;
                }
                n = n
                    .saturating_add(u64::from(i > 0))
                    .saturating_add(nl(level + 1));
                n = n.saturating_add(child(v, level + 1, cap.saturating_sub(n)));
            }
            if !items.is_empty() {
                n = n.saturating_add(nl(level));
            }
            n
        }
        Json::Object(members, _) => {
            let mut n: u64 = 2;
            for (i, (k, v)) in members.iter().enumerate() {
                if n > cap {
                    return n;
                }
                let mut key = String::new();
                write_str(k, &mut key);
                n = n
                    .saturating_add(u64::from(i > 0))
                    .saturating_add(nl(level + 1))
                    .saturating_add(key.len() as u64 + 1 + u64::from(indent.is_some()));
                n = n.saturating_add(child(v, level + 1, cap.saturating_sub(n)));
            }
            if !members.is_empty() {
                n = n.saturating_add(nl(level));
            }
            n
        }
    }
}

impl Machine<'_> {
    fn json_option(&mut self, v: Option<Value>, span: Span) -> R<Value> {
        let ty = native::option_of(json_ty());
        let name = if v.is_some() { "Some" } else { "None" };
        let (tag, _) = self
            .layouts()
            .variant(ty, name)
            .ok_or_else(|| internal(span))?;
        self.new_enum(ty, tag, v.into_iter().collect(), span)
    }

    fn json_new(&mut self, v: Json, depth: usize, span: Span) -> R<Value> {
        if depth > MAX_DEPTH {
            return Err(Diagnostic::new(
                Code::StdArgument,
                span,
                msg::json_too_deep(MAX_DEPTH),
            ));
        }
        self.charge_work(NODE_BYTES * STR_BYTE_STEPS, span)?;
        let opt = native::option_of(json_ty());
        Ok(Value::Native(native::json_val(JsonNode { v, depth }, opt)))
    }

    // An array value of the given members (each already a `Json`).
    fn json_array(&mut self, items: Vec<Value>, span: Span) -> R<Value> {
        self.charge_work(
            (items.len() as u64).saturating_mul(ITEM_BYTES * STR_BYTE_STEPS),
            span,
        )?;
        let mut depth = 1;
        let mut out = Vec::with_capacity(items.len());
        for v in items {
            let n = node_rc(v, span)?;
            depth = depth.max(depth_of(&n) + 1);
            out.push(n);
        }
        self.json_new(Json::Array(out), depth, span)
    }

    /// A function of `std.json`. Arguments are evaluated (text already charged).
    pub fn json_fn(&mut self, function: &str, args: Vec<Value>, span: Span) -> R<Value> {
        let mut args = args.into_iter();
        let arg = args.next();
        match (function, arg) {
            ("parse", Some(Value::Str(s))) => self.json_parse(&s, span),
            ("stringify" | "pretty", Some(v)) => {
                let j = node(&v).ok_or_else(|| internal(span))?;
                let indent = (function == "pretty").then_some(2);
                // Size the text before building it, so the budget refuses huge results.
                let cap = self.steps_left() / STR_BYTE_STEPS;
                let n = text_len(j, indent, 0, cap);
                self.charge_work(n.saturating_mul(STR_BYTE_STEPS), span)?;
                let mut out = String::new();
                write_value(j, &mut out, indent, 0);
                Ok(Value::Str(Rc::from(out)))
            }
            ("ofNull", None) => self.json_new(Json::Null, 0, span),
            ("ofBoolean", Some(Value::Bool(b))) => self.json_new(Json::Bool(b), 0, span),
            ("ofInt", Some(Value::Int(n))) => self.json_new(Json::Int(n), 0, span),
            ("ofDouble", Some(Value::Double(x))) => {
                if !x.is_finite() {
                    return Err(Diagnostic::new(
                        Code::StdArgument,
                        span,
                        msg::json_not_finite(x),
                    ));
                }
                self.json_new(Json::Double(x), 0, span)
            }
            ("ofDouble", Some(Value::Int(n))) => self.json_new(Json::Double(n as f64), 0, span),
            ("ofString", Some(Value::Str(s))) => self.json_new(Json::Str(s), 0, span),
            ("ofArray", Some(Value::Array(a))) => {
                let items: Vec<Value> = {
                    let a = a.borrow();
                    (0..a.len()).map(|i| a.get(i)).collect()
                };
                self.json_array(items, span)
            }
            ("ofList", Some(Value::Native(l))) => {
                let items = native::list_items(&l).ok_or_else(|| internal(span))?;
                self.json_array(items, span)
            }
            ("ofMap", Some(Value::Native(m))) => {
                let entries = native::map_entries(&m).ok_or_else(|| internal(span))?;
                self.charge(
                    (entries.len() as u64).saturating_mul(MEMBER_BYTES * STR_BYTE_STEPS),
                    span,
                )?;
                let mut members = Members::default();
                let mut depth = 1;
                for (k, v) in entries {
                    let Value::Str(k) = k else {
                        return Err(internal(span));
                    };
                    let n = node_rc(v, span)?;
                    depth = depth.max(depth_of(&n) + 1);
                    members.put(k, n);
                }
                self.json_new(Json::Object(members.list, members.index), depth, span)
            }
            _ => Err(internal(span)),
        }
    }

    // `Ok(value)`, or `Err("line L, column C: what")`.
    fn json_parse(&mut self, s: &str, span: Span) -> R<Value> {
        self.charge_text(s.len(), span)?;
        let ty = super::pure::parse_result(json_ty());
        let mut p = Parser {
            s: s.as_bytes(),
            i: 0,
            span,
            opt: native::option_of(json_ty()),
        };
        let parsed = p.value(self, 0).and_then(|v| {
            p.ws();
            if p.i < p.s.len() {
                p.fail(msg::json_trailing())
            } else {
                Ok(v)
            }
        });
        let (variant, payload) = match parsed {
            Ok(v) => ("Ok", Value::Native(v)),
            Err(Fail::Stop(d)) => return Err(d),
            Err(Fail::Syntax(at, what)) => {
                let (line, col) = line_col(s, at);
                let text = msg::json_syntax(line, col, &what);
                self.charge_text(text.len(), span)?;
                ("Err", Value::Str(Rc::from(text)))
            }
        };
        let (tag, _) = self
            .layouts()
            .variant(ty, variant)
            .ok_or_else(|| internal(span))?;
        self.new_enum(ty, tag, vec![payload], span)
    }

    /// A method of a `Json` value (`native_method` charged the call).
    pub fn json_method(
        &mut self,
        j: &JsonNode,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> R<Value> {
        let mut args = args.into_iter();
        let mut arg = || args.next().ok_or_else(|| internal(span));
        match name {
            "kind" => {
                let k = j.kind();
                self.charge_text(k.len(), span)?;
                Ok(Value::Str(Rc::from(k)))
            }
            "isNull" => Ok(Value::Bool(matches!(j.v, Json::Null))),
            "asBoolean" => match j.v {
                Json::Bool(b) => Ok(Value::Bool(b)),
                _ => Err(kind_error(j, "boolean", span)),
            },
            "asInt" => match j.v {
                Json::Int(n) => Ok(Value::Int(n)),
                _ => Err(kind_error(j, "int", span)),
            },
            "asDouble" => match j.v {
                Json::Int(n) => Ok(Value::Double(n as f64)),
                Json::Double(x) => Ok(Value::Double(x)),
                _ => Err(kind_error(j, "number", span)),
            },
            "asString" => match &j.v {
                Json::Str(s) => Ok(Value::Str(s.clone())),
                _ => Err(kind_error(j, "string", span)),
            },
            "size" => match &j.v {
                Json::Array(items) => Ok(Value::Int(items.len() as i64)),
                Json::Object(members, _) => Ok(Value::Int(members.len() as i64)),
                _ => Err(kind_error(j, "array or object", span)),
            },
            "at" => {
                let Json::Array(items) = &j.v else {
                    return Err(kind_error(j, "array", span));
                };
                let Value::Int(i) = arg()? else {
                    return Err(internal(span));
                };
                let k = checked_index(i, items.len(), span)?;
                Ok(Value::Native(items[k].clone()))
            }
            "get" | "has" => {
                let Json::Object(members, index) = &j.v else {
                    return Err(kind_error(j, "object", span));
                };
                let Value::Str(k) = arg()? else {
                    return Err(internal(span));
                };
                self.charge_text(k.len(), span)?;
                let found = index.get(&k).map(|&i| members[i].1.clone());
                if name == "has" {
                    return Ok(Value::Bool(found.is_some()));
                }
                self.json_option(found.map(Value::Native), span)
            }
            "keys" => {
                let Json::Object(members, _) = &j.v else {
                    return Err(kind_error(j, "object", span));
                };
                let bytes = members.iter().map(|(k, _)| k.len()).sum();
                self.charge_text(bytes, span)?;
                let vals = members.iter().map(|(k, _)| Value::Str(k.clone())).collect();
                self.array_lit(Ty::array_of(Ty::String), vals, span)
            }
            "items" => {
                let Json::Array(items) = &j.v else {
                    return Err(kind_error(j, "array", span));
                };
                let vals = items.iter().map(|v| Value::Native(v.clone())).collect();
                self.array_lit(Ty::array_of(json_ty()), vals, span)
            }
            _ => Err(internal(span)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::line_col;

    #[test]
    fn positions_count_characters() {
        assert_eq!(line_col("ab", 1), (1, 2));
        assert_eq!(line_col("a\nbé c", 6), (2, 4));
    }
}
