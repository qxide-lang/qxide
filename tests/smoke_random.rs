// Random-input smoke test for the `.qx` toolchain on stable Rust (no cargo-fuzz needed).
// Mutates the example programs token by token and runs `qxide check`, `fmt --check`, `qasm` and
// `run` on each mutant in a child process with a time limit. Any panic, crash or time-out fails.
//
//   cargo test --release --test smoke_random -- --ignored --nocapture
//   QXIDE_SMOKE_CASES=2000 QXIDE_SMOKE_SEED=7 QXIDE_SMOKE_TIMEOUT_MS=3000 (optional)
//   QXIDE_SMOKE_REPLAY=path/to/input.qx (optional; checked unchanged before the mutants)
//
// Ignored by default: it spawns thousands of processes, and the interpreter has no step limit
// yet (docs/security/BASELINE_FINDINGS.md), so a mutant with a huge loop bound shows up as a
// time-out. Failing inputs are saved under target/tmp/smoke-failures/.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

#[rustfmt::skip]
const DICT: &[&str] = &[
    "void", "int", "double", "boolean", "Bit", "BitReg", "Qubit", "QReg", "var", "final", "consume",
    "public", "import", "if", "else", "for", "in", "return", "true", "false", "main", "qubit",
    "qreg", "measure", "discard", "len", "println", "h", "x", "z", "s", "sdg", "t", "tdg", "cx",
    "ccx", "rx", "ry", "rz", "reset", "as_int", "to_binary", "PI", "(", ")", "{", "}", "[", "]",
    ";", ",", ".", "..", "=", "==", "!=", "<", "<=", ">", ">=", "+", "-", "*", "/", "%", "!",
    "&&", "||", "as", "\"{}\"", "\"{\"", "\"}\"", "\"\\\"", "0", "1", "-1", "2", "20", "21", "63",
    "64", "65", "1048576", "1048577", "2147483648", "4294967296", "9223372036854775807",
    "-9223372036854775807", "99999999999999999999", "1e308", "1e309", "0.0", "-0.0", "1.5", "//",
    "/*", "*/", "\u{feff}", "\u{202e}", "\0", "é", "\u{1F600}", "\r", "\t", "\n",
];

/// Splits source into identifier/number runs, whitespace runs and single other characters.
fn tokens(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    for c in src.chars() {
        let k = class(c);
        match out.last_mut() {
            Some(last) if k != 2 && last.chars().last().map(class) == Some(k) => last.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}

fn mutate(rng: &mut Rng, base: &[String], other: &[String]) -> String {
    let mut toks = base.to_vec();
    for _ in 0..1 + rng.below(4) {
        if toks.is_empty() {
            toks.push(rng.pick(DICT).to_string());
            continue;
        }
        let i = rng.below(toks.len());
        match rng.below(6) {
            0 => {
                toks.remove(i);
            }
            1 => toks[i] = rng.pick(DICT).to_string(),
            2 => toks.insert(i, rng.pick(DICT).to_string()),
            3 => {
                let t = toks[i].clone();
                toks.insert(i, t);
            }
            4 => {
                // Splice a slice of another example.
                let a = rng.below(other.len());
                let b = (a + rng.below(40)).min(other.len());
                for (k, t) in other[a..b].iter().enumerate() {
                    toks.insert(i + k, t.clone());
                }
            }
            _ => toks.truncate(i),
        }
    }
    toks.concat()
}

enum Outcome {
    Ok,
    Panic(String),
    Timeout,
}

fn run(args: &[&str], timeout: Duration) -> Outcome {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qxide");
    // Drain stderr on a thread so a chatty child never blocks on a full pipe.
    let mut stderr = child.stderr.take().expect("stderr");
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut s);
        s
    });
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("wait qxide") {
            let err = reader.join().unwrap_or_default();
            // 0 success, 1 diagnostics, 2 usage errors; anything else is a panic or crash.
            let normal = matches!(status.code(), Some(0..=2));
            return if normal && !err.contains("panicked at") {
                Outcome::Ok
            } else {
                Outcome::Panic(format!("{status}: {}", err.lines().last().unwrap_or("")))
            };
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Outcome::Timeout;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "slow; run with --ignored"]
fn random_mutants_never_panic_or_hang() {
    let cases = env_u64("QXIDE_SMOKE_CASES", 300);
    let seed = env_u64("QXIDE_SMOKE_SEED", 0x5eed);
    let timeout = Duration::from_millis(env_u64("QXIDE_SMOKE_TIMEOUT_MS", 3000));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("qx");
    let mut examples: Vec<Vec<String>> = std::fs::read_dir(&dir)
        .expect("examples/qx")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "qx"))
        .map(|p| tokens(&std::fs::read_to_string(p).unwrap()))
        .collect();
    examples.sort();
    assert!(!examples.is_empty());

    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke-failures");
    let work =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("smoke-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let mut rng = Rng(seed | 1);
    let mut failures = Vec::new();
    // QXIDE_SMOKE_REPLAY=<file.qx> runs that file unchanged first, e.g. a saved failure.
    let replay = std::env::var("QXIDE_SMOKE_REPLAY")
        .ok()
        .map(|p| std::fs::read_to_string(p).expect("read QXIDE_SMOKE_REPLAY"));
    for case in 0..cases + u64::from(replay.is_some()) {
        let src = match (&replay, case) {
            (Some(src), 0) => src.clone(),
            _ => {
                let base = &examples[rng.below(examples.len())];
                let other = &examples[rng.below(examples.len())];
                mutate(&mut rng, base, other)
            }
        };
        let file = work.join("case.qx");
        std::fs::write(&file, &src).unwrap();
        let path = file.to_string_lossy().into_owned();
        let shots = (1 + rng.below(4)).to_string();
        let seed_arg = rng.next().to_string();
        let commands: [Vec<&str>; 4] = [
            vec!["check", &path],
            vec!["fmt", &path, "--check"],
            vec!["qasm", &path],
            vec!["run", &path, "--shots", &shots, "--seed", &seed_arg],
        ];
        for args in commands {
            let what = match run(&args, timeout) {
                Outcome::Ok => continue,
                Outcome::Panic(m) => format!("panic: {m}"),
                Outcome::Timeout => format!("time-out after {timeout:?}"),
            };
            std::fs::create_dir_all(&out_dir).unwrap();
            let saved = out_dir.join(format!("seed{seed}-case{case}-{}.qx", args[0]));
            std::fs::write(&saved, &src).unwrap();
            failures.push(format!(
                "qxide {} -> {what}\n  input: {}",
                args.join(" "),
                saved.display()
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    println!(
        "{cases} mutants x 4 commands, seed {seed}: {} failure(s)",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
