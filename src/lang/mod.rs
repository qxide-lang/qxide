// The `.qx` language front end and interpreter (docs/design/qx-language.md).
// Private to the `qxide` binary: it only uses the library's public API.

mod ast;
mod check;
mod diag;
mod explain;
mod fmt;
mod interp;
mod lexer;
mod link;
mod msg;
mod parser;
pub mod repl;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;

#[cfg(test)]
mod module_tests;

use std::collections::HashMap;
use std::path::Path;

use qxide::{Backend, Shot, SimError, DEFAULT_MEMORY_LIMIT};

use diag::{Code, Diagnostic, SourceMap};
use interp::{BitsFmt, Elaborated, OutPiece};
// The package manager (CLI only) builds `PackageRoot`s; the playground build never does.
#[allow(unused_imports)]
pub use link::{PackageRoot, Project, ReadFn};

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
) -> (
    SourceMap,
    Result<(ast::Program, Elaborated), Vec<Diagnostic>>,
) {
    let (map, program) = front_input(input, true);
    let built = program.and_then(|program| {
        let elaborated = interp::elaborate(&program).map_err(|d| vec![d])?;
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
    build_input(&Input::file("t.qx", src)).1
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

/// `qxide explain`: the catalog entry for an error code such as `Q0302` (case-insensitive).
pub fn explain(arg: &str) -> Option<String> {
    explain::explain(&explain::normalize(arg)?)
}

/// `qxide qasm`: elaborate once and print the OpenQASM 3 of the resulting circuit.
pub fn qasm_input(input: &Input) -> Result<String, Failure> {
    let (map, built) = build_input(input);
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

/// Resource limits for `run`. The CLI uses `Limits::CLI`; the browser playground passes tighter ones.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most qubit slots the state-vector backend may simulate; `None` leaves it to the memory limit.
    pub max_state_vector_qubits: Option<usize>,
    /// Largest stabilizer tableau, in bytes.
    pub stabilizer_bytes: u64,
}

impl Limits {
    pub const CLI: Limits = Limits {
        max_state_vector_qubits: None,
        stabilizer_bytes: DEFAULT_MEMORY_LIMIT,
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
    run_input_with(&Input::file(name, src), shots, seed, limits)
}

/// `qxide run` of an entry file and its imports; see `run`.
#[allow(dead_code)]
pub fn run_input(input: &Input, shots: u64, seed: u64) -> Result<String, Failure> {
    run_input_with(input, shots, seed, Limits::CLI)
}

fn run_input_with(input: &Input, shots: u64, seed: u64, limits: Limits) -> Result<String, Failure> {
    let (map, built) = build_input(input);
    let (program, e) = built.map_err(|e| render(&map, &e))?;
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
        return Ok(render_output(&e.output, None));
    }
    let main_span = program
        .fns
        .iter()
        .find(|f| f.name.name == "main")
        .map_or(diag::Span::default(), |f| f.name.span);
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
        ));
    }
    let simulate = |k: u64| {
        e.circuit
            .simulate_with(Backend::Auto, seed.wrapping_add(k))
            .map_err(|err| render(&map, &[sim_error(err, main_span)]))
    };
    if shots == 1 {
        return Ok(render_output(&e.output, Some(&simulate(0)?)));
    }
    // The printed output depends only on the measured bits, so it is rendered once per record.
    let mut groups: HashMap<Vec<bool>, (u64, String)> = HashMap::new();
    for k in 0..shots {
        let shot = simulate(k)?;
        let key: Vec<bool> = e.measured.iter().map(|&b| shot.get(b)).collect();
        groups
            .entry(key)
            .or_insert_with(|| (0, render_output(&e.output, Some(&shot))))
            .0 += 1;
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
    Ok(s)
}
