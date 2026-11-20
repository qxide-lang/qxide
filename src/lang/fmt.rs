// `qxide fmt`: re-emits the token stream with canonical layout (4-space indent, `{` at the end of
// the line, `} else {`, one statement per line, spaces around binary operators). Comments are
// kept, every token is copied from its source slice, and the result must lex to the same tokens.

use super::ast::Ty;
use super::diag::{Diagnostic, Span};
use super::lexer::{self, Tok, Token};
use super::parser;

const INDENT: &str = "    ";

enum Item {
    Tok(usize),
    Comment(Span),
}

impl Item {
    fn start(&self, toks: &[Token]) -> usize {
        match self {
            Item::Tok(i) => toks[*i].span.start,
            Item::Comment(s) => s.start,
        }
    }
}

/// Formats `.qx` source. Fails with the lexer or parser diagnostic if the source does not parse;
/// keeps a leading byte order mark and CRLF line endings when the input uses them.
pub fn format(src: &str) -> Result<String, Vec<Diagnostic>> {
    let (bom, body) = match src.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", src),
    };
    let (toks, comments) = lexer::lex_with_comments(body).map_err(|d| vec![d])?;
    parser::parse(toks.clone()).map_err(|d| vec![d])?;
    let out = Printer::new(body, &toks).run(&comments);
    let out = if body.contains("\r\n") {
        out.replace('\n', "\r\n")
    } else {
        out
    };
    Ok(format!("{bom}{out}"))
}

/// Token kinds and comment texts, the parts of a program that formatting must not change.
pub fn fingerprint(src: &str) -> Option<(Vec<Tok>, Vec<String>)> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let (toks, comments) = lexer::lex_with_comments(src).ok()?;
    Some((
        toks.into_iter().map(|t| t.tok).collect(),
        comments.iter().map(|s| comment_text(src, *s)).collect(),
    ))
}

fn comment_text(src: &str, s: Span) -> String {
    src[s.start..s.end].trim_end().to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Break {
    Space,
    Newline,
}

struct Printer<'a> {
    src: &'a str,
    toks: &'a [Token],
    out: String,
    line: String,
    depth: usize,
    // Extra indent for a statement that a comment split across lines.
    continued: bool,
    // `)` tokens that close a cast such as `(int)`.
    cast_close: Vec<bool>,
    prev: Option<usize>,
    last_end: usize,
    last_was_open: bool,
}

impl<'a> Printer<'a> {
    fn new(src: &'a str, toks: &'a [Token]) -> Self {
        let mut cast_close = vec![false; toks.len()];
        for i in 2..toks.len() {
            if toks[i].tok == Tok::RParen && toks[i - 2].tok == Tok::LParen {
                if let Tok::Ident(name) = &toks[i - 1].tok {
                    cast_close[i] = Ty::from_name(name).is_some();
                }
            }
        }
        Self {
            src,
            toks,
            out: String::new(),
            line: String::new(),
            depth: 0,
            continued: false,
            cast_close,
            prev: None,
            last_end: 0,
            last_was_open: false,
        }
    }

