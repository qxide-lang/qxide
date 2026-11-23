// Regression tests for the resource limits in `Limits` (docs/security/EXECUTION_LIMITS_FIXES.md).
// Each test feeds the reproduction input of one finding; the ones that would run for minutes or
// grow without bound before the fix use tight limits and run under a watchdog so they fail fast.

use std::sync::mpsc;
use std::time::Duration;

use super::*;

/// Runs `f` on a thread with a large stack and fails if it does not finish within `secs`.
fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("did not finish within {secs} s"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the worker thread panicked"),
    }
}

fn codes(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|l| l.strip_prefix("error[")?.split_once(']'))
        .map(|(c, _)| c.to_string())
        .collect()
}

fn fails_with(result: Result<String, Failure>, code: &str) -> String {
    match result {
        Ok(out) => panic!("expected {code}, got {} bytes of output", out.len()),
        Err(Failure(d)) => {
            assert!(
                codes(&d).iter().any(|c| c == code),
                "expected {code}, got {:?}",
                codes(&d)
            );
            d
        }
    }
}

/// CLI limits with small budgets, so a test hits them in milliseconds.
fn small() -> Limits {
    Limits {
        max_steps: 1_000_000,
        max_ops: 100_000,
        max_sim_work: 100_000_000,
        max_output_bytes: 64 << 10,
        ..Limits::CLI
    }
}

fn run_limited(src: &str, shots: u64, limits: Limits) -> Result<String, Failure> {
    run_with("t.qx", src, shots, 0, limits)
}

fn qasm_limited(src: &str, limits: Limits) -> Result<String, Failure> {
    qasm_with("t.qx", src, &limits)
}

// ---- M-01: diagnostics ----

fn one_line_errors(n: usize) -> String {
    format!("void main(){{{}}}\n", "a;".repeat(n))
}

#[test]
fn m01_many_errors_on_one_line_render_a_bounded_report() {
    let Err(Failure(d)) = check("oneline_2000.qx", &one_line_errors(2000)) else {
        panic!("2000 unknown names must fail")
    };
    let reported = codes(&d);
    assert_eq!(reported.iter().filter(|c| *c == "Q0201").count(), 50);
    assert_eq!(
        reported.last().map(String::as_str),
        Some("Q0418"),
        "{reported:?}"
    );
    assert!(
        d.contains("1950 more errors"),
        "{}",
        &d[d.len().saturating_sub(400)..]
    );
    assert!(d.len() < 64 << 10, "{} bytes", d.len());
}

#[test]
fn m01_long_lines_are_cut_around_the_error_column() {
    let pad = " ".repeat(5000);
    let src = format!("void main(){{{pad}nope;{pad}}}\n");
    let Err(Failure(d)) = check("long.qx", &src) else {
        panic!("unknown name must fail")
    };
    assert!(d.contains("--> long.qx:1:5013"), "{d}");
    let lines: Vec<&str> = d.lines().collect();
    assert!(lines.iter().all(|l| l.chars().count() <= 140), "{d}");
    let echo = lines
        .iter()
        .find(|l| l.starts_with("1 | "))
        .expect("echoed line");
    let caret = lines.iter().find(|l| l.contains('^')).expect("caret line");
    let at = echo["1 | ".len()..].find("nope").expect("token is shown");
    assert_eq!(caret["  | ".len()..].find('^'), Some(at), "{echo}\n{caret}");
    assert_eq!(caret.matches('^').count(), 4, "{caret}");
}

#[test]
fn m01_lines_within_the_limit_are_echoed_unchanged() {
    let src = "void main() {\n    nope;\n}\n";
    let Err(Failure(d)) = check("t.qx", src) else {
        panic!("unknown name must fail")
    };
    assert!(d.contains("2 |     nope;\n  |     ^^^^\n"), "{d}");
}

