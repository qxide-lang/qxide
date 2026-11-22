// `qxide repl`: the session is replayed as one program. Accepted inputs are kept as source text,
// and each new input is checked and run together with them through the same front end as
// `qxide run`, so every rule (including linearity) applies unchanged. The only difference is that
// qubits may stay live at the end of `main` until the session ends.

use std::io::{BufRead, Write};

use qxide::Backend;

use super::diag::{render_all, Code, Diagnostic, SourceMap, Span};
use super::{
    ast, check_qasm_size, check_sim_work, front, interp, lexer, msg, parser, render_output,
    sim_error, Failure, Limits,
};

const NAME: &str = "<repl>";

#[derive(Clone)]
struct Entry {
    text: String,
    // Session line number of the entry's first line, counting accepted inputs as they were typed.
    line: usize,
    // Set when `text` wraps what was typed (`println("{}", code)` or `code;`): diagnostics show the
    // typed input instead.
    typed: Option<Typed>,
}

#[derive(Clone)]
struct Typed {
    input: String,
    // `text` is `prefix + code + suffix + rest`, where `input` is `code + rest`.
    prefix: usize,
    code: usize,
    suffix: usize,
}

// Where an entry's wrapped text sits in the assembled program.
struct Region {
    start: usize,
    len: usize,
    typed: Typed,
}

pub struct Session {
    fns: Vec<Entry>,
    stmts: Vec<Entry>,
    next_line: usize,
    seed: u64,
    // Output already shown; later runs reprint it first because the whole session is replayed.
    shown: String,
    // The random seed's stderr note, held back until the first input that simulates.
    seed_note: Option<String>,
    // Set by that input; `run` writes it to stderr before the input's reply.
    note_due: Option<String>,
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
    regions: Vec<Region>,
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
            regions: Vec::new(),
            labels: Vec::new(),
            input_at: 0,
            main_close: Span::default(),
        };
        for f in fns {
            p.push_entry(f);
            p.push("", 0);
        }
        if let New::Fn(f) = new {
            p.input_at = p.src.len();
            p.push_entry(f);
            p.push("", 0);
        }
        p.push("void main() {", 0);
        for s in stmts {
            p.push_entry(s);
        }
        if let New::Stmt(s) = new {
            p.input_at = p.src.len();
            p.push_entry(s);
        }
        p.main_close = Span::new(p.src.len(), p.src.len() + 1);
        p.src.push('}');
        p.labels.push(0);
        p
    }

    fn push_entry(&mut self, e: &Entry) {
        if let Some(typed) = &e.typed {
            self.regions.push(Region {
                start: self.src.len(),
                len: e.text.len(),
                typed: typed.clone(),
            });
        }
        self.push(&e.text, e.line);
    }

    // `src` with every wrapped entry replaced by what was typed, and the matching offset map.
    fn display(&self) -> (String, impl Fn(usize) -> usize + '_) {
        let mut shown = String::with_capacity(self.src.len());
        let mut at = 0;
        for r in &self.regions {
            shown.push_str(&self.src[at..r.start]);
            shown.push_str(&r.typed.input);
            at = r.start + r.len;
        }
        shown.push_str(&self.src[at..]);
        let map = move |off: usize| {
            let mut delta: isize = 0;
            for r in &self.regions {
                let t = &r.typed;
                if off < r.start {
                    break;
                }
                if off <= r.start + r.len {
                    let rel = off - r.start;
                    // Offsets inside the added prefix/suffix move to the nearest typed character.
                    let typed_rel = if rel <= t.prefix {
                        0
                    } else if rel - t.prefix <= t.code {
                        rel - t.prefix
                    } else {
                        (rel - t.prefix - t.suffix).max(t.code)
                    };
                    let typed_rel = typed_rel.min(t.input.len());
                    return (r.start as isize + delta) as usize + typed_rel;
                }
                delta += t.input.len() as isize - r.len as isize;
            }
            (off as isize + delta) as usize
        };
        (shown, map)
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
        let (shown, at) = self.display();
        let span = |s: Span| Span::new(at(s.start), at(s.end));
        let map = SourceMap::new(NAME, &shown).with_labels(self.labels.clone());
        let limits = &Limits::CLI;
        Failure(render_all(errors, limits, |d| {
            let mut d = d.clone();
            d.span = span(d.span);
            for n in &mut d.notes {
                n.span = n.span.map(span);
            }
            map.render(&d, limits)
        }))
    }

    // Qubits still live when `main` ends are fine between inputs; this is the leak `Q0302` reports there.
    fn is_open_leak(&self, d: &Diagnostic) -> bool {
        d.code == Code::NeverConsumed
            && d.notes.first().and_then(|n| n.span) == Some(self.main_close)
    }
}

/// Brackets still open in `text`, ignoring string literals and `//` comments.
pub fn open_brackets(text: &str) -> i64 {
    scan(text).depth
}

struct Scan {
    depth: i64,
    // Lowest depth reached: below zero, the input closes a bracket it did not open.
    min_depth: i64,
    // Byte offset of a `//` comment that runs to the end of `text`, if any.
    trailing_comment: Option<usize>,
}

fn scan(text: &str) -> Scan {
    let mut s = Scan {
        depth: 0,
        min_depth: 0,
        trailing_comment: None,
    };
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                while let Some((_, c)) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' | '\n' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek().map(|&(_, c)| c) == Some('/') => {
                s.trailing_comment = Some(i);
                for (_, c) in chars.by_ref() {
                    if c == '\n' {
                        s.trailing_comment = None;
                        break;
                    }
                }
            }
            '{' | '(' | '[' => s.depth += 1,
            '}' | ')' | ']' => {
                s.depth -= 1;
                s.min_depth = s.min_depth.min(s.depth);
            }
            _ => {}
        }
    }
    s
}

