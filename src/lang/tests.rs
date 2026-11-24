// Checker and interpreter tests. Assertions use error codes and line:column, never message text.

use super::diag::SourceMap;
use super::*;

const BELL: &str = include_str!("../../examples/qx/bell.qx");
const GHZ: &str = include_str!("../../examples/qx/ghz.qx");
const TELEPORT: &str = include_str!("../../examples/qx/teleport.qx");
const FUNCTIONS: &str = include_str!("../../examples/qx/functions.qx");
const ROTATION: &str = include_str!("../../examples/qx/rotation.qx");
const TELEPORT_FN: &str = include_str!("../../examples/qx/teleport_fn.qx");
pub(super) const GENERICS: &str = include_str!("../../examples/qx/generics.qx");
pub(super) const ERRORS: &str = include_str!("../../examples/qx/errors.qx");
pub(super) const INTERFACES: &str = include_str!("../../examples/qx/interfaces.qx");
pub(super) const COLLECTIONS: &str = include_str!("../../examples/qx/collections.qx");
pub(super) const JSON: &str = include_str!("../../examples/qx/json.qx");

fn located(src: &str, errors: &[Diagnostic]) -> Vec<(&'static str, usize, usize)> {
    let map = SourceMap::new("t.qx", src);
    errors
        .iter()
        .map(|d| {
            let (l, c) = map.line_col(d.span.start);
            (d.code.as_str(), l, c)
        })
        .collect()
}

/// Static errors as (code, line, column).
fn errors(src: &str) -> Vec<(&'static str, usize, usize)> {
    match front(src) {
        Ok(_) => Vec::new(),
        Err(e) => located(src, &e),
    }
}

fn codes(src: &str) -> Vec<&'static str> {
    errors(src).into_iter().map(|e| e.0).collect()
}

fn ok(src: &str) {
    assert_eq!(errors(src), Vec::new(), "unexpected errors in:\n{src}");
}

/// Runtime error of a program that passes the checker.
fn runtime_error(src: &str) -> (&'static str, usize, usize) {
    ok(src);
    match build(src) {
        Ok(_) => panic!("expected a runtime error in:\n{src}"),
        Err(e) => located(src, &e)[0],
    }
}

fn run_ok(src: &str, shots: u64) -> String {
    match run("t.qx", src, shots, 0) {
        Ok(s) => s,
        Err(Failure(d)) => panic!("run failed:\n{d}"),
    }
}

fn main_body(body: &str) -> String {
    format!("void main() {{\n{body}\n}}\n")
}

/// Parses a multi-shot table into (bits, count, printed lines) rows.
fn table(out: &str) -> Vec<(String, u64, Vec<String>)> {
    let mut rows: Vec<(String, u64, Vec<String>)> = Vec::new();
    for line in out.lines() {
        if let Some(text) = line.strip_prefix("  ") {
            rows.last_mut()
                .expect("output before a row")
                .2
                .push(text.into());
        } else {
            let (head, count) = line.rsplit_once("  ×").expect("row has a count");
            rows.push((head.into(), count.parse().unwrap(), Vec::new()));
        }
    }
    rows
}

#[test]
fn examples_and_doc_samples_pass_the_checker() {
    for src in [BELL, GHZ, TELEPORT, FUNCTIONS, ROTATION, TELEPORT_FN] {
        ok(src);
    }
    ok("boolean parity(int n) {\n    return n % 2 == 1;\n}\n\nvoid main() {\n    int total = 0;\n    for i in 0..10 {\n        if parity(i) {\n            total = total + i;\n        }\n    }\n    println(\"total={}\", total);\n}\n");
}

// The error samples of docs/design/qx-language.md section 4, with their positions.
#[test]
fn design_doc_error_samples() {
    let use_after =
        "void main() {\n    Qubit q = qubit();\n    Bit m = measure(q);\n    h(q);\n}\n";
    assert_eq!(errors(use_after), vec![("Q0301", 4, 7)]);

    let dup = "void main() {\n    Qubit a = qubit();\n    cx(a, a);\n    discard(a);\n}\n";
    assert_eq!(errors(dup), vec![("Q0303", 3, 11)]);

    let leak = "void main() {\n    Qubit q = qubit();\n    h(q);\n}\n";
    assert_eq!(errors(leak), vec![("Q0302", 2, 11)]);

    let in_loop = "void main() {\n    Qubit q = qubit();\n    for i in 0..3 {\n        Bit m = measure(q);\n    }\n}\n";
    // The loop may also run zero times, so `q` additionally leaks at the end of `main`.
    assert_eq!(errors(in_loop), vec![("Q0302", 2, 11), ("Q0305", 4, 17)]);

    let bit_if = "void main() {\n    Qubit a = qubit();\n    Qubit t = qubit();\n    h(a);\n    Bit m = measure(a);\n    if m {\n        x(t);\n        Bit m2 = measure(t);\n    }\n}\n";
    assert_eq!(errors(bit_if), vec![("Q0308", 8, 18)]);
}

#[test]
fn removed_rust_syntax_points_to_the_new_form() {
    assert_eq!(codes("fn main() {}"), ["Q0110"]);
    assert_eq!(codes(&main_body("let x = 1;")), ["Q0110"]);
    let Err(Failure(d)) = check("t.qx", &main_body("let mut x = 1;")) else {
        panic!("expected an error")
    };
    assert!(
        d.starts_with("error[Q0110]") && d.contains("\nhelp: ") && d.contains("int x = 1;"),
        "{d}"
    );
    // Tail expressions are gone too: functions return with `return`.
    assert_eq!(codes("int f() { 1 }\nvoid main() {}"), ["Q0105"]);
}

// A scripted terminal for `std.io`, `std.env` and `main(String[] args)`: stdin lines, captured
// writes and fixed arguments. Under `cfg(test)` every run goes through both engines, the second
// replaying the first one's host calls.
#[derive(Default)]
struct Term {
    args: Vec<String>,
    input: Vec<String>,
    env: Vec<(String, String)>,
    out: String,
}

impl Host for Term {
    fn call(
        &mut self,
        module: &str,
        function: &str,
        args: Vec<stdlib::HostValue>,
    ) -> stdlib::StdResult {
        use stdlib::HostValue as V;
        let text = |i: usize| match args.get(i) {
            Some(V::Str(s)) => s.clone(),
            _ => String::new(),
        };
        Ok(match (module, function) {
            ("process", "arg_count") => V::Int(self.args.len() as i64),
            ("process", "arg") => match args.first() {
                Some(V::Int(i)) => V::Str(self.args[*i as usize].clone()),
                _ => V::Unit,
            },
            ("io", "write") => {
                self.out.push_str(&text(0));
                V::Unit
            }
            ("io", "writeLine") => {
                self.out.push_str(&text(0));
                self.out.push('\n');
                V::Unit
            }
            ("io", "readLine") if self.input.is_empty() => V::Unit,
            ("io", "readLine") => V::Str(self.input.remove(0)),
            ("io", "readAll") => V::Str(std::mem::take(&mut self.input).join("\n")),
            ("env", "get") => self
                .env
                .iter()
                .find(|(k, _)| *k == text(0))
                .map_or(V::Unit, |(_, v)| V::Str(v.clone())),
            _ => return stdlib::LogOnly::default().call(module, function, args),
        })
    }
}

// Runs `src` on `term`: the exit code and the deferred `println` output of one shot.
fn run_term(src: &str, term: &mut Term) -> Result<(u8, String), Vec<&'static str>> {
    let (_, built) = build_input(&Input::file("t.qx", src), term, &Limits::CLI);
    match built {
        Ok((_, e)) => Ok((e.exit_code, render_output(&e.output, None))),
        Err(d) => Err(d.iter().map(|d| d.code.as_str()).collect()),
    }
}

#[test]
fn process_io_args_env_and_exit_code() {
    let src = "import std.io;\nimport std.env;\n\nint main(String[] args) {\n    println(\"deferred\");\n    io.write(\"args \" + len(args));\n    io.writeLine(\" first \" + args[0]);\n    int total = 0;\n    boolean more = true;\n    while more {\n        match io.readLine() {\n            Some(line) => {\n                total += line.parseInt();\n            }\n            None => {\n                more = false;\n            }\n        }\n    }\n    io.writeLine(\"sum \" + total);\n    io.writeLine(env.get(\"HOME\").orElse(\"?\") + \" \" + env.get(\"NOPE\").orElse(\"unset\"));\n    return total % 7;\n}\n";
    let mut term = Term {
        args: vec!["a".into(), "b c".into()],
        input: vec!["4".into(), "5".into()],
        env: vec![("HOME".into(), "/h".into())],
        ..Term::default()
    };
    assert_eq!(run_term(src, &mut term), Ok((2, "deferred\n".to_string())));
    assert_eq!(term.out, "args 2 first a\nsum 9\n/h unset\n");
    // `void main(String[] args)` on a host without arguments gets an empty array.
    let mut term = Term::default();
    let none = "import std.io;\n\nvoid main(String[] args) {\n    io.writeLine(\"n=\" + len(args) + \" \" + io.readAll().length());\n}\n";
    assert_eq!(run_term(none, &mut term), Ok((0, String::new())));
    assert_eq!(term.out, "n=0 0\n");
    // Exit codes are 0..=255.
    let bad = "int main() {\n    return 256;\n}\n";
    assert_eq!(run_term(bad, &mut Term::default()), Err(vec!["Q0706"]));
    let neg = "int main() {\n    return -1;\n}\n";
    assert_eq!(run_term(neg, &mut Term::default()), Err(vec!["Q0706"]));
    // Terminal effects are unconditional: not inside an `if` on a measured bit.
    assert_eq!(
        codes_cli("import std.io;\n\nvoid main() {\n    Qubit q = qubit();\n    h(q);\n    Bit b = measure(q);\n    if b {\n        io.writeLine(\"x\");\n    }\n}\n"),
        ["Q0314"]
    );
}

// Checker codes with the CLI's permissions (`std.io` and `std.env` allowed).
fn codes_cli(src: &str) -> Vec<&'static str> {
    match front_input(&Input::file("t.qx", src), true).1 {
        Ok(_) => Vec::new(),
        Err(d) => d.iter().map(|d| d.code.as_str()).collect(),
    }
}

#[test]
fn java_style_declarations_and_functions() {
    let src = "int add(int a, int b) {\n    return a + b;\n}\n\ndouble half(double x) {\n    return x / 2;\n}\n\nvoid main() {\n    final int n = 3;\n    double d = n;\n    d = d * 2;\n    boolean big = add(n, 4) > 6;\n    println(\"{} {} {} {} {}\", add(n, 4), half(n), d, big, 7 / 2);\n}\n";
    assert_eq!(run_ok(src, 1), "7 1.5 6.0 true 3\n");
    assert_eq!(codes(&main_body("final int x = 1;\nx = 2;")), ["Q0207"]);
    assert_eq!(codes(&main_body("int x = 1.5;")), ["Q0204"]);
    assert_eq!(codes(&main_body("void x = 1;")), ["Q0204"]);
    assert_eq!(codes("void f(void v) {}\nvoid main() {}"), ["Q0204"]);
    assert_eq!(codes("double main() {\n    return 0.0;\n}"), ["Q0206"]);
    assert_eq!(codes(&main_body("boolean b = 1 == true;")), ["Q0204"]);
    // Doubles follow IEEE 754: division by zero is not an error.
    assert_eq!(
        run_ok(&main_body("double z = 0.0;\nprintln(\"{}\", 1.0 / z);"), 1),
        "inf\n"
    );
}

#[test]
fn rotation_gates_take_double_angles() {
    let src =
        main_body("Qubit q = qubit();\nrx(q, 0.5);\nry(q, 1);\nrz(q, -0.25);\nBit m = measure(q);");
    let q = qasm("t.qx", &src).ok().unwrap();
    assert!(
        q.contains("rx(0.5) q[0];\nry(1.0) q[0];\nrz(-0.25) q[0];"),
        "{q}"
    );
    assert_eq!(
        codes(&main_body("Qubit q = qubit();\nrx(q, true);\ndiscard(q);")),
        ["Q0204"]
    );
}

#[test]
fn linear_qubit_rules() {
    // Moving a qubit into another variable consumes the original.
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); Qubit p = q; h(q); discard(p);"
        )),
        ["Q0301"]
    );
    ok(&main_body(
        "Qubit q = qubit(); Qubit p = q; h(p); discard(p);",
    ));
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); Qubit q = qubit(); discard(q);"
        )),
        ["Q0310"]
    );
    // Shadowing in an inner scope does not leak the outer qubit.
    ok(&main_body(
        "Qubit q = qubit(); if true { Qubit q = qubit(); discard(q); } discard(q);",
    ));
    assert_eq!(
        codes(&main_body("Qubit q = qubit(); if true { discard(q); }")),
        ["Q0304"]
    );
    ok(&main_body(
        "Qubit q = qubit(); if true { discard(q); } else { Bit m = measure(q); }",
    ));
    // A branch that returns does not have to agree with the other one.
    ok(&main_body(
        "Qubit q = qubit(); if true { discard(q); return; } discard(q);",
    ));
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); if true { return; } discard(q);"
        )),
        ["Q0302"]
    );
    assert_eq!(
        codes(&main_body(
            "QReg r = qreg(2); Bit m = measure(r[0]); discard(r);"
        )),
        ["Q0306"]
    );
    assert_eq!(
        codes(&main_body("QReg r = qreg(2); Qubit q = r[0]; discard(r);")),
        ["Q0306"]
    );
    assert_eq!(
        codes(&main_body("Qubit q = qubit(); q = qubit(); discard(q);")),
        ["Q0307"]
    );
    assert_eq!(codes(&main_body("h(qubit());")), ["Q0311"]);
    ok(&main_body("Bit m = measure(qubit()); discard(qreg(3));"));
    // Registers: literal indices are checked statically, others at run time.
    assert_eq!(
        codes(&main_body("QReg r = qreg(2); cx(r[0], r[0]); discard(r);")),
        ["Q0303"]
    );
    ok(&main_body(
        "QReg r = qreg(3); ccx(r[0], r[1], r[2]); int i = 1; cx(r[i], r[0]); discard(r);",
    ));
    assert_eq!(
        codes(&main_body(
            "Qubit a = qubit(); Qubit b = qubit(); ccx(a, b, a); discard(a); discard(b);"
        )),
        ["Q0303"]
    );
    // Allocating and consuming inside a loop body is fine.
    ok(&main_body(
        "for i in 0..3 { Qubit q = qubit(); h(q); Bit m = measure(q); println(\"{}\", m); }",
    ));
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); Bit m = measure(q); Bit n = measure(q);"
        )),
        ["Q0301"]
    );
}

#[test]
fn unbound_quantum_temporaries_are_rejected() {
    assert_eq!(codes(&main_body("qubit();")), ["Q0312"]);
    assert_eq!(codes(&main_body("qreg(3);")), ["Q0312"]);
    assert_eq!(codes(&main_body("int n = len(qreg(3));")), ["Q0312"]);
    assert_eq!(codes(&main_body("Bit m = measure(qreg(2)[0]);")), ["Q0312"]);
    assert_eq!(
        codes(&main_body("Qubit q = qreg(2)[1]; discard(q);")),
        ["Q0312"]
    );
    // A statement that only names an existing qubit neither allocates nor consumes anything.
    ok(&main_body("Qubit q = qubit();\nq;\nh(q);\ndiscard(q);"));
    ok(&main_body("QReg r = qreg(2);\nr;\nr[1];\ndiscard(r);"));
    // Reading a register variable is fine.
    ok(&main_body(
        "QReg r = qreg(3); int n = len(r); h(r[n - 1]); discard(r);",
    ));
}

#[test]
fn byte_order_mark_is_ignored() {
    let src = format!("\u{feff}{BELL}");
    assert!(check("t.qx", &src).is_ok());
    let bad = "\u{feff}void main() {\n    Qubit q = qubit();\n}\n";
    let Err(Failure(d)) = check("t.qx", bad) else {
        panic!("expected an error")
    };
    // Columns are counted from after the mark.
    assert!(d.contains("--> t.qx:2:11\n"), "{d}");
}

