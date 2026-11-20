// Checker and interpreter tests. Assertions use error codes and line:column, never message text.

use super::diag::SourceMap;
use super::*;

const BELL: &str = include_str!("../../examples/qx/bell.qx");
const GHZ: &str = include_str!("../../examples/qx/ghz.qx");
const TELEPORT: &str = include_str!("../../examples/qx/teleport.qx");

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
    format!("fn main() {{\n{body}\n}}\n")
}

#[test]
fn examples_and_doc_samples_pass_the_checker() {
    for src in [BELL, GHZ, TELEPORT] {
        ok(src);
    }
    ok("fn parity(n: i64) -> bool {\n    n % 2 == 1\n}\n\nfn main() {\n    let mut total = 0;\n    for i in 0..10 {\n        if parity(i) {\n            total = total + i;\n        }\n    }\n    println(\"total={}\", total);\n}\n");
}

// The four error samples of docs/design/qx-language.md section 4, with their positions.
#[test]
fn design_doc_error_samples() {
    let use_after = "fn main() {\n    let q = qubit();\n    let m = measure(q);\n    h(q);\n}\n";
    assert_eq!(errors(use_after), vec![("Q0301", 4, 7)]);

    let dup = "fn main() {\n    let a = qubit();\n    cx(a, a);\n    discard(a);\n}\n";
    assert_eq!(errors(dup), vec![("Q0303", 3, 11)]);

    let leak = "fn main() {\n    let q = qubit();\n    h(q);\n}\n";
    assert_eq!(errors(leak), vec![("Q0302", 2, 9)]);

    let in_loop = "fn main() {\n    let q = qubit();\n    for i in 0..3 {\n        let m = measure(q);\n    }\n}\n";
    // The loop may also run zero times, so `q` additionally leaks at the end of `main`.
    assert_eq!(errors(in_loop), vec![("Q0302", 2, 9), ("Q0305", 4, 17)]);

    let bit_if = "fn main() {\n    let a = qubit();\n    let t = qubit();\n    h(a);\n    let m = measure(a);\n    if m {\n        x(t);\n        let m2 = measure(t);\n    }\n}\n";
    assert_eq!(errors(bit_if), vec![("Q0308", 8, 18)]);
}

#[test]
fn linear_qubit_rules() {
    // Moving a qubit into another variable consumes the original.
    assert_eq!(
        codes(&main_body("let q = qubit(); let p = q; h(q); discard(p);")),
        ["Q0301"]
    );
    ok(&main_body("let q = qubit(); let p = q; h(p); discard(p);"));
    assert_eq!(
        codes(&main_body("let q = qubit(); let q = qubit(); discard(q);")),
        ["Q0310"]
    );
    // Shadowing in an inner scope does not leak the outer qubit.
    ok(&main_body(
        "let q = qubit(); if true { let q = qubit(); discard(q); } discard(q);",
    ));
    assert_eq!(
        codes(&main_body("let q = qubit(); if true { discard(q); }")),
        ["Q0304"]
    );
    ok(&main_body(
        "let q = qubit(); if true { discard(q); } else { let m = measure(q); }",
    ));
    // A branch that returns does not have to agree with the other one.
    ok(&main_body(
        "let q = qubit(); if true { discard(q); return; } discard(q);",
    ));
    assert_eq!(
        codes(&main_body(
            "let q = qubit(); if true { return; } discard(q);"
        )),
        ["Q0302"]
    );
    assert_eq!(
        codes(&main_body(
            "let r = qreg(2); let m = measure(r[0]); discard(r);"
        )),
        ["Q0306"]
    );
    assert_eq!(
        codes(&main_body("let r = qreg(2); let q = r[0]; discard(r);")),
        ["Q0306"]
    );
    assert_eq!(
        codes(&main_body("let q = qubit(); q = qubit(); discard(q);")),
        ["Q0307"]
    );
    assert_eq!(codes(&main_body("h(qubit());")), ["Q0311"]);
    ok(&main_body("let m = measure(qubit()); discard(qreg(3));"));
    // Registers: literal indices are checked statically, others at run time.
    assert_eq!(
        codes(&main_body("let r = qreg(2); cx(r[0], r[0]); discard(r);")),
        ["Q0303"]
    );
    ok(&main_body(
        "let r = qreg(3); ccx(r[0], r[1], r[2]); let i = 1; cx(r[i], r[0]); discard(r);",
    ));
    assert_eq!(
        codes(&main_body(
            "let a = qubit(); let b = qubit(); ccx(a, b, a); discard(a); discard(b);"
        )),
        ["Q0303"]
    );
    // Allocating and consuming inside a loop body is fine.
    ok(&main_body(
        "for i in 0..3 { let q = qubit(); h(q); let m = measure(q); println(\"{}\", m); }",
    ));
    assert_eq!(
        codes(&main_body(
            "let q = qubit(); let m = measure(q); let n = measure(q);"
        )),
        ["Q0301"]
    );
}

