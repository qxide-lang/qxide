// Editor support for `qxide lsp` (docs/design/lsp.md): diagnostics, outline, go to definition,
// hover and completion on top of the front end. Plain data in and out, no protocol types, so this
// file also compiles into the playground build (which never calls it).

use super::ast::*;
use super::check::CONSTANTS;
use super::diag::{Diagnostic, SourceMap, Span};
use super::{check, explain, lexer, link, parser, strip_bom, Input};

/// A zero-based line and UTF-16 column, as the Language Server Protocol counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

/// A range in a named file: the analysed document (its `Input::name`) or a module it imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    pub location: Location,
    pub message: String,
}

/// One `qxide check` error, placed in the analysed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub code: &'static str,
    pub range: Range,
    /// The message, then `note:` lines without a location and the `help:` line, as `check` prints them.
    pub message: String,
    pub related: Vec<Related>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// Signature of a function, declaration of a variable.
    pub detail: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection: Range,
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Keyword,
    Type,
    Function,
    Variable,
    Constant,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    pub detail: Option<String>,
}

/// Every keyword of the language; a test keeps this in sync with the lexer.
pub const KEYWORDS: &[&str] = &[
    "final", "var", "consume", "import", "public", "if", "else", "for", "in", "return", "true",
    "false",
];

/// Type names as written in source.
pub const TYPES: &[&str] = &[
    "int", "double", "boolean", "Bit", "BitReg", "Qubit", "QReg", "void",
];

/// `BitReg` methods.
pub const METHODS: &[(&str, &str)] = &[
    ("as_int", "BitReg.as_int()"),
    ("to_binary", "BitReg.to_binary()"),
];

/// Signatures of the built-in functions (one line per overload); a test covers every built-in.
pub const BUILTIN_SIGNATURES: &[(&str, &str)] = &[
    ("qubit", "Qubit qubit()"),
    ("qreg", "QReg qreg(int n)"),
    ("h", "void h(Qubit q)"),
    ("x", "void x(Qubit q)"),
    ("z", "void z(Qubit q)"),
    ("s", "void s(Qubit q)"),
    ("sdg", "void sdg(Qubit q)"),
    ("t", "void t(Qubit q)"),
    ("tdg", "void tdg(Qubit q)"),
    ("reset", "void reset(Qubit q)"),
    ("rx", "void rx(Qubit q, double theta)"),
    ("ry", "void ry(Qubit q, double theta)"),
    ("rz", "void rz(Qubit q, double theta)"),
    ("cx", "void cx(Qubit control, Qubit target)"),
    (
        "ccx",
        "void ccx(Qubit control1, Qubit control2, Qubit target)",
    ),
    (
        "measure",
        "Bit measure(consume Qubit q)\nBitReg measure(consume QReg r)",
    ),
    (
        "discard",
        "void discard(consume Qubit q)\nvoid discard(consume QReg r)",
    ),
    ("len", "int len(QReg r)\nint len(BitReg r)"),
    ("println", "void println(\"format {}\", args...)"),
];

pub fn builtin_signature(name: &str) -> Option<&'static str> {
    BUILTIN_SIGNATURES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, s)| s)
}

/// Zero-based line and UTF-16 column of byte `offset` in `text` (clamped to the text).
pub fn position(text: &str, offset: usize) -> Pos {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = text[line_start..offset].encode_utf16().count();
    Pos {
        line: line as u32,
        character: character as u32,
    }
}