#[test]
fn m01_rendered_diagnostics_stop_at_the_byte_limit() {
    // Each message quotes the 100 kB name, so even a few diagnostics exceed 4 KiB.
    let name = "a".repeat(100_000);
    let src = format!("void main() {{\n    {name};\n    {name};\n}}\n");
    let limits = Limits {
        max_output_bytes: 4096,
        ..Limits::CLI
    };
    let Err(Failure(d)) = check_with("t.qx", &src, &limits) else {
        panic!("unknown names must fail")
    };
    assert!(d.len() <= 4096 + 512, "{} bytes", d.len());
    assert_eq!(codes(&d).last().map(String::as_str), Some("Q0418"), "{d}");
}

// ---- B-01: steps ----

#[test]
fn b01_endless_loop_stops_at_the_step_limit() {
    let src = "void main() {\n    for i in 0..9223372036854775807 {\n    }\n}\n";
    within(20, move || {
        fails_with(run_limited(src, 1, small()), "Q0413")
    });
    within(20, move || fails_with(qasm_limited(src, small()), "Q0413"));
}

#[test]
fn b01_exponential_recursion_stops_at_the_step_limit() {
    let src = "int f(int n) {\n    if (n == 0) { return 1; }\n    return f(n - 1) + f(n - 1);\n}\nvoid main() {\n    println(\"{}\", f(62));\n}\n";
    within(20, move || {
        fails_with(run_limited(src, 1, small()), "Q0413")
    });
}

// ---- B-02: circuit operations and output ----

#[test]
fn b02_println_in_a_loop_stops_at_the_output_limit() {
    // The report's loop runs to i64::MAX; this bound keeps the run finite without the fix.
    let src = format!(
        "void main() {{\n    for i in 0..200000 {{\n        println(\"{}\");\n    }}\n}}\n",
        "x".repeat(100)
    );
    within(60, move || {
        fails_with(run_limited(&src, 1, small()), "Q0415")
    });
}

#[test]
fn b02_gates_in_a_loop_stop_at_the_operation_limit() {
    let src = "void main() {\n    Qubit q = qubit();\n    for i in 0..2000000 {\n        h(q);\n    }\n    discard(q);\n}\n";
    within(60, move || {
        fails_with(run_limited(src, 1, small()), "Q0414")
    });
    within(60, move || fails_with(qasm_limited(src, small()), "Q0414"));
}

// ---- B-03: multi-shot output ----

#[test]
fn b03_multi_shot_output_stops_at_the_output_limit() {
    // The report's program has 17 qubits, 100 lines and 100000 shots (about 10 GB); this one
    // would print about 10 MB without the limit.
    let src = format!(
        "void main() {{\n    QReg r = qreg(10);\n    for i in 0..10 {{ h(r[i]); }}\n    BitReg b = measure(r);\n    for i in 0..10 {{\n        println(\"{}\");\n    }}\n    println(\"{{}}\", b);\n}}\n",
        "y".repeat(1000)
    );
    let limits = Limits {
        max_output_bytes: 1 << 20,
        ..Limits::CLI
    };
    within(60, move || {
        fails_with(run_limited(&src, 2000, limits), "Q0415")
    });
}

#[test]
fn b03_many_shots_with_few_records_still_run() {
    let src = "void main() {\n    Qubit a = qubit();\n    Qubit b = qubit();\n    h(a);\n    cx(a, b);\n    println(\"{} {}\", measure(a), measure(b));\n}\n";
    let out = within(60, move || run_limited(src, 100_000, small()))
        .unwrap_or_else(|Failure(d)| panic!("{d}"));
    assert_eq!(out.lines().filter(|l| l.contains('×')).count(), 2, "{out}");
}

// ---- B-08: simulation work ----

fn ghz(n: usize) -> String {
    format!("void main() {{\n    int n = {n};\n    QReg r = qreg(n);\n    h(r[0]);\n    for i in 1..n {{\n        cx(r[0], r[i]);\n    }}\n    BitReg m = measure(r);\n    println(\"{{}}\", m[0]);\n}}\n")
}

#[test]
fn b08_huge_stabilizer_run_is_refused_before_simulating() {
    let src = ghz(32768);
    within(30, move || {
        fails_with(run_limited(&src, 1, Limits::CLI), "Q0417")
    });
}

