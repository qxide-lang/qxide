//! `qxide` command-line entry point.
//!
//! The crate is a library first; this binary is a thin wrapper so the project
//! can be installed as a standalone tool (cargo-dist installers, PyPI wheel).

use qxide::Circuit;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() -> &'static str {
    "Usage: qxide <COMMAND>

Commands:
  bell [SHOTS]   Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)

Options:
  -V, --version  Print version
  -h, --help     Print help
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("qxide {VERSION}"),
        Some("--help" | "-h") | None => print!("{}", usage()),
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
