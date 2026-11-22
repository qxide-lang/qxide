// JIT tests: which functions compile, edge cases of native arithmetic, every step-limit position
// and cancellation polls in native code, each against the tree-walking interpreter.

use std::cell::Cell;

use super::super::engine::{self, Engine};
use super::super::ir::exec::Native;
use super::super::stdlib::{Host, HostValue, StdResult};
use super::super::{front, qasm_with, run_with, Failure, Limits};
use super::*;

fn module(src: &str) -> Module {
    let p = front(src).unwrap_or_else(|e| panic!("{e:?}"));
    super::super::ir::build(&p)
}

fn text(r: Result<String, Failure>) -> String {
    match r {
        Ok(s) => format!("ok\n{s}"),
        Err(Failure(d)) => format!("error\n{d}"),
    }
}

fn outputs(engine: Engine, src: &str, limits: Limits) -> (String, String) {
    engine::with(engine, || {
        (
            text(run_with("t.qx", src, 1, 0, limits)),
            text(qasm_with("t.qx", src, &limits)),
        )
    })
}

fn same(src: &str, limits: Limits) -> String {
    let ast = outputs(Engine::Ast, src, limits);
    assert_eq!(ast, outputs(Engine::Ir, src, limits), "ir on\n{src}");
    assert_eq!(ast, outputs(Engine::Jit, src, limits), "jit on\n{src}");
    ast.0
}

