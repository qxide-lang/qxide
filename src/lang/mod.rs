// The `.qx` language front end and interpreter (docs/design/qx-language.md).
// Private to the `qxide` binary: it only uses the library's public API.

mod ast;
mod check;
// The desktop runtime (crates/qxide-app-runtime) uses all of it; the CLI only checks commands.
#[allow(dead_code)]
pub mod command;
mod diag;
mod explain;
mod fmt;
mod interp;
mod lexer;
mod link;
mod msg;
mod parser;
pub mod repl;
pub mod stdlib;
// Used by the desktop runtime for Qxide Studio (`std.qxide`).
#[allow(dead_code)]
pub mod studio;

#[cfg(test)]
mod sampling_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;

#[cfg(test)]
mod module_tests;

#[cfg(test)]
mod app_tests;

use std::collections::HashMap;
use std::path::Path;

use qxide::{Backend, Shot, SimError, DEFAULT_MEMORY_LIMIT};

use diag::{Code, Diagnostic, SourceMap};
use interp::{BitsFmt, Elaborated, OutPiece};
use stdlib::{Host, Permissions};
// The package manager (CLI only) builds `PackageRoot`s; the playground build never does.
#[allow(unused_imports)]
pub use link::{PackageRoot, Project, ReadFn};

/// Rendered diagnostics, ready for stderr.
#[derive(Debug)]
pub struct Failure(pub String);

/// The entry file of `qxide run`/`check`/`qasm` and the packages its imports may reach.
pub struct Input<'a> {
    pub name: &'a str,
    pub src: &'a str,
    pub project: Option<&'a Project>,
    /// Reads imported module files; `read_file` outside of tests.
    pub read: ReadFn<'a>,
    /// Standard modules this host offers and the program may import.
    pub perms: Permissions,
}