/// Byte offset of `pos` in `text`; positions past the end of a line or of the text are clamped.
pub fn offset(text: &str, pos: Pos) -> usize {
    let mut start = 0;
    for _ in 0..pos.line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let mut units = 0;
    for (i, c) in text[start..end].char_indices() {
        if units >= pos.character as usize {
            return start + i;
        }
        units += c.len_utf16();
    }
    end
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalKind {
    Param { consume: bool },
    Decl { is_final: bool },
    Loop,
}

#[derive(Debug, Clone)]
struct Local {
    name: String,
    decl: Span,
    // `None` for `var`.
    ty: Option<Ty>,
    kind: LocalKind,
    // Byte range where the name is in scope.
    visible: (usize, usize),
    // Index of the declaring function in the walked program.
    func: usize,
}

impl Local {
    fn declaration(&self) -> String {
        let ty = self.ty.map_or("var", Ty::name);
        match self.kind {
            LocalKind::Param { consume: true } => format!("consume {ty} {}", self.name),
            LocalKind::Decl { is_final: true } => format!("final {ty} {}", self.name),
            _ => format!("{ty} {}", self.name),
        }
    }
}

#[derive(Debug, Clone)]
struct Use {
    span: Span,
    name: String,
    call: bool,
}

#[derive(Default)]
struct Scopes {
    locals: Vec<Local>,
    uses: Vec<Use>,
}

impl Scopes {
    fn of(fns: &[&FnDecl]) -> Self {
        let mut s = Scopes::default();
        for (i, f) in fns.iter().enumerate() {
            for p in &f.params {
                s.locals.push(Local {
                    name: p.name.name.clone(),
                    decl: p.name.span,
                    ty: Some(p.ty),
                    kind: LocalKind::Param {
                        consume: p.consume.is_some(),
                    },
                    visible: (p.name.span.end, f.body.close.end),
                    func: i,
                });
            }
            s.block(&f.body, i);
        }
        s
    }

    fn block(&mut self, b: &Block, func: usize) {
        for stmt in &b.stmts {
            match stmt {
                Stmt::Decl {
                    is_final,
                    ty,
                    name,
                    init,
                    ..
                } => {
                    self.expr(init);
                    self.locals.push(Local {
                        name: name.name.clone(),
                        decl: name.span,
                        ty: *ty,
                        kind: LocalKind::Decl {
                            is_final: *is_final,
                        },
                        visible: (init.span.end, b.close.end),
                        func,
                    });
                }
                Stmt::Assign { name, value } => {
                    self.uses.push(Use {
                        span: name.span,
                        name: name.name.clone(),
                        call: false,
                    });
                    self.expr(value);
                }
                Stmt::If(i) => self.if_stmt(i, func),
                Stmt::For {
                    var,
                    start,
                    end,
                    body,
                } => {
                    self.expr(start);
                    self.expr(end);
                    self.locals.push(Local {
                        name: var.name.clone(),
                        decl: var.span,
                        ty: Some(Ty::Int),
                        kind: LocalKind::Loop,
                        visible: (end.span.end, body.close.end),
                        func,
                    });
                    self.block(body, func);
                }
                Stmt::Return { value, .. } => {
                    if let Some(v) = value {
                        self.expr(v);
                    }
                }
                Stmt::Expr(e) => self.expr(e),
            }
        }
    }

    fn if_stmt(&mut self, i: &IfStmt, func: usize) {
        self.expr(&i.cond);
        self.block(&i.then, func);
        match &i.els {
            Some(Else::Block(b)) => self.block(b, func),
            Some(Else::If(e)) => self.if_stmt(e, func),
            None => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Var(name) => self.uses.push(Use {
                span: e.span,
                name: name.clone(),
                call: false,
            }),
            ExprKind::Call { name, args } => {
                self.uses.push(Use {
                    span: name.span,
                    name: name.name.clone(),
                    call: true,
                });
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Method { recv, .. } => self.expr(recv),
            ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Int(_) | ExprKind::Double(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        }
    }

    // The innermost declaration of `name` in scope at `at`.
    fn resolve(&self, name: &str, at: usize) -> Option<&Local> {
        self.locals
            .iter()
            .filter(|l| l.name == name && l.visible.0 <= at && at <= l.visible.1)
            .max_by_key(|l| l.decl.start)
    }
}

fn contains(span: Span, at: usize) -> bool {
    span.start <= at && at <= span.end
}

/// `public void prepare(Qubit a, consume Qubit b)`, with `name` as given (it may be qualified).
fn signature(f: &FnDecl, name: &str) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let consume = if p.consume.is_some() { "consume " } else { "" };
            format!("{consume}{} {}", p.ty.name(), p.name.name)
        })
        .collect();
    let public = if f.public.is_some() { "public " } else { "" };
    format!("{public}{} {name}({})", f.ret.name(), params.join(", "))
}

// Module path of an import as the linker qualifies it: `pkg.lib` is the package root itself.
fn import_prefix(segments: &[String]) -> String {
    match segments {
        [pkg, lib] if lib == "lib" => pkg.clone(),
        _ => segments.join("."),
    }
}

fn parse(src: &str) -> Option<Program> {
    parser::parse(lexer::lex(src).ok()?).ok()
}

