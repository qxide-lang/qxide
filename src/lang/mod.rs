// The `.qx` language front end and interpreter (docs/design/qx-language.md).
// Private to the `qxide` binary: it only uses the library's public API.

mod ast;
mod check;
mod diag;
mod explain;
mod fmt;
pub mod ide;
mod interp;
mod lexer;
mod link;
mod msg;
mod parser;
pub mod repl;

#[cfg(test)]
mod sampling_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;

#[cfg(test)]
mod module_tests;

#[cfg(test)]
mod limits_tests;

use std::collections::HashMap;
use std::path::Path;

use qxide::{Backend, Shot, SimError, DEFAULT_MEMORY_LIMIT};

use diag::{Code, Diagnostic, SourceMap};
use interp::{BitsFmt, Elaborated, OpCounts, OutPiece};
// The package manager (CLI only) builds `PackageRoot`s; the playground build never does.
#[allow(unused_imports)]
pub use link::{PackageRoot, Project, ReadFn};
// The package manager prints registry and archive text through it too.
#[allow(unused_imports)]
pub use diag::printable;

// Stack for threads that run the interpreter. The deepest legal program nests 128 syntax levels
// in each of 200 calls, except that `if` on a `Bit` nests at most `max_cond_depth` (64) deep
// across all calls; such programs need at most ~100 MiB in an unoptimized build and ~45 MiB
// optimized (measured 2026-11-21 before the `Bit` limit, so an upper bound).
pub const STACK_SIZE: usize = 256 << 20;

// The CLI notes a random seed; the playground shows it itself.
#[allow(unused_imports)]
pub use msg::seed_note;

/// Rendered diagnostics, ready for stderr.
pub struct Failure(pub String);

/// The entry file of `qxide run`/`check`/`qasm` and the packages its imports may reach.
pub struct Input<'a> {
    pub name: &'a str,
    pub src: &'a str,
    pub project: Option<&'a Project>,
    /// Reads imported module files; `read_file` outside of tests.
    pub read: ReadFn<'a>,
}

