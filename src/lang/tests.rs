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
    for src in [BELL, GHZ, TELEPORT] {
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

#[test]
fn java_style_declarations_and_functions() {
    let src = "int add(int a, int b) {\n    return a + b;\n}\n\ndouble half(double x) {\n    return x / 2;\n}\n\nvoid main() {\n    final int n = 3;\n    double d = n;\n    d = d * 2;\n    boolean big = add(n, 4) > 6;\n    println(\"{} {} {} {} {}\", add(n, 4), half(n), d, big, 7 / 2);\n}\n";
    assert_eq!(run_ok(src, 1), "7 1.5 6.0 true 3\n");
    assert_eq!(codes(&main_body("final int x = 1;\nx = 2;")), ["Q0207"]);
    assert_eq!(codes(&main_body("int x = 1.5;")), ["Q0204"]);
    assert_eq!(codes(&main_body("void x = 1;")), ["Q0204"]);
    assert_eq!(codes("void f(void v) {}\nvoid main() {}"), ["Q0204"]);
    assert_eq!(codes("int main() {\n    return 0;\n}"), ["Q0206"]);
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
        ["Q0308", "Q0308", "Q0308"]
    );
    assert_eq!(codes(&with("if m { return; }")), ["Q0308"]);
    assert_eq!(
        codes(&format!("void f() {{}}\n{}", with("if m { f(); }"))),
        ["Q0308"]
    );
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
    assert_eq!(codes(&main_body("int s = \"hi\";")), ["Q0214"]);
    assert_eq!(codes("void f() {}\nvoid f() {}\nvoid main() {}"), ["Q0205"]);
    assert_eq!(codes("void f(int a, int a) {}\nvoid main() {}"), ["Q0205"]);
    assert_eq!(codes("void helper() {}"), ["Q0206"]);
    assert_eq!(codes("void main(int x) {}"), ["Q0206"]);
    assert_eq!(codes("void h() {}\nvoid main() {}"), ["Q0217"]);
    assert_eq!(codes("void f(Qubit q) {}\nvoid main() {}"), ["Q0219"]);
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
            "BitReg m = measure(qreg(65));\nprintln(\"{}\", m.as_int());"
        )),
        ("Q0410", 3, 15)
    );
}

// `[0, 1, 1]` lists r[0] first; `0b110` and `as_int() == 6` read r[0] as the least significant bit.
#[test]
fn bit_registers_print_as_array_binary_and_int() {
    let src = main_body(
        "QReg r = qreg(3);\nx(r[1]);\nx(r[2]);\nBitReg m = measure(r);\nprintln(\"{} {} {} {}\", m, m.to_binary(), m.as_int(), m[0]);",
    );
    assert_eq!(run_ok(&src, 1), "[0, 1, 1] 0b110 6 0\n");
    assert_eq!(
        run_ok(&src, 5),
        "[0, 1, 1]  0b110 = 6  ×5\n  [0, 1, 1] 0b110 6 0\n"
    );
    let empty = main_body(
        "BitReg m = measure(qreg(0));\nprintln(\"{} {} {}\", m, m.to_binary(), m.as_int());",
    );
    assert_eq!(run_ok(&empty, 1), "[] 0b0 0\n");
    let m = "BitReg m = measure(qreg(2));";
    assert_eq!(
        codes(&main_body(&format!("{m} int v = m.as_int();"))),
        ["Q0218"]
    );
    assert_eq!(
        codes(&main_body(&format!("{m} println(\"{{}}\", m.foo());"))),
        ["Q0220"]
    );
    assert_eq!(
        codes(&main_body(
            "Qubit q = qubit(); println(\"{}\", q.to_binary()); discard(q);"
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

// Diagnostics are English: no Hangul in any message, note or help (the library's SimError
// text is Korean, so out-of-memory errors are re-worded here).
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
        main_body("BitReg m = measure(qreg(65));\nprintln(\"{}\", m.as_int());"),
        "int f(int n) {\n    return f(n + 1);\n}\nvoid main() {\n    int x = f(0);\n}\n".to_string(),
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
