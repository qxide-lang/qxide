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
    for f in [
        "fib", "half", "odd", "nothing", "viaFib", "name", "usesName",
    ] {
        assert!(jit.compiles(id(f)), "{f}");
    }
    for f in ["gate", "main"] {
        assert!(!jit.compiles(id(f)), "{f}");
    }
    // Compiling one function compiles the functions it calls too.
    assert!(jit.has(id("viaFib")));
    assert!(jit.entries[id("fib") as usize].is_some());
    assert!(jit.entries[id("half") as usize].is_none());
    assert_eq!(same(src, Limits::CLI), "ok\n6 1.5 true 2 1 2\n");
}

// `while`, `break`, `continue` and compound assignment are plain jumps, so such functions compile.
// `match` on `int`, `boolean` and enum values compiles (enum tags and payloads through `qx_heap`).
#[test]
fn int_and_enum_matches_compile() {
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
    assert!(jit.compiles(m.index["unwrap"]));
    assert_eq!(same(src, Limits::CLI), "ok\n10 20 7 3\n");
}

// Instances of generic functions are ordinary functions, so they compile like the others.
#[test]
fn numeric_instances_of_generic_functions_compile() {
    let src = "<T> T pick(boolean c, T a, T b) {
    if c {
        return a;
    }
    return b;
}
<T> T id(T x) {
    return pick(true, x, x);
}
void main() {
    println(\"{} {} {}\", id(3), pick(false, 1, 2.5), id(\"s\"));
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    for f in [
        "pick<int>",
        "id<int>",
        "pick<double>",
        "id<String>",
        "pick<String>",
    ] {
        assert!(jit.compiles(m.index[f]), "{f}");
    }
    assert_eq!(same(src, Limits::CLI), "ok\n3 2.5 s\n");
}

// Methods with a struct or enum receiver compile; functions that call through an interface
// stay interpreted, and dynamic calls run the same in every engine.
#[test]
fn methods_and_interface_calls_run_alongside_native_code() {
    let src = super::super::tests::INTERFACES;
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    let flags: Vec<(&str, bool)> = ["Rect.area", "Light.name", "total<Shape>", "max_of<Money>"]
        .iter()
        .map(|f| (*f, jit.compiles(m.index[*f])))
        .collect();
    // `max_of<Money>` calls `Money.compare` directly once instantiated.
    assert_eq!(
        flags,
        vec![
            ("Rect.area", true),
            ("Light.name", true),
            ("total<Shape>", false),
            ("max_of<Money>", true)
        ]
    );
    let out = same(src, Limits::CLI);
    assert!(out.contains("all: 27.0"), "{out}");
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

// Stage 3c: `std.math` calls run in native code through `qx_math`, with the interpreters'
// results (NaN, infinities, `-0.0`), errors (`powInt` overflow) and step positions; functions
// that call other std modules stay interpreted.
#[test]
fn std_math_calls_compile() {
    let src = "import std.math;
import std.log;
double wave(double x) { return math.sin(x) * math.exp(-x) + math.hypot(x, 1.0) + math.pow(x, 0.5); }
int ipow(int b, int e) { return math.powInt(b, e) + math.absInt(-b) + math.maxInt(b, e); }
boolean odd(double x) { return math.isNaN(x) || math.isFinite(x) && math.floor(x) != x; }
double edges(double x) { return math.sqrt(x) + math.log(x) + math.min(x, -0.0) + math.atan2(x, -1); }
int note(int n) { log.info(\"n\"); return n; }
void main() {
    double s = 0.0;
    for i in 0..20 {
        s += wave(i * 0.25);
    }
    println(\"{} {} {} {}\", math.round(s * 1e9) / 1e9, ipow(3, 5), odd(2.5), odd(0.0 / 0.0));
    println(\"{} {} {}\", edges(0.0), edges(-1.0), edges(4.0));
    println(\"{}\", ipow(2, 63) > 0);
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    let id = |n: &str| m.index[n];
    for f in ["wave", "ipow", "odd", "edges"] {
        assert!(jit.compiles(id(f)), "{f}");
    }
    assert!(!jit.compiles(id("note")));
    let out = same(src, Limits::CLI);
    assert!(out.starts_with("error\n"), "{out}");
    assert!(out.contains("Q0401"), "{out}");
    let ok = src.replace("println(\"{}\", ipow(2, 63) > 0);", "");
    let out = same(&ok, Limits::CLI);
    assert!(out.starts_with("ok\n"), "{out}");
    // Every step limit stops native code where the interpreters stop.
    let total = engine::with(Engine::Ast, || {
        let p = front(&ok).unwrap();
        let mut host = super::super::stdlib::LogOnly::default();
        super::super::interp::elaborate(&p, &mut host, &Limits::CLI)
            .unwrap()
            .usage
            .steps
    });
    for max in (0..=total + 1).step_by(5).chain([total - 1, total]) {
        same(
            &ok,
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

// Bit operators compile to native instructions; a shift amount outside 0..=63 stops the run at
// the same place with the same error as in the interpreters.
#[test]
fn bit_operators_compile() {
    let src = "int mix(int x, int k) { return (x << 3 ^ x >>> 2 | ~x & 255) >> k; }
int pop(int x) { int n = 0; int v = x; while v != 0 { n += v & 1; v = v >>> 1; } return n; }
void main() {
    println(\"{} {} {} {}\", mix(12345, 1), mix(-7, 63), pop(-1), pop(1 << 40));
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    for f in ["mix", "pop"] {
        assert!(jit.compiles(m.index[f]), "{f}");
    }
    assert_eq!(same(src, Limits::CLI), "ok\n50915 -1 64 1\n");
    let bad = "int sh(int x, int k) { return x << k; }
void main() {
    println(\"{}\", sh(1, 3) + sh(1, 64));
}
";
    let out = same(bad, Limits::CLI);
    assert!(out.contains("Q0420"), "{out}");
}

// Roadmap 3c: strings, arrays, structs, enums and `List`/`Map` in native code, against both
// interpreters (results, errors and every step-limit position).
#[test]
fn reference_values_compile() {
    let src = "struct P {
    double x;
    int[] hits;
}
enum Shape { Circle(double r), Square(double side), Dot }
int sieve(int n) {
    boolean[] c = new boolean[n + 1];
    int count = 0;
    for i in 2..n + 1 {
        if !c[i] {
            count += 1;
            int j = i * i;
            while j <= n {
                c[j] = true;
                j += i;
            }
        }
    }
    return count;
}
double area(Shape s) {
    match s {
        Circle(r) => {
            return 3.0 * r * r;
        }
        Square(side) => {
            return side * side;
        }
        _ => {
            return 0.0;
        }
    }
}
P step(P p, int k) {
    p.x += 0.5 * k;
    p.hits[k % len(p.hits)] += 1;
    return p;
}
String label(int n) {
    String s = \"n\";
    for i in 0..n {
        s = s + i % 10;
    }
    return s + \"/\" + s.length();
}
int grid(int n) {
    int[][] g = new int[n][n];
    for i in 0..n {
        for j in 0..n {
            g[i][j] = i * j;
        }
    }
    int[] row = g[n - 1];
    return row[n - 1] + len(g);
}
int listed(int n) {
    List<int> xs = new List<int>();
    for i in 0..n {
        xs.add(i * i);
    }
    return xs.size() + xs.get(n - 1);
}
void main() {
    P p = new P(0.0, new int[3]);
    for k in 0..7 {
        p = step(p, k);
    }
    double total = area(Shape.Circle(1.5)) + area(Shape.Square(2.0)) + area(Shape.Dot);
    println(\"{} {} {} {}\", sieve(500), p.x, p.hits[0] + p.hits[1] * 10 + p.hits[2] * 100, total);
    println(\"{} {} {}\", label(12), grid(9), listed(20));
}
";
    let m = module(src);
    let jit = Jit::new(&m).expect("Cranelift supports this machine");
    for f in ["sieve", "area", "step", "label", "grid", "listed"] {
        assert!(jit.compiles(m.index[f]), "{f}");
    }
    assert!(!jit.compiles(m.index["main"]));
    let out = same(src, Limits::CLI);
    assert_eq!(out, "ok\n95 10.5 223 10.75\nn012345678901/13 73 381\n");
    let total = engine::with(Engine::Ast, || {
        let p = front(src).unwrap();
        let mut host = super::super::stdlib::LogOnly::default();
        super::super::interp::elaborate(&p, &mut host, &Limits::CLI)
            .unwrap()
            .usage
            .steps
    });
    for max in (0..=total + 1).step_by(101).chain([total - 1, total]) {
        same(
            src,
            Limits {
                max_steps: max,
                ..Limits::CLI
            },
        );
    }
    // Run-time errors inside native code: an index, overflow in a compound store, a string limit.
    for (bad, code) in [
        ("int f(int[] a) { return a[3]; } void main() { println(\"{}\", f(new int[3])); }", "Q0403"),
        ("void f(int[] a) { a[0] += 9223372036854775807; } void main() { int[] a = {1}; f(a); println(\"{}\", a[0]); }", "Q0401"),
        ("int f(int n) { int[] a = new int[n]; return len(a); } void main() { println(\"{}\", f(-1)); }", "Q04"),
        ("String f(String s) { for i in 0..80 { s = s + s; } return s; } void main() { println(\"{}\", f(\"ab\").length()); }", "Q04"),
    ] {
        let out = same(bad, Limits::CLI);
        assert!(out.starts_with("error\n") && out.contains(code), "{bad}\n{out}");
    }
}

// Every count native code takes of a reference is given back: on return, on a run-time error,
// and for values it stored in structs or handed to other native functions.
#[test]
fn reference_counts_balance_in_native_code() {
    use super::super::array::ArrayData;
    use std::cell::RefCell;
    let src = "struct Box2 { int[] xs; String s; }
int[] keep(int[] a, String t, int n) {
    int[] b = a;
    Box2 box = new Box2(b, t + n);
    int[] c = box.xs;
    if n > 0 {
        c[0] = n;
        return keep(c, box.s, n - 1);
    }
    return c;
}
int fails(int[] a, String t) {
    Box2 box = new Box2(a, t);
    int[] b = box.xs;
    return b[5] + len(t);
}
int[] fresh(int n) {
    int[] a = new int[n];
    int[] b = a;
    return b;
}
String echo(String s) {
    return s;
}
int[] churn(int[] a, String t, int n) {
    int[] cur = a;
    String s = t;
    for i in 0..n {
        cur = fresh(1);
        cur = a;
        s = s + \"x\";
        fresh(2);
        echo(s);
        if i == 3 {
            break;
        }
    }
    for i in 0..n {
        Box2 b = new Box2(cur, s);
        if i == 2 {
            return b.xs;
        }
    }
    return a;
}
void main() {
    int[] a = new int[2];
    println(\"{} {}\", keep(a, \"t\", 3)[0], fresh(2)[0] + fails(a, \"t\") + len(churn(a, \"t\", 9)));
}
";
    let m = module(src);
    let mut jit = Jit::new(&m).expect("Cranelift supports this machine");
    let id = |n: &str| m.index[n];
    for f in ["keep", "fails", "fresh", "churn"] {
        assert!(jit.has(id(f)), "{f}");
    }
    let arr = Rc::new(RefCell::new(ArrayData::Int(vec![0, 0])));
    let text: Rc<str> = Rc::from("t");
    let mut host = super::super::stdlib::LogOnly::default();
    let mut machine = Machine::new(&mut host, &Limits::CLI, m.layouts.clone());
    let args = || {
        vec![
            Value::Array(arr.clone()),
            Value::Str(text.clone()),
            Value::Int(3),
        ]
    };
    let back = jit.call(id("keep"), args(), &mut machine).unwrap();
    let Value::Array(same_arr) = back else {
        panic!("{back:?}");
    };
    assert!(Rc::ptr_eq(&same_arr, &arr));
    drop(same_arr);
    assert_eq!(Rc::strong_count(&arr), 1);
    assert_eq!(Rc::strong_count(&text), 1);
    let err = jit
        .call(id("fails"), args()[..2].to_vec(), &mut machine)
        .unwrap_err();
    assert_eq!(err.code, Code::IndexOutOfBounds);
    assert_eq!(Rc::strong_count(&arr), 1);
    assert_eq!(Rc::strong_count(&text), 1);
    let Value::Array(made) = jit
        .call(id("fresh"), vec![Value::Int(4)], &mut machine)
        .unwrap()
    else {
        panic!("not an array");
    };
    assert_eq!(Rc::strong_count(&made), 1);
    assert_eq!(made.borrow().len(), 4);
    // Reassigned in a loop, left by `break` and by `return` inside a loop, results discarded.
    let back = jit.call(id("churn"), args(), &mut machine).unwrap();
    assert!(matches!(&back, Value::Array(r) if Rc::ptr_eq(r, &arr)));
    drop(back);
    assert_eq!(Rc::strong_count(&arr), 1);
    assert_eq!(Rc::strong_count(&text), 1);
    assert!(same(src, Limits::CLI).contains("Q0403"));
}