#[test]
fn huge_stabilizer_runs_are_refused_before_allocating() {
    // About 20 GB of tableau; must become Q0406 instead of aborting on allocation.
    let src = main_body("QReg r = qreg(200000);\nBitReg m = measure(r);\nprintln(\"{}\", m);");
    let Err(Failure(d)) = run("t.qx", &src, 1, 0) else {
        panic!("expected a simulation error")
    };
    assert!(d.contains("error[Q0406]"), "{d}");
    assert!(qasm("t.qx", &src).is_ok());
    // Slots freed by `measure` are reused, so 1000 sequential qubits need a single slot.
    let reuse = main_body(
        "for i in 0..1000 { Qubit q = qubit(); h(q); Bit m = measure(q); if i == 999 { println(\"{}\", m); } }",
    );
    let q = qasm("t.qx", &reuse).ok().unwrap();
    assert!(q.contains("qubit[1] q;"), "{}", &q[..80]);
    let (_, e) = build(&reuse).ok().unwrap();
    assert_eq!(e.slots, 1);
    let out = run_ok(&reuse, 1);
    assert!(out == "0\n" || out == "1\n", "{out}");
}

#[test]
fn bit_if_restrictions() {
    let pre = "Qubit a = qubit(); Qubit t = qubit(); h(a); Bit m = measure(a);";
    let with = |body: &str| main_body(&format!("{pre} {body} discard(t);"));
    ok(&with(
        "if m { x(t); for i in 0..2 { z(t); } int k = 3; if k > 2 { h(t); } } else { reset(t); }",
    ));
    ok(&with(
        "Bit r = measure(qubit()); if m { x(t); } else if r { z(t); }",
    ));
    assert_eq!(codes(&with("if m { println(\"x\"); }")), ["Q0308"]);
    assert_eq!(
        codes(&with("if m { Qubit q = qubit(); discard(q); }")),
        // `qubit()` and `discard`; the declaration is not reported again.
        ["Q0308", "Q0308"]
    );
    assert_eq!(codes(&with("if m { return; }")), ["Q0308"]);
    // Functions that only apply gates may be called inside (section 14 of the design doc).
    ok(&format!("void f() {{}}\n{}", with("if m { f(); }")));
    assert_eq!(codes(&with("int k = 0; if m { k = 1; }")), ["Q0309"]);
    // Locals declared inside the body may be assigned there.
    ok(&with("if m { int k = 0; k = k + 1; }"));
}

#[test]
fn types_and_names() {
    let m = "Bit m = measure(qubit());";
    assert_eq!(codes(&main_body(&format!("{m} int x = m + 1;"))), ["Q0218"]);
    assert_eq!(
        codes(&main_body(&format!("{m} for i in 0..m {{}}"))),
        ["Q0218"]
    );
    assert_eq!(codes(&main_body(&format!("{m} boolean b = m;"))), ["Q0218"]);
    assert_eq!(codes(&main_body("if 1 {}")), ["Q0215"]);
    assert_eq!(codes(&main_body("boolean x = 1;")), ["Q0204"]);
    assert_eq!(codes(&main_body("int x = 1 + true;")), ["Q0204"]);
    assert_eq!(codes(&main_body("int x = y;")), ["Q0201"]);
    assert_eq!(codes(&main_body("foo();")), ["Q0202"]);
    assert_eq!(
        codes(&main_body("Qubit q = qubit(1); discard(q);")),
        ["Q0203"]
    );
    assert_eq!(codes(&main_body("int x = 3; int y = x[0];")), ["Q0209"]);
    assert_eq!(codes(&main_body("println(\"{} {}\", 1);")), ["Q0210"]);
    assert_eq!(codes(&main_body("println(1);")), ["Q0211"]);
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); println(\"{}\", q); discard(q);"
        )),
        ["Q0212"]
    );
    assert_eq!(codes(&main_body("int s = \"hi\";")), ["Q0204"]);
    assert_eq!(codes(&main_body("String s = \"a{}\";")), ["Q0214"]);
    assert_eq!(codes("void f() {}\nvoid f() {}\nvoid main() {}"), ["Q0205"]);
    assert_eq!(codes("void f(int a, int a) {}\nvoid main() {}"), ["Q0205"]);
    assert_eq!(codes("void helper() {}"), ["Q0206"]);
    assert_eq!(codes("void main(int x) {}"), ["Q0206"]);
    // `main` may return the exit code and take the command-line arguments.
    ok("int main() {\n    return 0;\n}");
    ok("void main(String[] args) {}");
    ok("int main(String[] args) {\n    return len(args);\n}");
    assert_eq!(codes("void main(int[] args) {}"), ["Q0206"]);
    assert_eq!(codes("void main(String[] a, String[] b) {}"), ["Q0206"]);
    assert_eq!(codes("String main() {\n    return \"\";\n}"), ["Q0206"]);
    assert_eq!(codes("int main() {}"), ["Q0213"]);
    assert_eq!(codes("void h() {}\nvoid main() {}"), ["Q0217"]);
    // Qubit parameters and quantum return types are supported; Q0219 is retired.
    ok("void f(Qubit q) {}\nvoid main() {}");
    ok("Qubit f() {\n    return qubit();\n}\nvoid main() {\n    discard(f());\n}");
    assert_eq!(
        codes("int f() { if true { return 1; } }\nvoid main() {}"),
        ["Q0213"]
    );
    ok("int f() { if true { return 1; } else { return 2; } }\nvoid main() { int x = f(); }");
    assert_eq!(codes("int f() { return true; }\nvoid main() {}"), ["Q0204"]);
    // Every error is reported, not just the first.
    assert_eq!(
        codes(&main_body("int a = y; int b = z; boolean c = 1;")),
        ["Q0201", "Q0201", "Q0204"]
    );
}

#[test]
fn runtime_errors_have_positions() {
    assert_eq!(
        runtime_error(&main_body("int x = 9223372036854775807;\nint y = x + 1;")),
        ("Q0401", 3, 9)
    );
    assert_eq!(
        runtime_error(&main_body("int z = 0;\nint y = 7 % z;")),
        ("Q0402", 3, 9)
    );
    assert_eq!(
        runtime_error(&main_body("QReg r = qreg(2);\nh(r[2]);\ndiscard(r);")).0,
        "Q0403"
    );
    assert_eq!(
        runtime_error(&main_body(
            "QReg r = qreg(2);\nint i = 1;\ncx(r[i], r[1]);\ndiscard(r);"
        )),
        ("Q0303", 4, 1)
    );
    assert_eq!(
        runtime_error(
            "int f(int n) {\n    return f(n + 1);\n}\nvoid main() {\n    int x = f(0);\n}\n"
        )
        .0,
        "Q0405"
    );
    assert_eq!(
        runtime_error(&main_body("int n = 0 - 1;\nQReg r = qreg(n);\ndiscard(r);")).0,
        "Q0408"
    );
    assert_eq!(
        runtime_error(&main_body("QReg r = qreg(2000000);\ndiscard(r);")).0,
        "Q0409"
    );
    assert_eq!(
        runtime_error(&main_body(
            "BitReg m = measure(qreg(65));\nprintln(\"{}\", m.asInt());"
        )),
        ("Q0410", 3, 15)
    );
    // Sizes beyond `usize` on 32-bit targets (the wasm playground) must not wrap around.
    for n in ["4294967296", "4294967297", "9223372036854775807"] {
        assert_eq!(
            runtime_error(&main_body(&format!("QReg r = qreg({n});\ndiscard(r);"))).0,
            "Q0409",
            "{n}"
        );
    }
    // NaN or ±inf would poison the whole state vector (every outcome 0) and print `rx(NaN)`.
    for angle in ["0.0 / 0.0", "1.0 / 0.0", "-1.0 / 0.0", "1e308 * 10.0"] {
        for gate in ["rx", "ry", "rz"] {
            let body = format!("Qubit q = qubit();\n{gate}(q, {angle});\ndiscard(q);");
            assert_eq!(
                runtime_error(&main_body(&body)),
                ("Q0411", 3, 7),
                "{gate}({angle})"
            );
            assert!(qasm("t.qx", &main_body(&body)).is_err(), "{gate}({angle})");
        }
    }
}

// `int.MIN % -1` is 0 (as in Java); only `int.MIN / -1` overflows.
#[test]
fn remainder_of_min_by_minus_one_is_zero() {
    let min = "int a = -9223372036854775807 - 1;";
    assert_eq!(
        run_ok(
            &main_body(&format!("{min}\nprintln(\"{{}} {{}}\", a % -1, a % 1);")),
            1
        ),
        "0 0\n"
    );
    assert_eq!(
        runtime_error(&main_body(&format!("{min}\nint b = a / -1;"))).0,
        "Q0401"
    );
}

// `[0, 1, 1]` lists r[0] first; `0b110` and `asInt() == 6` read r[0] as the least significant bit.
#[test]
fn bit_registers_print_as_array_binary_and_int() {
    let src = main_body(
        "QReg r = qreg(3);\nx(r[1]);\nx(r[2]);\nBitReg m = measure(r);\nprintln(\"{} {} {} {}\", m, m.toBinary(), m.asInt(), m[0]);",
    );
    assert_eq!(run_ok(&src, 1), "[0, 1, 1] 0b110 6 0\n");
    assert_eq!(
        run_ok(&src, 5),
        "[0, 1, 1]  0b110 = 6  ×5\n  [0, 1, 1] 0b110 6 0\n"
    );
    let empty = main_body(
        "BitReg m = measure(qreg(0));\nprintln(\"{} {} {}\", m, m.toBinary(), m.asInt());",
    );
    assert_eq!(run_ok(&empty, 1), "[] 0b0 0\n");
    let m = "BitReg m = measure(qreg(2));";
    assert_eq!(
        codes(&main_body(&format!("{m} int v = m.asInt();"))),
        ["Q0218"]
    );
    assert_eq!(
        codes(&main_body(&format!("{m} println(\"{{}}\", m.foo());"))),
        ["Q0220"]
    );
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); println(\"{}\", q.toBinary()); discard(q);"
        )),
        ["Q0220"]
    );
}

#[test]
fn bell_and_ghz_outcomes_are_correlated() {
    let rows = table(&run_ok(BELL, 200));
    let mut keys: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["[0, 0]  0b00 = 0", "[1, 1]  0b11 = 3"]);
    assert_eq!(rows.iter().map(|r| r.1).sum::<u64>(), 200);
    for (bits, _, lines) in &rows {
        let want = if bits.starts_with("[0") {
            "a=0 b=0"
        } else {
            "a=1 b=1"
        };
        assert_eq!(lines, &[want]);
    }
    // Rows are sorted by count, largest first.
    assert!(rows[0].1 >= rows[1].1);
    let one = run_ok(GHZ, 1);
    assert!(
        one == "[0, 0, 0, 0, 0]  0b00000 = 0\n" || one == "[1, 1, 1, 1, 1]  0b11111 = 31\n",
        "{one}"
    );
    for (bits, _, _) in table(&run_ok(GHZ, 100)) {
        assert!(
            bits == "[0, 0, 0, 0, 0]  0b00000 = 0" || bits == "[1, 1, 1, 1, 1]  0b11111 = 31",
            "{bits}"
        );
    }
}

#[test]
fn teleport_always_delivers_one() {
    let rows = table(&run_ok(TELEPORT, 64));
    assert_eq!(rows.iter().map(|r| r.1).sum::<u64>(), 64);
    for (bits, _, lines) in rows {
        // c[2] is bob's measurement: always 1.
        assert!(
            bits.starts_with('[') && bits[1..].contains(", 1]"),
            "{bits}"
        );
        assert_eq!(lines, ["bob=1"]);
    }
}

#[test]
fn qasm_matches_the_library_circuit() {
    let mut c = qxide::Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.measure(a);
    c.measure(b);
    assert_eq!(qasm("t.qx", BELL).ok().unwrap(), c.to_qasm());

    let tele = qasm("t.qx", TELEPORT).ok().unwrap();
    assert!(tele.contains("if (c[1] == true) {\n  x q[2];\n}"), "{tele}");
    let els = main_body("Qubit a = qubit(); Qubit t = qubit(); h(a); Bit m = measure(a); if m { x(t); } else { z(t); } discard(t);");
    let q = qasm("t.qx", &els).ok().unwrap();
    assert!(
        q.contains("if (c[0] == true) {\n  x q[1];\n}\nif (c[0] == false) {\n  z q[1];\n}"),
        "{q}"
    );
    // A `boolean` condition is decided at build time: only the taken branch is emitted.
    let q = qasm(
        "t.qx",
        &main_body("Qubit q = qubit(); if 1 < 2 { x(q); } else { h(q); } discard(q);"),
    )
    .ok()
    .unwrap();
    assert!(q.contains("x q[0];") && !q.contains("h q[0];"), "{q}");
}

#[test]
fn classical_programs_run_without_simulation() {
    let src = "int fib(int n) {\n    if n < 2 {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}\nvoid main() {\n    int s = 0;\n    for i in 0..10 {\n        if i % 2 == 1 && !(i == 5) {\n            s = s + i;\n        }\n    }\n    println(\"fib={} s={} {{ok}}\", fib(15), s);\n}\n";
    assert_eq!(run_ok(src, 1), "fib=610 s=20 {ok}\n");
    // No measurement: the output does not depend on the shot, so it is printed once.
    assert_eq!(run_ok(src, 3), "fib=610 s=20 {ok}\n");
}

#[test]
fn simulation_errors_become_diagnostics() {
    // 30 qubits with a non-Clifford gate need a 16 GiB state vector, over the 1 GiB default.
    let src =
        main_body("QReg r = qreg(30);\nt(r[0]);\nBitReg m = measure(r);\nprintln(\"{}\", m);");
    ok(&src);
    let Err(Failure(d)) = run("t.qx", &src, 1, 0) else {
        panic!("expected a simulation error")
    };
    assert!(d.contains("error[Q0406]"), "{d}");
    // QASM export does not simulate, so it still works.
    assert!(qasm("t.qx", &src).is_ok());
}

#[test]
fn rendering_shows_position_snippet_and_notes() {
    let src =
        "void main() {\r\n    Qubit q = qubit();\r\n    Bit m = measure(q);\r\n    h(q);\r\n}\r\n";
    let Err(Failure(d)) = check("bad.qx", src) else {
        panic!("expected errors")
    };
    assert!(d.starts_with("error[Q0301]: "), "{d}");
    assert!(d.contains("--> bad.qx:4:7\n"), "{d}");
    assert!(d.contains("4 |     h(q);\n  |       ^\n"), "{d}");
    assert!(d.contains("--> bad.qx:3:13\n"), "{d}");
    assert!(
        d.contains("3 |     Bit m = measure(q);\n  |             ^^^^^^^^^^\n"),
        "{d}"
    );
    assert!(!d.contains('\r'), "{d:?}");
}

fn rendered(src: &str) -> String {
    match run("t.qx", src, 1, 0) {
        Ok(out) => panic!("expected errors, got output {out:?}"),
        Err(Failure(d)) => d,
    }
}

// Diagnostics are English: no Hangul in any message, note or help (out-of-memory errors are
// re-worded with GiB sizes and a help line).
#[test]
fn diagnostics_are_english_with_help_lines() {
    let hangul = |s: &str| s.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c));
    let samples = [
        "void main() { int x = 1 # 2; }".to_string(),
        "void main() { int x = \"\\q\"; }".to_string(),
        "void main() { long x = 1; }".to_string(),
        "void main() { boolean b = 1 < 2 < 3; }".to_string(),
        "fn main() {}".to_string(),
        main_body("Qubit q = qubit();\nh(q);"),
        main_body("Qubit q = qubit();\nBit m = measure(q);\nh(q);"),
        main_body("final int x = 1;\nx = 2;\nint y = z;\nqubit();"),
        main_body("Qubit a = qubit(); Qubit t = qubit(); Bit m = measure(a); if m { println(\"x\"); } discard(t);"),
        main_body("QReg r = qreg(30);\nt(r[0]);\nBitReg m = measure(r);\nprintln(\"{}\", m);"),
        main_body("QReg r = qreg(200000);\nBitReg m = measure(r);\nprintln(\"{}\", m);"),
        main_body("BitReg m = measure(qreg(65));\nprintln(\"{}\", m.asInt());"),
        "int f(int n) {\n    return f(n + 1);\n}\nvoid main() {\n    int x = f(0);\n}\n".to_string(),
        "void f(Qubit q) {\n    discard(q);\n}\nvoid main() {}\n".to_string(),
        "void f(consume int n, var x) {}\nvoid main() {}\n".to_string(),
        main_body("var q = qubit();\nboolean b = (boolean) 1;\ndouble PI = 1.0;"),
        format!("void f(Qubit q) {{\n    Bit m = measure(q);\n}}\n{}", main_body("Qubit a = qubit();\nBit m = measure(a);\nQubit t = qubit();\nif m {\n    f(t);\n}\ndiscard(t);")),
    ];
    for src in &samples {
        let d = rendered(src);
        assert!(d.starts_with("error[Q"), "{d}");
        assert!(!hangul(&d), "non-English diagnostic:\n{d}");
    }
    let d = rendered(&main_body("Qubit q = qubit();\nh(q);"));
    assert!(d.contains("\nhelp: "), "{d}");
    let d = rendered("void main() { boolean b = 1 < 2 < 3; }");
    assert!(d.contains("\nhelp: ") && d.contains("&&"), "{d}");
    let d = rendered(&main_body(
        "QReg r = qreg(30);\nt(r[0]);\nBitReg m = measure(r);\nprintln(\"{}\", m);",
    ));
    assert!(
        d.contains("error[Q0406]") && d.contains("16.0 GiB") && d.contains("\nhelp: "),
        "{d}"
    );
}

