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
        crlf(&out)
    } else {
        out
    };
    Ok(format!("{bom}{out}"))
}

// LF -> CRLF, leaving existing `\r\n` (such as a line break inside a string literal) alone.
fn crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    let mut prev = '\0';
    for c in text.chars() {
        if c == '\n' && prev != '\r' {
            out.push('\r');
        }
        out.push(c);
        prev = c;
    }
    out
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

// Marks the `<` and `>` of type parameter lists (a `<` that starts a top-level item) and of type
// arguments (a `<` right after the name of a generic struct or enum of the file).
fn angle_brackets(toks: &[Token]) -> Vec<bool> {
    let mut generics = std::collections::HashSet::new();
    for w in toks.windows(3) {
        if matches!(w[0].tok, Tok::Struct | Tok::Enum) && w[2].tok == Tok::Lt {
            if let Tok::Ident(n) = &w[1].tok {
                generics.insert(n.as_str());
            }
        }
    }
    let mut angle = vec![false; toks.len()];
    let mut depth = 0usize;
    for i in 0..toks.len() {
        match &toks[i].tok {
            Tok::LBrace => depth += 1,
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Lt if !angle[i] => {
                let prev = i.checked_sub(1).map(|p| &toks[p].tok);
                let starts_item = depth == 0
                    && matches!(prev, None | Some(Tok::RBrace | Tok::Semi | Tok::Public));
                let after_generic =
                    matches!(prev, Some(Tok::Ident(n)) if generics.contains(n.as_str()));
                if !(starts_item || after_generic) {
                    continue;
                }
                angle[i] = true;
                let mut level = 1usize;
                for (j, t) in toks.iter().enumerate().skip(i + 1) {
                    match t.tok {
                        Tok::Lt => {
                            angle[j] = true;
                            level += 1;
                        }
                        Tok::Gt => {
                            angle[j] = true;
                            level -= 1;
                            if level == 0 {
                                break;
                            }
                        }
                        Tok::Ident(_) | Tok::Comma | Tok::LBracket | Tok::RBracket => {}
                        _ => break,
                    }
                }
            }
            _ => {}
        }
    }
    angle
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
    // `{` and `}` of array initializers (`{1, 2}`, `new int[]{1}`): laid out like parentheses.
    init_brace: Vec<bool>,
    // Commas between the variants of an `enum`: one variant per line.
    enum_comma: Vec<bool>,
    // `<` and `>` of type arguments and type parameters (`Pair<int, String>`, `<T> T f`):
    // written without spaces inside, unlike comparisons.
    angle: Vec<bool>,
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
        // An initializer `{` follows `=`, `[]`, or `,`/`{` inside another initializer; blocks
        // follow `)`, a condition or `else`.
        let mut init_brace = vec![false; toks.len()];
        let mut open: Vec<bool> = Vec::new();
        for i in 0..toks.len() {
            match toks[i].tok {
                Tok::LBrace => {
                    let prev = i.checked_sub(1).map(|p| &toks[p].tok);
                    let init = match prev {
                        Some(Tok::Eq) => true,
                        Some(Tok::RBracket) => i >= 2 && toks[i - 2].tok == Tok::LBracket,
                        Some(Tok::Comma | Tok::LBrace) => open.last() == Some(&true),
                        _ => false,
                    };
                    init_brace[i] = init;
                    open.push(init);
                }
                Tok::RBrace => init_brace[i] = open.pop().unwrap_or(false),
                _ => {}
            }
        }
        let mut enum_comma = vec![false; toks.len()];
        for e in 0..toks.len() {
            if toks[e].tok != Tok::Enum {
                continue;
            }
            // The body of `enum Name {` or `enum Name<T> {`.
            let Some(i) = (e + 1..toks.len()).find(|&j| toks[j].tok == Tok::LBrace) else {
                continue;
            };
            let mut parens = 0usize;
            for (j, t) in toks.iter().enumerate().skip(i + 1) {
                match t.tok {
                    Tok::LParen => parens += 1,
                    Tok::RParen => parens = parens.saturating_sub(1),
                    Tok::Comma if parens == 0 => enum_comma[j] = true,
                    Tok::RBrace | Tok::LBrace | Tok::Eof => break,
                    _ => {}
                }
            }
        }
        let angle = angle_brackets(toks);
        Self {
            src,
            toks,
            out: String::new(),
            line: String::new(),
            depth: 0,
            continued: false,
            cast_close,
            init_brace,
            enum_comma,
            angle,
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
                        if !self.init_brace[i] {
                            self.depth -= 1;
                        }
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
        if self.prev.is_some_and(|p| self.init_brace[p]) {
            return Break::Space;
        }
        let after_block = |p: usize| {
            p.checked_sub(1)
                .is_some_and(|b| self.toks[b].tok == Tok::RBrace && !self.init_brace[b])
        };
        match self.prev_tok() {
            None => Break::Newline,
            Some(Tok::LBrace | Tok::Semi) => Break::Newline,
            // Enum variants, and `match` arms written with a comma after the block.
            Some(Tok::Comma)
                if self
                    .prev
                    .is_some_and(|p| self.enum_comma[p] || after_block(p)) =>
            {
                Break::Newline
            }
            Some(Tok::RBrace) if matches!(next, Tok::Else | Tok::Comma) => Break::Space,
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
        let block = !self.init_brace[i];
        if *tok == Tok::RBrace && block {
            self.depth = self.depth.saturating_sub(1);
        }
        let brk = if *tok == Tok::RBrace && block {
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
        if *tok == Tok::LBrace && block {
            self.depth += 1;
        }
        self.last_was_open = *tok == Tok::LBrace && block;
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
        // `{1, 2}` and `new int[]{1}`, like parentheses.
        if (*prev == Tok::LBrace && self.init_brace[p])
            || (*tok == Tok::RBrace && self.init_brace[i])
            || (*tok == Tok::LBrace && self.init_brace[i] && *prev == Tok::RBracket)
        {
            return false;
        }
        if matches!(prev, Tok::Minus | Tok::Bang) && self.is_unary(p) {
            return false;
        }
        // `Pair<int, String>`, `new Pair<int, String>(1, "a")`, `<T> T first(T[] a)`.
        if (*prev == Tok::Lt && self.angle[p]) || (*tok == Tok::Gt && self.angle[i]) {
            return false;
        }
        if *tok == Tok::Lt && self.angle[i] {
            return !matches!(prev, Tok::Ident(_));
        }
        let closes_type = *prev == Tok::Gt && self.angle[p];
        match tok {
            Tok::RParen | Tok::RBracket | Tok::Comma | Tok::Semi | Tok::Dot | Tok::DotDot => false,
            // Calls and indexing: `f(x)`, `r[i]`, `measure(q)[0]`.
            Tok::LParen | Tok::LBracket => {
                !(matches!(prev, Tok::Ident(_) | Tok::RParen | Tok::RBracket) || closes_type)
                    || self.cast_close[p]
            }
            _ => true,
        }
    }
}
