// IR tests: text-dump snapshots, and explicit comparisons of the two engines on the examples,
// the benchmark programs, runtime errors, every possible step-limit position and cancellation
// polls. (Every other test that elaborates a program also compares both engines: tests run with
// `Engine::Dual`.)

use std::cell::Cell;
use std::path::Path;

use super::super::engine::{self, Engine};
use super::super::stdlib::{Host, HostValue, LogOnly, StdResult};
use super::super::{front, qasm_with, run_with, Failure, Limits};

fn dump(src: &str) -> String {
    let p = front(src).unwrap_or_else(|e| panic!("{e:?}"));
    super::build(&p).to_string()
}

// The repository root, also when `src/lang` is compiled by crates/qxide-wasm or the app runtime.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|d| d.join("examples/qx").is_dir())
        .expect("repository root")
}

fn text(r: Result<String, Failure>) -> String {
    match r {
        Ok(s) => format!("ok\n{s}"),
        Err(Failure(d)) => format!("error\n{d}"),
    }
}

// `qxide run` and `qxide qasm` output of `src` under one engine.
fn outputs(engine: Engine, src: &str, shots: u64, seed: u64, limits: Limits) -> (String, String) {
    engine::with(engine, || {
        (
            text(run_with("t.qx", src, shots, seed, limits)),
            text(qasm_with("t.qx", src, &limits)),
        )
    })
}

fn same_outputs(src: &str, shots: u64, seed: u64, limits: Limits) {
    let ast = outputs(Engine::Ast, src, shots, seed, limits);
    let ir = outputs(Engine::Ir, src, shots, seed, limits);
    assert_eq!(
        ast, ir,
        "engines differ (shots {shots}, seed {seed}) on\n{src}"
    );
}

const SUM: &str = "int sum(int n) {
    int total = 0;
    for i in 0..n {
        total = total + i;
    }
    return total;
}

void main() {
    println(\"{}\", sum(4));
}
";

#[test]
#[ignore = "prints the dumps, for updating the snapshots below"]
fn print_dumps() {
    let src = std::fs::read_to_string(root().join("examples/qx/functions.qx")).unwrap();
    println!("{}", dump(&src));
    println!("{}", dump(SUM));
}

// docs/design/qx-ir.md shows this example; `charge n` is the steps the interpreter charges there.
#[test]
fn functions_example_snapshot() {
    let src = std::fs::read_to_string(root().join("examples/qx/functions.qx")).unwrap();
    let want = "\
fn bellPair(borrow $a: qubit, borrow $b: qubit) -> unit {
bb0:
    charge 4
    %0 = load $a
    h %0
    charge 4
    %2 = load $a
    charge 2
    %3 = load $b
    cx %2, %3
    ret
}

fn measureAll(consume $r: qreg) -> unit {
    slot $m: bits
bb0:
    charge 3
    %0 = load $r
    %1 = measure.reg %0
    store $m, %1
    charge 3
    %2 = load $m
    print \"{}\", %2
    ret
}

fn main() -> unit {
    slot $r: qreg
bb0:
    charge 2
    %0 = const.i64 2
    %1 = qalloc.reg %0
    store $r, %1
    charge 4
    %2 = load $r
    charge 1
    %3 = const.i64 0
    %4 = index %2, %3
    charge 3
    %5 = load $r
    charge 1
    %6 = const.i64 1
    %7 = index %5, %6
    call bellPair(%4, %7)
    charge 3
    %9 = load $r
    call measureAll(%9)
    ret
}
";
    assert_eq!(dump(&src), want);
}

#[test]
fn for_loop_snapshot() {
    let want = "\
fn sum(copy $n: i64) -> i64 {
    slot $total: i64
    slot $i.next: i64
    slot $i: i64
bb0:
    charge 1
    %0 = const.i64 0
    store $total, %0
    charge 1
    %1 = const.i64 0
    charge 3
    %2 = load $n
    store $i.next, %1
    jump bb1
bb1:
    %3 = load $i.next
    %4 = lt.i64 %3, %2
    br %4, bb2, bb3
bb2:
    charge 1
    store $i, %3
    charge 4
    %5 = load $total
    charge 2
    %6 = load $i
    %7 = add.i64 %5, %6
    store $total, %7
    charge 2
    %8 = const.i64 1
    %9 = add.i64 %3, %8
    store $i.next, %9
    jump bb1
bb3:
    charge 2
    %10 = load $total
    ret %10
}

