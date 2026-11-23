// The `.qx` language front end and interpreter (docs/design/qx-language.md).
// Private to the `qxide` binary: it only uses the library's public API.

mod array;
mod ast;
mod check;
// The desktop runtime (crates/qxide-app-runtime) uses all of it; the CLI only checks commands.
#[allow(dead_code)]
pub mod command;
mod diag;
// `qxide doc`: the CLI only.
#[allow(dead_code)]
pub mod doc;
mod engine;
mod explain;
mod ffi;
mod fmt;
pub mod ide;
mod interp;
pub mod ir;
// Cranelift JIT for `--engine jit`; the playground (wasm) and app runtime build without it.
#[cfg(feature = "jit")]
mod jit;
mod json;
mod lexer;
mod link;
mod machine;
mod mono;
mod msg;
mod names;
mod native;
mod parser;
mod pure;
mod record;
pub mod repl;
pub mod stdlib;
// Used by the desktop runtime for Qxide Studio (`std.qxide`).
#[allow(dead_code)]
pub mod studio;

#[cfg(test)]
mod bench;
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

#[cfg(test)]
mod limits_tests;

#[cfg(test)]
mod app_limits_tests;

#[cfg(test)]
mod names_tests;

#[cfg(test)]
mod data_tests;

use std::collections::HashMap;
use std::path::Path;

use qxide::{Backend, Shot, SimError, DEFAULT_MEMORY_LIMIT};

use diag::{Code, Diagnostic, SourceMap};
use interp::{BitsFmt, Elaborated, OpCounts, OutPiece};
use stdlib::{Host, Permissions};
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

// The CLI and the playground pick the interpreter per run (`--engine`); see engine.rs.
#[allow(unused_imports)]
pub use engine::{set as set_engine, Engine};

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
    set_warnings(&map, &[]);
    let checked = linked.and_then(|mut program| {
        let (errors, inferred) = check::check_infer(&program, require_main);
        set_warnings(&map, &inferred.warnings);
        if !errors.is_empty() {
            return Err(errors);
        }
        // Deprecated aliases become their canonical names before any engine sees the program.
        names::rename(&mut program, &inferred.renames);
        let program = mono::monomorphize(program, &inferred).map_err(|d| vec![d])?;
        // An instance such as `Wrap<Shape>` may reach itself through an interface.
        let cycles = check::type_cycles(&program);
        if !cycles.is_empty() {
            return Err(cycles);
        }
        // Instances are ordinary declarations: the concrete program checks cleanly again.
        #[cfg(debug_assertions)]
        {
            let again = check::check_with(&program, require_main);
            debug_assert!(again.is_empty(), "monomorphized program: {again:?}");
        }
        Ok(program)
    });
    (map, checked)
}