#[test]
fn b08_shot_count_is_part_of_the_simulation_budget() {
    let src =
        "void main() {\n    Qubit a = qubit();\n    h(a);\n    println(\"{}\", measure(a));\n}\n";
    within(30, move || {
        fails_with(run_limited(src, 1_000_000_000_000, Limits::CLI), "Q0417")
    });
}

// ---- B-09: register indexing ----

#[test]
fn b09_indexing_a_large_register_does_not_copy_it() {
    // About 86 s before the fix (each `r[i]` copied the whole register).
    let src = ghz(131072);
    let out = within(30, move || qasm_limited(&src, Limits::CLI))
        .unwrap_or_else(|Failure(d)| panic!("{d}"));
    assert!(out.contains("cx q[0], q[131071];"));
}

#[test]
fn b09_register_arguments_are_charged_per_qubit() {
    let src = "void f(QReg r) {\n    h(r[0]);\n}\nvoid main() {\n    QReg r = qreg(100000);\n    for i in 0..1000000 {\n        f(r);\n    }\n    discard(r);\n}\n";
    within(30, move || fails_with(qasm_limited(src, small()), "Q0413"));
}

// ---- M-10: conditional nesting ----

fn nest(calls: usize, levels: usize) -> String {
    format!(
        "void f(Qubit q, Bit b, int n){{{}if(n>0){{f(q,b,n-1);}}x(q);{}}}\nvoid main(){{\n Qubit a=qubit(); h(a); Bit b=measure(a);\n Qubit q=qubit();\n f(q,b,{calls});\n Bit r=measure(q);\n println(\"{{}}\", r);\n}}\n",
        "if(b){".repeat(levels),
        "}".repeat(levels)
    )
}

#[test]
fn m10_conditionals_nested_across_calls_stop_at_the_depth_limit() {
    let src = nest(198, 61);
    let d = within(30, {
        let src = src.clone();
        move || fails_with(qasm_limited(&src, Limits::CLI), "Q0416")
    });
    assert!(d.contains("64"), "{d}");
    within(30, move || {
        fails_with(run_limited(&src, 1, Limits::CLI), "Q0416")
    });
}

#[test]
fn m10_nesting_within_one_function_stays_valid() {
    // The parser admits about 61 levels in one function; the limit keeps all of them valid.
    let src = nest(0, 61);
    within(30, move || qasm_limited(&src, Limits::CLI)).unwrap_or_else(|Failure(d)| panic!("{d}"));
}

#[test]
fn m10_qasm_text_stops_at_the_output_limit() {
    let src = "void main() {\n    Qubit q = qubit();\n    for i in 0..10000 {\n        h(q);\n    }\n    println(\"{}\", measure(q));\n}\n";
    let limits = Limits {
        max_output_bytes: 4096,
        ..Limits::CLI
    };
    fails_with(qasm_limited(src, limits), "Q0415");
    // `run` prints one line, so the same program runs under the same limits.
    run_limited(src, 1, limits).unwrap_or_else(|Failure(d)| panic!("{d}"));
}

// ---- String values and std results (app framework) ----
// Building, comparing, measuring and passing text is work in proportion to its length, so it is
// charged to the step budget per byte; reading a `String` variable shares the text.

/// `String s` of `2^doublings` bytes, built by repeated concatenation.
fn doubled(doublings: usize) -> String {
    format!("String s = \"x\";\n    for i in 0..{doublings} {{\n        s = s + s;\n    }}\n")
}

/// Budgets that admit building a 1 MiB string but not repeating work on it a thousand times.
fn medium() -> Limits {
    Limits {
        max_steps: 1 << 26,
        ..Limits::CLI
    }
}

#[test]
fn str_doubling_stops_at_the_step_limit() {
    let src = format!(
        "void main() {{\n    {}    println(\"{{}}\", len(s));\n}}\n",
        doubled(24)
    );
    within(20, move || {
        fails_with(run_limited(&src, 1, small()), "Q0413")
    });
}

