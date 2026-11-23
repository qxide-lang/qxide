// Pure library functions shared by every engine: the `std.math` module and the methods of
// `String` values. They run inside the interpreter (never through `Host`), so every host allows
// them and they may appear inside an `if` on a measured bit. Text work is charged per byte before
// any large result is built, so the step budget also bounds the memory they allocate.

use std::rc::Rc;

use super::ast::Ty;
use super::diag::{Code, Diagnostic, Span};
use super::interp::Value;
use super::machine::{internal, Machine, R};
use super::msg;
use super::stdlib::HostValue;

/// Standard modules implemented here (and in src/lang/json.rs) instead of by the host.
pub const MODULES: &[&str] = &["math", "path", "json"];

pub fn is_pure_module(module: &str) -> bool {
    MODULES.contains(&module)
}

/// `std.math.sqrt` and the like: a qualified name of a pure function.
pub fn is_pure_fn(qualified: &str) -> bool {
    qualified
        .strip_prefix("std.")
        .and_then(|r| r.split_once('.'))
        .is_some_and(|(m, _)| is_pure_module(m))
}

/// Methods of `String` values: name, parameter types, result type.
pub const STRING_METHODS: &[(&str, &[Ty], Ty)] = &[
    ("length", &[], Ty::Int),
    ("substring", &[Ty::Int, Ty::Int], Ty::String),
    ("charAt", &[Ty::Int], Ty::String),
    ("indexOf", &[Ty::String], Ty::Int),
    ("contains", &[Ty::String], Ty::Boolean),
    ("startsWith", &[Ty::String], Ty::Boolean),
    ("endsWith", &[Ty::String], Ty::Boolean),
    ("toUpperCase", &[], Ty::String),
    ("toLowerCase", &[], Ty::String),
    ("trim", &[], Ty::String),
    ("replace", &[Ty::String, Ty::String], Ty::String),
    ("repeat", &[Ty::Int], Ty::String),
    ("parseInt", &[], Ty::Int),
    ("parseDouble", &[], Ty::Double),
    // Listed with their `Ok` type; they return `Result<int, String>` (`string_method`).
    ("tryParseInt", &[], Ty::Int),
    ("tryParseDouble", &[], Ty::Double),
];

/// The `Ok` type of `tryParseInt` and `tryParseDouble`.
pub fn try_parse_ok(name: &str) -> Option<Ty> {
    match name {
        "tryParseInt" => Some(Ty::Int),
        "tryParseDouble" => Some(Ty::Double),
        _ => None,
    }
}

/// `Result<ok, String>` of the prelude.
pub fn parse_result(ok: Ty) -> Ty {
    Ty::instance(Ty::enum_named("Result"), &[ok, Ty::String])
}

/// Methods of `String` values that take or return `String[]`.
pub const STRING_ARRAY_METHODS: &[&str] = &["split", "splitWhitespace", "lines", "chars", "join"];

/// Name, parameter types and result type of the `String` method `name`.
pub fn string_method(name: &str) -> Option<(&'static str, Vec<Ty>, Ty)> {
    let texts = || Ty::array_of(Ty::String);
    if let Some(&n) = STRING_ARRAY_METHODS.iter().find(|n| **n == name) {
        return Some(match n {
            "split" => (n, vec![Ty::String], texts()),
            "join" => (n, vec![texts()], Ty::String),
            _ => (n, vec![], texts()),
        });
    }
    STRING_METHODS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|&(n, p, r)| match try_parse_ok(n) {
            Some(ok) => (n, p.to_vec(), parse_result(ok)),
            None => (n, p.to_vec(), r),
        })
}

fn num(v: &HostValue, span: Span) -> R<f64> {
    match v {
        HostValue::Double(x) => Ok(*x),
        // `int` arguments widen to `double` like at any call.
        HostValue::Int(n) => Ok(*n as f64),
        _ => Err(internal(span)),
    }
}