// A `return` anywhere in the new input would end the session's `main` and skip later inputs.
fn has_return(stmts: &[ast::Stmt], from: usize) -> bool {
    stmts.iter().any(|s| match s {
        ast::Stmt::Return { span, .. } => span.start >= from,
        ast::Stmt::If(i) => if_has_return(i, from),
        ast::Stmt::For { body, .. } => has_return(&body.stmts, from),
        _ => false,
    })
}

fn if_has_return(i: &ast::IfStmt, from: usize) -> bool {
    has_return(&i.then.stmts, from)
        || match &i.els {
            Some(ast::Else::Block(b)) => has_return(&b.stmts, from),
            Some(ast::Else::If(inner)) => if_has_return(inner, from),
            None => false,
        }
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
            seed_note: None,
            note_due: None,
        }
    }

    /// Announces the seed on stderr once, when an input first draws measurement outcomes.
    pub fn announce_seed(&mut self) {
        self.seed_note = Some(msg::seed_note(self.seed));
    }

    /// The seed note due before the last input's reply, if any.
    pub fn take_seed_note(&mut self) -> Option<String> {
        self.note_due.take()
    }

    /// Handles one complete input: a `:command`, a function definition, statements, or an
    /// expression (printed like `println("{}", expr)`; a trailing `;` may be left out).
    pub fn input(&mut self, input: &str) -> Reply {
        // Piped files may start with a UTF-8 byte order mark, which `run` also ignores.
        let input = input.trim_start_matches('\u{feff}').trim();
        if input.is_empty() {
            return Reply::Out(String::new());
        }
        if let Some(cmd) = input.strip_prefix(':') {
            return self.command(cmd.trim());
        }
        let line = self.next_line;
        let entry = |text: String| Entry {
            text,
            line,
            typed: None,
        };
        let scanned = scan(input);
        // The code before a trailing `//` comment, which the added `);` or `;` must not follow.
        let code = input[..scanned.trailing_comment.unwrap_or(input.len())].trim_end();
        let rest = &input[code.len()..];
        // `prefix + code + suffix + rest`, shown in diagnostics as the typed `code + rest`.
        let wrap = |prefix: &str, suffix: &str| Entry {
            text: format!("{prefix}{code}{suffix}{rest}"),
            line,
            typed: Some(Typed {
                input: input.to_string(),
                prefix: prefix.len(),
                code: code.len(),
                suffix: suffix.len(),
            }),
        };
        let reply = if is_fn_decl(input) {
            let e = entry(input.to_string());
            self.try_run(New::Fn(&e)).map(|out| (e, true, out))
        } else if scanned.min_depth < 0 {
            // Closing `main` early would move every later input out of it.
            return Reply::Err(msg::repl_unbalanced());
        } else if code.ends_with(';') || code.ends_with('}') {
            let e = entry(input.to_string());
            self.try_run(New::Stmt(&e)).map(|out| (e, false, out))
        } else {
            // A bare expression is printed; anything else gets the `;` it was typed without.
            let echo = wrap("println(\"{}\", ", ");");
            match self.try_run(New::Stmt(&echo)) {
                Ok(out) => Ok((echo, false, out)),
                Err(_) => {
                    let e = wrap("", ";");
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
                // Same seed after a reset, so a note already shown is not repeated.
                let note = self.seed_note.take();
                *self = Session::new(self.seed);
                self.seed_note = note;
                Reply::Out(msg::repl_reset())
            }
            "qasm" => match self.build(New::Nothing) {
                Ok((p, e)) => {
                    let at = Span::new(p.input_at, p.input_at);
                    match check_qasm_size(&e, at, &Limits::CLI) {
                        Ok(()) => Reply::Out(e.circuit.to_qasm()),
                        Err(d) => Reply::Err(p.render(&[d]).0),
                    }
                }
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
            let top_return = main.is_some_and(|f| has_return(&f.body.stmts, p.input_at));
            // A top-level `return` would end `main` and silently skip every later input.
            if top_return {
                return Err(Failure(msg::repl_return()));
            }
        }
        // The session is replayed on every input, so effects other than logging are refused.
        let mut host = super::stdlib::LogOnly::default();
        let e =
            super::engine::elaborate_call(&program, "main", Vec::new(), &mut host, &Limits::CLI)
                .map(|(e, _)| e)
                .map_err(|d| p.render(&[d]))?;
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
            check_sim_work(&e, Backend::StateVector, 1, at, &Limits::CLI)
                .map_err(|d| p.render(&[d]))?;
            self.note_due = self.seed_note.take();
            // Shot 0 of `--seed S`, the same seed derivation as `qxide run`.
            let shot = e
                .circuit
                .simulate_shots(Backend::StateVector, self.seed, 1)
                .next()
                .expect("one shot")
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
/// With `announce`, the seed is noted on stderr once the session first simulates.
pub fn run(
    input: impl BufRead,
    out: &mut impl Write,
    err: &mut impl Write,
    seed: u64,
    announce: bool,
    interactive: bool,
) -> u8 {
    let mut session = Session::new(seed);
    if announce {
        session.announce_seed();
    }
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
                let reply = feed(&mut session, &buf, err);
                report(reply, out, err);
            }
            break;
        };
        buf.push_str(&line);
        buf.push('\n');
        if open_brackets(&buf) > 0 {
            continue;
        }
        let reply = feed(&mut session, &buf, err);
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

// One input, with the seed note (if this input is the first to simulate) written ahead of its reply.
fn feed(session: &mut Session, text: &str, err: &mut impl Write) -> Reply {
    let reply = session.input(text);
    if let Some(note) = session.take_seed_note() {
        let _ = write!(err, "{note}");
        let _ = err.flush();
    }
    reply
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