pub fn read_file(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

impl<'a> Input<'a> {
    /// A lone file: no project, so any `import` is an error.
    pub fn file(name: &'a str, src: &'a str) -> Self {
        Self {
            name,
            src,
            project: None,
            read: &read_file,
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
    let (map, linked) = link::link(input.name, src, input.project, input.read);
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
    limits: &Limits,
) -> (
    SourceMap,
    Result<(ast::Program, Elaborated), Vec<Diagnostic>>,
) {
    let (map, program) = front_input(input, true);
    let built = program.and_then(|program| {
        let elaborated = interp::elaborate(&program, limits).map_err(|d| vec![d])?;
        Ok((program, elaborated))
    });
    (map, built)
}

// A lone source without imports, as the REPL and the tests check it.
fn front(src: &str) -> Result<ast::Program, Vec<Diagnostic>> {
    front_input(&Input::file("t.qx", src), true).1
}

#[cfg(test)]
fn build(src: &str) -> Result<(ast::Program, Elaborated), Vec<Diagnostic>> {
    build_input(&Input::file("t.qx", src), &Limits::CLI).1
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

fn render(map: &SourceMap, errors: &[Diagnostic], limits: &Limits) -> Failure {
    Failure(diag::render_all(errors, limits, |d| map.render(d, limits)))
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
    check_input_with(input, &Limits::CLI)
}

/// `check` of a lone file with explicit limits on the rendered diagnostics.
#[allow(dead_code)]
pub fn check_with(name: &str, src: &str, limits: &Limits) -> Result<(), Failure> {
    check_input_with(&Input::file(name, src), limits)
}

fn check_input_with(input: &Input, limits: &Limits) -> Result<(), Failure> {
    let (map, program) = front_input(input, !input.is_library());
    program.map(|_| ()).map_err(|e| render(&map, &e, limits))
}

/// `qxide qasm` of a lone file.
#[cfg_attr(not(test), allow(dead_code))]
pub fn qasm(name: &str, src: &str) -> Result<String, Failure> {
    qasm_input(&Input::file(name, src))
}

/// `qxide fmt`: the formatted source. Refuses sources that do not parse, and double-checks that
/// formatting kept every token and comment before anything is written.
pub fn format(name: &str, src: &str) -> Result<String, Failure> {
    let out = fmt::format(src)
        .map_err(|e| render(&SourceMap::new(name, strip_bom(src)), &e, &Limits::CLI))?;
    if fmt::fingerprint(&out) != fmt::fingerprint(src) {
        return Err(Failure(msg::fmt_changed_tokens(name)));
    }
    Ok(out)
}

/// `qxide explain`: the catalog entry for an error code such as `Q0302` (case-insensitive).
pub fn explain(arg: &str) -> Option<String> {
    explain::explain(&explain::normalize(arg)?)
}

/// Every catalog entry as (code, text), for tests outside `lang`.
#[cfg(test)]
pub fn explain_entries() -> Vec<(String, String)> {
    explain::entries()
}

/// `qxide qasm`: elaborate once and print the OpenQASM 3 of the resulting circuit.
pub fn qasm_input(input: &Input) -> Result<String, Failure> {
    qasm_input_with(input, &Limits::CLI)
}

/// `qasm` of a lone file with explicit resource limits.
#[allow(dead_code)]
pub fn qasm_with(name: &str, src: &str, limits: &Limits) -> Result<String, Failure> {
    qasm_input_with(&Input::file(name, src), limits)
}

fn qasm_input_with(input: &Input, limits: &Limits) -> Result<String, Failure> {
    let (map, built) = build_input(input, limits);
    let (program, e) = built.map_err(|e| render(&map, &e, limits))?;
    check_qasm_size(&e, main_span(&program), limits).map_err(|d| render(&map, &[d], limits))?;
    Ok(e.circuit.to_qasm())
}

// The interpreter adds up an upper bound of every QASM line, so the text is never built when it
// would pass the output limit (M-10).
fn check_qasm_size(e: &Elaborated, at: diag::Span, limits: &Limits) -> Result<(), Diagnostic> {
    if e.qasm_bytes > limits.max_output_bytes {
        return Err(Diagnostic::new(
            Code::OutputLimit,
            at,
            msg::qasm_limit(e.qasm_bytes, limits.max_output_bytes),
        ));
    }
    Ok(())
}

fn main_span(program: &ast::Program) -> diag::Span {
    program
        .fns
        .iter()
        .find(|f| f.name.name == "main")
        .map_or(diag::Span::default(), |f| f.name.span)
}

// Estimated work of one shot in units of about one state-vector amplitude update (about 1.4 ns on
// the machine the defaults were measured on; see docs/security/EXECUTION_LIMITS_FIXES.md). On the
// tableau, a column pass visits 2n rows with poor cache locality (about 12 ns per row for large n,
// so 8 units), and a measurement may add one row of ceil(n/64) words into each of 2n rows (about
// 0.1 ns per word, so 1/8 unit). Branch bodies are counted as if they always run.
fn shot_work(backend: Backend, slots: usize, c: &OpCounts) -> u128 {
    const SHOT_OVERHEAD: u128 = 128;
    let n = slots as u128;
    match backend {
        Backend::Stabilizer => {
            let words = n.div_ceil(64).max(1);
            let rows = 2 * n + 1;
            let alloc = 2 * rows * words / 8;
            let pass = 16 * n;
            let measure = 16 * n + n * words / 4;
            alloc
                + u128::from(c.row_passes) * pass
                + u128::from(c.measures) * measure
                + SHOT_OVERHEAD
        }
        _ => {
            let dim = if slots < 120 {
                1u128 << slots
            } else {
                u128::MAX
            };
            dim.saturating_mul(u128::from(c.sim_ops) + 1)
                .saturating_add(SHOT_OVERHEAD)
        }
    }
}

// Refuses a run whose shots would together pass the simulation work limit (B-03, B-08).
fn check_sim_work(
    e: &Elaborated,
    backend: Backend,
    shots: u64,
    at: diag::Span,
    limits: &Limits,
) -> Result<(), Diagnostic> {
    // A state vector over the memory limit fails with `Q0406` before any work is done; keep that error.
    if backend == Backend::StateVector
        && qxide::state_vector_bytes(e.slots) > u128::from(DEFAULT_MEMORY_LIMIT)
    {
        return Ok(());
    }
    let work = shot_work(backend, e.slots, &e.counts).saturating_mul(u128::from(shots));
    if work > u128::from(limits.max_sim_work) {
        return Err(Diagnostic::new(
            Code::SimWorkLimit,
            at,
            msg::sim_work_limit(shots, e.slots, e.counts.sim_ops, limits.max_sim_work),
        ));
    }
    Ok(())
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

/// Resource limits for `check`, `qasm` and `run`. The CLI uses `Limits::CLI`; the browser
/// playground uses `Limits::PLAYGROUND`. docs/security/EXECUTION_LIMITS_FIXES.md records how the
/// defaults were chosen.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most qubit slots the state-vector backend may simulate; `None` leaves it to the memory limit.
    pub max_state_vector_qubits: Option<usize>,
    /// Largest stabilizer tableau, in bytes.
    pub stabilizer_bytes: u64,
    /// Interpreter steps: one per evaluated expression, statement, loop iteration and call, plus
    /// one per qubit or bit of whole-register work (`qreg`, `measure`, `discard`, calls, `println`).
    pub max_steps: u64,
    /// Operations recorded in the circuit: gates, measurements, resets and conditional blocks.
    pub max_ops: u64,
    /// Estimated simulator work for all shots of a run, in units of about one state-vector
    /// amplitude update (see `sim_work`).
    pub max_sim_work: u64,
    /// Bytes of program output, of OpenQASM text, or of rendered diagnostics.
    pub max_output_bytes: u64,
    /// Diagnostics rendered in full; the rest are counted in one closing diagnostic.
    pub max_diagnostics: usize,
    /// Characters of a source line echoed under a diagnostic; longer lines are cut around the column.
    pub max_line_chars: usize,
    /// `if` blocks on measured bits nested inside each other, counted across function calls.
    pub max_cond_depth: usize,
}

impl Limits {
    pub const CLI: Limits = Limits {
        max_state_vector_qubits: None,
        stabilizer_bytes: DEFAULT_MEMORY_LIMIT,
        max_steps: 1 << 30,
        max_ops: 1 << 24,
        max_sim_work: 1 << 36,
        max_output_bytes: 256 << 20,
        max_diagnostics: 50,
        max_line_chars: 120,
        max_cond_depth: 64,
    };

    /// The qxide.dev playground: 20 state-vector qubits (16 MiB), a 512 MiB tableau, and budgets
    /// that keep a run well inside the page's 30 s worker limit.
    // Read by the wasm binding only.
    #[allow(dead_code)]
    pub const PLAYGROUND: Limits = Limits {
        max_state_vector_qubits: Some(20),
        stabilizer_bytes: 512 << 20,
        max_steps: 1 << 26,
        max_ops: 1 << 20,
        max_sim_work: 1 << 32,
        max_output_bytes: 1 << 20,
        max_diagnostics: 50,
        max_line_chars: 120,
        max_cond_depth: 64,
    };
}

/// `qxide run`: elaborate once, then simulate. One shot prints the `println` output; several
/// shots print one table row per distinct measurement record (`c[0]` first) with its output.
// The playground binding calls `run_with`; the CLI calls `run_input`.
#[allow(dead_code)]
pub fn run(name: &str, src: &str, shots: u64, seed: u64) -> Result<String, Failure> {
    run_with(name, src, shots, seed, Limits::CLI)
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
    run_input_with(&Input::file(name, src), shots, seed, &limits).map(|r| r.out)
}

/// `qxide run` of an entry file and its imports; see `run`.
#[allow(dead_code)]
pub fn run_input(input: &Input, shots: u64, seed: u64) -> Result<String, Failure> {
    run_input_report(input, shots, seed).map(|r| r.out)
}

/// What `run` printed, and whether it simulated (and so drew outcomes from the seed).
#[allow(dead_code)]
pub struct Ran {
    pub out: String,
    pub simulated: bool,
}

/// `run_input`, also reporting whether the seed was used.
#[allow(dead_code)]
pub fn run_input_report(input: &Input, shots: u64, seed: u64) -> Result<Ran, Failure> {
    run_input_with(input, shots, seed, &Limits::CLI)
}

fn run_input_with(input: &Input, shots: u64, seed: u64, limits: &Limits) -> Result<Ran, Failure> {
    let (map, built) = build_input(input, limits);
    let (program, e) = built.map_err(|e| render(&map, &e, limits))?;
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
        return Ok(Ran {
            out: render_output(&e.output, None),
            simulated: false,
        });
    }
    let main_span = main_span(&program);
    let backend = e.circuit.select_backend();
    if let Some(limit) = limits.max_state_vector_qubits {
        if backend == Backend::StateVector && e.slots > limit {
            let reason = msg::playground_qubit_limit(e.slots, limit);
            return Err(render(
                &map,
                &[
                    Diagnostic::new(Code::Simulation, main_span, msg::simulation_failed(&reason))
                        .with_help(msg::help_playground_qubit_limit()),
                ],
                limits,
            ));
        }
    }
    // The stabilizer backend has no memory limit of its own; huge tableaus would abort on allocation.
    let tableau = stabilizer_bytes(e.slots);
    if backend == Backend::Stabilizer && tableau > u128::from(limits.stabilizer_bytes) {
        let reason =
            msg::stabilizer_too_large(e.slots, &gib(tableau), &gib(limits.stabilizer_bytes.into()));
        return Err(render(
            &map,
            &[
                Diagnostic::new(Code::Simulation, main_span, msg::simulation_failed(&reason))
                    .with_help(msg::help_too_large()),
            ],
            limits,
        ));
    }
    check_sim_work(&e, backend, shots, main_span, limits)
        .map_err(|d| render(&map, &[d], limits))?;
    // Every shot gets its own seed derived from `seed` (see `qxide::shot_seed`), shared with the
    // library and the playground, so runs with nearby seeds do not reuse each other's shots.
    let mut sims = e
        .circuit
        .simulate_shots(Backend::Auto, seed, shots)
        .map(|r| r.map_err(|err| render(&map, &[sim_error(err, main_span)], limits)));
    if shots == 1 {
        let shot = sims.next().expect("one shot")?;
        return Ok(Ran {
            out: render_output(&e.output, Some(&shot)),
            simulated: true,
        });
    }
    // The printed output depends only on the measured bits, so it is rendered once per record.
    // Each new record adds its table row, its indented output and its key to the output budget,
    // so many distinct records cannot multiply the output without bound (B-03).
    let mut groups: HashMap<Vec<bool>, (u64, String)> = HashMap::new();
    let mut bytes: u64 = 0;
    for shot in sims {
        let shot = shot?;
        let key: Vec<bool> = e.measured.iter().map(|&b| shot.get(b)).collect();
        if let Some(group) = groups.get_mut(&key) {
            group.0 += 1;
            continue;
        }
        let text = render_output(&e.output, Some(&shot));
        let bits = key.len() as u64;
        // `[0, 1]  0b10 = 2  ×N` is at most 4 bytes per bit plus 52; each output line gets
        // two spaces and a newline.
        let row = 4 * bits + 52;
        let lines = text.lines().count() as u64;
        bytes = bytes.saturating_add(row + bits + text.len() as u64 + 3 * lines);
        if bytes > limits.max_output_bytes {
            let d = Diagnostic::new(
                Code::OutputLimit,
                main_span,
                msg::shots_output_limit(shots, limits.max_output_bytes),
            );
            return Err(render(&map, &[d], limits));
        }
        groups.insert(key, (1, text));
    }
    let mut rows: Vec<(Vec<bool>, (u64, String))> = groups.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    let mut s = String::new();
    for (bits, (count, text)) in rows {
        s.push_str(&table_row(&bits, count));
        s.push('\n');
        for line in text.lines() {
            s.push_str("  ");
            s.push_str(line);
            s.push('\n');
        }
    }
    Ok(Ran {
        out: s,
        simulated: true,
    })
}
