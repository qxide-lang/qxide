// Module loading and name resolution for `import` (docs/design/qx-packages.md).
// Loads every module the entry file reaches, rejects cycles and private access, rewrites calls
// to qualified names (`bell_utils.prepare`), and merges all functions into one `Program`, so
// the checker and interpreter see a single program and enforce linearity across modules
// through ordinary function signatures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::ast::*;
use super::check::BUILTINS;
use super::diag::{Code, Diagnostic, Note, SourceMap, Span};
use super::{lexer, msg, parser};

/// One package whose modules can be imported.
#[derive(Debug, Clone)]
pub struct PackageRoot {
    /// Name used in `import` paths: `bell_utils` for the package `bell-utils`.
    pub name: String,
    /// Directory holding `lib.qx` and the other module files.
    pub src_dir: PathBuf,
    /// Indices into `Project::packages` of the packages this one may import.
    pub deps: Vec<usize>,
}

/// The packages an entry file can import from; built by the package manager from `qxide.toml`.
#[derive(Debug, Clone)]
pub struct Project {
    pub packages: Vec<PackageRoot>,
    pub entry_package: usize,
    /// Module path of the entry file if it is itself a module of the entry package (`[]` for
    /// `lib.qx`), so that importing it back is reported as a cycle.
    pub entry_module: Option<Vec<String>>,
}

pub type ReadFn<'a> = &'a dyn Fn(&Path) -> std::io::Result<String>;

// Caps the module graph so hostile input fails with a diagnostic instead of exhausting memory.
const MAX_MODULES: usize = 4096;

struct Module {
    pkg: Option<usize>,
    // `bell_utils.gates`; empty for the entry file.
    display: String,
    // Prefix of qualified function names: empty for the entry file, `bell_utils.gates.` otherwise.
    prefix: String,
    program: Program,
    // Target module of each import; `None` once an error has been reported for it.
    targets: Vec<Option<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active,
    Done,
}

struct Linker<'a> {
    project: Option<&'a Project>,
    read: ReadFn<'a>,
    map: SourceMap,
    modules: Vec<Module>,
    by_key: HashMap<(usize, Vec<String>), usize>,
    visit: Vec<Option<Visit>>,
    // Imports on the current DFS path as (importing module, import span).
    path: Vec<(usize, Span)>,
    errors: Vec<Diagnostic>,
}

/// Lexes, parses and links `src` plus every module it imports. The source map covers all files
/// loaded so far, also when errors are returned.
pub fn link(
    name: &str,
    src: &str,
    project: Option<&Project>,
    read: ReadFn,
) -> (SourceMap, Result<Program, Vec<Diagnostic>>) {
    let mut l = Linker {
        project,
        read,
        map: SourceMap::new(name, src),
        modules: Vec::new(),
        by_key: HashMap::new(),
        visit: Vec::new(),
        path: Vec::new(),
        errors: Vec::new(),
    };
    let program = match parse_at(src, 0) {
        Ok(p) => p,
        Err(d) => return (l.map, Err(vec![d])),
    };
    // Single-file programs without imports take the old path unchanged.
    if program.imports.is_empty() {
        return (l.map, Ok(program));
    }
    let pkg = project.map(|p| p.entry_package);
    l.push_module(pkg, String::new(), String::new(), program);
    if let (Some(p), Some(path)) = (project, project.and_then(|p| p.entry_module.clone())) {
        l.by_key.insert((p.entry_package, path), 0);
    }
    l.dfs(0);
    if l.errors.is_empty() {
        l.resolve_names();
    }
    if !l.errors.is_empty() {
        let mut errors = std::mem::take(&mut l.errors);
        errors.sort_by_key(|d| (d.span.start, d.code.as_str()));
        return (l.map, Err(errors));
    }
    let fns = l
        .modules
        .iter_mut()
        .flat_map(|m| std::mem::take(&mut m.program.fns))
        .collect();
    (
        l.map,
        Ok(Program {
            imports: Vec::new(),
            fns,
        }),
    )
}

fn shift(s: Span, base: usize) -> Span {
    Span::new(s.start + base, s.end + base)
}

fn shift_diag(mut d: Diagnostic, base: usize) -> Diagnostic {
    d.span = shift(d.span, base);
    for n in &mut d.notes {
        n.span = n.span.map(|s| shift(s, base));
    }
    d
}