pub fn read_file(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

impl<'a> Input<'a> {
    /// A lone file: no project, so any `import` other than `std` is an error. Standard modules
    /// as in `qxide run` without `--allow`.
    pub fn file(name: &'a str, src: &'a str) -> Self {
        Self {
            name,
            src,
            project: None,
            read: &read_file,
            perms: Permissions::cli(false),
        }
    }

    // A module file of a package (such as `lib.qx`) is a library: it needs no `main`.
    fn is_library(&self) -> bool {
        self.project.is_some_and(|p| p.entry_module.is_some())
    }
}

fn front_input(
    input: &Input,
    require_main: bool,
) -> (SourceMap, Result<ast::Program, Vec<Diagnostic>>) {
    let src = strip_bom(input.src);
    let (map, linked) = link::link(input.name, src, input.project, input.read, &input.perms);
    let checked = linked.and_then(|program| {
        let errors = check::check_with(&program, require_main);
        if errors.is_empty() {
            Ok(program)
        } else {
            Err(errors)
        }
    });
    (map, checked)
}

fn build_input(
    input: &Input,
    host: &mut dyn Host,
) -> (
    SourceMap,
    Result<(ast::Program, Elaborated), Vec<Diagnostic>>,
) {
    let (map, program) = front_input(input, true);
    let built = program.and_then(|program| {
        let elaborated = interp::elaborate(&program, host).map_err(|d| vec![d])?;
        Ok((program, elaborated))
    });
    (map, built)
}

// A lone source with REPL permissions (`std.log` only), as the REPL and the tests check it.
fn front(src: &str) -> Result<ast::Program, Vec<Diagnostic>> {
    let mut input = Input::file("t.qx", src);
    input.perms = Permissions::repl();
    front_input(&input, true).1
}

#[cfg(test)]
fn build(src: &str) -> Result<(ast::Program, Elaborated), Vec<Diagnostic>> {
    build_input(&Input::file("t.qx", src), &mut stdlib::LogOnly::default()).1
}

/// The `qxide run` host: `std.log` to stderr, `std.fs` under `data_root`.
pub fn cli_host(data_root: std::path::PathBuf) -> stdlib::NativeStd {
    stdlib::NativeStd::new(
        data_root,
        Box::new(|level, text| eprintln!("{}", stdlib::log_line(level, text))),
    )
}

// Tableau size of the library's stabilizer backend: x and z bit rows (2n + 1 of them) plus signs.
fn stabilizer_bytes(n: usize) -> u128 {
    let (n, rows) = (n as u128, 2 * n as u128 + 1);
    let words = n.div_ceil(64).max(1);
    2 * rows * words * 8 + rows
}

fn gib(bytes: u128) -> String {
    format!("{:.1} GiB", bytes as f64 / f64::from(1u32 << 30))
}

// Re-words `SimError::OutOfMemory` with GiB sizes and a `.qx`-specific help line.
fn sim_error(err: SimError, at: diag::Span) -> Diagnostic {
    match err {
        SimError::OutOfMemory {
            qubits,
            required_bytes,
            limit_bytes,
        } => {
            let reason =
                msg::state_vector_too_large(qubits, &gib(required_bytes), &gib(limit_bytes));
            Diagnostic::new(Code::Simulation, at, msg::simulation_failed(&reason))
                .with_help(msg::help_too_large())
        }
        // `Backend::Auto` never picks the stabilizer for non-Clifford circuits; keep any other reason verbatim.
        other => Diagnostic::new(
            Code::Simulation,
            at,
            msg::simulation_failed(&other.to_string()),
        ),
    }
}

fn render(map: &SourceMap, errors: &[Diagnostic]) -> Failure {
    Failure(
        errors
            .iter()
            .map(|d| map.render(d))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

// Notepad and PowerShell redirection may prepend a UTF-8 byte order mark.
fn strip_bom(src: &str) -> &str {
    src.strip_prefix('\u{feff}').unwrap_or(src)
}

/// `qxide check` of a lone file.
// The single-file entry points stay for callers that have no project (tests, a future REPL).
#[cfg_attr(not(test), allow(dead_code))]
pub fn check(name: &str, src: &str) -> Result<(), Failure> {
    check_input(&Input::file(name, src))
}

/// `qxide check`: lexing, linking and static checks only, for the entry file and its imports.
pub fn check_input(input: &Input) -> Result<(), Failure> {
    let (map, program) = front_input(input, !input.is_library());
    program.map(|_| ()).map_err(|e| render(&map, &e))
}

/// `qxide qasm` of a lone file.
#[cfg_attr(not(test), allow(dead_code))]
pub fn qasm(name: &str, src: &str) -> Result<String, Failure> {
    qasm_input(&Input::file(name, src))
}

/// `qxide fmt`: the formatted source. Refuses sources that do not parse, and double-checks that
/// formatting kept every token and comment before anything is written.
pub fn format(name: &str, src: &str) -> Result<String, Failure> {
    let out = fmt::format(src).map_err(|e| render(&SourceMap::new(name, strip_bom(src)), &e))?;
    if fmt::fingerprint(&out) != fmt::fingerprint(src) {
        return Err(Failure(msg::fmt_changed_tokens(name)));
    }
    Ok(out)
}

/// `std.X is not available in HOST` (Q0702), for hosts outside this module.
#[allow(dead_code)]
pub fn std_unavailable(what: &str, host: &str) -> String {
    msg::std_unavailable(what, host)
}

/// `set_size` outside 200..=8192 pixels (Q0706), for the app runtime.
#[allow(dead_code)]
pub fn std_size_range(width: i64, height: i64) -> String {
    msg::std_size_range(width, height)
}

/// `invalid [app] section in qxide.toml: {reason}` (Q0715), for `qxide app`.
pub fn bad_app_config(reason: &str) -> String {
    msg::bad_app_config(reason)
}

/// `qxide explain`: the catalog entry for an error code such as `Q0302` (case-insensitive).
pub fn explain(arg: &str) -> Option<String> {
    explain::explain(&explain::normalize(arg)?)
}

/// `qxide qasm`: elaborate once and print the OpenQASM 3 of the resulting circuit. Effects are
/// refused (`std.log` is dropped), so exporting a circuit never writes files.
pub fn qasm_input(input: &Input) -> Result<String, Failure> {
    let mut host = stdlib::NoEffects {
        what: "`qxide qasm`",
    };
    let (map, built) = build_input(input, &mut host);
    let (_, e) = built.map_err(|e| render(&map, &e))?;
    Ok(e.circuit.to_qasm())
}

/// `[0, 1, 1]`: register order, `r[0]` first.
fn fmt_array(bits: &[bool]) -> String {
    let items: Vec<&str> = bits.iter().map(|&b| if b { "1" } else { "0" }).collect();
    format!("[{}]", items.join(", "))
}

/// `0b110`: most significant bit first, so `r[0]` is the last digit; zero-padded to the register width.
fn fmt_binary(bits: &[bool]) -> String {
    if bits.is_empty() {
        return "0b0".into();
    }
    let digits: String = bits
        .iter()
        .rev()
        .map(|&b| if b { '1' } else { '0' })
        .collect();
    format!("0b{digits}")
}

/// Unsigned value with `r[0]` as the least significant bit; `None` above 64 bits.
fn bits_value(bits: &[bool]) -> Option<u64> {
    (bits.len() <= 64).then(|| {
        bits.iter()
            .enumerate()
            .fold(0u64, |v, (i, &b)| v | (u64::from(b) << i))
    })
}

fn render_output(out: &[OutPiece], shot: Option<&Shot>) -> String {
    let bit = |b: qxide::Bit| shot.is_some_and(|s| s.get(b));
    let mut s = String::new();
    for p in out {
        match p {
            OutPiece::Text(t) => s.push_str(t),
            OutPiece::Bit(b) => s.push(if bit(*b) { '1' } else { '0' }),
            OutPiece::Bits(v, fmt) => {
                let bits: Vec<bool> = v.iter().map(|&b| bit(b)).collect();
                s.push_str(&match fmt {
                    BitsFmt::Array => fmt_array(&bits),
                    BitsFmt::Binary => fmt_binary(&bits),
                    // The interpreter rejects `as_int()` on registers wider than 64 bits.
                    BitsFmt::Int => bits_value(&bits).unwrap_or_default().to_string(),
                });
            }
        }
    }
    s
}

/// One row of the multi-shot table: `[0, 1, 1]  0b110 = 6  ×503`.
fn table_row(bits: &[bool], count: u64) -> String {
    let value = bits_value(bits).map_or(String::new(), |v| format!(" = {v}"));
    format!("{}  {}{value}  ×{count}", fmt_array(bits), fmt_binary(bits))
}

/// Resource limits for `run`. The CLI uses `Limits::CLI`, desktop apps `Limits::APP`; the browser
/// playground passes its own.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most qubit slots the state-vector backend may simulate; `None` leaves it to the memory limit.
    pub max_state_vector_qubits: Option<usize>,
    /// Largest stabilizer tableau, in bytes.
    pub stabilizer_bytes: u64,
    /// Where the limits apply, for the wording of the diagnostic.
    pub scope: LimitScope,
}

// `Playground` is constructed only by crates/qxide-wasm.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitScope {
    Cli,
    Playground,
    App,
}

/// State-vector qubits of one desktop-app call: 2^24 amplitudes, 256 MiB.
pub const APP_STATE_VECTOR_QUBITS: usize = 24;
/// Stabilizer tableau bytes of one desktop-app call.
pub const APP_STABILIZER_BYTES: u64 = 256 << 20;

impl Limits {
    pub const CLI: Limits = Limits {
        max_state_vector_qubits: None,
        stabilizer_bytes: DEFAULT_MEMORY_LIMIT,
        scope: LimitScope::Cli,
    };

    /// Each call of a desktop app (commands and `std.qxide.run`); the runtime bounds how many
    /// run at once, so the total stays within one budget.
    #[allow(dead_code)]
    pub const APP: Limits = Limits {
        max_state_vector_qubits: Some(APP_STATE_VECTOR_QUBITS),
        stabilizer_bytes: APP_STABILIZER_BYTES,
        scope: LimitScope::App,
    };
}

/// `qxide run`: elaborate once, then simulate. One shot prints the `println` output; several
/// shots print one table row per distinct measurement record (`c[0]` first) with its output.
// The playground binding calls `run_with`; the CLI calls `run_input`.
#[allow(dead_code)]
pub fn run(name: &str, src: &str, shots: u64, seed: u64) -> Result<String, Failure> {
    let mut host = cli_host("data".into());
    run_input_with(&Input::file(name, src), shots, seed, Limits::CLI, &mut host)
}

/// `run` of a lone file with explicit resource limits.
#[allow(dead_code)]
pub fn run_with(
    name: &str,
    src: &str,
    shots: u64,
    seed: u64,
    limits: Limits,
) -> Result<String, Failure> {
    let mut host = cli_host("data".into());
    run_input_with(&Input::file(name, src), shots, seed, limits, &mut host)
}

/// `qxide run` of an entry file and its imports; see `run`. Std calls go to `host`.
#[allow(dead_code)]
pub fn run_input(
    input: &Input,
    shots: u64,
    seed: u64,
    host: &mut dyn Host,
) -> Result<String, Failure> {
    run_input_with(input, shots, seed, Limits::CLI, host)
}

/// `run_input` with explicit resource limits (the playground passes tighter ones).
#[allow(dead_code)]
pub fn run_input_with(
    input: &Input,
    shots: u64,
    seed: u64,
    limits: Limits,
    host: &mut dyn Host,
) -> Result<String, Failure> {
    let (map, run) = run_diag(input, shots, seed, limits, host);
    run.map(|r| r.text).map_err(|e| render(&map, &e))
}

/// A finished `run`: the CLI's stdout, plus the table rows (measurement record, count) when
/// several shots were simulated.
pub struct RunOut {
    pub text: String,
    pub rows: Vec<(Vec<bool>, u64)>,
}

/// `run_input_with` with unrendered diagnostics, for editors.
fn run_diag(
    input: &Input,
    shots: u64,
    seed: u64,
    limits: Limits,
    host: &mut dyn Host,
) -> (SourceMap, Result<RunOut, Vec<Diagnostic>>) {
    let (map, built) = build_input(input, host);
    let run = built.and_then(|(program, e)| run_built(&program, &e, shots, seed, limits, host));
    (map, run)
}

fn run_built(
    program: &ast::Program,
    e: &Elaborated,
    shots: u64,
    seed: u64,
    limits: Limits,
    host: &mut dyn Host,
) -> Result<RunOut, Vec<Diagnostic>> {
    let done = |text: String| RunOut {
        text,
        rows: Vec::new(),
    };
    let prints_bits = e
        .output
        .iter()
        .any(|p| matches!(p, OutPiece::Bit(_) | OutPiece::Bits(..)));
    let needs_sim = if shots == 1 {
        prints_bits
    } else {
        !e.measured.is_empty()
    };
    if !needs_sim {
        // Nothing observable depends on the shot.
        return Ok(done(render_output(&e.output, None)));
    }
    let main_span = program
        .fns
        .iter()
        .find(|f| f.name.name == "main")
        .map_or(diag::Span::default(), |f| f.name.span);
    let sims = sample(e, main_span, shots, seed, limits).map_err(|d| vec![d])?;
    let mut sims = sims.map(|r| {
        if host.cancelled() {
            return Err(vec![cancelled(main_span)]);
        }
        r.map_err(|d| vec![d])
    });
    if shots == 1 {
        let shot = sims.next().expect("one shot")?;
        return Ok(done(render_output(&e.output, Some(&shot))));
    }
    // The printed output depends only on the measured bits, so it is rendered once per record.
    let mut groups: HashMap<Vec<bool>, (u64, String)> = HashMap::new();
    for shot in sims {
        let shot = shot?;
        let key: Vec<bool> = e.measured.iter().map(|&b| shot.get(b)).collect();
        groups
            .entry(key)
            .or_insert_with(|| (0, render_output(&e.output, Some(&shot))))
            .0 += 1;
    }
    let mut s = String::new();
    let mut rows = Vec::new();
    for (bits, (count, text)) in sorted_groups(groups) {
        s.push_str(&table_row(&bits, count));
        s.push('\n');
        for line in text.lines() {
            s.push_str("  ");
            s.push_str(line);
            s.push('\n');
        }
        rows.push((bits, count));
    }
    Ok(RunOut { text: s, rows })
}

fn cancelled(at: diag::Span) -> Diagnostic {
    Diagnostic::new(Code::Cancelled, at, msg::cancelled())
}

/// Most frequent first, ties in bit order: the order of the `--shots` table.
fn sorted_groups<T>(groups: HashMap<Vec<bool>, (u64, T)>) -> Vec<(Vec<bool>, (u64, T))> {
    let mut rows: Vec<(Vec<bool>, (u64, T))> = groups.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    rows
}

/// Checks the resource limits for simulating `e`, then yields its shots. Every shot gets its own
/// seed derived from `seed` (see `qxide::shot_seed`), shared with the library and the
/// playground, so runs with nearby seeds do not reuse each other's shots.
fn sample(
    e: &Elaborated,
    at: diag::Span,
    shots: u64,
    seed: u64,
    limits: Limits,
) -> Result<impl Iterator<Item = Result<Shot, Diagnostic>> + '_, Diagnostic> {
    let backend = e.circuit.select_backend();
    if let Some(limit) = limits.max_state_vector_qubits {
        if backend == Backend::StateVector && e.slots > limit {
            let (reason, help) = if limits.scope == LimitScope::App {
                (
                    msg::app_qubit_limit(e.slots, limit),
                    msg::help_app_qubit_limit(),
                )
            } else {
                (
                    msg::playground_qubit_limit(e.slots, limit),
                    msg::help_playground_qubit_limit(),
                )
            };
            return Err(
                Diagnostic::new(Code::Simulation, at, msg::simulation_failed(&reason))
                    .with_help(help),
            );
        }
    }
    // The stabilizer backend has no memory limit of its own; huge tableaus would abort on allocation.
    let tableau = stabilizer_bytes(e.slots);
    if backend == Backend::Stabilizer && tableau > u128::from(limits.stabilizer_bytes) {
        let reason =
            msg::stabilizer_too_large(e.slots, &gib(tableau), &gib(limits.stabilizer_bytes.into()));
        return Err(
            Diagnostic::new(Code::Simulation, at, msg::simulation_failed(&reason))
                .with_help(msg::help_too_large()),
        );
    }
    Ok(e.circuit
        .simulate_shots(Backend::Auto, seed, shots)
        .map(move |r| r.map_err(|err| sim_error(err, at))))
}
