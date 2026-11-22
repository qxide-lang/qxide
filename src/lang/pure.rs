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

/// Standard modules implemented here instead of by the host.
pub const MODULES: &[&str] = &["math"];

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
    ("char_at", &[Ty::Int], Ty::String),
    ("index_of", &[Ty::String], Ty::Int),
    ("contains", &[Ty::String], Ty::Boolean),
    ("starts_with", &[Ty::String], Ty::Boolean),
    ("ends_with", &[Ty::String], Ty::Boolean),
    ("to_upper", &[], Ty::String),
    ("to_lower", &[], Ty::String),
    ("trim", &[], Ty::String),
    ("replace", &[Ty::String, Ty::String], Ty::String),
    ("repeat", &[Ty::Int], Ty::String),
    ("parse_int", &[], Ty::Int),
    ("parse_double", &[], Ty::Double),
    // Listed with their `Ok` type; they return `Result<int, String>` (`string_method`).
    ("try_parse_int", &[], Ty::Int),
    ("try_parse_double", &[], Ty::Double),
];

/// The `Ok` type of `try_parse_int` and `try_parse_double`.
pub fn try_parse_ok(name: &str) -> Option<Ty> {
    match name {
        "try_parse_int" => Some(Ty::Int),
        "try_parse_double" => Some(Ty::Double),
        _ => None,
    }
}

/// `Result<ok, String>` of the prelude.
pub fn parse_result(ok: Ty) -> Ty {
    Ty::instance(Ty::enum_named("Result"), &[ok, Ty::String])
}

pub fn string_method(name: &str) -> Option<(&'static str, &'static [Ty], Ty)> {
    STRING_METHODS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|&(n, p, r)| match try_parse_ok(n) {
            Some(ok) => (n, p, parse_result(ok)),
            None => (n, p, r),
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
        ("is_nan", 1) => Ok(HostValue::Bool(x(0)?.is_nan())),
        ("is_finite", 1) => Ok(HostValue::Bool(x(0)?.is_finite())),
        ("abs_int", 1) => n(0)?
            .checked_abs()
            .map(HostValue::Int)
            .ok_or_else(|| overflow(span)),
        ("min_int", 2) => Ok(HostValue::Int(n(0)?.min(n(1)?))),
        ("max_int", 2) => Ok(HostValue::Int(n(0)?.max(n(1)?))),
        ("pow_int", 2) => {
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
            "char_at" => {
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
            "index_of" => {
                let t = text(0)?;
                self.charge_text(t.len(), span)?;
                Ok(Value::Int(match s.find(&*t) {
                    Some(at) => s[..at].chars().count() as i64,
                    None => -1,
                }))
            }
            "contains" | "starts_with" | "ends_with" => {
                let t = text(0)?;
                self.charge_text(t.len(), span)?;
                Ok(Value::Bool(match name {
                    "contains" => s.contains(&*t),
                    "starts_with" => s.starts_with(&*t),
                    _ => s.ends_with(&*t),
                }))
            }
            // Case mapping can grow the text (`ß` -> `SS`), at most 3x per char.
            "to_upper" | "to_lower" => {
                self.charge_text(s.len().saturating_mul(3), span)?;
                let out = if name == "to_upper" {
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
            "parse_int" => s.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                Diagnostic::new(
                    Code::ParseNumber,
                    span,
                    msg::parse_number(&shown(&s), "int"),
                )
            }),
            "parse_double" => match s.trim().parse::<f64>() {
                Ok(v) => Ok(Value::Double(v)),
                Err(_) => Err(Diagnostic::new(
                    Code::ParseNumber,
                    span,
                    msg::parse_number(&shown(&s), "double"),
                )),
            },
            // `Ok(value)`, or `Err(message)` with the text `parse_int` would stop with.
            "try_parse_int" | "try_parse_double" => {
                let (ok, parsed) = if name == "try_parse_int" {
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
            _ => Err(internal(span)),
        }
    }
}