fn main() -> unit {
bb0:
    charge 3
    %0 = const.i64 4
    %1 = call sum(%0)
    print \"{}\", %1
    ret
}
";
    assert_eq!(dump(SUM), want);
}

#[test]
fn bit_if_regions_snapshot() {
    let src = "void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    Bit m = measure(a);
    if m {
        x(b);
    } else {
        z(b);
    }
    discard(b);
}
";
    let d = dump(src);
    assert!(d.contains("if.bit %"), "{d}");
    assert!(d.contains("end.region"), "{d}");
    let regions = d.matches("end.region").count();
    assert_eq!(regions, 2, "{d}");
}

fn qx_files(dir: &str) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(root().join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "qx"))
        .collect();
    v.sort();
    v
}

// Seeded measurement results, `println` output, tables and QASM of every example.
#[test]
fn examples_agree_across_seeds_and_shots() {
    let files = qx_files("examples/qx");
    assert!(files.len() >= 7, "examples/qx went missing");
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap();
        for (shots, seed) in [(1, 0), (1, 42), (1000, 0), (257, 7), (100, u64::MAX)] {
            same_outputs(&src, shots, seed, Limits::CLI);
            same_outputs(&src, shots, seed, Limits::PLAYGROUND);
        }
    }
}

#[test]
fn benchmark_programs_agree() {
    for path in qx_files("bench/qx") {
        let src = std::fs::read_to_string(&path).unwrap();
        same_outputs(&src, 3, 1, Limits::CLI);
    }
}

// One program per runtime error code the interpreter can report.
const RUNTIME_ERRORS: &[&str] = &[
    // Q0401 overflow (also unary minus), Q0402 division by zero
    "void main() { int a = 9223372036854775807; println(\"{}\", a + 1); }",
    "void main() { int a = -9223372036854775807 - 1; println(\"{}\", -a); }",
    "void main() { int z = 0; println(\"{}\", 5 % z); }",
    // Q0403 index out of bounds, Q0404 negative register size
    "void main() { QReg r = qreg(2); int i = 2; h(r[i]); discard(r); }",
    "void main() { int n = -1; QReg r = qreg(n); discard(r); }",
    // Q0405 recursion depth (a missing `return` is a checker error, so it never runs)
    "int f(int n) { return f(n + 1); } void main() { println(\"{}\", f(0)); }",
    // Q0303 aliased register elements with run-time indices
    "void two(Qubit a, Qubit b) { cx(a, b); } void main() { QReg r = qreg(2); int i = 1; two(r[i], r[1]); discard(r); }",
    "void main() { QReg r = qreg(2); int i = 1; cx(r[i], r[1]); discard(r); }",
    // Q0410 `as_int` too wide, Q0411 non-finite angle
    "void main() { QReg r = qreg(65); BitReg m = measure(r); println(\"{}\", m.as_int()); }",
    "void main() { Qubit q = qubit(); double z = 0.0; rx(q, 1.0 / z); discard(q); }",
    // `double` division by zero is IEEE 754, casts saturate
    "void main() { double z = 0.0; println(\"{} {} {}\", 1.0 / z, (int) (1.0 / z), (int) (0.0 / z)); }",
    // strings, `var`, widening at declarations, assignments, calls and returns
    "double half(double x) { return x / 2; } int one() { return 1; } void main() { var s = \"a\" + 1 + 2.5 + true; double d = 3; d = 4; String t = s + half(5) + one(); println(\"{} {} {} {} {}\", s, d, t, len(t), s == t); }",
    // short circuit
    "boolean f(int n) { println(\"f {}\", n); return n > 0; } void main() { boolean a = f(0) && f(1); boolean b = f(1) || f(2); boolean c = !f(0) || f(3); println(\"{} {} {}\", a, b, c); }",
    // `return` inside a loop, nested `else if`, shadowing in sibling blocks
    "int first(int n) { for i in 0..n { if i * i > n { return i; } } return -1; } void main() { int k = 7; if k < 3 { println(\"a\"); } else if k < 8 { int x = 1; println(\"b {}\", x); } else { int x = 2; println(\"c {}\", x); } println(\"{}\", first(30)); }",
];