#[test]
fn str_variable_reads_do_not_copy_the_text() {
    let src = format!(
        "void main() {{\n    {}    for i in 0..200000 {{\n        String t = s;\n    }}\n    println(\"done\");\n}}\n",
        doubled(20)
    );
    let out = within(20, move || run_limited(&src, 1, medium()))
        .unwrap_or_else(|Failure(d)| panic!("{d}"));
    assert_eq!(out, "done\n");
}

#[test]
fn str_len_is_charged_per_byte() {
    let src = format!(
        "void main() {{\n    {}    int n = 0;\n    for i in 0..1000 {{\n        n = len(s);\n    }}\n    println(\"{{}}\", n);\n}}\n",
        doubled(20)
    );
    within(20, move || {
        fails_with(run_limited(&src, 1, medium()), "Q0413")
    });
}

#[test]
fn str_comparison_is_charged_per_byte() {
    let src = format!(
        "void main() {{\n    {}    String t = s + \"\";\n    boolean same = false;\n    for i in 0..1000 {{\n        same = s == t;\n    }}\n    println(\"{{}}\", same);\n}}\n",
        doubled(20)
    );
    within(20, move || {
        fails_with(run_limited(&src, 1, medium()), "Q0413")
    });
}

#[test]
fn str_literals_are_charged_each_time_they_are_built() {
    let src = format!(
        "void main() {{\n    for i in 0..100000 {{\n        String t = \"{}\";\n    }}\n}}\n",
        "x".repeat(1000)
    );
    within(20, move || {
        fails_with(run_limited(&src, 1, small()), "Q0413")
    });
}

/// `std.fs` that never touches the disk: `readString` returns 1 MiB, the rest succeed.
struct BigFiles;

impl stdlib::Host for BigFiles {
    fn call(&mut self, _: &str, function: &str, _: Vec<stdlib::HostValue>) -> stdlib::StdResult {
        Ok(match function {
            "readString" => stdlib::HostValue::Str("y".repeat(1 << 20)),
            "exists" => stdlib::HostValue::Bool(true),
            _ => stdlib::HostValue::Unit,
        })
    }
}

fn run_with_files(body: &str, limits: Limits) -> Result<String, Failure> {
    let src = format!("import std.fs;\n\nvoid main() {{\n    {body}\n}}\n");
    let mut input = Input::file("t.qx", &src);
    input.perms = stdlib::Permissions::cli(true);
    run_input_with(&input, 1, 0, limits, &mut BigFiles)
}

#[test]
fn std_results_are_charged_per_byte() {
    let body =
        "String t = \"\";\n    for i in 0..1000 {\n        t = fs.readString(\"a.txt\");\n    }";
    within(20, move || {
        fails_with(run_with_files(body, medium()), "Q0413")
    });
    // One read of the same file is well within the budget.
    within(20, move || {
        run_with_files("String t = fs.readString(\"a.txt\");", medium())
    })
    .unwrap_or_else(|Failure(d)| panic!("{d}"));
}

#[test]
fn std_arguments_are_charged_per_byte() {
    let body = format!(
        "{}    for i in 0..1000 {{\n        fs.writeString(\"a.txt\", s);\n    }}",
        doubled(20)
    );
    within(20, move || {
        fails_with(run_with_files(&body, medium()), "Q0413")
    });
}

// D7: `--trusted` raises only the execution budgets; the depth limits that `STACK_SIZE` is sized
// for, and the diagnostic limits, stay those of the CLI.
#[test]
fn trusted_limits_raise_budgets_only() {
    let (t, c) = (Limits::TRUSTED, Limits::CLI);
    assert!(t.max_steps > c.max_steps && t.max_ops > c.max_ops);
    assert!(t.max_sim_work > c.max_sim_work && t.max_output_bytes > c.max_output_bytes);
    assert_eq!(t.max_cond_depth, c.max_cond_depth);
    assert_eq!(t.max_diagnostics, c.max_diagnostics);
    assert_eq!(t.max_line_chars, c.max_line_chars);
    assert_eq!(t.max_state_vector_qubits, c.max_state_vector_qubits);
    assert_eq!(t.stabilizer_bytes, c.stabilizer_bytes);
    assert!(t.scope == LimitScope::Cli);
}