fn int(v: &HostValue, span: Span) -> R<i64> {
    match v {
        HostValue::Int(n) => Ok(*n),
        _ => Err(internal(span)),
    }
}

fn overflow(span: Span) -> Diagnostic {
    Diagnostic::new(Code::Overflow, span, msg::overflow())
}

/// One `std.math` function. Results are those of Rust's `f64` methods on the running platform;
/// `sqrt`, `floor`, `ceil`, `round`, `trunc`, `abs`, `min` and `max` are exact everywhere.
pub fn math(function: &str, args: &[HostValue], span: Span) -> R<HostValue> {
    let x = |i: usize| num(&args[i], span);
    let n = |i: usize| int(&args[i], span);
    let d = |v: f64| Ok(HostValue::Double(v));
    match (function, args.len()) {
        ("sqrt", 1) => d(x(0)?.sqrt()),
        ("cbrt", 1) => d(x(0)?.cbrt()),
        ("exp", 1) => d(x(0)?.exp()),
        ("log", 1) => d(x(0)?.ln()),
        ("log10", 1) => d(x(0)?.log10()),
        ("log2", 1) => d(x(0)?.log2()),
        ("sin", 1) => d(x(0)?.sin()),
        ("cos", 1) => d(x(0)?.cos()),
        ("tan", 1) => d(x(0)?.tan()),
        ("asin", 1) => d(x(0)?.asin()),
        ("acos", 1) => d(x(0)?.acos()),
        ("atan", 1) => d(x(0)?.atan()),
        ("sinh", 1) => d(x(0)?.sinh()),
        ("cosh", 1) => d(x(0)?.cosh()),
        ("tanh", 1) => d(x(0)?.tanh()),
        ("floor", 1) => d(x(0)?.floor()),
        ("ceil", 1) => d(x(0)?.ceil()),
        // Halfway cases away from zero, like C `round`.
        ("round", 1) => d(x(0)?.round()),
        ("trunc", 1) => d(x(0)?.trunc()),
        ("abs", 1) => d(x(0)?.abs()),
        ("pow", 2) => d(x(0)?.powf(x(1)?)),
        ("atan2", 2) => d(x(0)?.atan2(x(1)?)),
        ("hypot", 2) => d(x(0)?.hypot(x(1)?)),
        // Java `Math.min`/`max`: NaN if either argument is NaN.
        ("min", 2) => {
            let (a, b) = (x(0)?, x(1)?);
            d(if a.is_nan() || b.is_nan() {
                f64::NAN
            } else {
                a.min(b)
            })
        }
        ("max", 2) => {
            let (a, b) = (x(0)?, x(1)?);
            d(if a.is_nan() || b.is_nan() {
                f64::NAN
            } else {
                a.max(b)
            })
        }
        ("isNaN", 1) => Ok(HostValue::Bool(x(0)?.is_nan())),
        ("isFinite", 1) => Ok(HostValue::Bool(x(0)?.is_finite())),
        ("absInt", 1) => n(0)?
            .checked_abs()
            .map(HostValue::Int)
            .ok_or_else(|| overflow(span)),
        ("minInt", 2) => Ok(HostValue::Int(n(0)?.min(n(1)?))),
        ("maxInt", 2) => Ok(HostValue::Int(n(0)?.max(n(1)?))),
        ("powInt", 2) => {
            let (b, e) = (n(0)?, n(1)?);
            if e < 0 {
                return Err(Diagnostic::new(
                    Code::StdArgument,
                    span,
                    msg::negative_exponent(e),
                ));
            }
            let e = u32::try_from(e).unwrap_or(u32::MAX);
            b.checked_pow(e)
                .or(match b {
                    // 0, 1 and -1 never overflow, whatever the exponent.
                    0 | 1 => Some(b),
                    -1 => Some(if e % 2 == 0 { 1 } else { -1 }),
                    _ => None,
                })
                .map(HostValue::Int)
                .ok_or_else(|| overflow(span))
        }
        // Two's-complement arithmetic that wraps instead of stopping (hashes, PRNGs).
        ("wrappingAdd", 2) => Ok(HostValue::Int(n(0)?.wrapping_add(n(1)?))),
        ("wrappingSub", 2) => Ok(HostValue::Int(n(0)?.wrapping_sub(n(1)?))),
        ("wrappingMul", 2) => Ok(HostValue::Int(n(0)?.wrapping_mul(n(1)?))),
        _ => Err(internal(span)),
    }
}

