// What normal desktop-app work uses of each `Limits` budget, measured headlessly: the Qxide Studio
// examples run as Studio runs them (up to its 1,000,000-shot field), the Studio and bell-app command
// modules with their std calls, and a mid-edit version of each example for the diagnostics count.
// `cargo test app_limits -- --nocapture` prints the table recorded in part 2 of
// docs/security/EXECUTION_LIMITS_FIXES.md; the test fails if `Limits::APP` loses that headroom.

use super::interp::Value;
use super::stdlib::{Host, HostValue, LogOnly, Permissions, StdResult};
use super::*;

const EXAMPLES: [(&str, &str); 7] = [
    ("bell", include_str!("../../examples/qx/bell.qx")),
    (
        "conditional_fn",
        include_str!("../../examples/qx/conditional_fn.qx"),
    ),
    ("functions", include_str!("../../examples/qx/functions.qx")),
    ("ghz", include_str!("../../examples/qx/ghz.qx")),
    ("rotation", include_str!("../../examples/qx/rotation.qx")),
    ("teleport", include_str!("../../examples/qx/teleport.qx")),
    (
        "teleport_fn",
        include_str!("../../examples/qx/teleport_fn.qx"),
    ),
];
const STUDIO: &str = include_str!("../../apps/studio/src/app.qx");
const BELL_APP: &str = include_str!("../../examples/apps/bell-app/src/app.qx");
// Most shots Studio's shots field accepts.
const STUDIO_MAX_SHOTS: u64 = 1_000_000;

#[derive(Debug, Default, Clone, Copy)]
struct Peak {
    steps: u64,
    ops: u64,
    out_bytes: u64,
    qasm_bytes: u64,
    sim_work: u128,
    diagnostics: usize,
    line_chars: usize,
    cond_depth: usize,
}

impl Peak {
    fn usage(&mut self, e: &Elaborated) {
        self.steps = self.steps.max(e.usage.steps);
        self.ops = self.ops.max(e.usage.ops);
        self.out_bytes = self.out_bytes.max(e.usage.out_bytes);
        self.qasm_bytes = self.qasm_bytes.max(e.qasm_bytes);
        self.cond_depth = self.cond_depth.max(e.usage.cond_depth);
    }

    fn source(&mut self, src: &str) {
        let longest = src.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        self.line_chars = self.line_chars.max(longest);
    }
}

fn input<'a>(src: &'a str, perms: Permissions) -> Input<'a> {
    let mut input = Input::file("app.qx", src);
    input.perms = perms;
    input
}

/// A program as Studio runs it: built, then sampled with `shots` (`std.qxide.run`).
fn studio_program(peak: &mut Peak, name: &str, src: &str, shots: u64) {
    let input = input(src, Permissions::playground());
    let (_, built) = build_input(&input, &mut LogOnly::default(), &Limits::APP);
    let (_, e) = built.unwrap_or_else(|d| panic!("{name}: {d:?}"));
    peak.usage(&e);
    peak.source(src);
    let work = shot_work(e.circuit.select_backend(), e.slots, &e.counts) * u128::from(shots);
    peak.sim_work = peak.sim_work.max(work);
    let (_, run) = run_diag(&input, shots, 7, Limits::APP, &mut LogOnly::default());
    let out = run.unwrap_or_else(|d| panic!("{name} x{shots}: {d:?}"));
    peak.out_bytes = peak.out_bytes.max(out.text.len() as u64);
}

/// `std` for command modules: `std.qxide` is Studio's real implementation, `std.fs` holds one file.
struct AppStd {
    file: String,
}

impl Host for AppStd {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        Ok(match (module, function) {
            ("qxide", f) => return studio::call(f, &args, &|| false),
            ("fs", "readString") => HostValue::Str(self.file.clone()),
            ("fs", "exists") => HostValue::Bool(true),
            ("time", "nowIso") => HostValue::Str("2026-11-21T00:00:00.000Z".into()),
            _ => HostValue::Unit,
        })
    }
}

/// One command call's elaboration (std effects included), as `command::call` runs it.
fn command(
    peak: &mut Peak,
    module: &str,
    perms: &[&str],
    name: &str,
    args: Vec<Value>,
    file: &str,
) {
    let perms: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    let input = input(module, Permissions::app(&perms));
    let (_, program) = front_input(&input, false);
    let program = program.unwrap_or_else(|d| panic!("{name}: {d:?}"));
    let mut host = AppStd { file: file.into() };
    let (e, _) = super::engine::elaborate_call(&program, name, args, &mut host, &Limits::APP)
        .unwrap_or_else(|d| panic!("{name}: {d:?}"));
    peak.usage(&e);
    peak.source(module);
}

fn text(s: &str) -> Value {
    Value::Str(s.into())
}

fn measure() -> Peak {
    let mut peak = Peak::default();
    for (name, src) in EXAMPLES {
        for shots in [1, 1000, STUDIO_MAX_SHOTS] {
            studio_program(&mut peak, name, src, shots);
        }
        // An edit in progress: every `;` missing.
        let broken = src.replace(';', "");
        let (_, r) = front_input(&input(&broken, Permissions::playground()), true);
        peak.diagnostics = peak.diagnostics.max(r.err().map_or(0, |d| d.len()));
        // Studio's own commands, with the example as the editor buffer.
        let studio = ["log", "fs", "app", "qxide"];
        let run = vec![text(src), Value::Int(1000), Value::Int(0)];
        command(&mut peak, STUDIO, &studio, "run", run, "");
        for f in ["check", "qasm", "format", "save"] {
            command(&mut peak, STUDIO, &studio, f, vec![text(src)], "");
        }
        command(&mut peak, STUDIO, &studio, "load", vec![], src);
        command(&mut peak, STUDIO, &studio, "set_file", vec![text(name)], "");
    }
    let bell = ["log", "time", "fs"];
    command(&mut peak, BELL_APP, &bell, "measure_bell", vec![], "");
    let saved = vec![text("00: 503, 11: 497")];
    command(&mut peak, BELL_APP, &bell, "save_result", saved, "");
    // A results file after a thousand saves.
    let history = "2026-11-21T00:00:00.000Z  00: 503, 11: 497\n".repeat(1000);
    command(&mut peak, BELL_APP, &bell, "history", vec![], &history);
    peak
}