#[test]
fn unbound_quantum_temporaries_are_rejected() {
    assert_eq!(codes(&main_body("qubit();")), ["Q0312"]);
    assert_eq!(codes(&main_body("qreg(3);")), ["Q0312"]);
    assert_eq!(codes(&main_body("let n = len(qreg(3));")), ["Q0312"]);
    assert_eq!(codes(&main_body("let m = measure(qreg(2)[0]);")), ["Q0312"]);
    assert_eq!(
        codes(&main_body("let q = qreg(2)[1]; discard(q);")),
        ["Q0312"]
    );
    // Reading a register variable is fine.
    ok(&main_body(
        "let r = qreg(3); let n = len(r); h(r[n - 1]); discard(r);",
    ));
}

#[test]
fn byte_order_mark_is_ignored() {
    let src = format!("\u{feff}{BELL}");
    assert!(check("t.qx", &src).is_ok());
    let bad = "\u{feff}fn main() {\n    let q = qubit();\n}\n";
    let Err(Failure(d)) = check("t.qx", bad) else {
        panic!("expected an error")
    };
    // Columns are counted from after the mark.
    assert!(d.contains("--> t.qx:2:9\n"), "{d}");
}

#[test]
fn huge_stabilizer_runs_are_refused_before_allocating() {
    // About 20 GB of tableau; must become Q0406 instead of aborting on allocation.
    let src = main_body("let r = qreg(200000);\nlet m = measure(r);\nprintln(\"{}\", m);");
    let Err(Failure(d)) = run("t.qx", &src, 1, 0) else {
        panic!("expected a simulation error")
    };
    assert!(d.contains("error[Q0406]"), "{d}");
    assert!(qasm("t.qx", &src).is_ok());
    // Slots freed by `measure` are reused, so 1000 sequential qubits need a single slot.
    let reuse = main_body(
        "for i in 0..1000 { let q = qubit(); h(q); let m = measure(q); if i == 999 { println(\"{}\", m); } }",
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
    let pre = "let a = qubit(); let t = qubit(); h(a); let m = measure(a);";
    let with = |body: &str| main_body(&format!("{pre} {body} discard(t);"));
    ok(&with(
        "if m { x(t); for i in 0..2 { z(t); } let k = 3; if k > 2 { h(t); } } else { reset(t); }",
    ));
    ok(&with(
        "let r = measure(qubit()); if m { x(t); } else if r { z(t); }",
    ));
    assert_eq!(codes(&with("if m { println(\"x\"); }")), ["Q0308"]);
    assert_eq!(
        codes(&with("if m { let q = qubit(); discard(q); }")),
        ["Q0308", "Q0308", "Q0308"]
    );
    assert_eq!(codes(&with("if m { return; }")), ["Q0308"]);
    assert_eq!(
        codes(&format!("fn f() {{}}\n{}", with("if m { f(); }"))),
        ["Q0308"]
    );
    assert_eq!(codes(&with("let mut k = 0; if m { k = 1; }")), ["Q0309"]);
    // Locals declared inside the body may be assigned there.
    ok(&with("if m { let mut k = 0; k = k + 1; }"));
}

#[test]
fn types_and_names() {
    let m = "let m = measure(qubit());";
    assert_eq!(codes(&main_body(&format!("{m} let x = m + 1;"))), ["Q0218"]);
    assert_eq!(
        codes(&main_body(&format!("{m} for i in 0..m {{}}"))),
        ["Q0218"]
    );
    assert_eq!(
        codes(&main_body(&format!("{m} let b: bool = m;"))),
        ["Q0218"]
    );
    assert_eq!(codes(&main_body("if 1 {}")), ["Q0215"]);
    assert_eq!(codes(&main_body("let x: bool = 1;")), ["Q0204"]);
    assert_eq!(codes(&main_body("let x = 1 + true;")), ["Q0204"]);
    assert_eq!(codes(&main_body("let x = 1; x = 2;")), ["Q0207"]);
    assert_eq!(codes(&main_body("let x = y;")), ["Q0201"]);
    assert_eq!(codes(&main_body("foo();")), ["Q0202"]);
    assert_eq!(
        codes(&main_body("let q = qubit(1); discard(q);")),
        ["Q0203"]
    );
    assert_eq!(codes(&main_body("let x = 3; let y = x[0];")), ["Q0209"]);
    assert_eq!(codes(&main_body("println(\"{} {}\", 1);")), ["Q0210"]);
    assert_eq!(codes(&main_body("println(1);")), ["Q0211"]);
    assert_eq!(
        codes(&main_body(
            "let q = qubit(); println(\"{}\", q); discard(q);"
        )),
        ["Q0212"]
    );
    assert_eq!(codes(&main_body("let s = \"hi\";")), ["Q0214"]);
    assert_eq!(codes("fn f() {}\nfn f() {}\nfn main() {}"), ["Q0205"]);
    assert_eq!(codes("fn f(a: i64, a: i64) {}\nfn main() {}"), ["Q0205"]);
    assert_eq!(codes("fn helper() {}"), ["Q0206"]);
    assert_eq!(codes("fn main(x: i64) {}"), ["Q0206"]);
    assert_eq!(codes("fn h() {}\nfn main() {}"), ["Q0217"]);
    assert_eq!(codes("fn f(q: Qubit) {}\nfn main() {}"), ["Q0219"]);
    assert_eq!(
        codes("fn f() -> i64 { if true { return 1; } }\nfn main() {}"),
        ["Q0213"]
    );
    ok("fn f() -> i64 { if true { return 1; } else { return 2; } }\nfn main() { let x = f(); }");
    assert_eq!(codes("fn f() -> i64 { true }\nfn main() {}"), ["Q0204"]);
    // Every error is reported, not just the first.
    assert_eq!(
        codes(&main_body("let a = y; let b = z; let c: bool = 1;")),
        ["Q0201", "Q0201", "Q0204"]
    );
}

#[test]
fn runtime_errors_have_positions() {
    assert_eq!(
        runtime_error(&main_body("let x = 9223372036854775807;\nlet y = x + 1;")),
        ("Q0401", 3, 9)
    );
    assert_eq!(
        runtime_error(&main_body("let z = 0;\nlet y = 7 % z;")),
        ("Q0402", 3, 9)
    );
    assert_eq!(
        runtime_error(&main_body("let r = qreg(2);\nh(r[2]);\ndiscard(r);")).0,
        "Q0403"
    );
    assert_eq!(
        runtime_error(&main_body(
            "let r = qreg(2);\nlet i = 1;\ncx(r[i], r[1]);\ndiscard(r);"
        )),
        ("Q0303", 4, 1)
    );
    assert_eq!(
        runtime_error(
            "fn f(n: i64) -> i64 {\n    f(n + 1)\n}\nfn main() {\n    let x = f(0);\n}\n"
        )
        .0,
        "Q0405"
    );
    assert_eq!(
        runtime_error(&main_body("let n = 0 - 1;\nlet r = qreg(n);\ndiscard(r);")).0,
        "Q0408"
    );
    assert_eq!(
        runtime_error(&main_body("let r = qreg(2000000);\ndiscard(r);")).0,
        "Q0409"
    );
}

#[test]
fn bell_and_ghz_outcomes_are_correlated() {
    let bell = run_ok(BELL, 200);
    let mut seen = 0;
    for line in bell.lines().filter(|l| !l.starts_with("==")) {
        assert!(line == "a=0 b=0" || line == "a=1 b=1", "{bell}");
        seen += 1;
    }
    assert_eq!(seen, 2, "both outcomes should appear in 200 shots:\n{bell}");
    let ghz = run_ok(GHZ, 100);
    for line in ghz.lines().filter(|l| !l.starts_with("==")) {
        assert!(line == "00000" || line == "11111", "{ghz}");
    }
    let one = run_ok(BELL, 1);
    assert!(one == "a=0 b=0\n" || one == "a=1 b=1\n", "{one}");
}

#[test]
fn teleport_always_delivers_one() {
    assert_eq!(run_ok(TELEPORT, 64), "== 64/64 ==\nbob=1\n");
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
    let els = main_body("let a = qubit(); let t = qubit(); h(a); let m = measure(a); if m { x(t); } else { z(t); } discard(t);");
    let q = qasm("t.qx", &els).ok().unwrap();
    assert!(
        q.contains("if (c[0] == true) {\n  x q[1];\n}\nif (c[0] == false) {\n  z q[1];\n}"),
        "{q}"
    );
    // A `bool` condition is decided at build time: only the taken branch is emitted.
    let q = qasm(
        "t.qx",
        &main_body("let q = qubit(); if 1 < 2 { x(q); } else { h(q); } discard(q);"),
    )
    .ok()
    .unwrap();
    assert!(q.contains("x q[0];") && !q.contains("h q[0];"), "{q}");
}

#[test]
fn classical_programs_run_without_simulation() {
    let src = "fn fib(n: i64) -> i64 {\n    if n < 2 {\n        return n;\n    }\n    fib(n - 1) + fib(n - 2)\n}\nfn main() {\n    let mut s = 0;\n    for i in 0..10 {\n        if i % 2 == 1 && !(i == 5) {\n            s = s + i;\n        }\n    }\n    println(\"fib={} s={} {{ok}}\", fib(15), s);\n}\n";
    assert_eq!(run_ok(src, 1), "fib=610 s=20 {ok}\n");
    assert_eq!(run_ok(src, 3), "== 3/3 ==\nfib=610 s=20 {ok}\n");
}

#[test]
fn simulation_errors_become_diagnostics() {
    // 30 qubits with a non-Clifford gate need a 16 GiB state vector, over the 1 GiB default.
    let src = main_body("let r = qreg(30);\nt(r[0]);\nlet m = measure(r);\nprintln(\"{}\", m);");
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
        "fn main() {\r\n    let q = qubit();\r\n    let m = measure(q);\r\n    h(q);\r\n}\r\n";
    let Err(Failure(d)) = check("bad.qx", src) else {
        panic!("expected errors")
    };
    assert!(d.starts_with("error[Q0301]: "), "{d}");
    assert!(d.contains("--> bad.qx:4:7\n"), "{d}");
    assert!(d.contains("4 |     h(q);\n  |       ^\n"), "{d}");
    assert!(d.contains("--> bad.qx:3:13\n"), "{d}");
    assert!(
        d.contains("3 |     let m = measure(q);\n  |             ^^^^^^^^^^\n"),
        "{d}"
    );
    assert!(!d.contains('\r'), "{d:?}");
}
