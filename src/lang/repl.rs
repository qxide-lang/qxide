// `qxide repl`: the session is replayed as one program. Accepted inputs are kept as source text,
// and each new input is checked and run together with them through the same front end as
// `qxide run`, so every rule (including linearity) applies unchanged. The only difference is that
// qubits may stay live at the end of `main` until the session ends.

use std::io::{BufRead, Write};

use qxide::Backend;

use super::diag::{Code, Diagnostic, SourceMap, Span};
use super::{ast, front, interp, lexer, msg, parser, render_output, sim_error, Failure};

const NAME: &str = "<repl>";

#[derive(Clone)]
struct Entry {
    text: String,
    // Session line number of the entry's first line, counting accepted inputs as they were typed.
    line: usize,
}

pub struct Session {
    fns: Vec<Entry>,
    stmts: Vec<Entry>,
    next_line: usize,
    seed: u64,
    // Output already shown; later runs reprint it first because the whole session is replayed.
    shown: String,
}

/// What one input produced: text for stdout, or rendered diagnostics for stderr.
pub enum Reply {
    Out(String),
    Err(String),
    Quit,
}

enum New<'a> {
    Nothing,
    Fn(&'a Entry),
    Stmt(&'a Entry),
}

struct Program {
    src: String,
    // Session line number of every line of `src`; 0 for the lines the REPL adds around `main`.
    labels: Vec<usize>,
    // Offset of the new input inside `src`, and the closing brace of `main`.
    input_at: usize,
    main_close: Span,
}

impl Program {
    fn assemble(fns: &[Entry], stmts: &[Entry], new: New) -> Program {
        let mut p = Program {
            src: String::new(),
            labels: Vec::new(),
            input_at: 0,
            main_close: Span::default(),
        };
        for f in fns {
            p.push(&f.text, f.line);
            p.push("", 0);
        }
        if let New::Fn(f) = new {
            p.input_at = p.src.len();
            p.push(&f.text, f.line);
            p.push("", 0);
        }
        p.push("void main() {", 0);
        for s in stmts {
            p.push(&s.text, s.line);
        }
        if let New::Stmt(s) = new {
            p.input_at = p.src.len();
            p.push(&s.text, s.line);
        }
        p.main_close = Span::new(p.src.len(), p.src.len() + 1);
        p.src.push('}');
        p.labels.push(0);
        p
    }

    fn push(&mut self, text: &str, first_line: usize) {
        for (i, line) in text.split('\n').enumerate() {
            self.src.push_str(line);
            self.src.push('\n');
            self.labels
                .push(if first_line == 0 { 0 } else { first_line + i });
        }
    }

    fn render(&self, errors: &[Diagnostic]) -> Failure {
        let map = SourceMap::new(NAME, &self.src).with_labels(self.labels.clone());
        let text: Vec<String> = errors.iter().map(|d| map.render(d)).collect();
        Failure(text.join("\n"))
    }

    // Qubits still live when `main` ends are fine between inputs; this is the leak `Q0302` reports there.
    fn is_open_leak(&self, d: &Diagnostic) -> bool {
        d.code == Code::NeverConsumed
            && d.notes.first().and_then(|n| n.span) == Some(self.main_close)
    }
}

/// Brackets still open in `text`, ignoring string literals and `//` comments.
pub fn open_brackets(text: &str) -> i64 {
    let (mut depth, mut chars) = (0i64, text.chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' | '\n' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn is_fn_decl(input: &str) -> bool {
    lexer::lex(input)
        .ok()
        .and_then(|toks| parser::parse(toks).ok())
        .is_some_and(|p| !p.fns.is_empty())
}

impl Session {
    pub fn new(seed: u64) -> Self {
        Self {
            fns: Vec::new(),
            stmts: Vec::new(),
            next_line: 1,
            seed,
            shown: String::new(),
        }
    }

    /// Handles one complete input: a `:command`, a function definition, statements, or an
    /// expression (printed like `println("{}", expr)`; a trailing `;` may be left out).
    pub fn input(&mut self, input: &str) -> Reply {
        let input = input.trim();
        if input.is_empty() {
            return Reply::Out(String::new());
        }
        if let Some(cmd) = input.strip_prefix(':') {
            return self.command(cmd.trim());
        }
        let line = self.next_line;
        let entry = |text: String| Entry { text, line };
        let reply = if is_fn_decl(input) {
            let e = entry(input.to_string());
            self.try_run(New::Fn(&e)).map(|out| (e, true, out))
        } else if input.ends_with(';') || input.ends_with('}') {
            let e = entry(input.to_string());
            self.try_run(New::Stmt(&e)).map(|out| (e, false, out))
        } else {
            // A bare expression is printed; anything else gets the `;` it was typed without.
            let echo = entry(format!("println(\"{{}}\", {input});"));
            match self.try_run(New::Stmt(&echo)) {
                Ok(out) => Ok((echo, false, out)),
                Err(_) => {
                    let e = entry(format!("{input};"));
                    self.try_run(New::Stmt(&e)).map(|out| (e, false, out))
                }
            }
        };
        match reply {
            Ok((e, is_fn, out)) => {
                self.next_line += input.lines().count();
                if is_fn {
                    self.fns.push(e);
                } else {
                    self.stmts.push(e);
                }
                Reply::Out(out)
            }
            Err(Failure(d)) => Reply::Err(d),
        }
    }

    fn command(&mut self, cmd: &str) -> Reply {
        match cmd {
            "help" => Reply::Out(msg::repl_help()),
            "quit" | "q" => Reply::Quit,
            "reset" => {
                *self = Session::new(self.seed);
                Reply::Out(msg::repl_reset())
            }
            "qasm" => match self.build(New::Nothing) {
                Ok((_, e)) => Reply::Out(e.circuit.to_qasm()),
                Err(f) => Reply::Err(f.0),
            },
            other => Reply::Err(msg::repl_unknown_command(other)),
        }
    }

    /// Diagnostics for qubits that are still live, reported when the session ends.
    pub fn leaks(&self) -> Option<String> {
        let p = Program::assemble(&self.fns, &self.stmts, New::Nothing);
        let mut errors = front(&p.src).err()?;
        // The closing brace of `main` is not something the user typed; leave its note out.
        for d in &mut errors {
            d.notes.retain(|n| n.span != Some(p.main_close));
        }
        Some(p.render(&errors).0)
    }

    fn build(&self, new: New) -> Result<(Program, interp::Elaborated), Failure> {
        let is_stmt = matches!(new, New::Stmt(_));
        let p = Program::assemble(&self.fns, &self.stmts, new);
        let program = match front(&p.src) {
            Ok(program) => program,
            Err(errors) => {
                let errors: Vec<Diagnostic> =
                    errors.into_iter().filter(|d| !p.is_open_leak(d)).collect();
                if !errors.is_empty() {
                    return Err(p.render(&errors));
                }
                // Only open leaks: parse again, since `front` drops the AST on any error.
                let toks = lexer::lex(&p.src).map_err(|d| p.render(&[d]))?;
                parser::parse(toks).map_err(|d| p.render(&[d]))?
            }
        };
        if is_stmt {
            let main = program.fns.iter().find(|f| f.name.name == "main");
            let top_return = main
                .into_iter()
                .flat_map(|f| &f.body.stmts)
                .any(|s| matches!(s, ast::Stmt::Return { span, .. } if span.start >= p.input_at));
            // A top-level `return` would end `main` and silently skip every later input.
            if top_return {
                return Err(Failure(msg::repl_return()));
            }
        }
        let e = interp::elaborate(&program).map_err(|d| p.render(&[d]))?;
        Ok((p, e))
    }

    // Replays the session plus the new input and returns only the output the input added.
    fn try_run(&mut self, new: New) -> Result<String, Failure> {
        let (p, e) = self.build(new)?;
        let prints_bits = e
            .output
            .iter()
            .any(|o| matches!(o, interp::OutPiece::Bit(_) | interp::OutPiece::Bits(..)));
        // Always the state-vector backend: `Auto` would switch backends once a non-Clifford gate
        // appears, and another backend may draw different outcomes for bits already shown.
        let out = if prints_bits {
            let at = Span::new(p.input_at, p.input_at);
            let shot = e
                .circuit
                .simulate_with(Backend::StateVector, self.seed)
                .map_err(|err| p.render(&[sim_error(err, at)]))?;
            render_output(&e.output, Some(&shot))
        } else {
            render_output(&e.output, None)
        };
        let new = out
            .strip_prefix(self.shown.as_str())
            .unwrap_or(&out)
            .to_string();
        self.shown = out;
        Ok(new)
    }
}

/// Runs the read-eval-print loop until `:quit` or end of input. Returns the process exit code:
/// 0, or 1 when the session ends with qubits that were never measured or discarded.
pub fn run(
    input: impl BufRead,
    out: &mut impl Write,
    err: &mut impl Write,
    seed: u64,
    interactive: bool,
) -> u8 {
    let mut session = Session::new(seed);
    if interactive {
        let _ = write!(out, "{}", msg::repl_banner(env!("CARGO_PKG_VERSION")));
    }
    let mut lines = input.lines();
    let mut buf = String::new();
    loop {
        if interactive {
            let _ = write!(out, "{}", if buf.is_empty() { "qx> " } else { "... " });
            let _ = out.flush();
        }
        let Some(Ok(line)) = lines.next() else {
            if !buf.trim().is_empty() {
                report(session.input(&buf), out, err);
            }
            break;
        };
        buf.push_str(&line);
        buf.push('\n');
        if open_brackets(&buf) > 0 {
            continue;
        }
        let reply = session.input(&buf);
        buf.clear();
        if matches!(reply, Reply::Quit) {
            break;
        }
        report(reply, out, err);
    }
    match session.leaks() {
        Some(d) => {
            let _ = write!(err, "{d}");
            1
        }
        None => 0,
    }
}

fn report(reply: Reply, out: &mut impl Write, err: &mut impl Write) {
    match reply {
        Reply::Out(s) => {
            let _ = write!(out, "{s}");
            let _ = out.flush();
        }
        Reply::Err(s) => {
            let _ = write!(err, "{s}");
            let _ = err.flush();
        }
        Reply::Quit => {}
    }
}