// The text with the line holding `at` replaced by spaces (same length, so offsets stay valid):
// while typing, the unfinished statement is usually the only thing that does not parse.
fn blank_line(src: &str, at: usize) -> String {
    let at = at.min(src.len());
    let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = src[at..].find('\n').map_or(src.len(), |i| at + i);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..start]);
    out.extend(std::iter::repeat_n(' ', end - start));
    out.push_str(&src[end..]);
    out
}

enum Sym {
    Local(Local),
    Fn { decl: Span, signature: String },
    // An imported name whose module could not be linked: its `import` item.
    ImportItem(Span),
    Module(Span),
    Builtin(&'static str),
    Const(String),
}

/// One analysed document: the result of `qxide check` plus what navigation needs.
pub struct Analysis {
    name: String,
    // The document as the editor holds it, and the length of a leading byte order mark.
    text: String,
    bom: usize,
    map: SourceMap,
    // The document alone (keeps its `import`s), if it parses.
    entry: Option<Program>,
    // Every reachable module after linking (qualified names), if linking succeeded.
    linked: Option<Program>,
    diags: Vec<Diagnostic>,
}

/// Runs the same front end as `qxide check` (lexing, linking, static checks) on `input`.
pub fn analyze(input: &Input) -> Analysis {
    let src = strip_bom(input.src);
    let (map, linked) = link::link(input.name, src, input.project, input.read);
    let (linked, diags) = match linked {
        Ok(program) => {
            let errors = check::check_with(&program, !input.is_library());
            (Some(program), errors)
        }
        Err(errors) => (None, errors),
    };
    Analysis {
        name: input.name.to_string(),
        text: input.src.to_string(),
        bom: input.src.len() - src.len(),
        map,
        entry: parse(src),
        linked,
        diags,
    }
}

impl Analysis {
    fn src(&self) -> &str {
        &self.text[self.bom..]
    }

    /// Byte offset in the analysed source of an editor position.
    pub fn offset_of(&self, pos: Pos) -> usize {
        offset(&self.text, pos).saturating_sub(self.bom)
    }

    fn entry_range(&self, start: usize, end: usize) -> Range {
        Range {
            start: position(&self.text, start + self.bom),
            end: position(&self.text, end.max(start) + self.bom),
        }
    }

    fn location(&self, span: Span) -> Location {
        let (name, text, base) = self.map.file_at(span.start);
        if base == 0 {
            return Location {
                file: self.name.clone(),
                range: self.entry_range(span.start, span.end),
            };
        }
        let local = |o: usize| position(text, o.saturating_sub(base));
        Location {
            file: name.to_string(),
            range: Range {
                start: local(span.start),
                end: local(span.end.max(span.start)),
            },
        }
    }

    fn in_entry(&self, span: Span) -> bool {
        span.start <= self.src().len()
    }

    // Where errors inside imported modules are shown: the document's `import` lines.
    fn imports_range(&self) -> Range {
        let imports = self.entry.as_ref().map(|p| p.imports.as_slice());
        match imports {
            Some([first, .., last]) => self.entry_range(first.span.start, last.span.end),
            Some([only]) => self.entry_range(only.span.start, only.span.end),
            _ => self.entry_range(0, 0),
        }
    }

    /// The errors `qxide check` reports, in the same order and at the same places. Errors inside
    /// an imported module are shown on the document's imports, prefixed with `file:line:col:`.
    pub fn diagnostics(&self) -> Vec<Diag> {
        self.diags
            .iter()
            .map(|d| {
                let mut message = d.message.clone();
                let mut related = Vec::new();
                let range = if self.in_entry(d.span) {
                    self.entry_range(d.span.start, d.span.end)
                } else {
                    let (line, col) = self.map.line_col(d.span.start);
                    let at = self.location(d.span);
                    message = format!("{}:{line}:{col}: {message}", at.file);
                    related.push(Related {
                        location: at,
                        message: d.message.clone(),
                    });
                    self.imports_range()
                };
                for n in &d.notes {
                    match n.span {
                        Some(s) => related.push(Related {
                            location: self.location(s),
                            message: n.message.clone(),
                        }),
                        None => {
                            message.push_str("\nnote: ");
                            message.push_str(&n.message);
                        }
                    }
                }
                if let Some(h) = &d.help {
                    message.push_str("\nhelp: ");
                    message.push_str(h);
                }
                Diag {
                    code: d.code.as_str(),
                    range,
                    message,
                    related,
                }
            })
            .collect()
    }