fn parse_at(src: &str, base: usize) -> Result<Program, Diagnostic> {
    let mut toks = lexer::lex(src).map_err(|d| shift_diag(d, base))?;
    for t in &mut toks {
        t.span = shift(t.span, base);
    }
    // Token spans are already shifted, so parser diagnostics are too.
    parser::parse(toks)
}

fn note(span: Span, message: String) -> Note {
    Note {
        span: Some(span),
        message,
    }
}

/// Module file of `path` inside a package: `lib.qx` for the package root, `a/b.qx` for `pkg.a.b`.
pub fn module_file(src_dir: &Path, path: &[String]) -> PathBuf {
    match path.split_last() {
        None => src_dir.join("lib.qx"),
        Some((last, dirs)) => {
            let mut p = src_dir.to_path_buf();
            for d in dirs {
                p.push(d);
            }
            p.push(format!("{last}.qx"));
            p
        }
    }
}

impl Linker<'_> {
    fn push_module(&mut self, pkg: Option<usize>, display: String, prefix: String, p: Program) {
        let n = p.imports.len();
        self.modules.push(Module {
            pkg,
            display,
            prefix,
            program: p,
            targets: vec![None; n],
        });
        self.visit.push(None);
    }

    fn err(&mut self, code: Code, span: Span, message: String) -> &mut Diagnostic {
        self.errors.push(Diagnostic::new(code, span, message));
        self.errors.last_mut().unwrap()
    }

    fn dfs(&mut self, m: usize) {
        self.visit[m] = Some(Visit::Active);
        for i in 0..self.modules[m].program.imports.len() {
            let imp = self.modules[m].program.imports[i].clone();
            let Some((pkg, path)) = self.import_module(m, &imp) else {
                continue;
            };
            let key = (pkg, path);
            let target = match self.by_key.get(&key) {
                Some(&t) => t,
                None => match self.load(key, &imp) {
                    Some(t) => t,
                    None => continue,
                },
            };
            if self.visit[target] == Some(Visit::Active) {
                self.cycle(m, target, imp.span);
                continue;
            }
            self.modules[m].targets[i] = Some(target);
            if self.visit[target].is_none() {
                self.path.push((m, imp.span));
                self.dfs(target);
                self.path.pop();
            }
        }
        self.visit[m] = Some(Visit::Done);
    }

    fn module_name(&self, m: usize) -> String {
        let md = &self.modules[m];
        if md.display.is_empty() {
            let name = self.map.file_name(0).to_string();
            return name;
        }
        md.display.clone()
    }

    fn cycle(&mut self, from: usize, to: usize, at: Span) {
        // The DFS path from `to` down to `from`, closed by the import at `at`.
        let start = self
            .path
            .iter()
            .position(|&(m, _)| m == to)
            .unwrap_or(self.path.len());
        let mut edges: Vec<(usize, Span)> = self.path[start..].to_vec();
        edges.push((from, at));
        let mut chain: Vec<String> = edges.iter().map(|&(m, _)| self.module_name(m)).collect();
        chain.push(self.module_name(to));
        let notes: Vec<Note> = edges[..edges.len() - 1]
            .iter()
            .map(|&(m, s)| note(s, msg::note_imported_by(&self.module_name(m))))
            .collect();
        let d = self.err(
            Code::ImportCycle,
            at,
            msg::import_cycle(&chain.join(" -> ")),
        );
        d.notes = notes;
    }

    // Resolves the package and module path of an import, reporting unknown packages.
    fn import_module(&mut self, m: usize, imp: &Import) -> Option<(usize, Vec<String>)> {
        let first = &imp.module[0];
        let Some(project) = self.project else {
            self.err(
                Code::ImportOutsideProject,
                imp.span,
                msg::import_outside_project(),
            );
            return None;
        };
        let from = self.modules[m].pkg.unwrap_or(project.entry_package);
        let own = &project.packages[from];
        let pkg = if own.name == first.name {
            Some(from)
        } else {
            own.deps
                .iter()
                .copied()
                .find(|&d| project.packages[d].name == first.name)
        };
        let Some(pkg) = pkg else {
            self.err(
                Code::UnknownPackage,
                first.span,
                msg::unknown_package(&first.name),
            );
            return None;
        };
        let mut path: Vec<String> = imp.module[1..].iter().map(|s| s.name.clone()).collect();
        // `pkg.lib` is the package root itself (`lib.qx`), not a second copy of it.
        if path == ["lib"] {
            path.clear();
        }
        Some((pkg, path))
    }

    fn load(&mut self, key: (usize, Vec<String>), imp: &Import) -> Option<usize> {
        let project = self.project?;
        let root = &project.packages[key.0];
        let display = std::iter::once(root.name.clone())
            .chain(key.1.iter().cloned())
            .collect::<Vec<_>>()
            .join(".");
        if self.modules.len() >= MAX_MODULES {
            self.err(
                Code::ModuleNotFound,
                imp.span,
                msg::too_many_modules(MAX_MODULES),
            );
            return None;
        }
        let file = module_file(&root.src_dir, &key.1);
        let src = match (self.read)(&file) {
            Ok(s) => s,
            Err(e) => {
                let d = self.err(
                    Code::ModuleNotFound,
                    imp.span,
                    msg::module_not_found(&display),
                );
                d.notes.push(Note {
                    span: None,
                    message: msg::note_module_file(&file.display().to_string(), &e.to_string()),
                });
                return None;
            }
        };
        let src = src.strip_prefix('\u{feff}').unwrap_or(&src).to_string();
        let base = self.map.add(&file.display().to_string(), &src);
        let program = match parse_at(&src, base) {
            Ok(p) => p,
            Err(d) => {
                self.errors.push(d);
                return None;
            }
        };
        let prefix = format!("{display}.");
        let id = self.modules.len();
        self.push_module(Some(key.0), display, prefix, program);
        self.by_key.insert(key, id);
        Some(id)
    }

    fn find_fn(&self, m: usize, name: &str) -> Option<&FnDecl> {
        self.modules[m]
            .program
            .fns
            .iter()
            .find(|f| f.name.name == name)
    }

    fn resolve_names(&mut self) {
        // Every scope is built before any function is renamed, since scopes look up bare names.
        let scopes: Vec<Scope> = (0..self.modules.len()).map(|m| self.scope(m)).collect();
        for (m, scope) in scopes.iter().enumerate() {
            let mut fns = std::mem::take(&mut self.modules[m].program.fns);
            for f in &mut fns {
                self.rewrite_block(scope, &mut f.body);
            }
            let prefix = self.modules[m].prefix.clone();
            for f in &mut fns {
                f.name.name = format!("{prefix}{}", f.name.name);
            }
            self.modules[m].program.fns = fns;
        }
    }

    // Names visible in module `m`, in priority order: its own functions, single-function
    // imports, then wildcard imports.
    fn scope(&mut self, m: usize) -> Scope {
        let prefix = self.modules[m].prefix.clone();
        let mut local = HashMap::new();
        for f in &self.modules[m].program.fns {
            local
                .entry(f.name.name.clone())
                .or_insert((format!("{prefix}{}", f.name.name), f.name.span));
        }
        let mut explicit: HashMap<String, (String, Span)> = HashMap::new();
        let mut wildcards = Vec::new();
        let imports = self.modules[m].program.imports.clone();
        let targets = self.modules[m].targets.clone();
        for (imp, target) in imports.iter().zip(targets) {
            let Some(t) = target else { continue };
            let Some(item) = &imp.item else {
                wildcards.push(t);
                continue;
            };
            let module = self.modules[t].display.clone();
            let found = self
                .find_fn(t, &item.name)
                .map(|f| (f.public.is_some(), f.name.span));
            let Some((public, decl)) = found else {
                self.err(
                    Code::UnknownImport,
                    item.span,
                    msg::unknown_import(&item.name, &module),
                );
                continue;
            };
            if !public {
                self.err(
                    Code::PrivateFunction,
                    item.span,
                    msg::private_function(&item.name, &module),
                )
                .notes
                .push(note(decl, msg::note_declared_here()));
                continue;
            }
            let qualified = format!("{}{}", self.modules[t].prefix, item.name);
            if let Some(&(_, at)) = local.get(&item.name) {
                self.err(
                    Code::ImportConflict,
                    item.span,
                    msg::import_conflicts_local(&item.name),
                )
                .notes
                .push(note(at, msg::note_declared_here()));
                continue;
            }
            match explicit.get(&item.name) {
                Some((q, at)) if *q != qualified => {
                    let at = *at;
                    self.err(
                        Code::ImportConflict,
                        item.span,
                        msg::import_conflicts_import(&item.name),
                    )
                    .notes
                    .push(note(at, msg::note_first_import()));
                }
                Some(_) => {}
                None => {
                    explicit.insert(item.name.clone(), (qualified, item.span));
                }
            }
        }
        // Public functions of each wildcard-imported module: bare name -> (qualified, module).
        let mut wild: HashMap<String, Vec<(String, String)>> = HashMap::new();
        wildcards.sort_unstable();
        wildcards.dedup();
        for t in wildcards {
            let md = &self.modules[t];
            for f in md.program.fns.iter().filter(|f| f.public.is_some()) {
                wild.entry(f.name.name.clone())
                    .or_default()
                    .push((format!("{}{}", md.prefix, f.name.name), md.display.clone()));
            }
        }
        Scope {
            prefix,
            local: local.into_iter().map(|(k, (q, _))| (k, q)).collect(),
            explicit: explicit.into_iter().map(|(k, (q, _))| (k, q)).collect(),
            wild,
        }
    }

    fn resolve_call(&mut self, scope: &Scope, name: &mut Ident) {
        let n = name.name.as_str();
        if BUILTINS.contains(&n) {
            return;
        }
        if let Some(q) = scope.local.get(n).or_else(|| scope.explicit.get(n)) {
            name.name = q.clone();
            return;
        }
        match scope.wild.get(n).map(Vec::as_slice) {
            Some([(q, _)]) => name.name = q.clone(),
            Some(all @ [(q, _), ..]) => {
                let modules: Vec<String> = all.iter().map(|(_, m)| format!("`{m}`")).collect();
                let q = q.clone();
                self.err(
                    Code::AmbiguousName,
                    name.span,
                    msg::ambiguous_name(n, &modules.join(", ")),
                );
                // Keep checking against the first candidate instead of cascading into Q0202.
                name.name = q;
            }
            // Unknown here: qualify it so it can never match a function of another module; the
            // checker then reports Q0202.
            _ => name.name = format!("{}{n}", scope.prefix),
        }
    }

    fn rewrite_block(&mut self, scope: &Scope, b: &mut Block) {
        for s in &mut b.stmts {
            match s {
                Stmt::Decl { init, .. } => self.rewrite_expr(scope, init),
                Stmt::Assign { value, .. } => self.rewrite_expr(scope, value),
                Stmt::If(i) => self.rewrite_if(scope, i),
                Stmt::For {
                    start, end, body, ..
                } => {
                    self.rewrite_expr(scope, start);
                    self.rewrite_expr(scope, end);
                    self.rewrite_block(scope, body);
                }
                Stmt::Return { value, .. } => {
                    if let Some(e) = value {
                        self.rewrite_expr(scope, e);
                    }
                }
                Stmt::Expr(e) => self.rewrite_expr(scope, e),
            }
        }
    }

    fn rewrite_if(&mut self, scope: &Scope, i: &mut IfStmt) {
        self.rewrite_expr(scope, &mut i.cond);
        self.rewrite_block(scope, &mut i.then);
        match &mut i.els {
            Some(Else::Block(b)) => self.rewrite_block(scope, b),
            Some(Else::If(inner)) => self.rewrite_if(scope, inner),
            None => {}
        }
    }

    fn rewrite_expr(&mut self, scope: &Scope, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Call { name, args } => {
                self.resolve_call(scope, name);
                for a in args {
                    self.rewrite_expr(scope, a);
                }
            }
            ExprKind::Index { base, index } => {
                self.rewrite_expr(scope, base);
                self.rewrite_expr(scope, index);
            }
            ExprKind::Method { recv, .. } => self.rewrite_expr(scope, recv),
            ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => {
                self.rewrite_expr(scope, expr)
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.rewrite_expr(scope, lhs);
                self.rewrite_expr(scope, rhs);
            }
            ExprKind::Int(_)
            | ExprKind::Double(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Var(_) => {}
        }
    }
}

struct Scope {
    prefix: String,
    local: HashMap<String, String>,
    explicit: HashMap<String, String>,
    wild: HashMap<String, Vec<(String, String)>>,
}
