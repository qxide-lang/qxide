// `qxide doc` (roadmap Stage 5): Markdown reference of one `.qx` file from its `///` comments.
// A run of `///` lines (not `////`) between the previous token and a declaration documents that
// declaration. Listed: every `public` function, every declaration with a doc comment, and all
// members (fields, variants, methods, interface methods) of a listed type, in source order.
// Signatures are the declaration's own source text up to its body, with whitespace collapsed.

use super::ast::{FnDecl, Program};
use super::diag::{Diagnostic, SourceMap, Span};
use super::lexer::{self, Tok, Token};
use super::{parser, render, strip_bom, Failure, Limits};

struct Item {
    // 2 for a top-level declaration, 3 for a member of a type.
    level: usize,
    start: usize,
    signature: String,
    doc: String,
}

struct Source<'a> {
    src: &'a str,
    toks: &'a [Token],
    comments: &'a [Span],
}

impl Source<'_> {
    // Declarations the parser made up (no name token of their own) are not listed.
    fn index_of(&self, name: Span) -> Option<usize> {
        self.toks.iter().position(|t| t.span.start == name.start)
    }

    // First token of the declaration whose name is token `i`: back to the previous `{`, `}`, `;`
    // or a `,` outside `<..>` (enum variants, generic parameter lists).
    fn decl_start(&self, i: usize) -> usize {
        let mut angle = 0i32;
        let mut s = i;
        while s > 0 {
            match self.toks[s - 1].tok {
                Tok::Gt => angle += 1,
                Tok::Lt => angle -= 1,
                Tok::LBrace | Tok::RBrace | Tok::Semi => break,
                Tok::Comma if angle <= 0 => break,
                _ => {}
            }
            s -= 1;
        }
        s
    }

    // Token after the declaration header that starts at name token `i`: the first `{` or `;`,
    // or for an enum variant the `,` or `}` after its payload.
    fn header_end(&self, i: usize, variant: bool) -> usize {
        let mut paren = 0i32;
        let mut e = i;
        while e < self.toks.len() {
            match self.toks[e].tok {
                Tok::LParen => paren += 1,
                Tok::RParen => paren -= 1,
                Tok::LBrace | Tok::Semi | Tok::Eof if paren <= 0 => break,
                Tok::Comma | Tok::RBrace if variant && paren <= 0 => break,
                _ => {}
            }
            e += 1;
        }
        e
    }

    // Source text of tokens `from..to`, comments left out, whitespace collapsed.
    fn text(&self, from: usize, to: usize) -> String {
        let (a, b) = (self.toks[from].span.start, self.toks[to - 1].span.end);
        let mut out = String::new();
        let mut at = a;
        for c in self.comments.iter().filter(|c| c.start >= a && c.end <= b) {
            out.push_str(&self.src[at..c.start]);
            out.push(' ');
            at = c.end;
        }
        out.push_str(&self.src[at..b]);
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // The `///` lines between the previous token and token `s`.
    fn doc_before(&self, s: usize) -> String {
        let from = if s == 0 { 0 } else { self.toks[s - 1].span.end };
        let to = self.toks[s].span.start;
        let lines: Vec<&str> = self
            .comments
            .iter()
            .filter(|c| c.start >= from && c.end <= to)
            .filter_map(|c| {
                let t = self.src[c.start..c.end].trim_end();
                let rest = t.strip_prefix("///")?;
                if rest.starts_with('/') {
                    return None;
                }
                Some(rest.strip_prefix(' ').unwrap_or(rest))
            })
            .collect();
        lines.join("\n").trim().to_string()
    }

    fn item(&self, level: usize, name: Span, variant: bool) -> Option<Item> {
        let i = self.index_of(name)?;
        let s = self.decl_start(i);
        let e = self.header_end(i, variant).max(i + 1);
        Some(Item {
            level,
            start: self.toks[s].span.start,
            signature: self.text(s, e),
            doc: self.doc_before(s),
        })
    }

    fn function(&self, level: usize, f: &FnDecl) -> Option<Item> {
        self.item(level, f.name.span, false)
    }
}

/// Markdown reference for file `name` (`# name`, followed by one `##` heading per listed
/// declaration and `###` per member), or the file's syntax errors.
pub fn markdown(name: &str, src: &str) -> Result<String, Failure> {
    let src = strip_bom(src);
    let fail = |d: Diagnostic| render(&SourceMap::new(name, src), &[d], &Limits::CLI);
    let (toks, comments) = lexer::lex_with_comments(src).map_err(fail)?;
    let program = parser::parse(toks.clone()).map_err(fail)?;
    let s = Source {
        src,
        toks: &toks,
        comments: &comments,
    };
    let mut items = collect(&s, &program);
    items.sort_by_key(|(top, _)| top.start);
    let mut out = format!("# `{name}`\n");
    for (top, members) in items {
        write_item(&mut out, &top);
        for m in members {
            write_item(&mut out, &m);
        }
    }
    Ok(out)
}