thread_local! {
    // Warnings (`Q0801`) of the latest front-end run on this thread, rendered.
    static WARNINGS: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn set_warnings(map: &SourceMap, warnings: &[Diagnostic]) {
    let text = if warnings.is_empty() {
        String::new()
    } else {
        diag::render_all(warnings, &Limits::CLI, |d| map.render(d, &Limits::CLI))
            + "
"
    };
    WARNINGS.with(|w| *w.borrow_mut() = text);
}

/// The rendered warnings of the latest `check`, `qasm` or `run` on this thread (empty if there
/// were none), for stderr. Warnings never change the output or the exit code.
pub fn take_warnings() -> String {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

fn build_input(
    input: &Input,
    host: &mut dyn Host,
    limits: &Limits,
) -> (
    SourceMap,
    Result<(ast::Program, Elaborated), Vec<Diagnostic>>,
) {
    let (map, program) = front_input(input, true);
    let built = program.and_then(|program| {
        let (mut elaborated, ret) =
            engine::elaborate_call(&program, "main", Vec::new(), host, limits)
                .map_err(|d| vec![d])?;
        if let interp::Value::Int(n) = ret {
            elaborated.exit_code = u8::try_from(n).map_err(|_| {
                vec![Diagnostic::new(
                    Code::StdArgument,
                    main_span(&program),
                    msg::exit_code_range(n),
                )]
            })?;
        }
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
    build_input(
        &Input::file("t.qx", src),
        &mut stdlib::LogOnly::default(),
        &Limits::CLI,
    )
    .1
}

/// One test function run by `qxide test`: its name and, when it failed, the rendered error.
pub struct TestOutcome {
    pub name: String,
    pub result: Result<(), String>,
}

// A test function: `void testName()` (or `test` alone, `test_x`, `test2`) declared in the file
// itself, not generic and not a method.
fn is_test(f: &ast::FnDecl) -> bool {
    let Some(rest) = f.name.name.strip_prefix("test") else {
        return false;
    };
    f.method.is_none()
        && f.params.is_empty()
        && f.tparams.is_empty()
        && f.ret == ast::Ty::Void
        && !f.name.name.contains('.')
        && rest
            .chars()
            .next()
            .is_none_or(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// `qxide test` of one file: each test function whose name contains `filter`, in source order,
/// elaborated on its own like a command (nothing is simulated). `Err` when the file does not
/// pass the checker.
#[allow(dead_code)]
pub fn test_input(
    input: &Input,
    filter: &str,
    host: &mut dyn Host,
    limits: &Limits,
) -> Result<Vec<TestOutcome>, Failure> {
    let (map, program) = front_input(input, false);
    let program = program.map_err(|e| render(&map, &e, limits))?;
    let names: Vec<String> = program
        .fns
        .iter()
        .filter(|f| is_test(f) && f.name.name.contains(filter))
        .map(|f| f.name.name.clone())
        .collect();
    Ok(names
        .into_iter()
        .map(|name| {
            let result = engine::elaborate_call(&program, &name, Vec::new(), host, limits)
                .map(drop)
                .map_err(|d| render(&map, &[d], limits).0);
            TestOutcome { name, result }
        })
        .collect())
}

/// Whether this build can call native functions (`extern`, `--allow ffi`).
pub const FFI_SUPPORTED: bool = ffi::SUPPORTED;

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

/// `check_input` with explicit limits on the rendered diagnostics (the playground passes its own).
#[allow(dead_code)]
pub fn check_input_with(input: &Input, limits: &Limits) -> Result<(), Failure> {
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

/// `std.X is not available in HOST` (Q0702), for hosts outside this module.
#[allow(dead_code)]
pub fn std_unavailable(what: &str, host: &str) -> String {
    msg::std_unavailable(what, host)
}

/// `setSize` outside 200..=8192 pixels (Q0706), for the app runtime.
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

/// Every catalog entry as (code, text), for tests outside `lang`.
#[cfg(test)]
pub fn explain_entries() -> Vec<(String, String)> {
    explain::entries()
}

/// `qxide qasm`: elaborate once and print the OpenQASM 3 of the resulting circuit. Effects are
/// refused (`std.log` is dropped), so exporting a circuit never writes files.
pub fn qasm_input(input: &Input) -> Result<String, Failure> {
    qasm_input_with(input, &Limits::CLI)
}

/// `qasm` of a lone file with explicit resource limits.
#[allow(dead_code)]
pub fn qasm_with(name: &str, src: &str, limits: &Limits) -> Result<String, Failure> {
    qasm_input_with(&Input::file(name, src), limits)
}

/// `qasm_input` with explicit resource limits (the playground passes its own).
#[allow(dead_code)]
pub fn qasm_input_with(input: &Input, limits: &Limits) -> Result<String, Failure> {
    let mut host = stdlib::NoEffects {
        what: "`qxide qasm`",
    };
    let (map, built) = build_input(input, &mut host, limits);
    let (program, e) = built.map_err(|e| render(&map, &e, limits))?;
    check_qasm_size(&e, main_span(&program), limits).map_err(|d| render(&map, &[d], limits))?;
    Ok(e.circuit.to_qasm())
}

/// A program elaborated for an external quantum device (`qxide run --backend`): its OpenQASM 3
/// and what it prints for each measurement record the device returns.
#[allow(dead_code)]
pub struct Exported {
    pub qasm: String,
    /// Width of the classical register `c`: every measurement result, in order.
    pub measured: usize,
    output: Vec<OutPiece>,
    bits: Vec<qxide::Bit>,
    max_output_bytes: u64,
}

/// Elaborates once like `qxide qasm` (effects refused) and keeps the output template, so records
/// measured elsewhere print the same way as simulated shots.
#[allow(dead_code)]
pub fn export_input(input: &Input) -> Result<Exported, Failure> {
    let limits = Limits::CLI;
    let mut host = stdlib::NoEffects {
        what: "`qxide run --backend`",
    };
    let (map, built) = build_input(input, &mut host, &limits);
    let (program, e) = built.map_err(|e| render(&map, &e, &limits))?;
    check_qasm_size(&e, main_span(&program), &limits).map_err(|d| render(&map, &[d], &limits))?;
    Ok(Exported {
        qasm: e.circuit.to_qasm(),
        measured: e.measured.len(),
        output: e.output,
        bits: e.measured,
        max_output_bytes: limits.max_output_bytes,
    })
}

#[allow(dead_code)]
impl Exported {
    /// The output of `qxide run` for these records (each one value of `c[0..measured]`): the
    /// program's output for a single record, else the `--shots` table. Errors with the reason
    /// when the table would pass the output limit.
    pub fn render(&self, records: &[Vec<bool>]) -> Result<String, String> {
        let text = |record: &[bool]| {
            render_output_with(&self.output, &|b| {
                self.bits
                    .iter()
                    .position(|&m| m == b)
                    .is_some_and(|i| record[i])
            })
        };
        if let [record] = records {
            return Ok(text(record));
        }
        let mut groups: HashMap<Vec<bool>, (u64, String)> = HashMap::new();
        let mut bytes: u64 = 0;
        for record in records {
            if let Some(group) = groups.get_mut(record) {
                group.0 += 1;
                continue;
            }
            let out = text(record);
            // Same accounting as the simulated table in `run_built`.
            let bits = record.len() as u64;
            let lines = out.lines().count() as u64;
            bytes = bytes.saturating_add(4 * bits + 52 + bits + out.len() as u64 + 3 * lines);
            if bytes > self.max_output_bytes {
                return Err(msg::shots_output_limit(
                    records.len() as u64,
                    self.max_output_bytes,
                ));
            }
            groups.insert(record.clone(), (1, out));
        }
        let mut s = String::new();
        for (bits, (count, out)) in sorted_groups(groups) {
            s.push_str(&table_row(&bits, count));
            s.push('\n');
            for line in out.lines() {
                s.push_str("  ");
                s.push_str(line);
                s.push('\n');
            }
        }
        Ok(s)
    }
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
    render_output_with(out, &|b| shot.is_some_and(|s| s.get(b)))
}

fn render_output_with(out: &[OutPiece], bit: &dyn Fn(qxide::Bit) -> bool) -> String {
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
                    // The interpreter rejects `asInt()` on registers wider than 64 bits.
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

/// Resource limits for `check`, `qasm` and `run`. The CLI uses `Limits::CLI`, desktop apps
/// `Limits::APP`, the browser playground `Limits::PLAYGROUND`.
/// docs/security/EXECUTION_LIMITS_FIXES.md records how the defaults were chosen.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most qubit slots the state-vector backend may simulate; `None` leaves it to the memory limit.
    pub max_state_vector_qubits: Option<usize>,
    /// Largest stabilizer tableau, in bytes.
    pub stabilizer_bytes: u64,
    /// Where the limits apply, for the wording of the diagnostic.
    pub scope: LimitScope,
    /// Interpreter steps: one per evaluated expression, statement, loop iteration and call, plus
    /// one per qubit or bit of whole-register work (`qreg`, `measure`, `discard`, calls, `println`).
    pub max_steps: u64,
    /// Operations recorded in the circuit: gates, measurements, resets and conditional blocks.
    pub max_ops: u64,
    /// Estimated simulator work for all shots of a run, in units of about one state-vector
    /// amplitude update (see `shot_work`).
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
        max_steps: 1 << 30,
        max_ops: 1 << 24,
        max_sim_work: 1 << 36,
        max_output_bytes: 256 << 20,
        max_diagnostics: 50,
        max_line_chars: 120,
        max_cond_depth: 64,
    };

    /// `qxide run --trusted` and `qxide qasm --trusted`: the CLI with larger execution budgets
    /// for local programs the user trusts (D7). Only the command line can select it, never a
    /// source file, a project file or a `qxide build` bundle. Nesting and condition depth stay
    /// as in `CLI`, since `STACK_SIZE` is sized for them.
    #[allow(dead_code)]
    pub const TRUSTED: Limits = Limits {
        max_steps: 1 << 36,
        max_ops: 1 << 26,
        max_sim_work: 1 << 40,
        max_output_bytes: 1 << 30,
        ..Limits::CLI
    };

    /// The qxide.dev playground: 20 state-vector qubits (16 MiB), a 512 MiB tableau, and budgets
    /// that keep a run well inside the page's 30 s worker limit.
    // Read by the wasm binding only.
    #[allow(dead_code)]
    pub const PLAYGROUND: Limits = Limits {
        max_state_vector_qubits: Some(20),
        stabilizer_bytes: 512 << 20,
        scope: LimitScope::Playground,
        max_steps: 1 << 26,
        max_ops: 1 << 20,
        max_sim_work: 1 << 32,
        max_output_bytes: 1 << 20,
        max_diagnostics: 50,
        max_line_chars: 120,
        max_cond_depth: 64,
    };

    /// Each call of a desktop app (commands and `std.qxide.run`); the runtime bounds how many
    /// run at once, so the total stays within one budget. Calls can be cancelled from the page,
    /// so time budgets are larger than the CLI's; memory budgets are per call and four calls may
    /// run at once. docs/security/EXECUTION_LIMITS_FIXES.md (part 2) records the measurements.
    #[allow(dead_code)]
    pub const APP: Limits = Limits {
        max_state_vector_qubits: Some(APP_STATE_VECTOR_QUBITS),
        stabilizer_bytes: APP_STABILIZER_BYTES,
        scope: LimitScope::App,
        max_steps: 1 << 31,
        // About 40 bytes of circuit per operation: 160 MiB per call.
        max_ops: 1 << 22,
        // One shot cannot be cancelled, so this stays at the CLI's worst case.
        max_sim_work: 1 << 36,
        // Anything larger could not reach the page (`MAX_SCRIPT_BYTES` in the runtime, Q0716).
        max_output_bytes: 32 << 20,
        max_diagnostics: 100,
        max_line_chars: 240,
        max_cond_depth: 128,
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
    run_input_report(input, shots, seed, host).map(|r| r.text)
}

/// `run_input`, also reporting whether the seed was used.
#[allow(dead_code)]
pub fn run_input_report(
    input: &Input,
    shots: u64,
    seed: u64,
    host: &mut dyn Host,
) -> Result<RunOut, Failure> {
    run_input_report_with(input, shots, seed, Limits::CLI, host)
}

/// `run_input_report` with explicit resource limits (`Limits::TRUSTED` for `run --trusted`).
#[allow(dead_code)]
pub fn run_input_report_with(
    input: &Input,
    shots: u64,
    seed: u64,
    limits: Limits,
    host: &mut dyn Host,
) -> Result<RunOut, Failure> {
    let (map, run) = run_diag(input, shots, seed, limits, host);
    run.map_err(|e| render(&map, &e, &limits))
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
    run.map(|r| r.text).map_err(|e| render(&map, &e, &limits))
}

/// A finished `run`: the CLI's stdout, plus the table rows (measurement record, count) when
/// several shots were simulated.
pub struct RunOut {
    pub text: String,
    pub rows: Vec<(Vec<bool>, u64)>,
    /// Whether it simulated, and so drew outcomes from the seed.
    pub simulated: bool,
    /// What `int main()` returned (0 for `void main()`).
    pub exit_code: u8,
}

/// `run_input_with` with unrendered diagnostics, for editors.
fn run_diag(
    input: &Input,
    shots: u64,
    seed: u64,
    limits: Limits,
    host: &mut dyn Host,
) -> (SourceMap, Result<RunOut, Vec<Diagnostic>>) {
    let (map, built) = build_input(input, host, &limits);
    let run = built.and_then(|(program, e)| {
        run_built(&program, &e, shots, seed, limits, host).map(|r| RunOut {
            exit_code: e.exit_code,
            ..r
        })
    });
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
    let done = |text: String, simulated: bool| RunOut {
        exit_code: 0,
        text,
        rows: Vec::new(),
        simulated,
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
        return Ok(done(render_output(&e.output, None), false));
    }
    let main_span = main_span(program);
    let sims = sample(e, main_span, shots, seed, limits).map_err(|d| vec![d])?;
    let mut sims = sims.map(|r| {
        if host.cancelled() {
            return Err(vec![cancelled(main_span)]);
        }
        r.map_err(|d| vec![d])
    });
    if shots == 1 {
        let shot = sims.next().expect("one shot")?;
        return Ok(done(render_output(&e.output, Some(&shot)), true));
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
            return Err(vec![Diagnostic::new(
                Code::OutputLimit,
                main_span,
                msg::shots_output_limit(shots, limits.max_output_bytes),
            )]);
        }
        groups.insert(key, (1, text));
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
    Ok(RunOut {
        text: s,
        rows,
        simulated: true,
        exit_code: 0,
    })
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
    check_sim_work(e, backend, shots, at, &limits)?;
    Ok(e.circuit
        .simulate_shots(Backend::Auto, seed, shots)
        .map(move |r| r.map_err(|err| sim_error(err, at))))
}
