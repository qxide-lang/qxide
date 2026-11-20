//! `qxide` command-line entry point.
//!
//! The crate is a library first; this binary is a thin wrapper so the project
//! can be installed as a standalone tool (cargo-dist installers, PyPI wheel).

mod lang;

use qxide::Circuit;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// The tree-walking interpreter recurses per nested call/block; give it more stack than the
// 1 MiB Windows main thread so the call-depth limit, not a stack overflow, is what stops it.
const LANG_STACK: usize = 64 << 20;

fn usage() -> &'static str {
    "Usage: qxide <COMMAND>

Commands:
  run <FILE> [--shots N] [--seed S]  Run a .qx program and simulate it (default: 1 shot, seed 0)
  check <FILE>                       Check a .qx program for errors without running it
  qasm <FILE>                        Print the OpenQASM 3 circuit built by a .qx program
  bell [SHOTS]                       Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)

Options:
  -V, --version                      Print version
  -h, --help                         Print help
"
}

fn bell(shots: u64) {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);

    println!("OpenQASM 3");
    print!("{}", c.to_qasm());

    println!("\nSimulation Counts");
    let mut counts = [0u64; 4];
    for seed in 0..shots {
        let shot = c.simulate(seed);
        counts[(shot.get(ma) as usize) << 1 | shot.get(mb) as usize] += 1;
    }
    for (k, n) in counts.iter().enumerate() {
        println!("{}{}: {n}", k >> 1, k & 1);
    }
}

fn parse_count(flag: &str, value: Option<&String>, min: u64) -> Result<u64, ExitCode> {
    match value.map(|s| (s, s.parse::<u64>())) {
        Some((_, Ok(n))) if n >= min => Ok(n),
        Some((s, _)) => {
            eprintln!("qxide: invalid value `{s}` for {flag}");
            Err(ExitCode::from(2))
        }
        None => {
            eprintln!("qxide: option {flag} requires a value");
            Err(ExitCode::from(2))
        }
    }
}

// Runs one `.qx` command: `run`, `check` or `qasm`.
fn lang_command(cmd: &str, args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("qxide: command `{cmd}` requires a .qx file path");
        eprint!("{}", usage());
        return ExitCode::from(2);
    };
    let (mut shots, mut seed) = (1, 0);
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let parsed = match (cmd, flag.as_str()) {
            ("run", "--shots") => parse_count(flag, rest.next(), 1).map(|n| shots = n),
            ("run", "--seed") => parse_count(flag, rest.next(), 0).map(|n| seed = n),
            _ => {
                eprintln!("qxide: unexpected argument `{flag}` for `{cmd}`");
                Err(ExitCode::from(2))
            }
        };
        if let Err(code) = parsed {
            return code;
        }
    }
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("qxide: failed to read `{path}`: {e}");
            return ExitCode::from(1);
        }
    };
    let (cmd, path) = (cmd.to_string(), path.clone());
    let worker = std::thread::Builder::new()
        .stack_size(LANG_STACK)
        .spawn(move || match cmd.as_str() {
            "check" => lang::check(&path, &src).map(|()| {
                let mut line = format!("{path}: ok");
                line.push('\n');
                line
            }),
            "qasm" => lang::qasm(&path, &src),
            _ => lang::run(&path, &src, shots, seed),
        });
    match worker.map(|h| h.join()) {
        Ok(Ok(Ok(out))) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Ok(Ok(Err(lang::Failure(diags)))) => {
            eprint!("{diags}");
            ExitCode::from(1)
        }
        // The worker only fails if the interpreter itself panicked; the panic message is already on stderr.
        _ => ExitCode::from(101),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("qxide {VERSION}"),
        Some("--help" | "-h") | None => print!("{}", usage()),
        Some(cmd @ ("run" | "check" | "qasm")) => return lang_command(cmd, &args[1..]),
        Some("bell") => {
            let shots = match args.get(1) {
                None => 1000,
                Some(s) => match s.parse::<u64>() {
                    Ok(n) if n > 0 => n,
                    _ => {
                        eprintln!("qxide: invalid shot count `{s}`");
                        return ExitCode::from(2);
                    }
                },
            };
            bell(shots);
        }
        Some(other) => {
            eprintln!("qxide: unknown command `{other}`");
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