/// The `///` comment attached to the declaration whose name begins at byte offset `name` in `src`
/// (for editor hovers; the file need not parse).
pub fn comment_of(src: &str, name: usize) -> Option<String> {
    let (toks, comments) = lexer::lex_with_comments(src).ok()?;
    let s = Source {
        src,
        toks: &toks,
        comments: &comments,
    };
    let i = s.index_of(Span::new(name, name))?;
    let doc = s.doc_before(s.decl_start(i));
    (!doc.is_empty()).then_some(doc)
}

fn collect(s: &Source, p: &Program) -> Vec<(Item, Vec<Item>)> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in &p.fns {
        // An `extern` declaration is a wrapper plus a stub that share the name's span.
        if !seen.insert(f.name.span.start) {
            continue;
        }
        let Some(item) = s.function(2, f) else {
            continue;
        };
        if f.public.is_some() || !item.doc.is_empty() {
            items.push((item, Vec::new()));
        }
    }
    let mut typed = |top: Option<Item>, mut members: Vec<Item>| {
        let Some(top) = top else { return };
        if !top.doc.is_empty() || members.iter().any(|m| !m.doc.is_empty()) {
            members.sort_by_key(|m| m.start);
            items.push((top, members));
        }
    };
    for t in &p.structs {
        let mut members: Vec<Item> = t
            .fields
            .iter()
            .filter_map(|f| s.item(3, f.name.span, false))
            .collect();
        members.extend(t.methods.iter().filter_map(|f| s.function(3, f)));
        typed(s.item(2, t.name.span, false), members);
    }
    for t in p.enums.iter().filter(|e| !e.prelude) {
        let mut members: Vec<Item> = t
            .variants
            .iter()
            .filter_map(|v| s.item(3, v.name.span, true))
            .collect();
        members.extend(t.methods.iter().filter_map(|f| s.function(3, f)));
        typed(s.item(2, t.name.span, false), members);
    }
    for t in &p.ifaces {
        let members = t
            .methods
            .iter()
            .filter_map(|m| s.item(3, m.name.span, false))
            .collect();
        typed(s.item(2, t.name.span, false), members);
    }
    items
}

fn write_item(out: &mut String, item: &Item) {
    out.push('\n');
    out.push_str(&"#".repeat(item.level));
    out.push_str(" `");
    out.push_str(&item.signature.replace('`', "'"));
    out.push_str("`\n");
    if !item.doc.is_empty() {
        out.push('\n');
        out.push_str(&item.doc);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::markdown;

    const SRC: &str = "import std.math;

/// Adds two numbers.
/// Overflow is an error.
public int add(int a, int b) {
    return a + b;
}

// A plain comment.
int helper() {
    return 1;
}

//// Four slashes.
int other() {
    return 2;
}

///A generic pick.
public <T, U> T first(T a, U b) {
    return a;
}

public void bare() {}

/// A point.
struct Point {
    /// Across.
    double x;
    double y;

    /// Length.
    double len() {
        return math.sqrt(this.x * this.x + this.y * this.y);
    }
}

struct Hidden {
    int a;
}

enum Shape {
    /// Round.
    Circle(double r),
    Empty
}

/// Things with names.
interface Named {
    /// The name.
    String name();
}

void main() {}
";

    const DOC: &str = "# `lib.qx`

## `public int add(int a, int b)`

Adds two numbers.
Overflow is an error.

## `public <T, U> T first(T a, U b)`

A generic pick.

## `public void bare()`

## `struct Point`

A point.

### `double x`

Across.

### `double y`

### `double len()`

Length.

## `enum Shape`

### `Circle(double r)`

Round.

### `Empty`

## `interface Named`

Things with names.

### `String name()`

The name.
";

    #[test]
    fn doc_comments_document_declarations() {
        super::super::check("lib.qx", SRC).unwrap();
        assert_eq!(markdown("lib.qx", SRC).unwrap(), DOC);
        // Formatting keeps them, so the reference does not change.
        let formatted = super::super::fmt::format(SRC).unwrap();
        assert_eq!(markdown("lib.qx", &formatted).unwrap(), DOC);
        // Comments inside a signature are left out of it.
        let split = "public int add(int a, // first\n        int b) {\n    return a + b;\n}\n";
        assert_eq!(
            markdown("s.qx", split).unwrap(),
            "# `s.qx`\n\n## `public int add(int a, int b)`\n"
        );
    }

    #[test]
    fn doc_reports_syntax_errors_and_lists_every_example() {
        let err = markdown("bad.qx", "public int f( {\n").unwrap_err();
        assert!(err.0.contains("error[Q01"), "{}", err.0);
        assert!(err.0.contains("bad.qx:1:"), "{}", err.0);
        // The qxide crate, or a crate below it that compiles src/lang too (the wasm build).
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|d| d.join("examples"))
            .find(|d| d.join("qx").is_dir())
            .unwrap();
        let mut dirs = vec![dir];
        let mut n = 0;
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "qx") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let name = p.display().to_string();
                    let doc = markdown(&name, &src).unwrap_or_else(|e| panic!("{name}: {}", e.0));
                    assert!(doc.starts_with(&format!("# `{name}`\n")));
                    n += 1;
                }
            }
        }
        assert!(n > 10, "{n} examples");
    }
}