    // Functions of the document: from the linked program (calls carry qualified names) when
    // linking succeeded, else from the document alone.
    fn nav_fns(&self) -> Vec<&FnDecl> {
        match (&self.linked, &self.entry) {
            (Some(p), _) => p
                .fns
                .iter()
                .filter(|f| self.in_entry(f.name.span))
                .collect(),
            (None, Some(p)) => p.fns.iter().collect(),
            (None, None) => Vec::new(),
        }
    }

    /// The document's functions with their parameters and local variables, or `None` if the
    /// document does not parse.
    pub fn symbols(&self) -> Option<Vec<Symbol>> {
        let entry = self.entry.as_ref()?;
        let fns: Vec<&FnDecl> = entry.fns.iter().collect();
        let scopes = Scopes::of(&fns);
        let symbols = fns
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let start = f.public.unwrap_or(f.ret_span).start;
                let children = scopes
                    .locals
                    .iter()
                    .filter(|l| l.func == i)
                    .map(|l| {
                        let r = self.entry_range(l.decl.start, l.decl.end);
                        Symbol {
                            name: l.name.clone(),
                            detail: l.declaration(),
                            kind: SymbolKind::Variable,
                            range: r,
                            selection: r,
                            children: Vec::new(),
                        }
                    })
                    .collect();
                Symbol {
                    name: f.name.name.clone(),
                    detail: signature(f, &f.name.name),
                    kind: SymbolKind::Function,
                    range: self.entry_range(start, f.body.close.end),
                    selection: self.entry_range(f.name.span.start, f.name.span.end),
                    children,
                }
            })
            .collect();
        Some(symbols)
    }

    fn find_fn(&self, name: &str) -> Option<&FnDecl> {
        let program = self.linked.as_ref().or(self.entry.as_ref())?;
        program.fns.iter().find(|f| f.name.name == name)
    }

    fn fn_sym(f: &FnDecl) -> Sym {
        Sym::Fn {
            decl: f.name.span,
            signature: signature(f, &f.name.name),
        }
    }

    fn call_sym(&self, name: &str) -> Option<Sym> {
        if let Some(f) = self.find_fn(name) {
            return Some(Self::fn_sym(f));
        }
        if let Some(s) = builtin_signature(name) {
            return Some(Sym::Builtin(s));
        }
        // Linking failed: an imported name can still lead to its `import` line.
        let entry = self.entry.as_ref()?;
        entry
            .imports
            .iter()
            .filter_map(|i| i.item.as_ref())
            .find(|item| item.name == name)
            .map(|item| Sym::ImportItem(item.span))
    }

    fn symbol_at(&self, at: usize) -> Option<Sym> {
        let fns = self.nav_fns();
        let scopes = Scopes::of(&fns);
        if let Some(u) = scopes.uses.iter().find(|u| contains(u.span, at)) {
            if u.call {
                return self.call_sym(&u.name);
            }
            if let Some(l) = scopes.resolve(&u.name, u.span.start) {
                return Some(Sym::Local(l.clone()));
            }
            return CONSTANTS
                .iter()
                .find(|(n, _)| *n == u.name)
                .map(|(n, v)| Sym::Const(format!("final double {n} = {v:?}")));
        }
        if let Some(l) = scopes.locals.iter().find(|l| contains(l.decl, at)) {
            return Some(Sym::Local(l.clone()));
        }
        if let Some(f) = fns.iter().find(|f| contains(f.name.span, at)) {
            return Some(Self::fn_sym(f));
        }
        for imp in self.entry.as_ref().map_or(&[][..], |p| &p.imports) {
            let segments: Vec<String> = imp.module.iter().map(|s| s.name.clone()).collect();
            if let Some(item) = imp.item.as_ref().filter(|i| contains(i.span, at)) {
                let qualified = format!("{}.{}", import_prefix(&segments), item.name);
                return Some(
                    self.find_fn(&qualified)
                        .map_or(Sym::ImportItem(item.span), Self::fn_sym),
                );
            }
            if let Some(k) = imp.module.iter().position(|s| contains(s.span, at)) {
                // Any function of the module (or of a module below it) locates its file.
                let prefix = format!("{}.", import_prefix(&segments[..=k]));
                let linked = self.linked.as_ref()?;
                return linked
                    .fns
                    .iter()
                    .find(|f| {
                        f.name.name.starts_with(&prefix)
                            && !f.name.name[prefix.len()..].contains('.')
                    })
                    .map(|f| Sym::Module(f.name.span));
            }
        }
        None
    }

    /// Where the name at `at` (a byte offset from `offset_of`) is declared.
    pub fn definition(&self, at: usize) -> Option<Location> {
        match self.symbol_at(at)? {
            Sym::Local(l) => Some(self.location(l.decl)),
            Sym::Fn { decl, .. } | Sym::ImportItem(decl) => Some(self.location(decl)),
            Sym::Module(any) => {
                let mut loc = self.location(any);
                let zero = Pos {
                    line: 0,
                    character: 0,
                };
                loc.range = Range {
                    start: zero,
                    end: zero,
                };
                Some(loc)
            }
            Sym::Builtin(_) | Sym::Const(_) => None,
        }
    }

    /// Markdown for the position `at`: the `qxide explain` entry of each error there, then the
    /// declaration or signature of the name under the cursor.
    pub fn hover(&self, at: usize) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        let mut codes: Vec<&str> = Vec::new();
        for d in self.diags.iter().filter(|d| self.in_entry(d.span)) {
            let code = d.code.as_str();
            if contains(d.span, at) && !codes.contains(&code) {
                codes.push(code);
                if let Some(text) = explain::explain(code) {
                    parts.push(format!("## {}", text.trim_end()));
                }
            }
        }
        let code = match self.symbol_at(at) {
            Some(Sym::Local(l)) => Some(l.declaration()),
            Some(Sym::Fn { signature, .. }) => Some(signature),
            Some(Sym::Builtin(s)) => Some(s.to_string()),
            Some(Sym::Const(s)) => Some(s),
            Some(Sym::ImportItem(_) | Sym::Module(_)) | None => None,
        };
        if let Some(code) = code {
            parts.insert(0, format!("```qx\n{code}\n```"));
        }
        (!parts.is_empty()).then(|| parts.join("\n\n---\n\n"))
    }

    /// Completion candidates at `at`: `BitReg` methods after a `.`, otherwise keywords, types,
    /// built-ins, constants, the document's and imported functions, and the local names in scope.
    pub fn completions(&self, at: usize) -> Vec<Item> {
        let src = self.src();
        let at = at.min(src.len());
        let word_start = src[..at]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(at, |(i, _)| i);
        let line_start = src[..word_start].rfind('\n').map_or(0, |i| i + 1);
        let line = src[line_start..word_start].trim_start();
        let import_line = line
            .strip_prefix("import")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
        if import_line || line.starts_with("//") {
            return Vec::new();
        }
        if src[..word_start].ends_with('.') {
            return METHODS
                .iter()
                .map(|(m, detail)| Item {
                    label: m.to_string(),
                    kind: ItemKind::Method,
                    detail: Some(detail.to_string()),
                })
                .collect();
        }
        let mut items: Vec<Item> = Vec::new();
        let mut push = |label: &str, kind: ItemKind, detail: Option<String>| {
            if !items.iter().any(|i| i.label == label) {
                items.push(Item {
                    label: label.to_string(),
                    kind,
                    detail,
                });
            }
        };
        // Labels are unique: the first (most specific) kind of a name wins, locals first.
        let blanked;
        let program = match &self.entry {
            Some(_) => None,
            None => {
                blanked = parse(&blank_line(src, at));
                blanked.as_ref()
            }
        };
        match program {
            None if self.entry.is_some() => {
                let fns = self.nav_fns();
                let scopes = Scopes::of(&fns);
                locals_at(&scopes, at, &mut push);
                for f in &fns {
                    push(
                        &f.name.name,
                        ItemKind::Function,
                        Some(signature(f, &f.name.name)),
                    );
                }
                let imports = self.entry.as_ref().map_or(&[][..], |p| &p.imports);
                self.imported(imports, &mut push);
            }
            Some(p) => {
                let fns: Vec<&FnDecl> = p.fns.iter().collect();
                locals_at(&Scopes::of(&fns), at, &mut push);
                for f in &fns {
                    push(
                        &f.name.name,
                        ItemKind::Function,
                        Some(signature(f, &f.name.name)),
                    );
                }
                self.imported(&p.imports, &mut push);
            }
            None => token_names(src, at, &mut push),
        }
        for (name, sig) in BUILTIN_SIGNATURES {
            push(name, ItemKind::Function, Some(sig.to_string()));
        }
        for (name, _) in CONSTANTS {
            push(name, ItemKind::Constant, Some("double".to_string()));
        }
        for t in TYPES {
            push(t, ItemKind::Type, None);
        }
        for k in KEYWORDS {
            push(k, ItemKind::Keyword, None);
        }
        items
    }

    // Names the document imports: single items, and the public functions of wildcard imports.
    fn imported(&self, imports: &[Import], push: &mut impl FnMut(&str, ItemKind, Option<String>)) {
        for imp in imports {
            let segments: Vec<String> = imp.module.iter().map(|s| s.name.clone()).collect();
            let prefix = format!("{}.", import_prefix(&segments));
            match &imp.item {
                Some(item) => {
                    let detail = self
                        .find_fn(&format!("{prefix}{}", item.name))
                        .map(|f| signature(f, &f.name.name));
                    push(&item.name, ItemKind::Function, detail);
                }
                None => {
                    let Some(linked) = &self.linked else { continue };
                    for f in linked.fns.iter().filter(|f| f.public.is_some()) {
                        if let Some(bare) = f.name.name.strip_prefix(&prefix) {
                            if !bare.contains('.') {
                                push(bare, ItemKind::Function, Some(signature(f, &f.name.name)));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn locals_at(scopes: &Scopes, at: usize, push: &mut impl FnMut(&str, ItemKind, Option<String>)) {
    let mut visible: Vec<&Local> = scopes
        .locals
        .iter()
        .filter(|l| l.visible.0 <= at && at <= l.visible.1)
        .collect();
    // Innermost first, so a shadowing declaration supplies the detail.
    visible.sort_by_key(|l| std::cmp::Reverse(l.decl.start));
    for l in visible {
        push(&l.name, ItemKind::Variable, Some(l.declaration()));
    }
}

// Last resort while the document does not parse even without the cursor's line: declarations
// spotted in the token stream (`Type name`, `var name`, `for name`) before `at`, and functions.
fn token_names(src: &str, at: usize, push: &mut impl FnMut(&str, ItemKind, Option<String>)) {
    let blanked = blank_line(src, at);
    let Ok(toks) = lexer::lex(&blanked) else {
        return;
    };
    let mut depth = 0usize;
    for (i, t) in toks.iter().enumerate() {
        match &t.tok {
            lexer::Tok::LBrace => depth += 1,
            lexer::Tok::RBrace => depth = depth.saturating_sub(1),
            lexer::Tok::Ident(name) if i > 0 => {
                let declares = match &toks[i - 1].tok {
                    lexer::Tok::Ident(ty) => Ty::from_name(ty).is_some(),
                    lexer::Tok::Var | lexer::Tok::For => true,
                    _ => false,
                };
                if !declares {
                    continue;
                }
                let is_fn =
                    depth == 0 && toks.get(i + 1).map(|n| &n.tok) == Some(&lexer::Tok::LParen);
                if is_fn {
                    push(name, ItemKind::Function, None);
                } else if t.span.end <= at {
                    push(name, ItemKind::Variable, None);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::check::BUILTINS;
    use super::*;

    #[test]
    fn positions_count_utf16_units_and_round_trip() {
        let text = "ab\r\n큐비트 😀x\nz";
        let x = text.find('x').unwrap();
        // `😀` is two UTF-16 units; the `\r` before `\n` belongs to line 0.
        assert_eq!(
            position(text, x),
            Pos {
                line: 1,
                character: 6
            }
        );
        assert_eq!(offset(text, position(text, x)), x);
        assert_eq!(position(text, 3).character, 3);
        assert_eq!(
            offset(
                text,
                Pos {
                    line: 1,
                    character: 99
                }
            ),
            text.find("\nz").unwrap()
        );
        assert_eq!(
            offset(
                text,
                Pos {
                    line: 9,
                    character: 0
                }
            ),
            text.len()
        );
    }

    #[test]
    fn keywords_and_types_match_the_lexer() {
        for k in KEYWORDS {
            let toks = lexer::lex(k).unwrap();
            assert!(
                !matches!(toks[0].tok, lexer::Tok::Ident(_)),
                "`{k}` is not a keyword"
            );
        }
        for t in TYPES {
            assert!(Ty::from_name(t).is_some(), "`{t}`");
        }
        for b in BUILTINS {
            assert!(builtin_signature(b).is_some(), "no signature for `{b}`");
        }
        assert_eq!(BUILTIN_SIGNATURES.len(), BUILTINS.len());
    }

    #[test]
    fn blank_line_keeps_offsets() {
        let src = "a\n  h(\nb";
        let out = blank_line(src, 5);
        assert_eq!(out, "a\n    \nb");
        assert_eq!(out.len(), src.len());
    }
}
