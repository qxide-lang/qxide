// Benchmark suite for the `.qx` pipeline (docs/perf/QX_BENCHMARKS.md). Ignored by default; run
// with an optimized build:
//   cargo test --release --bin qxide bench_suite -- --ignored --nocapture
// Environment: QXIDE_BENCH_REPEAT (default 5), QXIDE_BENCH_ENGINES (comma list, default `ast`),
// QXIDE_BENCH_ONLY (substring filter), QXIDE_BENCH_JSON (append one JSON line per result).

use std::path::Path;
use std::time::{Duration, Instant};

use super::engine::{self, Engine};
use super::{front_input, stdlib, Input, Limits};

enum Mode {
    // Front end, elaboration, then `run` with this many shots (sampling and rendering).
    Run(u64),
    // Front end, elaboration, then the OpenQASM text (circuits too large to simulate).
    Build,
    // Front end only (`qxide check`).
    Check,
}

struct Case {
    name: String,
    src: String,
    mode: Mode,
}

// `// bench: shots=N`, `// bench: build` or `// bench: check` on the first line.
fn mode_of(src: &str) -> Mode {
    let first = src.lines().next().unwrap_or_default();
    let spec = first
        .strip_prefix("// bench:")
        .map(str::trim)
        .unwrap_or("check");
    match spec {
        "build" => Mode::Build,
        "check" => Mode::Check,
        s => Mode::Run(
            s.strip_prefix("shots=")
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("bad bench header `{first}`")),
        ),
    }
}

// The `flat_functions` family of scripts/perf/checker_large_files.py: n functions that each
// allocate, measure and print, called from `main`.
fn flat_functions(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!(
            "void g{i}(){{\n    Qubit q = qubit();\n    h(q);\n    Bit b = measure(q);\n    println(\"{{}}\", b);\n}}\n"
        ));
    }
    s.push_str("void main(){\n");
    for i in 0..n {
        s.push_str(&format!("    g{i}();\n"));
    }
    s.push_str("}\n");
    s
}

fn cases() -> Vec<Case> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/qx");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("bench/qx")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "qx"))
        .collect();
    names.sort();
    let mut v: Vec<Case> = names
        .iter()
        .map(|p| {
            let src = std::fs::read_to_string(p).expect("read bench");
            Case {
                name: p.file_stem().unwrap().to_string_lossy().into_owned(),
                mode: mode_of(&src),
                src,
            }
        })
        .collect();
    v.push(Case {
        name: "check_flat_8000".into(),
        src: flat_functions(8000),
        mode: Mode::Check,
    });
    v.push(Case {
        name: "run_flat_8000".into(),
        src: flat_functions(8000),
        mode: Mode::Run(1),
    });
    v
}

#[derive(Default)]
struct Times {
    front: Vec<Duration>,
    elaborate: Vec<Duration>,
    back: Vec<Duration>,
    total: Vec<Duration>,
}

fn once(case: &Case, engine: Engine, t: &mut Times) -> String {
    let limits = Limits::CLI;
    let input = Input::file("bench.qx", &case.src);
    let start = Instant::now();
    let (_, program) = front_input(&input, true);
    let program = program.unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
    let front = start.elapsed();
    t.front.push(front);
    let mut host = stdlib::LogOnly::default();
    let out = match case.mode {
        Mode::Check => String::new(),
        Mode::Run(_) | Mode::Build => {
            let s = Instant::now();
            let e = engine::elaborate(&program, &mut host, &limits, engine)
                .unwrap_or_else(|d| panic!("{}: {d:?}", case.name));
            t.elaborate.push(s.elapsed());
            let s = Instant::now();
            let out = match case.mode {
                Mode::Run(shots) => {
                    super::run_built(&program, &e, shots, 1, limits, &mut host)
                        .unwrap_or_else(|d| panic!("{}: {d:?}", case.name))
                        .text
                }
                _ => e.circuit.to_qasm(),
            };
            t.back.push(s.elapsed());
            out
        }
    };
    t.total.push(start.elapsed());
    out
}

fn stats(v: &mut [Duration]) -> (f64, f64) {
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    v.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    (ms(v[0]), ms(v[v.len() / 2]))
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

#[test]
#[ignore = "benchmark; run with --release -- --ignored --nocapture"]
fn bench_suite() {
    let worker = std::thread::Builder::new()
        .stack_size(super::STACK_SIZE)
        .spawn(run_suite)
        .expect("spawn");
    worker.join().expect("bench panicked");
}

fn run_suite() {
    let repeat: usize = env_or("QXIDE_BENCH_REPEAT", "5").parse().expect("repeat");
    let only = env_or("QXIDE_BENCH_ONLY", "");
    let engines: Vec<Engine> = env_or("QXIDE_BENCH_ENGINES", "ast")
        .split(',')
        .map(|e| Engine::parse(e.trim()).unwrap_or_else(|| panic!("engine `{e}`")))
        .collect();
    let json = std::env::var("QXIDE_BENCH_JSON").ok();
    println!(
        "{:<18} {:<6} {:>14} {:>14} {:>14} {:>14}",
        "case", "engine", "front ms", "elaborate ms", "run/qasm ms", "total ms"
    );
    println!("{:<25} {:>14} (min / median of {repeat} runs)", "", "");
    for case in cases().iter().filter(|c| c.name.contains(only.as_str())) {
        let mut reference: Option<String> = None;
        for &engine in &engines {
            let mut t = Times::default();
            // One warm-up run, not recorded; its output must match across engines.
            let out = once(case, engine, &mut Times::default());
            match &reference {
                None => reference = Some(out),
                Some(r) => assert_eq!(r, &out, "{}: engines disagree", case.name),
            }
            for _ in 0..repeat {
                once(case, engine, &mut t);
            }
            let cols = [
                stats(&mut t.front),
                stats(&mut t.elaborate),
                stats(&mut t.back),
                stats(&mut t.total),
            ];
            let cell = |(min, med): (f64, f64)| {
                if min.is_nan() {
                    "-".to_string()
                } else {
                    format!("{min:.1} / {med:.1}")
                }
            };
            println!(
                "{:<18} {:<6} {:>14} {:>14} {:>14} {:>14}",
                case.name,
                engine.name(),
                cell(cols[0]),
                cell(cols[1]),
                cell(cols[2]),
                cell(cols[3])
            );
            if let Some(path) = &json {
                let num = |x: f64| {
                    if x.is_nan() {
                        "null".to_string()
                    } else {
                        format!("{x:.3}")
                    }
                };
                let line = format!(
                    "{{\"case\":\"{}\",\"engine\":\"{}\",\"repeat\":{repeat},\"front_ms\":[{},{}],\"elaborate_ms\":[{},{}],\"back_ms\":[{},{}],\"total_ms\":[{},{}]}}\n",
                    case.name,
                    engine.name(),
                    num(cols[0].0), num(cols[0].1),
                    num(cols[1].0), num(cols[1].1),
                    num(cols[2].0), num(cols[2].1),
                    num(cols[3].0), num(cols[3].1),
                );
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut f| f.write_all(line.as_bytes()))
                    .expect("write json");
            }
        }
    }
}
