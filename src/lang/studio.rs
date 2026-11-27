// `std.qxide`: Qxide Studio's access to the compiler (docs/design/qx-app.md §11). Programs typed
// into Studio run like in the playground: only `std.log`, collected and returned with the result.

use super::command::{json_str, CallError, MAX_SHOTS};
use super::diag::{Code, Diagnostic, SourceMap};
use super::stdlib::{
    Host, HostValue, LogOnly, NoEffects, Permissions, StdError, StdErrorKind, StdResult,
};
use super::{build_input, check_qasm_size, front_input, main_span, msg, run_diag, Input, Limits};

struct StudioHost<'a> {
    logs: LogOnly,
    cancelled: &'a dyn Fn() -> bool,
}

impl Host for StudioHost<'_> {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        self.logs.call(module, function, args)
    }

    fn cancelled(&self) -> bool {
        (self.cancelled)()
    }
}

// `std.qxide.qasm`: no effects, as for `qxide qasm`, but Stop still cancels the build.
struct QasmHost<'a> {
    inner: NoEffects,
    cancelled: &'a dyn Fn() -> bool,
}

impl Host for QasmHost<'_> {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        self.inner.call(module, function, args)
    }

    fn cancelled(&self) -> bool {
        (self.cancelled)()
    }
}

fn input(src: &str) -> Input<'_> {
    let mut input = Input::file("main.qx", src);
    input.perms = Permissions::playground();
    input
}

fn ok_json(fields: &[(&str, String)]) -> String {
    let mut s = String::from("{\"ok\":true");
    for (k, v) in fields {
        s.push(',');
        json_str(&mut s, k);
        s.push(':');
        s.push_str(v);
    }
    s.push('}');
    s
}

fn err_json(map: &SourceMap, diags: &[Diagnostic]) -> String {
    let e = CallError::from_diags(map, diags).to_json();
    format!("{{\"ok\":false,{}", &e[1..])
}

fn quoted(text: &str) -> String {
    let mut s = String::new();
    json_str(&mut s, text);
    s
}

/// `std.qxide.check|run|qasm|format(source, ...)`: a JSON string, `{"ok":true,...}` or
/// `{"ok":false,"code":...,"diagnostics":[...],"rendered":...}`.
pub fn call(function: &str, args: &[HostValue], cancelled: &dyn Fn() -> bool) -> StdResult {
    let src = match args.first() {
        Some(HostValue::Str(s)) => s.as_str(),
        _ => "",
    };
    let int = |i: usize| match args.get(i) {
        Some(HostValue::Int(n)) => *n,
        _ => 0,
    };
    let json = match function {
        "check" => {
            let (map, r) = front_input(&input(src), true);
            match r {
                Ok(_) => ok_json(&[]),
                Err(d) => err_json(&map, &d),
            }
        }
        "qasm" => {
            let mut host = QasmHost {
                inner: NoEffects {
                    what: "`qxide qasm`",
                },
                cancelled,
            };
            let (map, r) = build_input(&input(src), &mut host, &Limits::APP);
            let r = r.and_then(|(program, e)| {
                check_qasm_size(&e, main_span(&program), &Limits::APP).map_err(|d| vec![d])?;
                Ok(e)
            });
            match r {
                Ok(e) => ok_json(&[("qasm", quoted(&e.circuit.to_qasm()))]),
                Err(d) if d.iter().any(|d| d.code == Code::Cancelled) => {
                    return Err(StdError::new(StdErrorKind::Cancelled, msg::cancelled()))
                }
                Err(d) => err_json(&map, &d),
            }
        }
        "format" => match super::fmt::format(src) {
            Ok(text) => ok_json(&[("text", quoted(&text))]),
            Err(d) => err_json(&SourceMap::new("main.qx", src), &d),
        },
        "run" => {
            let (shots, seed) = (int(1), int(2));
            if !(1..=MAX_SHOTS as i64).contains(&shots) || seed < 0 {
                return Err(StdError::new(
                    StdErrorKind::Argument,
                    msg::too_many_shots(shots.max(0) as u64, MAX_SHOTS),
                ));
            }
            let mut host = StudioHost {
                logs: LogOnly::default(),
                cancelled,
            };
            let (map, r) = run_diag(
                &input(src),
                shots as u64,
                seed as u64,
                Limits::APP,
                &mut host,
            );
            let logs: Vec<String> = host.logs.lines.iter().map(|l| quoted(l)).collect();
            match r {
                Ok(out) => {
                    let rows: Vec<String> = out
                        .rows
                        .iter()
                        .map(|(bits, n)| {
                            let b: Vec<&str> =
                                bits.iter().map(|&x| if x { "1" } else { "0" }).collect();
                            format!("{{\"bits\":[{}],\"count\":{n}}}", b.join(","))
                        })
                        .collect();
                    ok_json(&[
                        ("output", quoted(&out.text)),
                        ("logs", format!("[{}]", logs.join(","))),
                        ("counts", format!("[{}]", rows.join(","))),
                        ("shots", shots.to_string()),
                        ("seed", seed.to_string()),
                    ])
                }
                Err(d) if d.iter().any(|d| d.code == Code::Cancelled) => {
                    return Err(StdError::new(StdErrorKind::Cancelled, msg::cancelled()))
                }
                Err(d) => err_json(&map, &d),
            }
        }
        other => {
            return Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::std_unknown_fn(other, "std.qxide"),
            ))
        }
    };
    Ok(HostValue::Str(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> HostValue {
        HostValue::Str(v.into())
    }

    fn text(r: StdResult) -> String {
        match r {
            Ok(HostValue::Str(j)) => j,
            other => panic!("{other:?}"),
        }
    }

    const BELL: &str = "import std.log;\n\nvoid main() {\n    log.info(\"hi\");\n    QReg q = qreg(2);\n    h(q[0]);\n    cx(q[0], q[1]);\n    BitReg r = measure(q);\n    println(\"{}\", r);\n}\n";

    #[test]
    fn run_returns_text_counts_and_logs() {
        let no = || false;
        let j = text(call(
            "run",
            &[s(BELL), HostValue::Int(100), HostValue::Int(5)],
            &no,
        ));
        assert!(j.starts_with("{\"ok\":true,\"output\":\"["), "{j}");
        assert!(j.contains("\"logs\":[\"info: hi\"]"), "{j}");
        assert!(j.contains("\"counts\":[{\"bits\":["), "{j}");
        // The text is exactly what `qxide run --shots 100 --seed 5` prints.
        let cli = super::super::run("main.qx", BELL, 100, 5).unwrap();
        assert!(j.contains(&quoted(&cli)[1..cli.len()]), "{j}");
    }

    #[test]
    fn errors_are_structured_and_effects_refused() {
        let no = || false;
        let j = text(call(
            "check",
            &[s("void main() {\n    int x = y;\n}\n")],
            &no,
        ));
        assert!(j.starts_with("{\"ok\":false,\"code\":\"Q0201\""), "{j}");
        assert!(j.contains("\"line\":2,\"column\":13"), "{j}");
        let fs = "import std.fs;\nvoid main() {}\n";
        assert!(text(call("check", &[s(fs)], &no)).contains("Q0702"));
        let q = text(call("qasm", &[s(BELL)], &no));
        assert!(q.contains("OPENQASM 3"), "{q}");
        let f = text(call("format", &[s("void main(){println(\"x\");}")], &no));
        assert!(f.contains("void main() {\\n    println"), "{f}");
    }
}