const BELL_PAIR: &str = "void bellPair(Qubit a, Qubit b) {\n    h(a);\n    cx(a, b);\n}\n\n";
const SINK: &str = "void sink(consume Qubit q) {\n    discard(q);\n}\n\n";

#[test]
fn qubit_parameters_are_borrowed_by_default() {
    let src = format!("{BELL_PAIR}void main() {{\n    Qubit a = qubit();\n    Qubit b = qubit();\n    bellPair(a, b);\n    Bit ma = measure(a);\n    Bit mb = measure(b);\n    println(\"a={{}} b={{}}\", ma, mb);\n}}\n");
    // A borrowed call lowers to the same circuit as writing the gates inline.
    assert_eq!(
        qasm("t.qx", &src).ok().unwrap(),
        qasm("t.qx", BELL).ok().unwrap()
    );
    let rows = table(&run_ok(&src, 200));
    assert_eq!(rows.len(), 2);
    for (bits, _, lines) in &rows {
        let want = if bits.starts_with("[0, 0]") {
            "a=0 b=0"
        } else {
            "a=1 b=1"
        };
        assert_eq!(lines, &[want.to_string()], "{bits}");
    }
    // Whole registers and register elements can be borrowed, also through nested calls.
    let reg = format!("{BELL_PAIR}void flipAll(QReg r) {{\n    for i in 0..len(r) {{\n        x(r[i]);\n    }}\n}}\n\nvoid twice(QReg r) {{\n    flipAll(r);\n    flipAll(r);\n    x(r[1]);\n}}\n\nvoid main() {{\n    QReg r = qreg(3);\n    twice(r);\n    flipAll(r);\n    BitReg m = measure(r);\n    println(\"{{}}\", m);\n}}\n");
    assert_eq!(run_ok(&reg, 1), "[1, 0, 1]\n");
    // A `return` in a function with borrowed parameters does not leak them.
    ok("void f(Qubit q) {\n    if true {\n        return;\n    }\n    h(q);\n}\nvoid main() {}\n");
}

#[test]
fn borrowed_parameters_cannot_be_consumed() {
    let measured = "void f(Qubit q) {\n    Bit m = measure(q);\n}\nvoid main() {}\n";
    assert_eq!(errors(measured), vec![("Q0313", 2, 21)]);
    let discarded = "void f(QReg r) {\n    discard(r);\n}\nvoid main() {}\n";
    assert_eq!(errors(discarded), vec![("Q0313", 2, 13)]);
    let moved = "void f(Qubit q) {\n    Qubit p = q;\n    discard(p);\n}\nvoid main() {}\n";
    assert_eq!(errors(moved), vec![("Q0313", 2, 15)]);
    // Passing a borrowed qubit on to a `consume` parameter would consume it too.
    let passed = format!("{SINK}void f(Qubit q) {{\n    sink(q);\n}}\nvoid main() {{}}\n");
    assert_eq!(errors(&passed), vec![("Q0313", 6, 10)]);
    let d = rendered(measured);
    assert!(
        d.contains("error[Q0313]") && d.contains("\nnote: ") && d.contains("consume Qubit q"),
        "{d}"
    );
}

#[test]
fn consume_parameters_transfer_ownership() {
    let src = "void measureAll(consume QReg r) {\n    BitReg m = measure(r);\n    println(\"{}\", m);\n}\n\nvoid main() {\n    QReg r = qreg(2);\n    x(r[0]);\n    measureAll(r);\n}\n";
    assert_eq!(run_ok(src, 1), "[1, 0]\n");
    // A fresh value can be handed over directly.
    ok("void sink(consume QReg r) {\n    discard(r);\n}\nvoid main() {\n    sink(qreg(2));\n}\n");
    // The callee must measure or discard what it consumes, on every path.
    let kept = "void keep(consume Qubit q) {\n    h(q);\n}\nvoid main() {}\n";
    assert_eq!(errors(kept), vec![("Q0302", 1, 25)]);
    let early = "void keep(consume Qubit q) {\n    if true {\n        return;\n    }\n    discard(q);\n}\nvoid main() {}\n";
    assert_eq!(errors(early), vec![("Q0302", 1, 25)]);
    // The caller cannot use it afterwards, nor hand it over inside a loop.
    let after =
        format!("{SINK}void main() {{\n    Qubit q = qubit();\n    sink(q);\n    h(q);\n}}\n");
    assert_eq!(errors(&after), vec![("Q0301", 8, 7)]);
    let looped = format!("{SINK}void main() {{\n    Qubit q = qubit();\n    for i in 0..2 {{\n        sink(q);\n    }}\n}}\n");
    assert_eq!(codes(&looped), ["Q0302", "Q0305"]);
    assert_eq!(codes("void f(consume int n) {}\nvoid main() {}"), ["Q0221"]);
    // Moving one element out of a register is still a partial move.
    let partial = format!(
        "{SINK}void main() {{\n    QReg r = qreg(2);\n    sink(r[0]);\n    discard(r);\n}}\n"
    );
    assert_eq!(codes(&partial), ["Q0306"]);
}

#[test]
fn quantum_arguments_must_not_alias() {
    let main = |body: &str| format!("{BELL_PAIR}void main() {{\n{body}\n}}\n");
    assert_eq!(
        errors(&main("Qubit a = qubit();\nbellPair(a, a);\ndiscard(a);")),
        vec![("Q0303", 8, 13)]
    );
    ok(&main(
        "QReg r = qreg(2);\nbellPair(r[0], r[1]);\ndiscard(r);",
    ));
    assert_eq!(
        codes(&main(
            "QReg r = qreg(2);\nbellPair(r[1], r[1]);\ndiscard(r);"
        )),
        ["Q0303"]
    );
    // A whole register overlaps each of its elements.
    let whole = "void g(QReg r, Qubit q) {}\nvoid main() {\n    QReg r = qreg(2);\n    g(r, r[0]);\n    discard(r);\n}\n";
    assert_eq!(codes(whole), ["Q0303"]);
    // Borrowing and consuming the same qubit in one call is rejected once.
    let mixed = "void g(Qubit a, consume Qubit b) {\n    discard(b);\n}\nvoid main() {\n    Qubit q = qubit();\n    g(q, q);\n}\n";
    assert_eq!(codes(mixed), ["Q0303"]);
    // Run-time indices are checked when the call runs.
    let dynamic = main("QReg r = qreg(2);\nint i = 1;\nbellPair(r[i], r[1]);\ndiscard(r);");
    assert_eq!(runtime_error(&dynamic), ("Q0303", 9, 1));
    // Borrowed arguments must be variables or register elements of the right type.
    assert_eq!(
        codes(&main(
            "Qubit b = qubit();\nbellPair(qubit(), b);\ndiscard(b);"
        )),
        ["Q0311"]
    );
    assert_eq!(
        codes(&main(
            "QReg r = qreg(2);\nQubit b = qubit();\nbellPair(r, b);\ndiscard(r);\ndiscard(b);"
        )),
        ["Q0204"]
    );
}

#[test]
fn var_infers_classical_local_types() {
    let src = main_body("var n = 3;\nvar t = 0.5;\nvar big = n > 2;\nfinal var k = n * 2;\nvar d = 1.0;\nd = 2;\nprintln(\"{} {} {} {} {}\", n, t, big, k, d);");
    assert_eq!(run_ok(&src, 1), "3 0.5 true 6 2.0\n");
    // The inferred type is fixed afterwards.
    assert_eq!(codes(&main_body("var n = 1;\nn = 2.5;")), ["Q0204"]);
    assert_eq!(codes(&main_body("final var k = 1;\nk = 2;")), ["Q0207"]);
    // Quantum values, measurement results and `void` need an explicit type.
    assert_eq!(
        errors(&main_body("var q = qubit();")),
        vec![("Q0222", 2, 1)]
    );
    assert_eq!(
        codes(&main_body("Qubit q = qubit();\nvar m = measure(q);")),
        ["Q0222"]
    );
    assert_eq!(
        codes(&main_body(
            "BitReg m = measure(qreg(2));\nvar v = m.asInt();"
        )),
        ["Q0222"]
    );
    assert_eq!(codes(&main_body("var v = println(\"\");")), ["Q0222"]);
    // `var` is for local variables with an initializer only.
    assert_eq!(codes("void f(var x) {}\nvoid main() {}"), ["Q0111"]);
    assert_eq!(
        codes("var f() {\n    return 1;\n}\nvoid main() {}"),
        ["Q0111"]
    );
    assert_eq!(codes(&main_body("var x;")), ["Q0105"]);
}

#[test]
fn casts_between_int_and_double() {
    let src = main_body("double x = 2.9;\nint n = 7;\nprintln(\"{} {} {} {} {} {} {} {}\", (int) x, (int) -x, -(int) x, (double) n / 2, (int) x * 2, (int) (x * 2), (int) 3, (double) 1.5);");
    assert_eq!(run_ok(&src, 1), "2 -2 -2 3.5 4 5 3 1.5\n");
    // Java's rules: NaN becomes 0, out-of-range values saturate (2^63 does too).
    let edge = main_body("double z = 0.0;\nprintln(\"{} {} {} {}\", (int) (z / z), (int) 1e300, (int) -1e300, (int) 9223372036854775807.0);");
    assert_eq!(
        run_ok(&edge, 1),
        format!("0 {} {} {}\n", i64::MAX, i64::MIN, i64::MAX)
    );
    assert_eq!(codes(&main_body("boolean b = (boolean) 1;")), ["Q0223"]);
    assert_eq!(codes(&main_body("int n = (int) true;")), ["Q0223"]);
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit();\nint n = (int) q;\ndiscard(q);"
        )),
        ["Q0223"]
    );
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit();\nBit m = measure(q);\nint n = (int) m;"
        )),
        ["Q0218"]
    );
    // Type names cannot name values, so `(int) x` is always a cast.
    assert_eq!(codes(&main_body("int int = 1;")), ["Q0112"]);
    assert_eq!(codes("void f(double double) {}\nvoid main() {}"), ["Q0112"]);
    assert_eq!(
        codes("int Qubit() {\n    return 1;\n}\nvoid main() {}"),
        ["Q0112"]
    );
    assert_eq!(codes(&main_body("for int in 0..3 {}")), ["Q0112"]);
    let deep = main_body(&format!("int x = {}1;", "(int) ".repeat(500)));
    assert_eq!(codes(&deep), ["Q0106"]);
}

#[test]
fn pi_is_a_builtin_double_constant() {
    use std::f64::consts::{FRAC_PI_2, PI};
    assert_eq!(
        run_ok(&main_body("println(\"{} {}\", PI, PI / 2);"), 1),
        format!("{PI:?} {FRAC_PI_2:?}\n")
    );
    let src = main_body("Qubit q = qubit();\nrz(q, PI);\nBit m = measure(q);");
    let q = qasm("t.qx", &src).ok().unwrap();
    assert!(q.contains(&format!("rz({PI:?}) q[0];")), "{q}");
    assert_eq!(codes(&main_body("double PI = 3.0;")), ["Q0217"]);
    assert_eq!(codes("void f(double PI) {}\nvoid main() {}"), ["Q0217"]);
    assert_eq!(
        codes("double PI() {\n    return 3.0;\n}\nvoid main() {}"),
        ["Q0217"]
    );
    assert_eq!(codes(&main_body("for PI in 0..2 {}")), ["Q0217"]);
    assert_eq!(codes(&main_body("PI = 3.0;")), ["Q0207"]);
    let d = rendered(&main_body("double PI = 3.0;"));
    assert!(
        d.contains("\nhelp: ") && d.contains("PI") && !d.contains("function"),
        "{d}"
    );
    // Assigning to `PI` suggests a local variable, not removing `final`.
    let d = rendered(&main_body("PI = 3.0;"));
    assert!(
        d.contains("\nhelp: ") && d.contains("double angle = PI;") && !d.contains("final"),
        "{d}"
    );
}

#[test]
fn new_examples_run() {
    let rows = table(&run_ok(FUNCTIONS, 200));
    let mut heads: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    heads.sort_unstable();
    assert_eq!(heads, ["[0, 0]  0b00 = 0", "[1, 1]  0b11 = 3"]);
    assert_eq!(run_ok(ROTATION, 1), "ry total 90 degrees (90.0), m=1\n");
    let q = qasm("t.qx", ROTATION).ok().unwrap();
    assert_eq!(q.matches("ry(").count(), 4, "{q}");
}

const CONDITIONAL_FN: &str = include_str!("../../examples/qx/conditional_fn.qx");
const MAKE: &str = "Qubit make() {\n    Qubit q = qubit();\n    h(q);\n    return q;\n}\n\n";

fn same_qasm(a: &str, b: &str) {
    assert_eq!(qasm("t.qx", a).ok().unwrap(), qasm("t.qx", b).ok().unwrap());
}

#[test]
fn functions_return_qubits_to_the_caller() {
    let ghz = "QReg ghz(int n) {\n    QReg r = qreg(n);\n    h(r[0]);\n    for i in 1..n {\n        cx(r[0], r[i]);\n    }\n    return r;\n}\n\nvoid main() {\n    QReg r = ghz(5);\n    BitReg m = measure(r);\n    println(\"{}  {} = {}\", m, m.toBinary(), m.asInt());\n}\n";
    // Building the register in a function gives the same circuit as examples/qx/ghz.qx.
    same_qasm(ghz, GHZ);
    let rows = table(&run_ok(ghz, 200));
    let mut heads: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    heads.sort_unstable();
    assert_eq!(
        heads,
        [
            "[0, 0, 0, 0, 0]  0b00000 = 0",
            "[1, 1, 1, 1, 1]  0b11111 = 31"
        ]
    );
    // `consume` in, return out: ownership passes through the function.
    ok("Qubit hadamard(consume Qubit q) {\n    h(q);\n    return q;\n}\nvoid main() {\n    Qubit q = hadamard(qubit());\n    Bit m = measure(q);\n}\n");
    ok(&format!(
        "{MAKE}void main() {{\n    Bit m = measure(make());\n}}\n"
    ));
}

#[test]
fn returned_qubits_follow_the_linear_rules() {
    let main = |body: &str| format!("{MAKE}void main() {{\n{body}\n}}\n");
    // The caller must consume the result, exactly once.
    assert_eq!(errors(&main("Qubit q = make();")), vec![("Q0302", 8, 7)]);
    assert_eq!(errors(&main("make();")), vec![("Q0312", 8, 1)]);
    assert_eq!(
        errors(&main(
            "Qubit q = make();\nBit a = measure(q);\nBit b = measure(q);"
        )),
        vec![("Q0301", 10, 17)]
    );
    assert_eq!(codes(&main("QReg r = make();\ndiscard(r);")), ["Q0204"]);
    assert_eq!(codes(&main("h(make());")), ["Q0311"]);
    // Qubits the function keeps instead of returning are still leaks.
    let kept = "Qubit f() {\n    Qubit a = qubit();\n    Qubit b = qubit();\n    return a;\n}\nvoid main() {}\n";
    assert_eq!(errors(kept), vec![("Q0302", 3, 11)]);
    // Borrowed parameters and single register elements cannot be returned.
    assert_eq!(
        errors("Qubit f(Qubit q) {\n    return q;\n}\nvoid main() {}\n"),
        vec![("Q0313", 2, 12)]
    );
    assert_eq!(
        codes("Qubit f() {\n    QReg r = qreg(2);\n    return r[0];\n}\nvoid main() {}\n"),
        ["Q0302", "Q0306"]
    );
    assert_eq!(
        codes("Qubit f() {\n    return 1;\n}\nvoid main() {}\n"),
        ["Q0204"]
    );
    assert_eq!(
        codes("Qubit f() {\n    Qubit q = qubit();\n    discard(q);\n}\nvoid main() {}\n"),
        ["Q0213"]
    );
}