#[test]
fn pure_classical_functions_compile_and_others_stay_interpreted() {
    let src = "int fib(int n) { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }
double half(double x) { return x / 2; }
boolean odd(int n) { return n % 2 == 1; }
void nothing(int n) { int k = n; }
int viaFib(int n) { return fib(n) + 1; }
String name(int n) { return \"n\" + n; }
int usesName(int n) { return len(name(n)); }
int gate(Qubit q) { h(q); return 1; }
void main() {
    Qubit q = qubit();
    println(\"{} {} {} {} {} {}\", viaFib(5), half(3.0), odd(3), usesName(4), gate(q), fib(3));
    nothing(1);
    discard(q);
}
";
    let m = module(src);
    let mut jit = Jit::new(&m).expect("Cranelift supports this machine");
    let id = |n: &str| m.index[n];
    for f in ["fib", "half", "odd", "nothing", "viaFib"] {
        assert!(jit.compiles(id(f)), "{f}");
    }
    for f in ["name", "usesName", "gate", "main"] {
        assert!(!jit.compiles(id(f)), "{f}");
    }
    // Compiling one function compiles the functions it calls too.
    assert!(jit.has(id("viaFib")));
    assert!(jit.entries[id("fib") as usize].is_some());
    assert!(jit.entries[id("half") as usize].is_none());
    assert_eq!(same(src, Limits::CLI), "ok\n6 1.5 true 2 1 2\n");
}

// `while`, `break`, `continue` and compound assignment are plain jumps, so such functions compile.
// `match` on `int` and `boolean` compiles; functions that touch enum values stay interpreted.
#[test]
fn int_matches_compile_and_enums_stay_interpreted() {
    let src = "enum E { A, B(int n) }
int pick(int n) {
    match n {
        0 => {
            return 10;
        }
        -1 => {
            return 20;
        }
        _ => {
            return n;
        }
    }
}
int unwrap(E e) {
    match e {
        B(n) => {
            return n;
        }
        _ => {
            return 0;
        }
    }
}
void main() {
    println(\"{} {} {} {}\", pick(0), pick(-1), pick(7), unwrap(E.B(3)));
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    assert!(jit.compiles(m.index["pick"]));
    assert!(!jit.compiles(m.index["unwrap"]));
    assert_eq!(same(src, Limits::CLI), "ok\n10 20 7 3\n");
}

#[test]
fn loops_with_break_and_continue_compile() {
    let src = "int collatz(int n) {
    int steps = 0;
    while n != 1 {
        if n % 2 == 0 { n /= 2; } else { n = 3 * n + 1; }
        steps += 1;
    }
    return steps;
}
int evens(int n) {
    int total = 0;
    for i in 0..n {
        if i > 20 { break; }
        if i % 2 == 1 { continue; }
        total += i;
    }
    return total;
}
void main() {
    println(\"{} {}\", collatz(27), evens(100));
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    for f in ["collatz", "evens"] {
        assert!(jit.compiles(m.index[f]), "{f}");
    }
    assert_eq!(same(src, Limits::CLI), "ok\n111 110\n");
}

// Overflow, division, remainder, casts and IEEE 754 corner cases inside compiled functions.
#[test]
fn native_arithmetic_matches_the_interpreter() {
    let ok = [
        (
            "i64",
            "int f(int a, int b) { return a / b + a % b; }",
            "f(-7, 2)",
        ),
        (
            "i64",
            "int f(int a, int b) { return a % b; }",
            "f(-9223372036854775807 - 1, -1)",
        ),
        ("i64", "int f(int a, int b) { return a % b; }", "f(7, -1)"),
        (
            "i64",
            "int f(int a, int b) { return a * b - b; }",
            "f(3037000499, 3037000499)",
        ),
        (
            "f64",
            "double f(double a, double b) { return a % b; }",
            "f(-7.5, 2.0)",
        ),
        (
            "f64",
            "double f(double a, double b) { return a / b; }",
            "f(1.0, 0.0)",
        ),
        (
            "bool",
            "boolean f(double a, double b) { return a != b; }",
            "f(0.0 / 0.0, 0.0 / 0.0)",
        ),
        (
            "bool",
            "boolean f(double a, double b) { return a <= b || a > b; }",
            "f(0.0 / 0.0, 1.0)",
        ),
        (
            "i64",
            "int f(double a, double b) { return (int) (a / b); }",
            "f(0.0, 0.0)",
        ),
        (
            "i64",
            "int f(double a, double b) { return (int) (a / b) + 0; }",
            "f(-1.0, 0.0)",
        ),
        (
            "i64",
            "int f(double a, double b) { return (int) a; }",
            "f(-2.9, 0.0)",
        ),
        (
            "f64",
            "double f(int a, int b) { return (double) a / b + a; }",
            "f(1, 3)",
        ),
        (
            "bool",
            "boolean f(boolean a, boolean b) { return !a == b && (a || !b); }",
            "f(true, false)",
        ),
        (
            "f64",
            "double f(double a, int b) { double x = -a; return x * b; }",
            "f(2.5, 4)",
        ),
    ];
    for (_, def, call) in ok {
        let src = format!("{def}\nvoid main() {{ println(\"{{}}\", {call}); }}\n");
        assert!(same(&src, Limits::CLI).starts_with("ok\n"), "{src}");
    }
    let errors = [
        "int f(int a, int b) { return a + b; }\nvoid main() { println(\"{}\", f(9223372036854775807, 1)); }",
        "int f(int a, int b) { return a - b; }\nvoid main() { println(\"{}\", f(-9223372036854775807, 2)); }",
        "int f(int a, int b) { return a * b; }\nvoid main() { println(\"{}\", f(4611686018427387904, 2)); }",
        "int f(int a, int b) { return a / b; }\nvoid main() { println(\"{}\", f(1, 0)); }",
        "int f(int a, int b) { return a % b; }\nvoid main() { println(\"{}\", f(1, 0)); }",
        "int f(int a, int b) { return a / b; }\nvoid main() { println(\"{}\", f(-9223372036854775807 - 1, -1)); }",
        "int f(int a) { return -a; }\nvoid main() { println(\"{}\", f(-9223372036854775807 - 1)); }",
        // Q0405 from native code, at the call that is one too deep.
        "int f(int n) { return f(n + 1) + 1; }\nvoid main() { println(\"{}\", f(0)); }",
    ];
    for src in errors {
        let out = same(src, Limits::CLI);
        assert!(out.starts_with("error\nerror[Q04"), "{src}\n{out}");
    }
    // Recursion up to the depth limit runs natively.
    let deep = "int down(int n) { if n == 0 { return 0; } return down(n - 1) + 1; }
void main() { println(\"{}\", down(198)); }";
    assert_eq!(same(deep, Limits::CLI), "ok\n198\n");
}

// Native code charges every step at the interpreter's span: for every budget from 0 to the
// program's total, all engines stop at the same place.
#[test]
fn every_step_limit_position_agrees_in_native_code() {
    let src = "int fib(int n) { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }
double mean(int a, int b) { var s = 0.0; for i in a..b { s = s + i; } return s / (b - a); }
void main() {
    int total = 0;
    for k in 0..3 {
        total = total + fib(k + 3);
    }
    println(\"{} {}\", total, mean(1, 5));
}
";
    let total = engine::with(Engine::Ast, || {
        let p = front(src).unwrap();
        let mut host = super::super::stdlib::LogOnly::default();
        super::super::interp::elaborate(&p, &mut host, &Limits::CLI)
            .unwrap()
            .usage
            .steps
    });
    assert!(total > 200, "{total}");
    for max in 0..=total + 1 {
        same(
            src,
            Limits {
                max_steps: max,
                ..Limits::CLI
            },
        );
    }
}

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

// Native code asks the host whether the run was cancelled at the interpreter's points.
#[test]
fn cancellation_polls_agree_in_native_code() {
    let src = "int work(int n) { int s = 0; for i in 0..n { for j in 0..n { s = (s + i * j) % 1000003; } } return s; }
void main() { println(\"{}\", work(300)); }";
    let p = front(src).unwrap();
    let polls = |e: Engine| {
        let mut host = Polls { n: Cell::new(0) };
        let r = engine::elaborate(&p, &mut host, &Limits::CLI, e).map(|e| e.usage.steps);
        (r.unwrap(), host.n.get())
    };
    let ast = polls(Engine::Ast);
    assert!(ast.1 > 100, "{ast:?}");
    assert_eq!(ast, polls(Engine::Ir));
    assert_eq!(ast, polls(Engine::Jit));
}

// A recursive function with `n` locals that stay live across its recursive call.
fn wide_recursion(n: usize, depth: usize) -> String {
    let mut s = String::from("int f(int n) {\n");
    for i in 0..n {
        s.push_str(&format!("    int a{i} = n + {i};\n"));
    }
    s.push_str("    if n == 0 {\n        return 0;\n    }\n    int s = f(n - 1);\n");
    for i in 0..n {
        s.push_str(&format!("    s = (s + a{i}) % 1000003;\n"));
    }
    s.push_str(&format!(
        "    return s;\n}}\nvoid main() {{\n    println(\"{{}}\", f({depth}));\n}}\n"
    ));
    s
}

fn on_stack<T: Send + 'static>(bytes: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

// The largest functions that still compile (`MAX_VALUES`) recurse to the depth limit in a stack
// far smaller than the interpreter thread's (`STACK_SIZE`, 256 MiB).
#[test]
fn deepest_recursion_of_the_widest_compiled_function_fits_the_stack() {
    let mut n = 64;
    while supported(&module(&wide_recursion(n * 2, 1)).funcs[0]) {
        n *= 2;
    }
    let src = wide_recursion(n, 198);
    let m = module(&src);
    assert!(Jit::new(&m).unwrap().compiles(m.index["f"]), "{n} locals");
    let s = src.clone();
    let want = on_stack(super::super::STACK_SIZE, move || {
        outputs(Engine::Ast, &s, Limits::CLI)
    });
    assert!(want.0.starts_with("ok\n"), "{}", want.0);
    let got = on_stack(16 << 20, move || outputs(Engine::Jit, &src, Limits::CLI));
    assert_eq!(want, got);
}

// Outside tests, short functions are interpreted until their 16th call; loops and direct
// recursion compile on the first call.
#[test]
fn short_functions_compile_once_they_are_hot() {
    let src = "int sq(int x) { return x * x; }
int sum(int n) { int t = 0; for i in 0..n { t = t + sq(i); } return t; }
int fact(int n) { if n < 2 { return 1; } return n * fact(n - 1); }
void main() { println(\"{} {} {}\", sum(3), sq(2), fact(5)); }";
    let m = module(src);
    set_hot_calls(DEFAULT_HOT_CALLS);
    let mut jit = Jit::new(&m).unwrap();
    let (sq, sum, fact) = (m.index["sq"], m.index["sum"], m.index["fact"]);
    for _ in 1..DEFAULT_HOT_CALLS {
        assert!(!jit.has(sq));
    }
    assert!(jit.has(sq), "compiled on call {DEFAULT_HOT_CALLS}");
    assert!(
        jit.has(sum) && jit.has(fact),
        "a loop and recursion compile at once"
    );
    set_hot_calls(1);
    same(src, Limits::CLI);
}

// After a failed compilation (a Cranelift bug), everything runs in the interpreter.
#[test]
fn a_failed_compilation_falls_back_to_the_interpreter() {
    let src = "int sq(int x) { return x * x; }
int sum(int n) { int t = 0; for i in 0..n { t = t + sq(i); } return t; }
void main() { println(\"{} {}\", sum(10), sq(7)); }";
    let m = module(src);
    let mut jit = Jit::new(&m).unwrap();
    jit.inject_failure = true;
    assert!(!jit.has(m.index["sum"]));
    assert!(!jit.has(m.index["sq"]), "nothing compiles after a failure");
    let mut jit = Jit::new(&m).unwrap();
    jit.inject_failure = true;
    let p = front(src).unwrap();
    let mut host = super::super::stdlib::LogOnly::default();
    let (e, _) = super::super::ir::exec::run_with(
        &m,
        "main",
        Vec::new(),
        &mut host,
        &Limits::CLI,
        Some(&mut jit),
    )
    .unwrap();
    let mut host = super::super::stdlib::LogOnly::default();
    let want = super::super::interp::elaborate(&p, &mut host, &Limits::CLI).unwrap();
    assert_eq!(format!("{:?}", e.output), format!("{:?}", want.output));
    assert_eq!(e.usage.steps, want.usage.steps);
}