#[test]
fn runtime_errors_agree() {
    for (k, src) in RUNTIME_ERRORS.iter().enumerate() {
        // Each one passes the checker, so the error (if any) comes from elaboration.
        assert!(front(src).is_ok(), "{src}: {:?}", front(src).err());
        let (run, _) = outputs(Engine::Ir, src, 1, 0, Limits::CLI);
        // The first ten fail at run time, the others succeed.
        let want = if k < 10 { "error\nerror[Q0" } else { "ok\n" };
        assert!(run.starts_with(want), "{src}\n{run}");
        same_outputs(src, 1, 0, Limits::CLI);
        same_outputs(src, 20, 5, Limits::PLAYGROUND);
    }
}

// The step budget is charged in the same order and at the same spans: for every budget from 0
// to the program's total, both engines stop at the same place with the same diagnostic.
#[test]
fn every_step_limit_position_agrees() {
    let src = "int fib(int n) { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }
String tag(int k) { return \"k=\" + k; }
void main() {
    QReg r = qreg(3);
    var total = 0;
    for i in 0..4 {
        total = total + fib(i);
        h(r[i % 3]);
    }
    BitReg m = measure(r);
    Bit b = m[0];
    Qubit q = qubit();
    if b {
        x(q);
    } else {
        z(q);
    }
    println(\"{} {} {} {}\", tag(total), m, m.to_binary(), len(tag(1)));
    discard(q);
}
";
    let total = engine::with(Engine::Ast, || {
        let p = front(src).unwrap();
        let mut host = LogOnly::default();
        super::super::interp::elaborate(&p, &mut host, &Limits::CLI)
            .unwrap()
            .usage
            .steps
    });
    assert!(total > 100, "{total}");
    for max in 0..=total + 1 {
        let limits = Limits {
            max_steps: max,
            ..Limits::CLI
        };
        same_outputs(src, 1, 3, limits);
    }
}

// The output, operation and conditional-depth budgets fail at the same place too.
#[test]
fn other_budgets_agree() {
    let src = "void main() {
    QReg r = qreg(4);
    for i in 0..4 { h(r[i]); cx(r[i], r[(i + 1) % 4]); }
    BitReg m = measure(r);
    Qubit q = qubit();
    if m[0] { if m[1] { if m[2] { x(q); } } }
    println(\"{} {}\", m, m.as_int());
    discard(q);
}
";
    for k in 0..40 {
        same_outputs(
            src,
            1,
            1,
            Limits {
                max_ops: k,
                ..Limits::CLI
            },
        );
        same_outputs(
            src,
            1,
            1,
            Limits {
                max_output_bytes: k,
                ..Limits::CLI
            },
        );
    }
    for k in 0..4 {
        same_outputs(
            src,
            1,
            1,
            Limits {
                max_cond_depth: k,
                ..Limits::CLI
            },
        );
    }
}

// Counts how often elaboration asks whether it was cancelled.
struct Polls {
    n: Cell<u64>,
}

impl Host for Polls {
    fn call(&mut self, _: &str, _: &str, _: Vec<HostValue>) -> StdResult {
        Ok(HostValue::Unit)
    }

    fn cancelled(&self) -> bool {
        self.n.set(self.n.get() + 1);
        false
    }
}

// Merged charges poll the host at exactly the same points as one-by-one charges.
#[test]
fn cancellation_polls_agree() {
    let src = std::fs::read_to_string(root().join("bench/qx/loops.qx")).unwrap();
    let p = front(&src).unwrap();
    let polls = |e: Engine| {
        let mut host = Polls { n: Cell::new(0) };
        let r = engine::elaborate(&p, &mut host, &Limits::CLI, e).map(|e| e.usage.steps);
        (r.unwrap(), host.n.get())
    };
    let (ast, ir) = (polls(Engine::Ast), polls(Engine::Ir));
    assert!(ast.1 > 100, "{ast:?}");
    assert_eq!(ast, ir);
}