#[test]
fn gate_only_functions_run_inside_bit_conditions() {
    // The function version lowers to the same conditional blocks as the inline teleport.
    same_qasm(TELEPORT_FN, TELEPORT);
    for (_, _, lines) in table(&run_ok(TELEPORT_FN, 200)) {
        assert_eq!(lines, ["bob=1"]);
    }
    // `else`, loops with rotations, a nested `if` on a `Bit` parameter and a classical helper.
    let inline = "void main() {\n    Qubit coin = qubit();\n    h(coin);\n    Bit m = measure(coin);\n    Qubit other = qubit();\n    h(other);\n    Bit f = measure(other);\n    QReg r = qreg(2);\n    if m {\n        for i in 0..len(r) {\n            rz(r[i], PI / 4);\n        }\n        if f {\n            x(r[0]);\n        }\n    } else {\n        for i in 0..2 {\n            x(r[1]);\n        }\n    }\n    BitReg out = measure(r);\n    println(\"m={} f={} r={}\", m, f, out);\n}\n";
    same_qasm(CONDITIONAL_FN, inline);
    let q = qasm("t.qx", CONDITIONAL_FN).ok().unwrap();
    assert!(q.contains("if (c[0] == false) {"), "{q}");
    assert!(q.contains("  if (c[1] == true) {\n    x q[0];\n  }"), "{q}");
    for (_, _, lines) in table(&run_ok(CONDITIONAL_FN, 300)) {
        // r[0] is flipped only when both coins are 1; the two `x(r[1])` cancel out.
        assert!(
            matches!(
                lines[0].as_str(),
                "m=1 f=1 r=[1, 0]" | "m=1 f=0 r=[0, 0]" | "m=0 f=1 r=[0, 0]" | "m=0 f=0 r=[0, 0]"
            ),
            "{lines:?}"
        );
    }
    // Recursion is fine as long as every reachable body only applies gates.
    let rec = "void flipN(Qubit q, int n) {\n    if n > 0 {\n        x(q);\n        flipN(q, n - 1);\n    }\n}\nvoid main() {\n    Qubit a = qubit();\n    h(a);\n    Bit m = measure(a);\n    Qubit t = qubit();\n    if m {\n        flipN(t, 3);\n    }\n    Bit out = measure(t);\n}\n";
    let q = qasm("t.qx", rec).ok().unwrap();
    assert!(
        q.contains("if (c[0] == true) {\n  x q[0];\n  x q[0];\n  x q[0];\n}"),
        "{q}"
    );
}

#[test]
fn conditional_calls_reject_what_blocks_cannot_hold() {
    let pre =
        "void main() {\n    Qubit a = qubit();\n    Bit m = measure(a);\n    Qubit t = qubit();\n";
    // `f` calls `g`, which prints; the note points at the call inside `f`.
    let transitive = format!("void g() {{\n    println(\"x\");\n}}\nvoid f(Qubit q) {{\n    h(q);\n    g();\n}}\n{pre}    if m {{\n        f(t);\n    }}\n    discard(t);\n}}\n");
    assert_eq!(errors(&transitive), vec![("Q0314", 13, 9)]);
    let d = rendered(&transitive);
    assert!(
        d.contains("--> t.qx:6:5") && d.contains("`g`") && d.contains("\nhelp: "),
        "{d}"
    );
    // A `consume` parameter would consume the qubit on only some shots; reported once, no leak.
    let consume = format!("void eat(consume Qubit q) {{\n    discard(q);\n}}\n{pre}    if m {{\n        eat(t);\n    }}\n}}\n");
    assert_eq!(errors(&consume), vec![("Q0314", 9, 9)]);
    assert!(rendered(&consume).contains("--> t.qx:1:24"));
    // A function returning a qubit would allocate inside the block; reported once.
    let make =
        format!("{MAKE}{pre}    if m {{\n        Qubit p = make();\n    }}\n    discard(t);\n}}\n");
    assert_eq!(errors(&make), vec![("Q0314", 12, 19)]);
    // Measuring inside the callee, also through recursion.
    let measured = format!("void probe() {{\n    Bit b = measure(qubit());\n}}\n{pre}    if m {{\n        probe();\n    }}\n    discard(t);\n}}\n");
    assert_eq!(codes(&measured), ["Q0314"]);
    let rec = format!("void bad(int n) {{\n    if n > 0 {{\n        bad(n - 1);\n    }} else {{\n        println(\"done\");\n    }}\n}}\n{pre}    if m {{\n        bad(2);\n    }}\n    discard(t);\n}}\n");
    assert_eq!(codes(&rec), ["Q0314"]);
    // Outside the `if` the same calls are fine.
    ok(&format!("void g() {{\n    println(\"x\");\n}}\nvoid f(Qubit q) {{\n    h(q);\n    g();\n}}\n{pre}    f(t);\n    discard(t);\n}}\n"));
}

#[test]
fn run_limits_cap_state_vector_qubits_and_tableaus() {
    let rotated = main_body(
        "    QReg r = qreg(3);\n    t(r[0]);\n    BitReg m = measure(r);\n    println(\"{}\", m);",
    );
    let clifford = main_body(
        "    QReg r = qreg(3);\n    h(r[0]);\n    BitReg m = measure(r);\n    println(\"{}\", m);",
    );
    let cap = |n| Limits {
        max_state_vector_qubits: Some(n),
        ..Limits::CLI
    };
    let Err(Failure(d)) = run_with("t.qx", &rotated, 1, 0, cap(2)) else {
        panic!("3 qubits must exceed a cap of 2");
    };
    assert!(
        d.starts_with("error[Q0406]: ") && d.contains("--> t.qx:1:6"),
        "{d}"
    );
    assert!(d.contains("\nhelp: "), "{d}");
    assert!(run_with("t.qx", &rotated, 1, 0, cap(3)).is_ok());
    // Clifford circuits run on the stabilizer backend, which the qubit cap does not cover.
    assert!(run_with("t.qx", &clifford, 1, 0, cap(2)).is_ok());
    let tiny = Limits {
        stabilizer_bytes: 8,
        ..Limits::CLI
    };
    let Err(Failure(d)) = run_with("t.qx", &clifford, 1, 0, tiny) else {
        panic!("an 8-byte limit must reject the tableau");
    };
    assert!(d.starts_with("error[Q0406]: "), "{d}");
    assert_eq!(
        run_with("t.qx", &rotated, 5, 9, Limits::CLI).ok(),
        run("t.qx", &rotated, 5, 9).ok()
    );
}

const EAT: &str = "double eat(consume Qubit q) {\n    Bit m = measure(q);\n    return 1.0;\n}\n\
                   int eatInt(consume Qubit q) {\n    discard(q);\n    return 0;\n}\n\
                   int eatReg(consume QReg r) {\n    discard(r);\n    return 0;\n}\n\
                   int peek(Qubit q) {\n    h(q);\n    return 0;\n}\n\
                   void g(Qubit a, int k) {\n    h(a);\n}\n";

// A qubit lent to a call stays borrowed until the call runs: a later argument (or the index of
// the same operand) must not consume it, or the gate would act on a measured qubit.
#[test]
fn borrowed_operand_cannot_be_consumed_by_a_later_argument() {
    let rejected = [
        "Qubit q = qubit();\nrx(q, eat(q));",
        "QReg r = qreg(2);\nh(r[eatReg(r)]);",
        "Qubit q = qubit();\ng(q, eatInt(q));",
        "Qubit q = qubit();\nQReg r = qreg(2);\ncx(q, r[eatInt(q)]);\ndiscard(r);",
        "QReg r = qreg(3);\nccx(r[0], r[1], r[eatReg(r)]);",
    ];
    for body in rejected {
        let src = format!("{EAT}{}", main_body(body));
        let c = codes(&src);
        assert!(c.contains(&"Q0301"), "{body}: {c:?}");
        assert!(build(&src).is_err(), "{body}");
    }
    // Consuming a different qubit, or borrowing in both places, is fine.
    for body in [
        "Qubit q = qubit();\nQubit p = qubit();\nrx(q, eat(p));\ndiscard(q);",
        "Qubit q = qubit();\ng(q, peek(q));\ndiscard(q);",
        "QReg r = qreg(2);\nh(r[peek(r[0])]);\ndiscard(r);",
        "Qubit q = qubit();\nint k = eatInt(q);\nQubit p = qubit();\ng(p, k);\ndiscard(p);",
    ] {
        let src = format!("{EAT}{}", main_body(body));
        ok(&src);
        assert!(build(&src).is_ok(), "{body}");
    }
}

// `&&`/`||` skip their right operand at run time, so consuming a qubit there is conditional.
#[test]
fn short_circuit_right_operand_cannot_consume() {
    for body in [
        "Qubit q = qubit();\nboolean b = true || eatInt(q) == 0;\nprintln(\"{}\", b);",
        "Qubit q = qubit();\nboolean b = false && eatInt(q) == 0;\ndiscard(q);",
        "Qubit q = qubit();\nboolean b = 1 < 2 && 0 < eatInt(q);\ndiscard(q);",
    ] {
        // Like a one-armed `if`, the qubit then counts as consumed (as in Q0304).
        let c = codes(&format!("{EAT}{}", main_body(body)));
        assert_eq!(c.first(), Some(&"Q0315"), "{body}: {c:?}");
        assert!(!c.contains(&"Q0302"), "{body}: {c:?}");
    }
    // The left operand always runs; borrowing on the right is fine.
    for body in [
        "Qubit q = qubit();\nboolean b = eatInt(q) == 0 || true;\nprintln(\"{}\", b);",
        "Qubit q = qubit();\nboolean b = true || peek(q) == 0;\ndiscard(q);",
    ] {
        ok(&format!("{EAT}{}", main_body(body)));
    }
}