// The start of a long text, for a diagnostic.
fn shown(s: &str) -> String {
    const MAX: usize = 32;
    match s.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

// `std.path` works on text alone, the same on every platform: `/` and `\` both separate
// components, `C:` before a separator is a drive, and `join` and `normalize` write `/`.
fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

// The root of `p` (separators, or `C:` and one separator; empty for a relative path) and the rest.
fn split_root(p: &str) -> (&str, &str) {
    let b = p.as_bytes();
    let end = if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && is_sep(b[2] as char)
    {
        3
    } else {
        p.len() - p.trim_start_matches(is_sep).len()
    };
    p.split_at(end)
}

// The root written with `/`.
fn root_text(root: &str) -> String {
    match root.as_bytes() {
        [] => String::new(),
        [d, b':', ..] => format!("{}:/", *d as char),
        _ => "/".into(),
    }
}

// The last component: what is left after trailing separators are dropped.
fn file_name(p: &str) -> &str {
    let (_, rest) = split_root(p);
    let rest = rest.trim_end_matches(is_sep);
    rest.rsplit(is_sep).next().unwrap_or("")
}

// `name` split at its last dot; a leading dot belongs to the stem (`.bashrc` has no extension).
fn stem_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i + 1..]),
    }
}

/// One `std.path` function on paths given as text.
pub fn path(function: &str, args: &[HostValue], span: Span) -> R<HostValue> {
    let text = |i: usize| match args.get(i) {
        Some(HostValue::Str(s)) => Ok(s.as_str()),
        _ => Err(internal(span)),
    };
    let s = |v: String| Ok(HostValue::Str(v));
    match function {
        // `child` alone when it is absolute, like Java's `Path.resolve`.
        "join" => {
            let (base, child) = (text(0)?, text(1)?);
            if base.is_empty() || !split_root(child).0.is_empty() {
                return s(child.to_string());
            }
            if child.is_empty() {
                return s(base.to_string());
            }
            let trimmed = base.trim_end_matches(is_sep);
            if trimmed.is_empty() || trimmed.len() < split_root(base).0.len() {
                // `base` is a root such as `/` or `C:/`.
                return s(format!("{base}{child}"));
            }
            s(format!("{trimmed}/{child}"))
        }
        "fileName" => s(file_name(text(0)?).to_string()),
        // Everything before the last component, without trailing separators; the root stays.
        "parent" => {
            let p = text(0)?;
            let (root, rest) = split_root(p);
            let rest = rest.trim_end_matches(is_sep);
            let parent = match rest.rfind(is_sep) {
                Some(i) => format!("{root}{}", rest[..i].trim_end_matches(is_sep)),
                None if rest.is_empty() => String::new(),
                None => root.to_string(),
            };
            s(parent)
        }
        "extension" => s(stem_ext(file_name(text(0)?)).1.to_string()),
        "stem" => s(stem_ext(file_name(text(0)?)).0.to_string()),
        "isAbsolute" => Ok(HostValue::Bool(!split_root(text(0)?).0.is_empty())),
        // `/` separators, no empty or `.` components, and `x/..` removed; `..` that would climb
        // above a root is dropped, above a relative start it is kept. An empty result is `.`.
        "normalize" => {
            let (root, rest) = split_root(text(0)?);
            let mut parts: Vec<&str> = Vec::new();
            for c in rest.split(is_sep) {
                match c {
                    "" | "." => {}
                    ".." if parts.last().is_some_and(|l| *l != "..") => {
                        parts.pop();
                    }
                    ".." if !root.is_empty() => {}
                    c => parts.push(c),
                }
            }
            let out = format!("{}{}", root_text(root), parts.join("/"));
            s(if out.is_empty() { ".".into() } else { out })
        }
        _ => Err(internal(span)),
    }
}

