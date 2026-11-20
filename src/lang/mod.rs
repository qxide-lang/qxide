// The `.qx` language front end and interpreter (docs/design/qx-language.md).
// Private to the `qxide` binary: it only uses the library's public API.

mod ast;
mod check;
mod diag;
mod interp;
mod lexer;
mod msg;
mod parser;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use qxide::{Backend, Shot};

use diag::{Code, Diagnostic, SourceMap};
use interp::{Elaborated, OutPiece};

/// Rendered diagnostics, ready for stderr.
pub struct Failure(pub String);

fn front(src: &str) -> Result<ast::Program, Vec<Diagnostic>> {
    let toks = lexer::lex(src).map_err(|d| vec![d])?;
    let program = parser::parse(toks).map_err(|d| vec![d])?;
    let errors = check::check(&program);
    if errors.is_empty() {
        Ok(program)
    } else {
        Err(errors)
    }
}

fn build(src: &str) -> Result<(ast::Program, Elaborated), Vec<Diagnostic>> {
    let program = front(src)?;
    let elaborated = interp::elaborate(&program).map_err(|d| vec![d])?;
    Ok((program, elaborated))
}

fn render(name: &str, src: &str, errors: &[Diagnostic]) -> Failure {
    let map = SourceMap::new(name, src);
    Failure(
        errors
            .iter()
            .map(|d| map.render(d))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// `qxide check`: lexing, parsing and static checks only.
pub fn check(name: &str, src: &str) -> Result<(), Failure> {
    front(src).map(|_| ()).map_err(|e| render(name, src, &e))
}

/// `qxide qasm`: elaborate once and print the OpenQASM 3 of the resulting circuit.
pub fn qasm(name: &str, src: &str) -> Result<String, Failure> {
    let (_, e) = build(src).map_err(|e| render(name, src, &e))?;
    Ok(e.circuit.to_qasm())
}

fn render_output(out: &[OutPiece], shot: Option<&Shot>) -> String {
    let bit = |b: qxide::Bit| match shot {
        Some(s) if s.get(b) => '1',
        _ => '0',
    };
    let mut s = String::new();
    for p in out {
        match p {
            OutPiece::Text(t) => s.push_str(t),
            OutPiece::Bit(b) => s.push(bit(*b)),
            OutPiece::Bits(v) => s.extend(v.iter().map(|&b| bit(b))),
        }
    }
    s
}

/// `qxide run`: elaborate once, simulate `shots` times, render the `println` output per shot.
pub fn run(name: &str, src: &str, shots: u64, seed: u64) -> Result<String, Failure> {
    let (program, e) = build(src).map_err(|e| render(name, src, &e))?;
    let needs_sim = e
        .output
        .iter()
        .any(|p| matches!(p, OutPiece::Bit(_) | OutPiece::Bits(_)));
    let mut counts: HashMap<String, u64> = HashMap::new();
    if needs_sim {
        for k in 0..shots {
            let shot = e
                .circuit
                .simulate_with(Backend::Auto, seed.wrapping_add(k))
                .map_err(|err| {
                    let at = program
                        .fns
                        .iter()
                        .find(|f| f.name.name == "main")
                        .map_or(diag::Span::default(), |f| f.name.span);
                    render(
                        name,
                        src,
                        &[Diagnostic::new(
                            Code::Simulation,
                            at,
                            msg::simulation_failed(&err.to_string()),
                        )],
                    )
                })?;
            *counts
                .entry(render_output(&e.output, Some(&shot)))
                .or_default() += 1;
        }
    } else {
        counts.insert(render_output(&e.output, None), shots);
    }
    if shots == 1 {
        return Ok(counts.into_keys().next().unwrap_or_default());
    }
    let mut rows: Vec<(String, u64)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut s = String::new();
    for (text, n) in rows {
        s.push_str(&format!("== {n}/{shots} ==\n{text}"));
        if !text.is_empty() && !text.ends_with('\n') {
            s.push('\n');
        }
    }
    Ok(s)
}