    fn run(mut self, comments: &[Span]) -> String {
        let mut items: Vec<Item> = (0..self.toks.len())
            .filter(|&i| self.toks[i].tok != Tok::Eof)
            .map(Item::Tok)
            .collect();
        items.extend(comments.iter().map(|&s| Item::Comment(s)));
        items.sort_by_key(|it| it.start(self.toks));
        let mut skip_next = false;
        for (k, item) in items.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }
            let start = item.start(self.toks);
            let gap = &self.src[self.last_end..start];
            let newlines = gap.matches('\n').count();
            match *item {
                Item::Comment(span) => self.comment(span, gap, newlines),
                Item::Tok(i) => {
                    // An empty block stays on one line: `{}`.
                    let empty = self.toks[i].tok == Tok::LBrace
                        && matches!(items.get(k + 1), Some(Item::Tok(j)) if self.toks[*j].tok == Tok::RBrace);
                    self.token(i, newlines);
                    if empty {
                        self.line.push('}');
                        self.depth -= 1;
                        self.last_was_open = false;
                        self.prev = Some(i + 1);
                        self.last_end = self.toks[i + 1].span.end;
                        skip_next = true;
                        continue;
                    }
                }
            }
            self.last_end = match *item {
                Item::Tok(i) => self.toks[i].span.end,
                Item::Comment(s) => s.end,
            };
        }
        self.flush();
        let trimmed = self.out.trim_end_matches('\n');
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}\n")
        }
    }

    fn flush(&mut self) {
        let line = self.line.trim_end();
        if !line.is_empty() {
            self.out.push_str(line);
            self.out.push('\n');
        }
        self.line.clear();
    }

    fn blank_line(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn start_line(&mut self, depth: usize) {
        self.flush();
        for _ in 0..depth {
            self.line.push_str(INDENT);
        }
    }

    fn prev_tok(&self) -> Option<&Tok> {
        self.prev.map(|i| &self.toks[i].tok)
    }

    /// The layout the previous token asks for before the next one.
    fn break_after_prev(&self, next: &Tok) -> Break {
        match self.prev_tok() {
            None => Break::Newline,
            Some(Tok::LBrace | Tok::Semi) => Break::Newline,
            Some(Tok::RBrace) if *next == Tok::Else => Break::Space,
            Some(Tok::RBrace) => Break::Newline,
            Some(_) => Break::Space,
        }
    }

    // Blank lines are kept (collapsed to one) except at the start of the file, right after `{`
    // and right before `}`.
    fn keeps_blank(&self, newlines: usize, next_is_close: bool) -> bool {
        let started = !self.out.is_empty() || !self.line.trim().is_empty();
        newlines >= 2 && started && !next_is_close && !self.last_was_open
    }

    fn comment(&mut self, span: Span, gap: &str, newlines: usize) {
        let text = comment_text(self.src, span);
        // A comment between two tokens of one statement indents the rest of the statement.
        let mid = matches!(self.prev_tok(), Some(t) if !matches!(t, Tok::LBrace | Tok::Semi | Tok::RBrace));
        if newlines == 0 && !self.line.trim().is_empty() {
            // Trailing comment: keep the spacing it was written with, so aligned comments stay aligned.
            self.line.push_str(if gap.is_empty() { " " } else { gap });
        } else {
            if self.keeps_blank(newlines, false) {
                self.flush();
                self.blank_line();
            }
            self.start_line(self.depth + usize::from(mid));
        }
        self.line.push_str(&text);
        self.continued |= mid;
        self.last_was_open = false;
        // Whatever follows a comment starts on a new line.
        self.flush();
    }

    fn token(&mut self, i: usize, newlines: usize) {
        let tok = &self.toks[i].tok;
        let text = &self.src[self.toks[i].span.start..self.toks[i].span.end];
        if *tok == Tok::RBrace {
            self.depth = self.depth.saturating_sub(1);
        }
        let brk = if *tok == Tok::RBrace {
            Break::Newline
        } else {
            self.break_after_prev(tok)
        };
        if brk == Break::Newline {
            self.continued = false;
            if self.keeps_blank(newlines, *tok == Tok::RBrace) {
                self.flush();
                self.blank_line();
            }
            let depth = self.depth;
            self.start_line(depth);
        } else if self.line.trim().is_empty() {
            // The line was started by a comment inside a statement: continue one level deeper.
            let depth = self.depth + usize::from(self.continued);
            self.start_line(depth);
        } else if self.space_before(i) {
            self.line.push(' ');
        }
        self.line.push_str(text);
        if *tok == Tok::LBrace {
            self.depth += 1;
        }
        self.last_was_open = *tok == Tok::LBrace;
        self.prev = Some(i);
    }

    // Whether the token before `i` makes a `-` or `!` at `i` unary.
    fn is_unary(&self, i: usize) -> bool {
        let Some(p) = i.checked_sub(1) else {
            return true;
        };
        match &self.toks[p].tok {
            Tok::Ident(_) | Tok::Int(_) | Tok::Double(_) | Tok::Str(_) | Tok::True | Tok::False => {
                false
            }
            Tok::RBracket => false,
            Tok::RParen => self.cast_close[p],
            _ => true,
        }
    }

    fn space_before(&self, i: usize) -> bool {
        let Some(p) = self.prev else {
            return false;
        };
        let (prev, tok) = (&self.toks[p].tok, &self.toks[i].tok);
        if matches!(prev, Tok::LParen | Tok::LBracket | Tok::Dot | Tok::DotDot) {
            return false;
        }
        if matches!(prev, Tok::Minus | Tok::Bang) && self.is_unary(p) {
            return false;
        }
        match tok {
            Tok::RParen | Tok::RBracket | Tok::Comma | Tok::Semi | Tok::Dot | Tok::DotDot => false,
            // Calls and indexing: `f(x)`, `r[i]`, `measure(q)[0]`.
            Tok::LParen | Tok::LBracket => {
                !matches!(prev, Tok::Ident(_) | Tok::RParen | Tok::RBracket) || self.cast_close[p]
            }
            _ => true,
        }
    }
}