#[test]
fn app_limits_keep_their_headroom_over_normal_use() {
    let p = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(measure)
        .unwrap()
        .join()
        .unwrap();
    let a = Limits::APP;
    let rows = [
        ("max_steps", p.steps as f64, a.max_steps as f64),
        ("max_ops", p.ops as f64, a.max_ops as f64),
        ("max_sim_work", p.sim_work as f64, a.max_sim_work as f64),
        (
            "max_output_bytes (run)",
            p.out_bytes as f64,
            a.max_output_bytes as f64,
        ),
        (
            "max_output_bytes (qasm)",
            p.qasm_bytes as f64,
            a.max_output_bytes as f64,
        ),
        (
            "max_diagnostics",
            p.diagnostics as f64,
            a.max_diagnostics as f64,
        ),
        (
            "max_line_chars",
            p.line_chars as f64,
            a.max_line_chars as f64,
        ),
        (
            "max_cond_depth",
            p.cond_depth as f64,
            a.max_cond_depth as f64,
        ),
    ];
    println!("| Limit | Highest normal use | Limits::APP | Headroom |");
    for (name, used, limit) in rows {
        println!(
            "| {name} | {used} | {limit} | {:.0}x |",
            limit / used.max(1.0)
        );
    }
    // Budgets that grow with the work get at least 10x; the structural caps cover every measured case.
    for (name, used, limit) in &rows[..5] {
        assert!(used * 10.0 <= *limit, "{name}: {used} of {limit}");
    }
    for (name, used, limit) in &rows[5..] {
        assert!(used <= limit, "{name}: {used} of {limit}");
    }
}

// Worst cases at `Limits::APP`, for the timings in EXECUTION_LIMITS_FIXES.md. Run one at a time in a
// release build and read the process's peak memory from outside:
// `cargo test --release --bin qxide app_worst_steps -- --ignored --nocapture`.
fn worst(name: &str, src: &str, shots: u64, code: &str) {
    let start = std::time::Instant::now();
    // The recorded timings are the tree-walking interpreter's (tests default to both engines).
    let r = engine::with(Engine::Ast, || run_with("t.qx", src, shots, 0, Limits::APP));
    let secs = start.elapsed().as_secs_f64();
    let got = match &r {
        Ok(out) => format!("ok, {} bytes", out.len()),
        Err(Failure(d)) => d.lines().next().unwrap_or("").to_string(),
    };
    println!(
        "{name}: {got} in {secs:.2} s, peak working set {}",
        peak_memory()
    );
    assert!(got.contains(code), "{name}: expected {code}, got {got}");
}

// The process's peak working set (Windows; elsewhere not measured).
#[cfg(windows)]
fn peak_memory() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        rest: [usize; 7],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: `c` is a PROCESS_MEMORY_COUNTERS of the size passed in `cb`.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 {
        return "unknown".into();
    }
    format!("{} MiB", c.peak_working_set >> 20)
}

#[cfg(not(windows))]
fn peak_memory() -> String {
    "not measured".into()
}

fn on_big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "about two minutes; measures the step limit"]
fn app_worst_steps() {
    on_big_stack(|| {
        let src = "void main() {\n    for i in 0..9000000000000000000 {\n    }\n}\n";
        worst("empty loop", src, 1, "Q0413");
    });
}

#[test]
#[ignore = "measures string memory at the step limit"]
fn app_worst_strings() {
    on_big_stack(|| {
        let src = "void main() {\n    String s = \"x\";\n    for i in 0..60 {\n        s = s + s;\n    }\n    println(\"{}\", len(s));\n}\n";
        worst("string doubling", src, 1, "Q0413");
    });
}

#[test]
#[ignore = "measures circuit memory at the operation limit"]
fn app_worst_ops() {
    on_big_stack(|| {
        let src = "void main() {\n    Qubit q = qubit();\n    for i in 0..100000000 {\n        h(q);\n    }\n    println(\"{}\", measure(q));\n}\n";
        worst("gates in a loop", src, 1, "Q0414");
    });
}

#[test]
#[ignore = "measures program output at the output limit"]
fn app_worst_output() {
    on_big_stack(|| {
        let src = "void main() {\n    for i in 0..100000000 {\n        println(\"0123456789012345678901234567890123456789\");\n    }\n}\n";
        worst("println in a loop", src, 1, "Q0415");
    });
}

#[test]
#[ignore = "about three minutes; one uncancellable shot just under the work limit"]
fn app_worst_sim_work() {
    on_big_stack(|| {
        // 2^24 amplitudes x (4070 + 24 + 1) operations + 128 stays just under 2^36.
        let src = "void main() {\n    QReg q = qreg(24);\n    for i in 0..4070 {\n        t(q[0]);\n    }\n    BitReg b = measure(q);\n    println(\"{}\", b[0]);\n}\n";
        worst("24 qubits x 4094 operations", src, 1, "ok");
        let over = src.replace("0..4070", "0..4071");
        worst("one operation more", &over, 1, "Q0417");
    });
}