// Programs at the nesting limit (n levels; n + 1 is Q0106) whose recursive call sits at the
// innermost level, so 199 calls each hold the full nesting on the interpreter's stack. The bit
// kinds are the exception: nested `if m` blocks count across calls toward `max_cond_depth` (64,
// Q0416), so they recurse to the call limit first and nest only in the bottom call.
type Deepest = (&'static str, usize, fn(usize) -> String);

fn deepest_programs() -> Vec<Deepest> {
    fn wrap(body: String) -> String {
        format!(
            "int f(int k) {{\n    if k > 0 {{\n        {body}\n    }}\n    return 0;\n}}\n\nvoid main() {{\n    println(\"{{}}\", f(198));\n}}\n"
        )
    }
    fn bits(body: String) -> String {
        format!(
            "void g(int k, Bit m) {{\n}}\n\nvoid f(int k, Bit m) {{\n    if k > 0 {{\n        f(k - 1, m);\n    }} else {{\n        {body}\n    }}\n}}\n\nvoid main() {{\n    Qubit q = qubit();\n    Bit m = measure(q);\n    f(197, m);\n}}\n"
        )
    }
    fn nest(n: usize, open: &str, inner: &str, close: &str) -> String {
        format!("{}{inner}{}", open.repeat(n), close.repeat(n))
    }
    vec![
        ("chain", 124, |n| {
            wrap(format!("return f(k - 1){};", " + 0".repeat(n)))
        }),
        ("unary", 122, |n| {
            wrap(format!("return {}f(k - 1);", "- ".repeat(n)))
        }),
        ("cast", 122, |n| {
            wrap(format!("return {}f(k - 1);", "(int) ".repeat(n)))
        }),
        ("call_args", 122, |n| {
            let call = nest(n, "g(", "f(k - 1)", ")");
            format!(
                "int g(int x) {{\n    return x;\n}}\n\n{}",
                wrap(format!("return {call};"))
            )
        }),
        ("method_args", 61, |n| {
            let call = nest(n, "w.g(", "f(k - 1)", ")");
            format!(
                "struct W {{\n    int z;\n\n    int g(int x) {{\n        return x;\n    }}\n}}\n\n{}",
                wrap(format!("W w = new W(0);\n        return {call};"))
            )
        }),
        ("builtin_arg", 123, |n| {
            wrap(format!("println(\"{{}}\", f(k - 1){});", " + 0".repeat(n)))
        }),
        // n triples, so 3n nested builtin calls.
        ("builtins", 40, |n| {
            wrap(format!(
                "return {};",
                nest(n, "len(measure(qreg(", "f(k - 1)", ")))")
            ))
        }),
        ("for", 122, |n| {
            wrap((0..n).fold("return f(k - 1);".to_string(), |s, i| {
                format!("for i{i} in 0..1 {{ {s} }}")
            }))
        }),
        ("if", 61, |n| {
            wrap(nest(n, "if k > 0 { ", "return f(k - 1);", " }"))
        }),
        ("else_if", 120, |n| {
            wrap(format!(
                "if k < 0 {{ return 0; }}{} else {{ return f(k - 1); }}",
                " else if k < 0 { return 0; }".repeat(n)
            ))
        }),
        ("bit_if", 61, |n| {
            bits(nest(n, "if m { ", "g(k - 1, m);", " }"))
        }),
        ("bit_else", 61, |n| {
            bits(nest(n, "if m { } else { ", "g(k - 1, m);", " }"))
        }),
    ]
}

#[test]
fn while_break_continue_and_compound_assignment() {
    let src = "int collatz(int n) {
    int steps = 0;
    while n != 1 {
        if n % 2 == 0 {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    return steps;
}

int firstAbove(int limit) {
    int i = 0;
    while true {
        i += 1;
        if i * i <= limit {
            continue;
        }
        return i;
    }
}

void main() {
    int total = 0;
    for i in 0..10 {
        if i == 7 {
            break;
        }
        if i % 2 == 1 {
            continue;
        }
        total += i;
    }
    double d = 1;
    d *= 2.5;
    d -= 1;
    int r = 17;
    r %= 5;
    String s = \"a\";
    s += 1;
    println(\"{} {} {} {} {} {}\", total, collatz(27), firstAbove(50), d, r, s);
}
";
    assert_eq!(run_ok(src, 1), "12 111 8 1.5 2 a1\n");
    // Allocating and measuring inside a `while` body, leaving with `break` after consuming.
    let q = main_body(
        "    int k = 0;
    while true {
        Qubit q = qubit();
        x(q);
        Bit b = measure(q);
        println(\"{}\", b);
        k += 1;
        if k == 2 {
            break;
        }
    }",
    );
    assert_eq!(run_ok(&q, 1), "1\n1\n");
}

#[test]
fn loop_control_rules() {
    // `break` and `continue` outside a loop, and inside a function called from a loop.
    assert_eq!(errors(&main_body("    break;")), [("Q0224", 2, 5)]);
    assert_eq!(
        errors("void f() { continue; } void main() { for i in 0..2 { f(); } }"),
        [("Q0224", 1, 12)]
    );
    // A `while` condition is a build-time `boolean`.
    assert_eq!(codes(&main_body("    while 1 { }")), ["Q0215"]);
    assert_eq!(
        codes(&main_body(
            "    Qubit q = qubit();\n    Bit b = measure(q);\n    while b { }"
        )),
        ["Q0215"]
    );
    // `break` may not leave an `if` on a measured bit, but loops inside one may use it.
    assert_eq!(
        codes(&main_body(
            "    Qubit q = qubit();
    Bit b = measure(q);
    Qubit t = qubit();
    for i in 0..3 {
        if b {
            break;
        }
    }
    discard(t);"
        )),
        ["Q0308"]
    );
    ok(&main_body(
        "    Qubit q = qubit();
    Bit b = measure(q);
    Qubit t = qubit();
    if b {
        for i in 0..3 {
            if i == 1 {
                break;
            }
            x(t);
        }
    }
    discard(t);",
    ));
    // A qubit of the loop body still live at `break` or `continue` leaks.
    assert_eq!(
        errors(&main_body(
            "    while true {\n        Qubit q = qubit();\n        break;\n    }"
        )),
        [("Q0302", 3, 15)]
    );
    assert_eq!(
        codes(&main_body(
            "    for i in 0..2 {\n        Qubit q = qubit();\n        if i == 0 {\n            continue;\n        }\n        discard(q);\n    }"
        )),
        ["Q0302"]
    );
    // Outer qubits cannot be consumed inside a `while`, even right before `break`.
    assert_eq!(
        codes(&main_body(
            "    Qubit q = qubit();\n    while true {\n        discard(q);\n        break;\n    }"
        )),
        ["Q0302", "Q0305"]
    );
    assert_eq!(
        codes(&main_body(
            "    Qubit q = qubit();\n    int k = 0;\n    while k < 2 {\n        discard(q);\n        k += 1;\n    }"
        )),
        ["Q0302", "Q0305"]
    );
    // `while true` without `break` never completes, so no `return` is needed after it.
    ok("int f() { while true { return 1; } } void main() { println(\"{}\", f()); }");
    assert_eq!(
        codes("int f() { while true { break; } } void main() { println(\"{}\", f()); }"),
        ["Q0213"]
    );
    // Compound assignment follows the rules of `x = x op e`.
    assert_eq!(
        codes(&main_body("    final int n = 1;\n    n += 1;")),
        ["Q0207"]
    );
    assert_eq!(
        codes(&main_body("    boolean b = true;\n    b += 1;")),
        ["Q0204", "Q0204"]
    );
    assert_eq!(
        runtime_error(&main_body("    int n = 9223372036854775807;\n    n += 1;")),
        ("Q0401", 3, 5)
    );
}

#[test]
fn string_methods_and_std_math() {
    let src = "import std.math;
void main() {
    String t = \"  Hello, Qxide  \".trim();
    println(\"{} {} {} {}\", t, t.length(), t.toUpperCase(), t.toLowerCase());
    println(\"{} {} {} {}\", t.substring(7, 12), t.charAt(0), t.indexOf(\"Qx\"), t.indexOf(\"zz\"));
    println(\"{} {} {}\", t.contains(\"lo\"), t.startsWith(\"He\"), t.endsWith(\"x\"));
    println(\"{} {} {}\", t.replace(\"l\", \"L\"), \"ab\".repeat(3), \"x\".replace(\"\", \"-\"));
    println(\"{} {}\", \" 42 \".parseInt() + 1, \"2.5e3\".parseDouble());
    println(\"{} {} {} {}\", \"한글\".length(), \"한글\".substring(1, 2), \"한글\".indexOf(\"글\"), \"ß\".toUpperCase());
    println(\"{} {} {} {} {}\", math.sqrt(16), math.floor(-2.5), math.ceil(2.1), math.round(2.5), math.trunc(-2.7));
    println(\"{} {} {} {}\", math.abs(-1.5), math.min(1, 2.5), math.max(0.0 / 0.0, 1.0), math.hypot(3, 4));
    println(\"{} {} {} {}\", math.absInt(-9), math.minInt(3, 7), math.maxInt(3, 7), math.powInt(-2, 5));
    println(\"{} {} {}\", math.powInt(1, 1000000), math.isNaN(0.0 / 0.0), math.isFinite(1.0 / 0.0));
}
";
    assert_eq!(
        run_ok(src, 1),
        "Hello, Qxide 12 HELLO, QXIDE hello, qxide
Qxide H 7 -1
true true false
HeLLo, Qxide ababab -x-
43 2500.0
2 글 1 SS
4.0 -3.0 3.0 3.0 -2.0
1.5 1.0 NaN 5.0
9 3 7 -32
1 true false
"
    );
    // Pure math is allowed inside an `if` on a measured bit; other std modules are not.
    ok("import std.math;
void main() {
    Qubit q = qubit();
    Qubit t = qubit();
    h(q);
    Bit b = measure(q);
    if b {
        rx(t, math.sqrt(2.0));
    }
    discard(t);
}
");
}

#[test]
fn string_method_and_std_math_errors() {
    let m = |body: &str| main_body(body);
    assert_eq!(
        codes(&m("    String s = \"a\";\n    int n = s.size();")),
        ["Q0220"]
    );
    assert_eq!(
        codes(&m("    String s = \"a\";\n    String t = s.substring(1);")),
        ["Q0203"]
    );
    assert_eq!(
        codes(&m("    String s = \"a\";\n    String t = s.repeat(1.5);")),
        ["Q0204"]
    );
    assert_eq!(
        codes(&m("    String s = \"a\";\n    int n = s.trim();")),
        ["Q0204"]
    );
    assert_eq!(
        codes("import std.math;\nvoid main() { double d = math.cube(2.0); }"),
        ["Q0701"]
    );
    let rt = |body: &str| runtime_error(&m(body)).0;
    assert_eq!(
        rt("    String s = \"abc\";\n    println(\"{}\", s.substring(2, 1));"),
        "Q0403"
    );
    assert_eq!(
        rt("    String s = \"abc\";\n    println(\"{}\", s.substring(0, 4));"),
        "Q0403"
    );
    assert_eq!(
        rt("    String s = \"abc\";\n    println(\"{}\", s.charAt(-1));"),
        "Q0403"
    );
    assert_eq!(
        rt("    String s = \"abc\";\n    println(\"{}\", s.repeat(-1));"),
        "Q0706"
    );
    assert_eq!(
        rt("    String s = \"4x\";\n    println(\"{}\", s.parseInt());"),
        "Q0419"
    );
    assert_eq!(
        rt("    String s = \"\";\n    println(\"{}\", s.parseDouble());"),
        "Q0419"
    );
    let math = |body: &str| {
        let src = format!("import std.math;\nvoid main() {{\n{body}\n}}\n");
        runtime_error(&src).0
    };
    assert_eq!(
        math("    println(\"{}\", math.absInt(-9223372036854775807 - 1));"),
        "Q0401"
    );
    assert_eq!(math("    println(\"{}\", math.powInt(3, 40));"), "Q0401");
    assert_eq!(math("    println(\"{}\", math.powInt(2, -1));"), "Q0706");
    // A huge result is refused by the step budget before it is built.
    assert_eq!(
        rt("    String s = \"ab\";\n    println(\"{}\", s.repeat(4611686018427387903).length());"),
        "Q0413"
    );
    assert_eq!(
        rt("    String s = \"aaaaaaaa\".repeat(1000000);\n    String t = s.replace(\"a\", s);"),
        "Q0413"
    );
}

pub(super) const ARRAYS: &str = "double sum(double[] xs) {
    double total = 0;
    for i in 0..len(xs) {
        total += xs[i];
    }
    return total;
}

void fill(int[] a, int v) {
    for i in 0..len(a) {
        a[i] = v * i;
    }
}

int[] primes(int n) {
    boolean[] composite = new boolean[n];
    int count = 0;
    for i in 2..n {
        if !composite[i] {
            count += 1;
            int j = i * i;
            while j < n {
                composite[j] = true;
                j += i;
            }
        }
    }
    int[] out = new int[count];
    int k = 0;
    for i in 2..n {
        if !composite[i] {
            out[k] = i;
            k += 1;
        }
    }
    return out;
}

void main() {
    double[] xs = {1, 2.5, 3};
    int[] a = new int[4];
    int[] alias = a;
    fill(alias, 3);
    a[1] += 10;
    double[][] m = new double[2][3];
    m[1][2] = 7.5;
    String[] names = new String[]{\"ada\", \"alan\"};
    names[0] += \"!\";
    int[][] jag = new int[2][];
    jag[1] = new int[]{4, 5, 6};
    var p = primes(30);
    int new = 1;
    println(\"{} {} {} {} {} {}\", sum(xs), a[1], a[3], m[1][2], len(m[0]), names[0]);
    println(\"{} {} {} {} {} {}\", len(jag[0]), jag[1][2], len(p), p[len(p) - 1], new, len(names[1]));
}
";

#[test]
fn arrays_are_shared_mutable_and_typed() {
    assert_eq!(run_ok(ARRAYS, 1), "6.5 13 9 7.5 3 ada!\n0 6 10 29 1 4\n");
    // Arrays and the qubit rules mix: a register measured per element into an int array.
    let src = main_body(
        "    QReg r = qreg(3);
    x(r[1]);
    BitReg m = measure(r);
    int[] counts = new int[2];
    counts[1] = len(m);
    println(\"{} {}\", counts[1], m);",
    );
    assert_eq!(run_ok(&src, 1), "3 [0, 1, 0]\n");
}

#[test]
fn array_errors() {
    let m = |body: &str| codes(&main_body(body));
    // Element types are build-time classical values (Q0225).
    assert_eq!(m("    Qubit[] qs = new Qubit[2];"), ["Q0225", "Q0225"]);
    assert_eq!(codes("void f(Bit[] b) {} void main() {}"), ["Q0225"]);
    // `{...}` needs a declared array type; elements must match it.
    assert_eq!(m("    var a = {1, 2};"), ["Q0204"]);
    assert_eq!(m("    int[] a = {1, true};"), ["Q0204"]);
    assert_eq!(m("    int[] a = {1.5};"), ["Q0204"]);
    // Indices and lengths are ints; only arrays take element stores.
    assert_eq!(m("    int[] a = new int[2.0];"), ["Q0204"]);
    assert_eq!(m("    int[] a = {1};\n    a[true] = 1;"), ["Q0204"]);
    assert_eq!(m("    String s = \"ab\";\n    s[0] = 1;"), ["Q0204"]);
    assert_eq!(m("    int[] a = {1};\n    a[0] += 1.5;"), ["Q0204"]);
    assert_eq!(m("    boolean[] a = {true};\n    a[0] += 1;"), ["Q0204"]);
    // Arrays are not printable and have no `==`.
    assert_eq!(m("    int[] a = {1};\n    println(\"{}\", a);"), ["Q0212"]);
    assert_eq!(
        m("    int[] a = {1};\n    boolean b = a == a;"),
        ["Q0204", "Q0204"]
    );
    // Element stores may not run inside an `if` on a measured bit, directly or via a call.
    assert_eq!(
        m("    int[] a = {1};\n    Qubit q = qubit();\n    Bit b = measure(q);\n    if b {\n        a[0] = 2;\n    }"),
        ["Q0308"]
    );
    assert_eq!(
        codes(
            "void set(int[] a) { a[0] = 1; }
void main() {
    int[] a = {0};
    Qubit q = qubit();
    Bit b = measure(q);
    if b {
        set(a);
    }
}"
        ),
        ["Q0314"]
    );
    // Runtime: bounds, negative lengths, and huge allocations stop at the step budget.
    let rt = |body: &str| runtime_error(&main_body(body)).0;
    assert_eq!(
        rt("    int[] a = new int[2];\n    int i = 2;\n    println(\"{}\", a[i]);"),
        "Q0403"
    );
    assert_eq!(
        rt("    int[] a = new int[2];\n    int i = -1;\n    a[i] = 1;"),
        "Q0403"
    );
    assert_eq!(rt("    int n = -1;\n    int[] a = new int[n];"), "Q0408");
    assert_eq!(
        rt("    int n = 1099511627776;\n    double[] a = new double[n];"),
        "Q0413"
    );
    assert_eq!(
        rt("    double[][] a = new double[1000000][1000000];"),
        "Q0413"
    );
    assert_eq!(
        rt("    int[] a = {1};\n    a[0] += 9223372036854775807;"),
        "Q0401"
    );
}

pub(super) const STRUCTS: &str = "struct Vec2 {
    double x;
    double y;
}

struct Body {
    String name;
    Vec2 pos;
    Vec2 vel;
    double mass;
    int[] tags;
}

void step(Body b, double dt) {
    b.pos.x += b.vel.x * dt;
    b.pos.y += b.vel.y * dt;
}

double energy(Body[] bodies) {
    double e = 0;
    for i in 0..len(bodies) {
        Vec2 v = bodies[i].vel;
        e += 0.5 * bodies[i].mass * (v.x * v.x + v.y * v.y);
    }
    return e;
}

void main() {
    Body[] bodies = new Body[2];
    bodies[0] = new Body(\"sun\", new Vec2(0, 0), new Vec2(0, 0), 100, new int[0]);
    bodies[1] = new Body(\"probe\", new Vec2(1, 0), new Vec2(0, 2), 1, new int[]{7});
    Body p = bodies[1];
    for k in 0..10 {
        step(p, 0.1);
    }
    bodies[1].mass *= 2;
    var d = new Vec2(1, 2);
    Vec2 alias = d;
    alias.y = 5;
    println(\"{} {} {} {} {}\", p.name, bodies[1].pos.y, energy(bodies), d.y, bodies[1].tags[0]);
    Body[] blanks = new Body[1];
    Body blank = blanks[0];
    println(\"[{}] {} {} {}\", blank.name, blank.pos.x, len(blank.tags), blank.mass);
}
";

#[test]
fn structs_are_shared_references_with_defaults() {
    assert_eq!(
        run_ok(STRUCTS, 1),
        "probe 1.9999999999999998 4.0 5.0 7
[] 0.0 0 0.0
"
    );
    // Types may be used before their declaration; `new` and field names stay usable as names.
    ok(
        "void main() { P p = new P(1); int new = p.x; println(\"{}\", new); }
struct P { int x; }",
    );
}

// Field lookups cost the same for wide structs, and long chains of struct types check in one
// pass (no per-field search).
#[test]
fn wide_structs_and_long_struct_chains() {
    let n = 3000;
    let fields: String = (0..n).map(|i| format!("int f{i}; ")).collect();
    let args = vec!["1"; n].join(", ");
    let src = format!(
        "struct W {{ {fields}}}\nvoid main() {{\n    W w = new W({args});\n    int t = 0;\n    for i in 0..1000 {{\n        t += w.f{last};\n        w.f{last} = i;\n    }}\n    println(\"{{}}\", t);\n}}\n",
        last = n - 1
    );
    assert_eq!(run_ok(&src, 1), "498502\n");
    let chain: String = (0..n)
        .map(|i| format!("struct S{i} {{ S{}[] next; }}\n", i + 1))
        .collect();
    let src =
        format!("{chain}struct S{n} {{ int x; }}\nvoid main() {{ S0 s = new S0(new S1[0]); }}\n");
    ok(&src);
    let looped = src.replace(
        &format!("struct S{n} {{ int x; }}"),
        &format!("struct S{n} {{ S0 back; }}"),
    );
    assert_eq!(codes(&looped), ["Q0227"]);
}

pub(super) const ENUMS: &str = "enum Shape {
    Circle(double r),
    Rect(double w, double h),
    Empty,
}

enum Tree {
    Leaf,
    Node(Tree left, int value, Tree right),
}

struct Holder {
    Shape shape;
    String label;
}

double area(Shape s) {
    match s {
        Circle(r) => {
            return 3 * r * r;
        }
        Shape.Rect(w, h) => {
            return w * h;
        }
        Empty => {
            return 0;
        }
    }
}

Tree insert(Tree t, int v) {
    match t {
        Leaf => {
            return Tree.Node(Tree.Leaf, v, Tree.Leaf);
        }
        Node(l, x, r) => {
            if v < x {
                return Tree.Node(insert(l, v), x, r);
            }
            return Tree.Node(l, x, insert(r, v));
        }
    }
}

int total(Tree t) {
    match t {
        Node(l, x, r) => {
            return total(l) + x + total(r);
        }
        _ => {
            return 0;
        }
    }
}

String kind(int n, String s, boolean b) {
    String out = \"\";
    match n {
        -1 => {
            out += \"neg\";
        }
        0 => {
            out += \"zero\";
        }
        _ => {
            out += \"other\";
        }
    }
    match s {
        \"a\" => {
            out += \"A\";
        }
        _ => {
            out += \"?\";
        }
    }
    match b {
        true => {
            out += \"T\";
        }
        false => {
            out += \"F\";
        }
    }
    return out;
}

void main() {
    Shape[] shapes = {Shape.Circle(1), Shape.Rect(2, 3.5), Shape.Empty};
    double sum = 0;
    for i in 0..len(shapes) {
        sum += area(shapes[i]);
    }
    Tree t = Tree.Leaf;
    int[] keys = {5, 2, 8, 1, 9, 3};
    for i in 0..len(keys) {
        t = insert(t, keys[i]);
    }
    Holder h = new Holder(Shape.Rect(1, 1), \"x\");
    Holder[] blanks = new Holder[1];
    Shape[] defaults = new Shape[2];
    var d = defaults[1];
    println(\"{} {} {} {}\", sum, total(t), area(h.shape), area(blanks[0].shape) + area(d));
    println(\"{} {} {}\", kind(-1, \"a\", true), kind(0, \"b\", false), kind(7, \"\", true));
}
";

#[test]
fn enums_and_match() {
    assert_eq!(run_ok(ENUMS, 1), "10.0 28 1.0 0.0\nnegAT zero?F other?T\n");
    // Types and variants may be used before their declaration; `match` arms may end with a comma.
    ok(
        "void main() { E e = E.B(1); match e { A => {}, B(n) => { println(\"{}\", n); } } }
enum E { A, B(int n) }",
    );
}

// Dropping a long recursive value is iterative in every engine (no native stack overflow).
#[test]
fn long_enum_lists_drop_without_recursion() {
    let src = "enum L {
    Nil,
    Cons(int head, L tail),
}

void main() {
    L list = L.Nil;
    for i in 0..1000000 {
        list = L.Cons(i, list);
    }
    int n = 0;
    boolean more = true;
    while more {
        match list {
            Cons(h, rest) => {
                n += 1;
                list = rest;
            }
            Nil => {
                more = false;
            }
        }
    }
    println(\"{}\", n);
}
";
    assert_eq!(run_ok(src, 1), "1000000\n");
    let keep = src.replace("        list = rest;\n", "").replace(
        "            Cons(h, rest) => {\n                n += 1;\n",
        "            Cons(h, rest) => {\n                n += 1;\n                more = false;\n",
    );
    assert_eq!(run_ok(&keep, 1), "1\n");
}

#[test]
fn enum_errors() {
    // Duplicate names (enums share the struct namespace), variants and payload fields.
    assert_eq!(codes("enum E { A } enum E { B } void main() {}"), ["Q0205"]);
    assert_eq!(
        codes("struct E { int x; } enum E { A } void main() {}"),
        ["Q0205"]
    );
    assert_eq!(codes("enum E { A, A } void main() {}"), ["Q0205"]);
    assert_eq!(
        codes("enum E { A(int x, int x) } void main() {}"),
        ["Q0205"]
    );
    // Payload types, and recursion through the first variant (the default value).
    assert_eq!(codes("enum E { A(Qubit q) } void main() {}"), ["Q0227"]);
    assert_eq!(
        codes("enum L { Cons(int h, L t), Nil } void main() {}"),
        ["Q0227"]
    );
    assert_eq!(
        codes("struct S { E e; } enum E { A(S[] s) } void main() {}"),
        ["Q0227"]
    );
    ok("enum L { Nil, Cons(int h, L t) } struct S { L l; } void main() {}");
    ok("enum T { Leaf, Node(T l, T r), Pair(P p) } struct P { int[] xs; } void main() {}");
    // A cycle through a struct or an array could make a value refer to itself.
    assert_eq!(
        errors("enum L { Nil, Cons(S s) } struct S { L l; } void main() {}"),
        [("Q0227", 1, 38)]
    );
    assert_eq!(
        errors("enum L { Nil, Many(L[] kids) } void main() {}"),
        [("Q0227", 1, 20)]
    );
    assert_eq!(codes("enum E {} void main() {}"), ["Q0105"]);
    let p = |body: &str| {
        codes(&format!(
            "enum E {{ A, B(int n, String s) }}
void main() {{
    E e = E.B(1, \"s\");
{body}
}}
"
        ))
    };
    // Construction.
    assert_eq!(p("    E f = E.B(1);"), ["Q0203"]);
    assert_eq!(p("    E f = E.B(1.5, \"s\");"), ["Q0204"]);
    assert_eq!(p("    E f = E.C;"), ["Q0229"]);
    assert_eq!(p("    boolean same = e == E.A;"), ["Q0204", "Q0204"]);
    assert_eq!(p("    println(\"{}\", e);"), ["Q0212"]);
    assert_eq!(p("    var f = e;\n    E g = f;"), Vec::<&str>::new());
    // Exhaustiveness.
    assert_eq!(p("    match e { A => {} }"), ["Q0228"]);
    assert_eq!(p("    match 1 { 0 => {} }"), ["Q0228"]);
    assert_eq!(p("    match \"a\" { \"a\" => {} }"), ["Q0228"]);
    assert_eq!(p("    match true { true => {} }"), ["Q0228"]);
    assert_eq!(
        p("    match true { true => {} false => {} }"),
        Vec::<&str>::new()
    );
    // Patterns.
    assert_eq!(p("    match e { A => {} C => {} _ => {} }"), ["Q0229"]);
    assert_eq!(p("    match e { A => {} B(n) => {} }"), ["Q0229"]);
    assert_eq!(
        p("    match e { A => {} A => {} B(n, s) => {} }"),
        ["Q0229"]
    );
    assert_eq!(p("    match e { _ => {} A => {} }"), ["Q0229"]);
    assert_eq!(p("    match e { 1 => {} _ => {} }"), ["Q0229"]);
    assert_eq!(p("    match 1 { \"a\" => {} _ => {} }"), ["Q0229"]);
    assert_eq!(p("    match e { F.A => {} _ => {} }"), ["Q0229"]);
    assert_eq!(
        p("    match e { E.A => {} E.B(_, _) => {} }"),
        Vec::<&str>::new()
    );
    assert_eq!(p("    match 1.5 { _ => {} }"), ["Q0229"]);
    assert_eq!(p("    match e { A => {} B(n, n) => {} }"), ["Q0205"]);
    // Bindings are final and scoped to their arm.
    assert_eq!(
        p("    match e { A => {} B(n, s) => { n = 2; } }"),
        ["Q0207"]
    );
    assert_eq!(
        p("    match e { A => {} B(n, s) => {} }\n    int m = n;"),
        ["Q0201"]
    );
}

// Arms are branches: the same qubit rules as `if`/`else`.
#[test]
fn match_arms_follow_the_qubit_rules() {
    let p = |body: &str| {
        codes(&format!(
            "enum E {{ A, B }}
int f(E e) {{
{body}
}}
void main() {{
    println(\"{{}}\", f(E.A));
}}
"
        ))
    };
    let both = "    Qubit q = qubit();
    match e {
        A => {
            discard(q);
        }
        B => {
            Bit b = measure(q);
        }
    }
    return 1;";
    assert_eq!(p(both), Vec::<&str>::new());
    let one = "    Qubit q = qubit();
    match e {
        A => {
            discard(q);
        }
        B => {}
    }
    return 1;";
    assert_eq!(p(one), ["Q0304"]);
    let leak = "    match e {
        A => {
            Qubit q = qubit();
        }
        B => {}
    }
    return 1;";
    assert_eq!(p(leak), ["Q0302"]);
    // An exhaustive match whose arms all return ends the function.
    assert_eq!(
        p("    match e {\n        A => {\n            return 1;\n        }\n        B => {\n            return 2;\n        }\n    }"),
        Vec::<&str>::new()
    );
    assert_eq!(
        p("    match e {\n        A => {\n            return 1;\n        }\n        _ => {\n            return 2;\n        }\n    }"),
        Vec::<&str>::new()
    );
    // A loop body may not consume an outer qubit in an arm.
    assert_eq!(
        p("    Qubit q = qubit();
    for i in 0..2 {
        match e {
            A => {
                discard(q);
            }
            B => {
                discard(q);
            }
        }
    }
    return 1;"),
        ["Q0302", "Q0305"]
    );
}

#[test]
fn struct_errors() {
    // Duplicate structs and fields, field types, recursion (directly, through arrays, mutually).
    assert_eq!(
        codes("struct P { int x; } struct P { int y; } void main() {}"),
        ["Q0205"]
    );
    assert_eq!(
        codes("struct P { int x; double x; } void main() {}"),
        ["Q0205"]
    );
    assert_eq!(codes("struct P { Qubit q; } void main() {}"), ["Q0227"]);
    assert_eq!(codes("struct N { N next; } void main() {}"), ["Q0227"]);
    assert_eq!(codes("struct N { N[] kids; } void main() {}"), ["Q0227"]);
    // One error per cycle, at the field that closes it.
    assert_eq!(
        errors("struct A { B b; } struct B { A[] a; } void main() {}"),
        [("Q0227", 1, 30)]
    );
    // Construction arity and types, unknown fields, field stores.
    let p = |body: &str| {
        codes(&format!(
            "struct P {{ int x; String s; }}
void main() {{
{body}
}}
"
        ))
    };
    assert_eq!(p("    P a = new P(1);"), ["Q0203"]);
    assert_eq!(p("    P a = new P(1.5, \"s\");"), ["Q0204"]);
    assert_eq!(
        p("    P a = new P(1, \"s\");
    int y = a.y;"),
        ["Q0226"]
    );
    assert_eq!(
        p("    int n = 1;
    int y = n.x;"),
        ["Q0226"]
    );
    assert_eq!(
        p("    P a = new P(1, \"s\");
    a.x = \"t\";"),
        ["Q0204"]
    );
    assert_eq!(
        p("    P a = new P(1, \"s\");
    a.s += 1;
    a.x += 2;"),
        Vec::<&str>::new()
    );
    assert_eq!(
        p("    P a = new P(1, \"s\");
    println(\"{}\", a);"),
        ["Q0212"]
    );
    assert_eq!(
        p("    Qubit q = qubit();
    int y = q.x;
    discard(q);"),
        ["Q0226"]
    );
    // Field stores may not run inside an `if` on a measured bit, directly, through an array
    // element, or via a call.
    let bit = |body: &str| {
        codes(&format!(
            "struct P {{ int x; }}
void set(P p) {{ p.x = 1; }}
void main() {{
    P a = new P(0);
    P[] ps = {{a}};
    Qubit q = qubit();
    Bit b = measure(q);
    if b {{
{body}
    }}
}}
"
        ))
    };
    assert_eq!(bit("        a.x = 2;"), ["Q0308"]);
    assert_eq!(bit("        ps[0].x = 2;"), ["Q0308"]);
    assert_eq!(bit("        set(a);"), ["Q0314"]);
    assert_eq!(bit("        int k = a.x + ps[0].x;"), Vec::<&str>::new());
    // Construction is charged before the struct is built.
    let rt = |src: &str| runtime_error(src).0;
    assert_eq!(
        rt("struct P { int x; double y; String s; }
void main() {
    P[] ps = new P[100000000];
}
"),
        "Q0413"
    );
}

#[test]
fn generic_types_and_functions() {
    assert_eq!(
        run_ok(GENERICS, 1),
        "7 ada\none 1\n2.5\n2\n-1 5 9\nok 42\nerror: empty input\n"
    );
    // `<` after a value is still a comparison, and `>>` closes two argument lists.
    let src = "struct Box<T> {\n    T v;\n}\n\nvoid main() {\n    int a = 1;\n    int b = 2;\n    boolean less = a < b;\n    Box<Box<int>> bb = new Box(new Box(a));\n    Box<int>[] boxes = new Box<int>[2];\n    println(\"{} {} {} {}\", less, bb.v.v, boxes[1].v, a > b);\n}\n";
    assert_eq!(run_ok(src, 1), "true 1 0 false\n");
    // Inference: from the expected type (declarations, assignments, returns, arguments, stores
    // and fields) or from the values; `int` widens to `double`.
    let src = "enum Option<T> {\n    None,\n    Some(T v),\n}\n\nstruct Holder {\n    Option<String> o;\n}\n\nOption<int> none() {\n    return Option.None;\n}\n\nint get(Option<int> o) {\n    match o {\n        Some(v) => {\n            return v;\n        }\n        None => {\n            return -1;\n        }\n    }\n}\n\n<T> T pick(boolean c, T a, T b) {\n    if c {\n        return a;\n    }\n    return b;\n}\n\nvoid main() {\n    Option<int> o = Option.None;\n    o = Option.Some(3);\n    Holder h = new Holder(Option.None);\n    h.o = Option.Some(\"x\");\n    Option<int>[] os = {Option.None, Option.Some(4)};\n    os[0] = Option.Some(5);\n    Option<double> d = Option.Some(1);\n    println(\"{} {} {} {} {} {}\", get(o), get(none()), get(Option.None), get(os[0]), pick(true, 1, 2.5), pick(false, \"a\", \"b\"));\n    match d {\n        Some(x) => {\n            println(\"{}\", x / 2);\n        }\n        None => {}\n    }\n}\n";
    assert_eq!(run_ok(src, 1), "3 -1 -1 5 1.0 b\n0.5\n");
    // Generic bodies are checked once, even when never called, and instances may recurse.
    let src = "enum Tree<T> {\n    Leaf,\n    Node(Tree<T> left, T value, Tree<T> right),\n}\n\n<T> int size(Tree<T> t) {\n    match t {\n        Node(l, _, r) => {\n            return size(l) + 1 + size(r);\n        }\n        Leaf => {\n            return 0;\n        }\n    }\n}\n\n<T> T unused(T x) {\n    return x;\n}\n\nvoid main() {\n    Tree<String> t = Tree.Node(Tree.Leaf, \"a\", Tree.Node(Tree.Leaf, \"b\", Tree.Leaf));\n    println(\"{}\", size(t));\n}\n";
    assert_eq!(run_ok(src, 1), "2\n");
}

#[test]
fn generic_errors() {
    let decls = "struct Pair<A, B> { A a; B b; } enum Option<T> { None, Some(T v) } ";
    let with = |main: &str| format!("{decls}{main}");
    // Type arguments: required, counted, classical.
    assert_eq!(
        codes(&with("void main() { Pair p = new Pair(1, 2); }")),
        ["Q0230"]
    );
    assert_eq!(
        codes(&with(
            "void main() { Pair<int> p = new Pair<int, int>(1, 2); }"
        )),
        ["Q0230"]
    );
    assert_eq!(
        codes(&with("void main() { Option<Qubit> o = Option.None; }")),
        ["Q0230"]
    );
    assert_eq!(
        codes(&with("void f(Option<void> o) {} void main() {}")),
        ["Q0230"]
    );
    // Inference needs the values or an expected type.
    assert_eq!(
        codes(&with("void main() { var o = Option.None; }")),
        ["Q0230"]
    );
    // A type parameter must be inferable from the parameters.
    assert_eq!(
        errors("<T> T make(int n) { return make(n); } void main() {}"),
        [("Q0230", 1, 2)]
    );
    // A value of a type parameter can only be moved.
    for body in [
        "return a + b;",
        "return a == b;",
        "println(\"{}\", a); return true;",
        "return a.length() == 0;",
        "return (int) a == 0;",
    ] {
        let src = format!("<T> boolean f(T a, T b) {{ {body} }} void main() {{}}");
        assert_eq!(codes(&src), ["Q0230"], "{src}");
    }
    // Quantum values are not type arguments.
    assert_eq!(
        codes("<T> T id(T x) { return x; } void main() { Qubit q = qubit(); id(q); discard(q); }"),
        ["Q0230"]
    );
    // Arguments are checked against the inferred types.
    assert_eq!(
        codes("<T> T first(T[] a) { return a[0]; } void main() { int x = first(5); }"),
        ["Q0204"]
    );
    assert_eq!(
        codes(&with("void main() { Option<int> o = Option.Some(\"x\"); }")),
        ["Q0204"]
    );
    // Type parameter names are distinct and are not type names.
    assert_eq!(codes("<T, T> void f(T a) {} void main() {}"), ["Q0205"]);
    assert_eq!(
        codes("struct T { int x; } <T> void f(T a) {} void main() {}"),
        ["Q0205"]
    );
    // A struct may not contain itself through an instance either.
    assert_eq!(
        codes(&with("struct N { Option<N> next; } void main() {}")),
        ["Q0227"]
    );
}

// The prelude's `Result` and `Option` need no declaration; `?` returns the `Err` or `None` early.
#[test]
fn result_option_and_try() {
    let out = run_ok(ERRORS, 1);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    assert_eq!(lines[0], "sum = 6");
    assert!(
        lines[1].starts_with("error: ") && lines[1].contains("x5"),
        "{out}"
    );
    assert_eq!(&lines[2..], ["half = 4", "no even number", "5.0"]);
    // `?` in generic code, in a loop condition, on a call result and as a statement.
    let src = "<T> Result<T, String> at(T[] a, int i) {
    if i < 0 || i >= len(a) {
        return Result.Err(\"out of range\");
    }
    return Result.Ok(a[i]);
}

<T> Result<T, String> second_of(T[] a, int i) {
    at(a, i)?;
    return at(a, i + 1);
}

Result<int, String> count(String[] items) {
    int i = 0;
    while items[i].tryParseInt()? > 0 {
        i += 1;
    }
    return Result.Ok(i);
}

int get(Result<int, String> r) {
    match r {
        Ok(v) => {
            return v;
        }
        Err(_) => {
            return -1;
        }
    }
}

void main() {
    int[] xs = {10, 20, 30};
    String[] ss = {\"3\", \"2\", \"0\", \"x\"};
    String[] bad = {\"3\", \"x\"};
    println(\"{} {} {} {} {}\", get(second_of(xs, 1)), get(second_of(xs, 2)), get(second_of(xs, 5)), get(count(ss)), get(count(bad)));
}
";
    assert_eq!(run_ok(src, 1), "30 -1 -1 2 -1\n");
    // A program's own `Option` replaces the prelude's, and a non-generic one is fine too.
    let src = "enum Option {\n    A,\n    B,\n}\n\nvoid main() {\n    Option o = Option.B;\n    match o {\n        A => {\n            println(\"a\");\n        }\n        B => {\n            println(\"b\");\n        }\n    }\n}\n";
    assert_eq!(run_ok(src, 1), "b\n");
    // A borrowed qubit parameter may stay live at a `?`; the caller owns it.
    ok("Result<int, String> f(Qubit q) {\n    h(q);\n    int x = \"1\".tryParseInt()?;\n    return Result.Ok(x);\n}\n\nvoid main() {}\n");
}

#[test]
fn try_errors() {
    // The operand must be a `Result` or an `Option`.
    assert_eq!(
        errors("Result<int, String> f() {\n    int x = 3;\n    return Result.Ok(x?);\n}\nvoid main() {}"),
        [("Q0231", 3, 23)]
    );
    // The function must return the same kind with the same error type; `main` returns nothing.
    assert_eq!(
        errors("void main() {\n    int x = \"1\".tryParseInt()?;\n}"),
        [("Q0231", 2, 30)]
    );
    assert_eq!(
        codes("Result<int, int> g() { return Result.Ok(1); } Result<int, String> f() { int x = g()?; return Result.Ok(x); } void main() {}"),
        ["Q0231"]
    );
    assert_eq!(
        codes("Option<int> f() { int x = \"1\".tryParseInt()?; return Option.Some(x); } void main() {}"),
        ["Q0231"]
    );
    assert_eq!(
        codes("Result<int, String> f() { int x = Option.Some(1)?; return Result.Ok(x); } void main() {}"),
        ["Q0231"]
    );
    // Like `return`: an owned qubit may not be live at a `?`, also inside a loop.
    assert_eq!(
        errors("Result<int, String> f() {\n    Qubit q = qubit();\n    int x = \"1\".tryParseInt()?;\n    discard(q);\n    return Result.Ok(x);\n}\nvoid main() {}"),
        [("Q0302", 2, 11)]
    );
    assert_eq!(
        codes("Result<int, String> f(String[] a) { int t = 0; for i in 0..len(a) { Qubit q = qubit(); t += a[i].tryParseInt()?; discard(q); } return Result.Ok(t); } void main() {}"),
        ["Q0302"]
    );
    // Like `return`: not inside an `if` on a measured bit.
    assert_eq!(
        codes("Result<int, String> f() { Qubit q = qubit(); h(q); Bit b = measure(q); if b { int x = \"1\".tryParseInt()?; } return Result.Ok(1); } void main() {}"),
        ["Q0308"]
    );
    // `tryParseInt` needs the prelude's shape of `Result`.
    assert_eq!(
        codes("enum Result { Ok(int v), Err(String e), Other } void main() { var r = \"1\".tryParseInt(); }"),
        ["Q0231"]
    );
}

// Polymorphic recursion would instantiate forever: a size limit and a count limit stop it at the
// use that asked for the next instance.
#[test]
fn instantiation_is_bounded() {
    let deeper = "struct Box<T> {\n    T v;\n}\n\n<T> void f(T x) {\n    f(new Box(x));\n}\n\nvoid main() {\n    f(1);\n}\n";
    assert_eq!(errors(deeper), [("Q0230", 6, 7)]);
    let wider = "struct Box<T> {\n    T v;\n}\n\n<A, B> void f(int n, A a, B b) {\n    if n > 0 {\n        f(n - 1, new Box(a), b);\n        f(n - 1, a, new Box(b));\n    }\n}\n\nvoid main() {\n    f(3, 1, 2);\n}\n";
    let e = errors(wider);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0].0, "Q0230");
    // Within the limits, every instance is created once.
    let src = "<T> T id(T x) {\n    return id2(x);\n}\n\n<T> T id2(T x) {\n    return x;\n}\n\nvoid main() {\n    println(\"{} {}\", id(1), id(id(\"s\")));\n}\n";
    assert_eq!(run_ok(src, 1), "1 s\n");
}

// Each kind at the limit, inside the deepest allowed recursion, must run on half of `STACK_SIZE`
// (measured worst case: ~100 MiB unoptimized, ~45 MiB optimized). The parser and checker also
// run on that thread, as in the CLI: unoptimized they need more than a test thread's 2 MiB.
#[test]
fn deepest_legal_programs_fit_in_half_the_interpreter_stack() {
    let worker = std::thread::Builder::new().stack_size(STACK_SIZE / 2);
    let failures = worker
        .spawn(|| {
            let mut failures = Vec::new();
            for (kind, n, program) in deepest_programs() {
                let over = codes(&program(n + 1));
                if over != ["Q0106"] {
                    failures.push(format!("{kind} at depth {}: {over:?}", n + 1));
                }
                let src = program(n);
                if let Err(Failure(d)) = run("t.qx", &src, 1, 0) {
                    failures.push(format!("{kind} at depth {n}: {d}"));
                }
            }
            failures
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}

// Methods, interfaces as bounds and as values, and the prelude's methods (examples/qx/interfaces.qx).
#[test]
fn interfaces_example() {
    assert_eq!(
        run_ok(INTERFACES, 1),
        "rect 4.0 x 6.0 = 24.0\ncircles: 16.0\ncircle 3.0\nrect 24.0\ngreen 0.0\nall: 27.0\nmax: 990\ngood: 6\nbad: -1 ok=false\n"
    );
}

#[test]
fn methods_of_structs_enums_and_generic_types() {
    let src = "struct Counter {
    int n;

    void add(int k) {
        this.n += k;
    }

    int get() {
        return this.n;
    }

    int twice() {
        return this.get() * 2;
    }
}

struct Box<T> {
    T v;

    T get() {
        return this.v;
    }

    void set(T v) {
        this.v = v;
    }
}

enum Coin {
    Heads,
    Tails;

    Coin flip() {
        match this {
            Heads => {
                return Coin.Tails;
            }
            Tails => {
                return Coin.Heads;
            }
        }
    }

    boolean heads() {
        match this {
            Heads => {
                return true;
            }
            _ => {
                return false;
            }
        }
    }
}

<T> T unbox(Box<T> b) {
    return b.get();
}

void main() {
    Counter c = new Counter(1);
    c.add(4);
    Box<String> b = new Box(\"a\");
    b.set(b.get() + \"b\");
    Box<Box<int>> bb = new Box(new Box(7));
    Option<int> none = Option.None;
    println(\"{} {} {} {}\", c.get(), c.twice(), b.get(), unbox(bb).get());
    println(\"{} {}\", Coin.Heads.flip().heads(), Coin.Tails.flip().heads());
    println(\"{} {} {}\", none.isEmpty(), none.orElse(3), Option.Some(5).orElse(0));
    println(\"{} {}\", \"x\".tryParseInt().isErr(), \"12\".tryParseInt().orElse(0));
}
";
    assert_eq!(run_ok(src, 1), "5 10 ab 7\nfalse true\ntrue 3 5\ntrue 12\n");
    // The words are contextual: still ordinary names outside declarations.
    ok("void main() {\n    int interface = 1;\n    int implements = 2;\n    int extends = 3;\n    int this = interface + implements + extends;\n}\n");
}

#[test]
fn interface_errors() {
    let shape = "interface Shape {\n    double area();\n}\n\n";
    // A missing method, a signature that differs, and a type that is not an interface.
    assert_eq!(
        errors(&format!(
            "{shape}struct A implements Shape {{\n    int x;\n}}\n\nvoid main() {{}}\n"
        )),
        [("Q0232", 5, 21)]
    );
    assert_eq!(
        errors(&format!("{shape}struct A implements Shape {{\n    int area() {{\n        return 1;\n    }}\n}}\n\nvoid main() {{}}\n")),
        [("Q0232", 6, 9)]
    );
    assert_eq!(
        errors("struct B {\n    int x;\n}\n\nstruct A implements B {\n    int y;\n}\n\nvoid main() {}\n"),
        [("Q0232", 5, 21)]
    );
    // Bounds: an interface, satisfied by the argument type.
    let bound = format!("{shape}struct Sq implements Shape {{\n    double s;\n\n    double area() {{\n        return this.s * this.s;\n    }}\n}}\n\n<T extends Shape> double sum(T[] xs) {{\n    return xs[0].area();\n}}\n\n");
    ok(&format!(
        "{bound}void main() {{\n    Sq[] a = {{new Sq(2)}};\n    double d = sum(a);\n}}\n"
    ));
    assert_eq!(
        codes(&format!(
            "{bound}void main() {{\n    int[] a = {{1}};\n    double d = sum(a);\n}}\n"
        )),
        ["Q0232"]
    );
    assert_eq!(
        codes("<T extends int> void f(T x) {}\n\nvoid main() {}\n"),
        ["Q0232"]
    );
    // Without a bound a type parameter has no methods.
    assert_eq!(
        codes(&format!(
            "{shape}<T> double f(T x) {{\n    return x.area();\n}}\n\nvoid main() {{}}\n"
        )),
        ["Q0230"]
    );
    // Unknown methods, wrong arity, a field and a method of one name, reassigning `this`.
    assert_eq!(
        codes("struct P {\n    int x;\n}\n\nvoid main() {\n    P p = new P(1);\n    p.y();\n}\n"),
        ["Q0220"]
    );
    assert_eq!(
        codes(&format!(
            "{shape}void f(Shape s) {{\n    s.area(1);\n    s.size();\n}}\n\nvoid main() {{}}\n"
        )),
        ["Q0203", "Q0220"]
    );
    assert_eq!(
        codes("struct P {\n    int x;\n\n    int x() {\n        return 1;\n    }\n}\n\nvoid main() {}\n"),
        ["Q0205"]
    );
    assert_eq!(
        codes("struct P {\n    int x;\n\n    void reset() {\n        this = new P(0);\n    }\n}\n\nvoid main() {}\n"),
        ["Q0207"]
    );
    // Interface values: no default (`new I[n]`, also through a struct field or generic code),
    // no printing, no `match`.
    assert_eq!(
        codes(&format!(
            "{shape}void main() {{\n    Shape[] a = new Shape[2];\n}}\n"
        )),
        ["Q0225"]
    );
    assert_eq!(
        codes(&format!("{shape}struct H {{\n    Shape s;\n}}\n\nvoid main() {{\n    H[] a = new H[2];\n    H[][] b = new H[2][];\n}}\n")),
        ["Q0225"]
    );
    assert_eq!(
        codes(&format!("{shape}struct Sq implements Shape {{\n    double s;\n\n    double area() {{\n        return this.s;\n    }}\n}}\n\n<T> T[] fill(T x) {{\n    T[] a = new T[1];\n    a[0] = x;\n    return a;\n}}\n\nvoid main() {{\n    int[] ok = fill(1);\n    Shape s = new Sq(1);\n    Shape[] bad = fill(s);\n}}\n")),
        ["Q0225"]
    );
    assert_eq!(
        codes(&format!("{shape}void f(Shape s) {{\n    println(\"{{}}\", s);\n    match s {{\n        _ => {{}}\n    }}\n}}\n\nvoid main() {{}}\n")),
        ["Q0212", "Q0229"]
    );
}

// Methods may take and consume qubits like functions; method calls inside an `if` on a measured
// bit follow the function rules, an interface call those of every method of its name.
#[test]
fn methods_follow_the_qubit_rules() {
    let lab = "struct Lab {\n    int runs;\n\n    void flip(Qubit q) {\n        x(q);\n    }\n\n    Bit read(consume Qubit q) {\n        this.runs += 1;\n        return measure(q);\n    }\n}\n\n";
    ok(&format!("{lab}void main() {{\n    Lab l = new Lab(0);\n    Qubit q = qubit();\n    l.flip(q);\n    Bit b = l.read(q);\n}}\n"));
    assert_eq!(
        codes(&format!("{lab}void main() {{\n    Lab l = new Lab(0);\n    Qubit q = qubit();\n    Bit b = l.read(q);\n    l.flip(q);\n}}\n")),
        ["Q0301"]
    );
    assert_eq!(
        codes(&format!("{lab}void main() {{\n    Lab l = new Lab(0);\n    Qubit q = qubit();\n    l.flip(q);\n}}\n")),
        ["Q0302"]
    );
    // `flip` only applies a gate: allowed under a measured bit; `read` measures: not.
    let cond = |call: &str| {
        format!("{lab}void main() {{\n    Lab l = new Lab(0);\n    Qubit q = qubit();\n    Qubit r = qubit();\n    if measure(r) {{\n        {call}\n    }}\n    discard(q);\n}}\n")
    };
    ok(&cond("l.flip(q);"));
    assert!(codes(&cond("Bit b = l.read(q);")).contains(&"Q0314"));
    let gates = "interface Gate {\n    void act(Qubit q);\n}\n\nstruct X implements Gate {\n    int n;\n\n    void act(Qubit q) {\n        x(q);\n    }\n}\n\nstruct H implements Gate {\n    int n;\n\n    void act(Qubit q) {\n        h(q);\n    }\n}\n\n";
    let use_gate = |extra: &str| {
        format!("{gates}{extra}void main() {{\n    Gate g = new H(0);\n    Qubit q = qubit();\n    Qubit r = qubit();\n    if measure(r) {{\n        g.act(q);\n    }}\n    discard(q);\n}}\n")
    };
    ok(&use_gate(""));
    // Some method named `act` allocates a qubit: the interface call may run it.
    assert_eq!(
        codes(&use_gate("struct M {\n    int n;\n\n    void act(Qubit q) {\n        Qubit t = qubit();\n        discard(t);\n    }\n}\n\n")),
        ["Q0314"]
    );
}

// Values linked through interface-typed payloads of an (immutable) enum may form long chains;
// dropping one does not recurse. A struct (mutable) that can reach itself through an interface
// is rejected like any recursive struct (`Q0227`): it could refer to itself and never be freed.
#[test]
fn recursion_through_interfaces() {
    let link = "interface Link {\n    int depth();\n}\n\n";
    let chain = format!("{link}enum Chain implements Link {{\n    End,\n    Node(Link next);\n\n    int depth() {{\n        match this {{\n            End => {{\n                return 0;\n            }}\n            Node(_) => {{\n                return 1;\n            }}\n        }}\n    }}\n}}\n\nstruct Holder {{\n    Link first;\n}}\n\nvoid main() {{\n    Link cur = Chain.End;\n    for i in 0..1000000 {{\n        cur = Chain.Node(cur);\n    }}\n    Holder h = new Holder(cur);\n    println(\"{{}}\", h.first.depth());\n}}\n");
    assert_eq!(run_ok(&chain, 1), "1\n");
    let node = format!("{link}struct Node implements Link {{\n    Link next;\n\n    int depth() {{\n        return 1;\n    }}\n}}\n\nvoid main() {{}}\n");
    assert_eq!(errors(&node), [("Q0227", 6, 5)]);
    let bag = format!("{link}enum Bag implements Link {{\n    Empty,\n    Items(Link[] items);\n\n    int depth() {{\n        return 0;\n    }}\n}}\n\nvoid main() {{}}\n");
    assert_eq!(codes(&bag), ["Q0227"]);
    // Only an instance can: `Wrap<int>` is fine, `Wrap<Link>` could hold itself.
    let wrap = |arg: &str| {
        format!("{link}struct Wrap<T> implements Link {{\n    T v;\n\n    int depth() {{\n        return 0;\n    }}\n}}\n\nvoid main() {{\n    Wrap<{arg}> w = new Wrap(Chain.End);\n}}\n\nenum Chain implements Link {{\n    End;\n\n    int depth() {{\n        return 0;\n    }}\n}}\n")
    };
    assert_eq!(codes(&wrap("Link")), ["Q0227"]);
    assert_eq!(codes(&wrap("Chain")), Vec::<&str>::new());
}

// Interface calls reach the methods of generic implementors' instances and of generic
// interfaces (the monomorphized program is checked again in debug builds).
#[test]
fn interface_calls_on_generic_types() {
    let src = "interface Shape {\n    double area();\n}\n\ninterface Holder<T> {\n    T get();\n}\n\nstruct Wrap<T> implements Shape, Holder<T> {\n    T v;\n    double k;\n\n    double area() {\n        return this.k;\n    }\n\n    T get() {\n        return this.v;\n    }\n}\n\nvoid main() {\n    Shape a = new Wrap(5, 2.5);\n    Shape b = new Wrap(\"s\", 1);\n    Holder<String> h = new Wrap(\"held\", 0);\n    Option<Shape> maybe = Option.Some(a);\n    println(\"{} {} {} {}\", a.area(), b.area(), h.get(), maybe.orElse(b).area());\n}\n";
    assert_eq!(run_ok(src, 1), "2.5 1.0 held 2.5\n");
}

#[test]
fn collections_example() {
    assert_eq!(
        run_ok(COLLECTIONS, 1),
        "the x3\n9 distinct words\njumps, brown, quick\nmost frequent face: 112 rolls\nflags 41 has 3 bits set, bit 5 is true\nlast long word: jumps\n"
    );
}

#[test]
fn json_example() {
    assert_eq!(
        run_ok(JSON, 1),
        "{\"round\":2.5,\"totals\":{\"Ada\":16,\"Bo\":7}}\n{\n  \"Ada\": 16,\n  \"Bo\": 7\n}\ninvalid JSON at line 1, column 6: unexpected end of input\nresults/2026/scores.final.json results/2026 scores.final json\nresults/2027/scores.json\n"
    );
}

// Bit operators on `int`: Rust's precedence (tighter than comparisons), arithmetic `>>`,
// logical `>>>`, and shift amounts outside 0..=63 are a run-time error.
#[test]
fn bitwise_operators() {
    let src = main_body("    int x = 12;\n    println(\"{} {} {} {} {} {}\", x & 10, x | 3, x ^ 5, ~x, x << 2, -16 >> 2);\n    println(\"{} {} {}\", -1 >>> 60, x & 1 == 0, 1 | 2 ^ 3 & 4 << 1);\n    int y = 1;\n    y <<= 10;\n    y |= 5;\n    y >>= 1;\n    y >>>= 1;\n    y ^= 3;\n    y &= 255;\n    int[] a = {6};\n    a[0] &= 3;\n    println(\"{} {}\", y, a[0]);");
    assert_eq!(run_ok(&src, 1), "8 15 9 -13 48 -4\n15 true 3\n2 2\n");
    // `>>` still closes nested type arguments.
    ok("void main() {\n    Option<Option<int>> o = Option.Some(Option.Some(1));\n    int k = 8 >> 1 >>> 1;\n}\n");
    assert_eq!(
        runtime_error(&main_body("    int k = 64;\n    int v = 1 << k;")),
        ("Q0420", 3, 13)
    );
    assert_eq!(
        runtime_error(&main_body("    int k = -1;\n    int v = 8 >> k;")),
        ("Q0420", 3, 13)
    );
    assert_eq!(
        codes(&main_body(
            "    double d = 1.5 & 1;\n    boolean b = true | false;\n    int n = ~2.0;"
        )),
        ["Q0204", "Q0204", "Q0204", "Q0204"]
    );
    // Wrapping arithmetic for hashes and generators; plain `*` still stops on overflow.
    let src = "import std.math;\n\nvoid main() {\n    println(\"{} {}\", math.wrappingMul(9223372036854775807, 3), math.wrappingAdd(9223372036854775807, 1));\n}\n";
    assert_eq!(run_ok(src, 1), "9223372036854775805 -9223372036854775808\n");
}

#[test]
fn string_split_join_lines_and_chars() {
    let src = main_body("    String[] p = \"a,,b\".split(\",\");\n    println(\"{} {}\", len(p), \"|\".join(p));\n    String t = \" one  two\\tthree \\nfour\\nfive\\n\";\n    println(\"{} {}\", len(t.splitWhitespace()), \"/\".join(t.lines()));\n    println(\"{} {}\", \"-\".join(\"héllo\".chars()), len(\"\".split(\",\")));\n    println(\"[{}]\", \", \".join(new String[0]));");
    assert_eq!(
        run_ok(&src, 1),
        "3 a||b\n5  one  two\tthree /four/five\nh-é-l-l-o 1\n[]\n"
    );
    assert_eq!(
        runtime_error(&main_body("    String[] p = \"abc\".split(\"\");")),
        ("Q0706", 2, 18)
    );
}

// `for x in xs` over arrays and Lists: the collection and its length are read once, and the loop
// follows the rules of `for` (break, continue, qubits).
#[test]
fn for_each_loops() {
    let src = main_body("    int[][] grid = {{1, 2}, {3, 4}};\n    int total = 0;\n    for row in grid {\n        for v in row {\n            if v == 2 {\n                continue;\n            }\n            if v == 4 {\n                break;\n            }\n            total += v;\n        }\n    }\n    List<String> names = new List();\n    names.add(\"a\");\n    names.add(\"b\");\n    String joined = \"\";\n    for n in names {\n        joined += n;\n    }\n    println(\"{} {}\", total, joined);");
    assert_eq!(run_ok(&src, 1), "4 ab\n");
    assert_eq!(
        codes(&main_body("    Qubit q = qubit();\n    int[] a = {1, 2};\n    for v in a {\n        discard(q);\n    }")),
        ["Q0302", "Q0305"]
    );
    assert_eq!(
        codes(&main_body(
            "    int[] a = {1, 2};\n    for v in a {\n        Qubit q = qubit();\n    }"
        )),
        ["Q0302"]
    );
    assert_eq!(codes(&main_body("    for v in 5 {\n    }")), ["Q0204"]);
}

#[test]
fn lists_maps_and_random() {
    let src = "struct Body {\n    double mass;\n    List<double> trail;\n}\n\nvoid main() {\n    List<int> xs = new List<int>();\n    for i in 0..5 {\n        xs.add(i * i);\n    }\n    xs.set(0, 100);\n    xs.insert(1, 7);\n    int removed = xs.removeAt(2);\n    println(\"{} {} {} {}\", xs.size(), removed, xs[0], xs.pop().orElse(-1));\n    xs.clear();\n    println(\"{} {}\", xs.isEmpty(), xs.pop().isEmpty());\n    Map<int, double> m = new Map();\n    m.put(3, 1);\n    m.put(1, 2.5);\n    m.put(3, 4);\n    println(\"{} {} {}\", m.get(3).orElse(0), m.remove(1).isPresent(), m.containsKey(1));\n    var keys = m.keys();\n    println(\"{} {}\", len(keys), len(m.values()));\n    Body b = new Body(1.5, new List());\n    b.trail.add(2);\n    Body[] bodies = new Body[2];\n    bodies[1].trail.add(3.5);\n    println(\"{} {} {}\", b.trail.get(0), bodies[1].trail.size(), bodies[0].trail.isEmpty());\n    List<List<String>> nested = new List();\n    nested.add(new List());\n    nested.get(0).add(\"deep\");\n    println(\"{}\", nested.get(0).get(0));\n}\n";
    assert_eq!(
        run_ok(src, 1),
        "5 1 100 16\ntrue true\n4.0 true false\n1 1\n2.0 1 true\ndeep\n"
    );
    // Insertion order survives removals and re-insertions.
    let order = main_body("    Map<String, int> m = new Map();\n    for i in 0..40 {\n        m.put(\"k\" + i, i);\n    }\n    for i in 0..35 {\n        m.remove(\"k\" + i);\n    }\n    m.put(\"k0\", 0);\n    println(\"{}\", \",\".join(m.keys()));");
    assert_eq!(run_ok(&order, 1), "k35,k36,k37,k38,k39,k0\n");
    assert_eq!(
        runtime_error(&main_body(
            "    List<int> xs = new List();\n    int v = xs.get(0);"
        )),
        ("Q0403", 3, 13)
    );
}

// The same seed gives the same numbers on every engine and host (splitmix64).
#[test]
fn random_is_seeded_and_uniform_in_range() {
    let src = main_body("    Random a = new Random(7);\n    Random b = new Random(7);\n    boolean same = true;\n    int low = 0;\n    for i in 0..2000 {\n        int x = a.nextInt(10);\n        same = same && x == b.nextInt(10);\n        if x < 5 {\n            low += 1;\n        }\n    }\n    double d = a.nextDouble();\n    println(\"{} {} {} {}\", same, low > 900 && low < 1100, d >= 0.0 && d < 1.0, new Random(1).nextInt(1000000));");
    let out = run_ok(&src, 1);
    assert!(out.starts_with("true true true "), "{out}");
    assert_eq!(out, run_ok(&src, 1));
    assert_eq!(
        runtime_error(&main_body(
            "    Random r = new Random(1);\n    int v = r.nextInt(0);"
        )),
        ("Q0706", 3, 13)
    );
}

#[test]
fn collection_errors() {
    // Keys must compare by value; type arguments come from `new` or the declared type.
    assert_eq!(
        codes(&main_body(
            "    Map<double, int> m = new Map<double, int>();"
        )),
        ["Q0230", "Q0230"]
    );
    assert_eq!(codes(&main_body("    var xs = new List();")), ["Q0230"]);
    assert_eq!(
        codes(&main_body("    List xs = new List<int>();")),
        ["Q0230"]
    );
    assert_eq!(
        codes(&main_body("    List<Qubit> xs = new List();")),
        ["Q0230"]
    );
    assert_eq!(codes(&main_body("    Random r = new Random();")), ["Q0203"]);
    assert_eq!(
        codes(&main_body(
            "    List<int> xs = new List();\n    xs.push(1);"
        )),
        ["Q0220"]
    );
    assert_eq!(
        codes(&main_body(
            "    List<int> xs = new List();\n    xs.add(\"a\");"
        )),
        ["Q0204"]
    );
    assert_eq!(
        codes(&main_body("    Random[] rs = new Random[2];")),
        ["Q0225"]
    );
    // Changing a shared collection under a measured bit is like storing into an array.
    let bit = |stmt: &str| {
        format!("void touch(List<int> xs) {{\n    xs.add(1);\n}}\n\nvoid main() {{\n    List<int> xs = new List();\n    Qubit q = qubit();\n    if measure(q) {{\n        {stmt}\n    }}\n}}\n")
    };
    assert_eq!(codes(&bit("xs.add(2);")), ["Q0308"]);
    assert_eq!(codes(&bit("touch(xs);")), ["Q0314"]);
    ok(&bit("int n = xs.size();"));
    // A program's own `List` replaces the built-in one.
    assert_eq!(
        run_ok("enum List<T> {\n    Nil,\n    Cons(T head, List<T> tail)\n}\n\nvoid main() {\n    List<int> l = List.Cons(1, List.Nil);\n    println(\"ok\");\n}\n", 1),
        "ok\n"
    );
}

// Java/C loop syntax gets `Q0113` with the Qxide form in the help (roadmap A1).
#[test]
fn java_and_c_loop_syntax_points_to_the_qxide_form() {
    for body in [
        "for (int i = 0; i < 3; i++) {\n}",
        "int[] xs = {1};\nfor (int x : xs) {\n}",
        "int i = 0;\ni++;",
        "int i = 0;\ni--;",
        "int i = 0;\n++i;",
        "int i = 0;\nint j = i++;",
        "int[] a = {1};\na[0]++;",
        "int i = 0;\nwhile (i < 3) i += 1;",
        "int i = 0;\nif (i < 3) i += 1;",
        "for i in 0..3 println(\"{}\", i);",
        "do {\n} while (true);",
    ] {
        assert_eq!(codes(&main_body(body)), ["Q0113"], "{body}");
    }
    let help = |body: &str| match check("t.qx", &main_body(body)) {
        Err(Failure(d)) => d,
        Ok(_) => panic!("expected an error in {body}"),
    };
    assert!(help("for (int k = 0; k < 3; k++) {\n}").contains("`for k in 0..n {"));
    assert!(help("int[] xs = {1};\nfor (int v : xs) {\n}").contains("`for v in xs {"));
    assert!(help("int n = 0;\nn++;").contains("`n += 1;`"));
    // Parenthesized conditions are ordinary expressions, so `while (c) { }` stays valid (D1), and
    // `-` signs written apart or followed by an operand are still subtraction and negation.
    let src = main_body(
        "int i = 0;\nwhile (i < 3) {\n    i += 1;\n}\nif (i == 3) {\n    println(\"{}\", i);\n}\nint a = i--1;\nint b = - -a;\nint c = a - -b;\nprintln(\"{} {} {}\", a, b, c);",
    );
    assert_eq!(run_ok(&src, 1), "3\n4 4 8\n");
}

// `--i` is double negation, as in 0.8.0: it runs unchanged but warns (`Q0802`); `- -i` and
// `-(-i)` do not warn.
#[test]
fn joined_double_minus_warns() {
    let src = main_body("int i = 3;\n--i;\nint j = --i + - -i + -(-i);\nprintln(\"{} {}\", i, j);");
    assert_eq!(run_ok(&src, 1), "3 9\n");
    match check("t.qx", &src) {
        Ok(_) => {}
        Err(Failure(d)) => panic!("{d}"),
    }
    let warned = take_warnings();
    assert_eq!(warned.matches("warning[Q0802]").count(), 2, "{warned}");
    assert!(warned.contains("`i -= 1;`"), "{warned}");
}

// `\r` and Java's `\uXXXX` let a program write terminal control sequences (roadmap A3).
#[test]
fn carriage_return_and_unicode_escapes() {
    let src = "import std.io;\n\nvoid main() {\n    io.write(\"\\u001B[2J\\r\\u00e9\\u0041\");\n    println(\"{}\", \"\\u001B\".length());\n}\n";
    let mut term = Term::default();
    assert_eq!(run_term(src, &mut term), Ok((0, "1\n".to_string())));
    assert_eq!(term.out, "\u{1b}[2J\r\u{e9}A");
    for bad in [
        r#""\u12""#,
        r#""\uD800""#,
        r#""\uZZZZ""#,
        r#""\u""#,
        r#""\u{1b}""#,
    ] {
        assert_eq!(
            codes(&main_body(&format!("String s = {bad};"))),
            ["Q0103"],
            "{bad}"
        );
    }
}

// Standard modules need no project; package imports do, with a hint for a missing `std.`.
#[test]
fn std_imports_need_no_project() {
    assert_eq!(
        codes_cli("import std.time;\nimport std.io;\nimport std.math.*;\n\nvoid main() {\n    io.write(\"\" + sqrt(4.0));\n    time.sleep(0);\n}\n"),
        Vec::<&str>::new()
    );
    let err = match check("t.qx", "import time.sleep;\n\nvoid main() {}\n") {
        Err(Failure(d)) => d,
        Ok(_) => panic!("expected Q0508"),
    };
    assert!(
        err.starts_with("error[Q0508]") && err.contains("`import std.time;`"),
        "{err}"
    );
    // An unknown module lists every module this build has.
    let err = match check("t.qx", "import std.tme;\n\nvoid main() {}\n") {
        Err(Failure(d)) => d,
        Ok(_) => panic!("expected Q0701"),
    };
    assert!(
        err.contains("`std.io`") && err.contains("`std.json`"),
        "{err}"
    );
}
