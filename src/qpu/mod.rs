//! `qxide run --backend PROVIDER:DEVICE`: runs the circuit of a `.qx` program on a quantum
//! computer service instead of the local simulator (docs/design/qx-qpu.md).
//!
//! The program is elaborated once, like `qxide qasm`; its OpenQASM 3 goes to the provider, which
//! returns one measurement record per shot, and the records print like simulated `--shots`.
//! Binary-only: the wasm playground and the app runtime never reach the network from here.

mod http;
mod ibm;
mod isa;
mod keychain;
mod msg;
mod results;

use std::io::{BufRead as _, IsTerminal as _, Write as _};

/// What to run: the program's OpenQASM 3 (from `Circuit::to_qasm`), the number of shots, and
/// the width of its classical register `c`.
pub struct Job<'a> {
    pub qasm: &'a str,
    pub shots: u64,
    pub bits: usize,
}

pub struct Settings {
    /// Print the request instead of sending it; no network access, no credentials.
    pub dry_run: bool,
    /// Submit without asking.
    pub yes: bool,
    /// Most seconds of QPU time the service may spend on the job.
    pub max_seconds: u64,
    /// A saved device configuration, so `--dry-run` can show the exact device circuit.
    pub device_config: Option<String>,
}

pub const DEFAULT_MAX_SECONDS: u64 = 120;
// The service's limit for a job's `cost` (three hours; the API schema's maximum).
const MAX_SECONDS: u64 = 10_800;

/// Progress and confirmation, so tests can drive a run without a terminal.
pub trait Ui {
    fn note(&mut self, text: &str);
    /// Whether `confirm` can ask a person.
    fn interactive(&self) -> bool;
    /// Shows `notice` and asks whether to submit.
    fn confirm(&mut self, notice: &str) -> bool;
}

/// A quantum computer service.
pub trait Provider {
    /// The request that `run` would send, without network access or credentials.
    fn dry_run(&self, job: &Job, settings: &Settings) -> Result<String, String>;
    /// Submits the job, waits for it, and returns one record (`c[0..bits]`) per shot.
    fn run(
        &self,
        job: &Job,
        settings: &Settings,
        ui: &mut dyn Ui,
    ) -> Result<Vec<Vec<bool>>, String>;
}

/// The provider for `--backend SPEC`: `ibm:<device>`.
pub fn provider(spec: &str) -> Result<Box<dyn Provider>, String> {
    match spec.split_once(':') {
        Some(("ibm", device)) => Ok(Box::new(ibm::parse_device(device)?)),
        _ => Err(msg::invalid_backend(spec)),
    }
}

/// Runs (or with `dry_run`, describes) `exported` on `provider`; returns what `qxide run` prints.
pub fn run(
    exported: &crate::lang::Exported,
    provider: &dyn Provider,
    shots: u64,
    settings: &Settings,
    ui: &mut dyn Ui,
) -> Result<String, String> {
    if exported.measured == 0 {
        return Err(msg::nothing_measured());
    }
    let job = Job {
        qasm: &exported.qasm,
        shots,
        bits: exported.measured,
    };
    if settings.dry_run {
        return provider.dry_run(&job, settings);
    }
    let records = provider.run(&job, settings, ui)?;
    exported.render(&records)
}

/// Checks the flags of `qxide run` that belong to `--backend`.
pub fn check_flags(
    backend: bool,
    dry_run: bool,
    yes: bool,
    max_seconds: Option<u64>,
    device_config: bool,
    seed: bool,
    allow: bool,
) -> Result<(), String> {
    if !backend {
        for (set, flag) in [
            (dry_run, "--dry-run"),
            (yes, "--yes"),
            (max_seconds.is_some(), "--max-seconds"),
            (device_config, "--device-config"),
        ] {
            if set {
                return Err(msg::needs_backend(flag));
            }
        }
        return Ok(());
    }
    for (set, flag) in [(seed, "--seed"), (allow, "--allow")] {
        if set {
            return Err(msg::conflicts_with_backend(flag));
        }
    }
    if device_config && !dry_run {
        return Err(msg::device_config_needs_dry_run());
    }
    if max_seconds.is_some_and(|s| s > MAX_SECONDS) {
        return Err(msg::max_seconds_range(MAX_SECONDS));
    }
    Ok(())
}

/// Progress on stderr; confirmation on the terminal.
pub struct TerminalUi;

impl Ui for TerminalUi {
    fn note(&mut self, text: &str) {
        eprintln!("{text}");
    }

    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }

    fn confirm(&mut self, notice: &str) -> bool {
        eprintln!("{notice}");
        eprint!("{}", msg::confirm_prompt());
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().lock().read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }
}