// Byte offset of the char at `index` (chars, like `len`), or the end for `index == chars`.
fn char_offset(s: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return Some(0);
    }
    s.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()))
        .nth(index)
}

impl Machine<'_> {
    /// A method of a `String` value. `recv` and `args` are evaluated; every method charges the
    /// bytes it reads and builds (`STR_BYTE_STEPS` each).
    pub fn str_method(
        &mut self,
        name: &str,
        recv: Value,
        args: Vec<Value>,
        span: Span,
    ) -> R<Value> {
        let Value::Str(s) = recv else {
            return Err(internal(span));
        };
        let text = |i: usize| match args.get(i) {
            Some(Value::Str(t)) => Ok(t.clone()),
            _ => Err(internal(span)),
        };
        let int = |i: usize| match args.get(i) {
            Some(Value::Int(n)) => Ok(*n),
            _ => Err(internal(span)),
        };
        self.charge_text(s.len(), span)?;
        let built = |m: &mut Self, out: String| -> R<Value> {
            m.charge_text(out.len(), span)?;
            Ok(Value::Str(Rc::from(out)))
        };
        match name {
            "length" => Ok(Value::Int(s.chars().count() as i64)),
            "substring" => {
                let (a, b) = (int(0)?, int(1)?);
                let n = s.chars().count();
                let range = usize::try_from(a)
                    .ok()
                    .zip(usize::try_from(b).ok())
                    .filter(|&(a, b)| a <= b && b <= n);
                let Some((a, b)) = range else {
                    return Err(Diagnostic::new(
                        Code::IndexOutOfBounds,
                        span,
                        msg::substring_range(a, b, n),
                    ));
                };
                let (i, j) = (char_offset(&s, a), char_offset(&s, b));
                let (Some(i), Some(j)) = (i, j) else {
                    return Err(internal(span));
                };
                built(self, s[i..j].to_string())
            }
            "charAt" => {
                let k = int(0)?;
                let n = s.chars().count();
                match usize::try_from(k).ok().and_then(|k| s.chars().nth(k)) {
                    Some(c) => built(self, c.to_string()),
                    None => Err(Diagnostic::new(
                        Code::IndexOutOfBounds,
                        span,
                        msg::index_out_of_bounds(k, n),
                    )),
                }
            }
            "indexOf" => {
                let t = text(0)?;
                self.charge_text(t.len(), span)?;
                Ok(Value::Int(match s.find(&*t) {
                    Some(at) => s[..at].chars().count() as i64,
                    None => -1,
                }))
            }
            "contains" | "startsWith" | "endsWith" => {
                let t = text(0)?;
                self.charge_text(t.len(), span)?;
                Ok(Value::Bool(match name {
                    "contains" => s.contains(&*t),
                    "startsWith" => s.starts_with(&*t),
                    _ => s.ends_with(&*t),
                }))
            }
            // Case mapping can grow the text (`ß` -> `SS`), at most 3x per char.
            "toUpperCase" | "toLowerCase" => {
                self.charge_text(s.len().saturating_mul(3), span)?;
                let out = if name == "toUpperCase" {
                    s.to_uppercase()
                } else {
                    s.to_lowercase()
                };
                built(self, out)
            }
            "trim" => built(self, s.trim().to_string()),
            "replace" => {
                let (from, to) = (text(0)?, text(1)?);
                self.charge_text(from.len().saturating_add(to.len()), span)?;
                // Size the result before building it, so the budget refuses huge results.
                let hits = if from.is_empty() {
                    s.chars().count() + 1
                } else {
                    s.matches(&*from).count()
                };
                let size = (s.len() - hits.saturating_mul(from.len()).min(s.len()))
                    .saturating_add(hits.saturating_mul(to.len()));
                self.charge_text(size, span)?;
                Ok(Value::Str(Rc::from(s.replace(&*from, &to))))
            }
            "repeat" => {
                let k = int(0)?;
                let Ok(k) = usize::try_from(k) else {
                    return Err(Diagnostic::new(
                        Code::StdArgument,
                        span,
                        msg::negative_repeat(k),
                    ));
                };
                self.charge_text(s.len().saturating_mul(k), span)?;
                Ok(Value::Str(Rc::from(s.repeat(k))))
            }
            "parseInt" => s.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                Diagnostic::new(
                    Code::ParseNumber,
                    span,
                    msg::parse_number(&shown(&s), "int"),
                )
            }),
            "parseDouble" => match s.trim().parse::<f64>() {
                Ok(v) => Ok(Value::Double(v)),
                Err(_) => Err(Diagnostic::new(
                    Code::ParseNumber,
                    span,
                    msg::parse_number(&shown(&s), "double"),
                )),
            },
            // `Ok(value)`, or `Err(message)` with the text `parseInt` would stop with.
            "tryParseInt" | "tryParseDouble" => {
                let (ok, parsed) = if name == "tryParseInt" {
                    (Ty::Int, s.trim().parse::<i64>().ok().map(Value::Int))
                } else {
                    (Ty::Double, s.trim().parse::<f64>().ok().map(Value::Double))
                };
                let ty = parse_result(ok);
                let (variant, payload) = match parsed {
                    Some(v) => ("Ok", v),
                    None => {
                        let text = msg::parse_number(&shown(&s), ok.name());
                        ("Err", built(self, text)?)
                    }
                };
                let (tag, _) = self
                    .layouts()
                    .variant(ty, variant)
                    .ok_or_else(|| internal(span))?;
                self.new_enum(ty, tag, vec![payload], span)
            }
            // Pieces between separators, empty ones included (`"a,,b"` gives three), like Rust's
            // `split`; an empty separator is an error.
            "split" => {
                let sep = text(0)?;
                self.charge_text(sep.len(), span)?;
                if sep.is_empty() {
                    return Err(Diagnostic::new(
                        Code::StdArgument,
                        span,
                        msg::empty_separator(),
                    ));
                }
                let n = s.matches(&*sep).count() + 1;
                self.charge_texts(n, s.len() - (n - 1) * sep.len(), span)?;
                Ok(self.texts(s.split(&*sep)))
            }
            // Runs of Unicode white space separate the pieces; none are empty.
            "splitWhitespace" => {
                let n = s.split_whitespace().count();
                self.charge_texts(n, s.len(), span)?;
                Ok(self.texts(s.split_whitespace()))
            }
            // Lines end at a line feed (a carriage return before it is dropped); a final line feed
            // adds no empty line.
            "lines" => {
                let n = s.lines().count();
                self.charge_texts(n, s.len(), span)?;
                Ok(self.texts(s.lines()))
            }
            // One one-character `String` per Unicode scalar value.
            "chars" => {
                let n = s.chars().count();
                self.charge_texts(n, s.len(), span)?;
                Ok(self.texts(s.chars().map(String::from)))
            }
            // `", ".join(parts)`: the parts with the receiver between them.
            "join" => {
                let Some(Value::Array(parts)) = args.first() else {
                    return Err(internal(span));
                };
                let parts = parts.borrow();
                let n = parts.len();
                let mut size = s.len().saturating_mul(n.saturating_sub(1));
                let mut pieces = Vec::with_capacity(n);
                for i in 0..n {
                    let Value::Str(p) = parts.get(i) else {
                        return Err(internal(span));
                    };
                    size = size.saturating_add(p.len());
                    pieces.push(p);
                }
                self.charge_text(size, span)?;
                let out = pieces.iter().map(|p| &**p).collect::<Vec<&str>>().join(&s);
                Ok(Value::Str(Rc::from(out)))
            }
            _ => Err(internal(span)),
        }
    }
}
