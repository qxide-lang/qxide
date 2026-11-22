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
