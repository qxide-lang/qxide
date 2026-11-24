// Experimental value semantics (docs/design/memory-model.md), on with
// `--experimental value-semantics` (src/lang/features.rs). Without the feature nothing here runs
// and the language is unchanged.
//
// `check` adds the ownership rules (§4 of the design) to a program the type checker accepted,
// using the types the checker recorded (`Types`). `lower` (src/lang/values/lower.rs) then rewrites
// the monomorphized program so the engines, which keep their shared-reference representation,
// run it with value semantics. That works because of the rules checked here: a large value
// reaches a second name only by a move (the source is dead afterwards) or by an explicit
// `.copy()`; a parameter without `inout` or `consume` cannot be changed; the places passed as
// `inout` arguments of one call do not overlap. Small structs are copied implicitly by `lower`.
// What stays shared is what a `Shared<T>` handle shares.
//
// Provisional choices (§11 of the design, each marked there "임시 결정") live in one place each:
// the syntax in `parser::inout_word` and `parser::call_args`, the rules here (`implicit_copy`
// for the size rule, `Kind::ForEach` for loop variables, `call_args` for call-site `inout`).

pub mod lower;

use std::collections::HashMap;

use super::ast::*;
use super::diag::{Code, Diagnostic, Fix, Note, Span};
use super::msg;
use super::native;

/// The types the checker recorded while checking a program with value semantics on, by the
/// index of the function in `Program::fns` (instances of one generic function share spans).
#[derive(Debug, Default)]
pub struct Types {
    /// The function being checked.
    pub current: usize,
    /// Every checked expression, by function and span (`start`, `end`).
    pub exprs: HashMap<(usize, usize, usize), Ty>,
    /// Every declared variable (parameters, locals, loop and pattern variables), by function
    /// and the span of its name.
    pub vars: HashMap<(usize, usize, usize), Ty>,
}

impl Types {
    pub fn expr(&self, f: usize, e: &Expr) -> Option<Ty> {
        self.exprs.get(&(f, e.span.start, e.span.end)).copied()
    }

    pub fn var(&self, f: usize, name: &Ident) -> Option<Ty> {
        self.vars.get(&(f, name.span.start, name.span.end)).copied()
    }
}

/// How a parameter receives its argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The default: a read-only borrow of the caller's value.
    Borrow,
    /// `inout`: a borrow through which the callee changes the caller's value.
    Inout,
    /// `consume`: the callee owns the value (a move, or a copy of a small value).
    Consume,
}

impl Mode {
    pub fn of(p: &Param) -> Mode {
        if p.inout.is_some() {
            Mode::Inout
        } else if p.consume.is_some() {
            Mode::Consume
        } else {
            Mode::Borrow
        }
    }
}

/// `List` and `Map` methods whose value arguments the collection keeps (moved in).
const STORING: &[&str] = &["add", "set", "insert", "put"];

/// `List` and `Map` methods whose result shares values the collection keeps: like an element
/// read, a large result must be copied before it is kept.
const PROJECTING: &[&str] = &["get", "getOrDefault", "keys", "values", "toArray"];

/// The prelude's shared-handle type (`Shared<T>`, §2 rule 5 of the design).
pub const SHARED: &str = "Shared";

// The type information the rules and the lowering both need.
pub struct Decls<'p> {
    structs: HashMap<&'p str, &'p StructDecl>,
    enums: HashMap<&'p str, &'p EnumDecl>,
    ifaces: HashMap<&'p str, &'p IfaceDecl>,
    fns: HashMap<&'p str, &'p FnDecl>,
}

impl<'p> Decls<'p> {
    pub fn new(p: &'p Program) -> Self {
        Decls {
            structs: p
                .structs
                .iter()
                .map(|s| (s.name.name.as_str(), s))
                .collect(),
            enums: p.enums.iter().map(|e| (e.name.name.as_str(), e)).collect(),
            ifaces: p.ifaces.iter().map(|i| (i.name.name.as_str(), i)).collect(),
            fns: p.fns.iter().map(|f| (f.name.name.as_str(), f)).collect(),
        }
    }

    pub fn fn_decl(&self, name: &str) -> Option<&'p FnDecl> {
        self.fns.get(name).copied()
    }

    /// The field types of struct type `t` (an instance's type arguments substituted).
    pub fn fields(&self, t: Ty) -> Option<Vec<(String, Ty)>> {
        let (base, args) = split(t);
        let s = self.structs.get(base.name())?;
        let map = |f: Ty| subst(f, &s.params, &args);
        Some(
            s.fields
                .iter()
                .map(|f| (f.name.name.clone(), map(f.ty)))
                .collect(),
        )
    }

    /// The payload types of every variant of enum type `t`.
    pub fn payloads(&self, t: Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        let (base, args) = split(t);
        let e = self.enums.get(base.name())?;
        Some(
            e.variants
                .iter()
                .map(|v| {
                    let tys = v.fields.iter().map(|f| subst(f.ty, &e.params, &args));
                    (v.name.name.clone(), tys.collect())
                })
                .collect(),
        )
    }

    /// Is `f` a method of the prelude's `Result` or `Option`? They are checked as library
    /// code: the rules do not apply, and `lower` copies the payloads they return.
    pub fn prelude_fn(&self, f: &FnDecl) -> bool {
        f.method.is_some()
            && f.name
                .name
                .split(['.', '<'])
                .next()
                .is_some_and(|owner| self.enums.get(owner).is_some_and(|e| e.prelude))
    }

    /// Can `lower` copy a value of type `t` (the checker's `uncopyable` agrees)?
    pub fn copyable(&self, t: Ty) -> bool {
        self.copyable_in(t, &mut Vec::new())
    }

    fn copyable_in(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        if seen.contains(&t) || self.frozen(t) || self.is_shared(t) {
            return true;
        }
        seen.push(t);
        let ok = match t {
            Ty::Array(_) => t.elem().is_some_and(|e| self.copyable_in(e, seen)),
            Ty::Struct(_) => self
                .fields(t)
                .is_some_and(|fs| fs.iter().all(|(_, f)| self.copyable_in(*f, seen))),
            Ty::Native(_) => native::kind(t).is_some_and(|(k, args)| {
                (k == "List" || k == "Map") && args.iter().all(|a| self.copyable_in(*a, seen))
            }),
            _ => false,
        };
        seen.pop();
        ok
    }

    /// Is `t` the prelude's `Shared<T>` (not a program's own struct of that name)?
    pub fn is_shared(&self, t: Ty) -> bool {
        let (base, _) = split(t);
        matches!(t, Ty::Struct(_))
            && [t.name(), base.name()]
                .iter()
                .any(|n| self.structs.get(n).is_some_and(|s| s.prelude))
    }

    /// Values copied without `.copy()`: numbers, `boolean`, enums and structs made only of such
    /// values (no heap memory of their own), and `Shared<T>` handles. Provisional (§11 "큰 값의
    /// 복사 방식과 기준"): everything else, `String` included, is large and moves.
    pub fn implicit_copy(&self, t: Ty) -> bool {
        self.small(t, &mut Vec::new())
    }

    fn small(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        if seen.contains(&t) {
            return false;
        }
        match t {
            Ty::Int | Ty::Double | Ty::Boolean => true,
            Ty::Struct(_) if self.is_shared(t) => true,
            Ty::Struct(_) => {
                seen.push(t);
                let ok = self
                    .fields(t)
                    .is_some_and(|fs| fs.iter().all(|(_, f)| self.small(*f, seen)));
                seen.pop();
                ok
            }
            Ty::Enum(_) => {
                seen.push(t);
                let ok = self.payloads(t).is_some_and(|vs| {
                    vs.iter()
                        .all(|(_, tys)| tys.iter().all(|p| self.small(*p, seen)))
                });
                seen.pop();
                ok
            }
            _ => false,
        }
    }

    /// Values that never change once made (`String`, `Json`, enums of such values): a copy can
    /// share them.
    pub fn frozen(&self, t: Ty) -> bool {
        self.frozen_in(t, &mut Vec::new())
    }

    fn frozen_in(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        if seen.contains(&t) {
            return true;
        }
        match t {
            Ty::Int | Ty::Double | Ty::Boolean | Ty::String => true,
            Ty::Native(_) => native::kind(t).is_some_and(|(k, _)| k == "Json"),
            Ty::Enum(_) => {
                seen.push(t);
                let ok = self.payloads(t).is_some_and(|vs| {
                    vs.iter()
                        .all(|(_, tys)| tys.iter().all(|p| self.frozen_in(*p, seen)))
                });
                seen.pop();
                ok
            }
            _ => false,
        }
    }

    /// The parameter modes of method `name` of a value of type `t`, `this` first, or `None`
    /// for a built-in type or an unknown method.
    pub fn method_modes(&self, t: Ty, name: &str) -> Option<Vec<Mode>> {
        match t {
            Ty::Struct(_) | Ty::Enum(_) => {
                let (base, _) = split(t);
                let f = self.fns.get(format!("{}.{name}", base.name()).as_str())?;
                Some(f.params.iter().map(Mode::of).collect())
            }
            Ty::Iface(_) => {
                let (base, _) = split(t);
                let i = self.ifaces.get(base.name())?;
                let m = i.methods.iter().find(|m| m.name.name == name)?;
                let mut modes = vec![Mode::Borrow];
                modes.extend(m.params.iter().map(Mode::of));
                Some(modes)
            }
            _ => None,
        }
    }
}

// `Box<int>` is `(Box, [int])`; a type without arguments is itself.
fn split(t: Ty) -> (Ty, Vec<Ty>) {
    t.generic_args().unwrap_or((t, Vec::new()))
}

fn subst(t: Ty, params: &[Ident], args: &[Ty]) -> Ty {
    if params.is_empty() || params.len() != args.len() {
        return t;
    }
    t.subst(&|p| {
        params
            .iter()
            .position(|q| p == Ty::param_named(&q.name))
            .map(|i| args[i])
    })
}

/// Hidden variables of `for x in xs` loops (`parser::for_stmt`): `#it` holds the collection and
/// `#i` the index. A program cannot write these names.
fn is_loop_collection(name: &str) -> bool {
    name.starts_with("#it")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Live,
    Moved(Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    // A local the function owns: a declared variable or a `consume` parameter.
    Owned,
    // A parameter without `inout` or `consume`; `fix_at` is where `inout ` would go (before the
    // parameter type, or before the return type of a method for `this`).
    Borrowed { fix_at: usize, this: bool },
    Inout,
    // The variable of `for x in xs`: a read-only view of the element (provisional, §11
    // "for-each 변수의 의미"; no silent change from today's copy of the reference).
    ForEach,
    // A variable bound by a `match` pattern: a read-only view of the payload.
    Bound,
    // The hidden collection of a `for x in xs` loop.
    Collection,
}

struct Local {
    name: String,
    kind: Kind,
    decl: Span,
    state: State,
    // A small struct copied from a field or element (`P p = ps[i];`): its first change is
    // reported (`Q0803`), since it used to change `ps[i]` too.
    copy_of: Option<(Span, String)>,
}

// One step of a place below its variable: a field, or an element (`Some` for a literal index).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Field(String),
    Elem(Option<i64>),
}

/// The ownership rules for every function of `p`: errors, then warnings (`Q0803`).
pub fn check(p: &Program, types: &Types) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let decls = Decls::new(p);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (fi, f) in p.fns.iter().enumerate() {
        if decls.prelude_fn(f) {
            continue;
        }
        decl_rules(f, &decls, &mut errors);
        let mut w = Walker {
            decls: &decls,
            types,
            fi,
            locals: Vec::new(),
            scopes: vec![Vec::new()],
            loops: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        };
        w.function(f);
        errors.append(&mut w.errors);
        warnings.append(&mut w.warnings);
    }
    for i in &p.ifaces {
        for m in &i.methods {
            for q in &m.params {
                if let Some(at) = q.inout {
                    let d = Diagnostic::new(Code::InoutArgument, at, msg::vs_iface_inout());
                    errors.push(remove_word(d, at));
                }
            }
        }
    }
    // An `inout` method cannot implement an interface method yet: calls through the interface
    // would not know that `this` changes.
    for s in &p.structs {
        for (iface, _) in &s.implements {
            let (base, _) = split(*iface);
            let Some(i) = decls.ifaces.get(base.name()) else {
                continue;
            };
            for m in &i.methods {
                let name = format!("{}.{}", s.name.name, m.name.name);
                if let Some(at) = decls.fns.get(name.as_str()).and_then(|f| f.mutating) {
                    let d = Diagnostic::new(
                        Code::InoutArgument,
                        at,
                        msg::vs_iface_mutating(&m.name.name, base.name()),
                    );
                    errors.push(remove_word(d, at));
                }
            }
        }
    }
    errors.sort_by_key(|d| (d.span.start, d.code.as_str()));
    errors.dedup_by(|a, b| a.span == b.span && a.code == b.code);
    warnings.sort_by_key(|d| d.span.start);
    (errors, warnings)
}

// A fix that deletes the word at `at` and the space after it.
fn remove_word(mut d: Diagnostic, at: Span) -> Diagnostic {
    d.fixes.push(Fix::new(
        msg::vs_fix_remove("inout"),
        Span::new(at.start, at.end + 1),
        "",
    ));
    d
}

// Rules about a declaration alone: `inout` on qubits, `inout` with `consume`.
fn decl_rules(f: &FnDecl, decls: &Decls, errors: &mut Vec<Diagnostic>) {
    let _ = decls;
    for q in &f.params {
        let Some(at) = q.inout else { continue };
        if q.ty.is_quantum() {
            let d = Diagnostic::new(Code::InoutArgument, at, msg::vs_inout_qubit(q.ty.name()));
            errors.push(remove_word(d, at));
        } else if q.consume.is_some() {
            let d = Diagnostic::new(Code::InoutArgument, at, msg::vs_inout_consume());
            errors.push(remove_word(d, at));
        }
    }
}

struct Loop {
    // `locals.len()` when the loop began: older locals belong outside it.
    outer: usize,
    before: Vec<State>,
}

struct Walker<'a, 'p> {
    decls: &'a Decls<'p>,
    types: &'a Types,
    // The index of the function in `Program::fns`.
    fi: usize,
    locals: Vec<Local>,
    scopes: Vec<Vec<usize>>,
    loops: Vec<Loop>,
    errors: Vec<Diagnostic>,
    warnings: Vec<Diagnostic>,
}

impl Walker<'_, '_> {
    fn ty(&self, e: &Expr) -> Option<Ty> {
        self.types.expr(self.fi, e)
    }

    fn classical(t: Ty) -> bool {
        !t.is_quantum() && !matches!(t, Ty::Error | Ty::Void)
    }

    fn declare(&mut self, name: &Ident, ty: Ty, kind: Kind) {
        let _ = ty;
        self.locals.push(Local {
            name: name.name.clone(),
            kind,
            decl: name.span,
            state: State::Live,
            copy_of: None,
        });
        let id = self.locals.len() - 1;
        self.scopes.last_mut().unwrap().push(id);
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .copied()
            .find(|&id| self.locals[id].name == name)
    }

    fn states(&self) -> Vec<State> {
        self.locals.iter().map(|l| l.state).collect()
    }

    fn restore(&mut self, states: &[State]) {
        for (l, s) in self.locals.iter_mut().zip(states) {
            l.state = *s;
        }
    }

    // After two paths that both continue: moved on either is moved.
    fn merge(&mut self, a: &[State], b: &[State]) {
        for (l, (x, y)) in self.locals.iter_mut().zip(a.iter().zip(b)) {
            l.state = match (x, y) {
                (State::Moved(s), _) | (_, State::Moved(s)) => State::Moved(*s),
                _ => State::Live,
            };
        }
    }

    fn function(&mut self, f: &FnDecl) {
        for (i, q) in f.params.iter().enumerate() {
            if !Self::classical(q.ty) {
                continue;
            }
            let this = i == 0 && f.method.is_some();
            let kind = match Mode::of(q) {
                Mode::Consume => Kind::Owned,
                Mode::Inout => Kind::Inout,
                Mode::Borrow => Kind::Borrowed {
                    fix_at: if this {
                        f.ret_span.start
                    } else {
                        q.ty_span.start
                    },
                    this,
                },
            };
            self.declare(&q.name, q.ty, kind);
        }
        self.block(&f.body);
    }

    fn block(&mut self, b: &Block) -> bool {
        self.scopes.push(Vec::new());
        let mut diverges = false;
        for s in &b.stmts {
            diverges |= self.stmt(s);
        }
        self.scopes.pop();
        diverges
    }

    fn stmt(&mut self, s: &Stmt) -> bool {
        match s {
            Stmt::Decl { name, init, .. } => {
                let ty = self.types.var(self.fi, name).unwrap_or(Ty::Error);
                let foreach = matches!(&init.kind, ExprKind::Index { base, .. }
                    if matches!(&base.kind, ExprKind::Var(n) if is_loop_collection(n)));
                let kind = if is_loop_collection(&name.name) {
                    // The loop reads the collection; it does not take it.
                    self.read(init);
                    Kind::Collection
                } else if foreach {
                    self.read(init);
                    Kind::ForEach
                } else {
                    self.value(init);
                    Kind::Owned
                };
                if Self::classical(ty) {
                    self.declare(name, ty, kind);
                    let small_struct = matches!(ty, Ty::Struct(_))
                        && self.decls.implicit_copy(ty)
                        && !self.decls.is_shared(ty);
                    let inner = !matches!(init.kind, ExprKind::Var(_)) && self.is_place(init);
                    if kind == Kind::Owned && small_struct && inner {
                        self.locals.last_mut().unwrap().copy_of = Some((init.span, text_of(init)));
                    }
                }
                false
            }
            Stmt::Assign { name, value } => {
                self.value(value);
                if let Some(id) = self.lookup(&name.name) {
                    self.write_var(id, name.span, true);
                }
                false
            }
            Stmt::SetField {
                base, op, value, ..
            } => {
                self.store(*op, value);
                // `s.value = v` changes what a `Shared<T>` handle shares, not the handle.
                if self.ty(base).is_some_and(|t| self.decls.is_shared(t)) {
                    self.read(base);
                } else {
                    self.write_place(base);
                }
                false
            }
            Stmt::SetIndex {
                base,
                index,
                op,
                value,
                ..
            } => {
                self.store(*op, value);
                self.read(index);
                self.write_place(base);
                false
            }
            Stmt::If(i) => self.if_stmt(i),
            Stmt::For {
                start, end, body, ..
            } => {
                self.read(start);
                self.read(end);
                self.looped(None, body);
                false
            }
            Stmt::While { cond, body, .. } => {
                self.looped(Some(cond), body);
                matches!(cond.kind, ExprKind::Bool(true))
            }
            Stmt::Break { .. } => true,
            Stmt::Continue { .. } => {
                self.moved_in_loop();
                true
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.value(v);
                }
                true
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.read(scrutinee);
                let before = self.states();
                let mut after: Option<Vec<State>> = None;
                let mut all_diverge = !arms.is_empty();
                for arm in arms {
                    self.restore(&before);
                    self.scopes.push(Vec::new());
                    if let PatKind::Variant { binds, .. } = &arm.pat.kind {
                        for b in binds.iter().filter(|b| b.name != "_") {
                            let ty = self.types.var(self.fi, b);
                            if let Some(t) = ty.filter(|t| Self::classical(*t)) {
                                self.declare(b, t, Kind::Bound);
                            }
                        }
                    }
                    let d = self.block(&arm.body);
                    self.scopes.pop();
                    if !d {
                        all_diverge = false;
                        let now = self.states();
                        after = Some(match after {
                            Some(prev) => {
                                self.merge(&prev, &now);
                                self.states()
                            }
                            None => now,
                        });
                    }
                }
                match after {
                    Some(s) => self.restore(&s),
                    None => self.restore(&before),
                }
                all_diverge
            }
            Stmt::Expr(e) => {
                self.read(e);
                false
            }
        }
    }

    fn if_stmt(&mut self, i: &IfStmt) -> bool {
        self.read(&i.cond);
        let before = self.states();
        let d1 = self.block(&i.then);
        let s1 = self.states();
        self.restore(&before);
        let d2 = match &i.els {
            Some(Else::Block(b)) => self.block(b),
            Some(Else::If(inner)) => self.if_stmt(inner),
            None => false,
        };
        let s2 = self.states();
        match (d1, d2) {
            (true, false) => self.restore(&s2),
            (false, true) => self.restore(&s1),
            (true, true) => {}
            (false, false) => self.merge(&s1, &s2),
        }
        d1 && d2
    }

    // A loop body (and `while` condition): a value moved out of an outer variable would be gone
    // on the next iteration (`Q0324`).
    fn looped(&mut self, cond: Option<&Expr>, body: &Block) {
        let before = self.states();
        self.loops.push(Loop {
            outer: self.locals.len(),
            before: before.clone(),
        });
        if let Some(c) = cond {
            self.read(c);
        }
        if !self.block(body) {
            self.moved_in_loop();
        }
        self.loops.pop();
        self.restore(&before);
    }

    fn moved_in_loop(&mut self) {
        let Some(l) = self.loops.last() else { return };
        let mut found = Vec::new();
        for (id, s) in l.before.iter().enumerate().take(l.outer) {
            if let (State::Live, State::Moved(at)) = (s, self.locals[id].state) {
                found.push((id, at));
            }
        }
        for (id, at) in found {
            let local = &self.locals[id];
            let mut d = Diagnostic::new(Code::MovedInLoop, at, msg::vs_moved_in_loop(&local.name));
            d.notes.push(note(local.decl, msg::note_declared_here()));
            d.fixes.push(Fix::new(
                msg::vs_fix_copy(&local.name),
                Span::new(at.end, at.end),
                ".copy()",
            ));
            self.errors.push(d);
            // Reported once; the next iteration sees it live again.
            self.locals[id].state = State::Live;
        }
    }

    // The value of `p.f = v` or `a[i] op= v`: stored (a move) unless combined with an operator.
    fn store(&mut self, op: Option<BinOp>, value: &Expr) {
        if op.is_some() {
            self.read(value);
        } else {
            self.value(value);
        }
    }

    // Reads every variable `e` mentions; calls pass their arguments by parameter mode.
    fn read(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Var(n) => {
                if let Some(id) = self.lookup(n) {
                    self.used(id, e.span);
                }
            }
            ExprKind::Call { name, args } => {
                let f = self.decls.fn_decl(&name.name);
                let modes = f.map(|f| f.params.iter().map(Mode::of).collect::<Vec<_>>());
                match (f, modes.as_deref(), args.split_first()) {
                    // A method call after monomorphization: the receiver is the first argument.
                    (Some(f), Some([this, rest @ ..]), Some((recv, args)))
                        if f.method.is_some() =>
                    {
                        self.call_args(Some(rest), Some((recv, *this)), args);
                    }
                    _ => self.call_args(modes.as_deref(), None, args),
                }
            }
            ExprKind::Method { recv, name, args } => self.method(recv, name, args),
            ExprKind::Index { base, index } => {
                self.read(base);
                self.read(index);
            }
            ExprKind::Field { recv, .. } => self.read(recv),
            ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => self.read(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.read(lhs);
                self.read(rhs);
            }
            ExprKind::NewStruct { args: items, .. }
            | ExprKind::Variant { args: items, .. }
            | ExprKind::ArrayLit { elems: items, .. } => {
                for a in items {
                    self.value(a);
                }
            }
            ExprKind::NewArray { dims, .. } => {
                for d in dims {
                    self.read(d);
                }
            }
            ExprKind::Try { expr, .. } => self.value(expr),
            ExprKind::Inout { expr } => {
                let d = Diagnostic::new(Code::InoutArgument, e.span, msg::vs_inout_not_argument());
                let word = Span::new(e.span.start, expr.span.start);
                self.errors.push(remove_word(d, word));
                self.read(expr);
            }
            ExprKind::Int(_) | ExprKind::Double(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        }
    }

    fn used(&mut self, id: usize, at: Span) {
        if let State::Moved(was) = self.locals[id].state {
            let name = self.locals[id].name.clone();
            let mut d = Diagnostic::new(Code::UseAfterMove, at, msg::vs_use_after_move(&name));
            d.notes.push(note(was, msg::vs_note_moved_here()));
            d.fixes.push(Fix::new(
                msg::vs_fix_copy(&name),
                Span::new(was.end, was.end),
                ".copy()",
            ));
            self.errors.push(d);
        }
    }

    // `e` where a value is kept: a declaration, assignment, store, `return`, constructor or
    // `consume` argument. A large value moves out of a variable it owns; copying it from anything
    // else needs `.copy()` (`Q0322`). Small values are copied (by `lower`).
    fn value(&mut self, e: &Expr) {
        let Some(ty) = self.ty(e) else {
            return self.read(e);
        };
        if !Self::classical(ty) || self.decls.implicit_copy(ty) || !self.is_place(e) {
            return self.read(e);
        }
        if let ExprKind::Var(n) = &e.kind {
            if let Some(id) = self.lookup(n) {
                self.used(id, e.span);
                let (kind, name) = (self.locals[id].kind, self.locals[id].name.clone());
                match kind {
                    Kind::Owned => self.locals[id].state = State::Moved(e.span),
                    Kind::Collection => {}
                    Kind::Borrowed { fix_at, this } => {
                        let mut d = self.copy_required(e, ty, msg::vs_move_borrowed(&name));
                        if !this {
                            d.fixes.push(Fix::new(
                                msg::vs_fix_param("consume", &name),
                                Span::new(fix_at, fix_at),
                                "consume ",
                            ));
                        }
                        self.errors.push(d);
                    }
                    Kind::Inout | Kind::ForEach | Kind::Bound => {
                        let d = self.copy_required(e, ty, msg::vs_move_borrowed(&name));
                        self.errors.push(d);
                    }
                }
                return;
            }
        }
        self.read(e);
        let d = self.copy_required(e, ty, msg::vs_copy_required(ty.name()));
        self.errors.push(d);
    }

    fn copy_required(&self, e: &Expr, ty: Ty, message: String) -> Diagnostic {
        let mut d = Diagnostic::new(Code::CopyRequired, e.span, message);
        if matches!(ty, Ty::Param(_)) {
            d.help = Some(msg::vs_help_param_copy(ty.name()));
            return d;
        }
        // `r.get(k)?` copies the payload: `.copy()` goes before the `?`.
        let at = match &e.kind {
            ExprKind::Try { expr, .. } => expr.span.end,
            _ => e.span.end,
        };
        d.fixes.push(Fix::new(
            msg::vs_fix_copy(&text_of(e)),
            Span::new(at, at),
            ".copy()",
        ));
        d
    }

    // Does `e` name a value something else keeps (a variable, field, element, collection read or
    // the payload of one) rather than a new one?
    fn is_place(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Var(_) | ExprKind::Field { .. } | ExprKind::Index { .. } => true,
            ExprKind::Method { recv, name, .. } => {
                PROJECTING.contains(&name.name.as_str())
                    && self.ty(recv).is_some_and(|t| {
                        native::kind(t).is_some_and(|(k, _)| k == "List" || k == "Map")
                    })
            }
            ExprKind::Try { expr, .. } => self.is_place(expr),
            _ => false,
        }
    }

    // The variable and path of a place (`a.b[2]` is `a`, [b, 2]), or `None` for a temporary or
    // a place reached through a `Shared<T>` handle, which may be changed by anyone holding it.
    fn place(&self, e: &Expr) -> Option<(usize, Vec<Step>)> {
        match &e.kind {
            ExprKind::Var(n) => self.lookup(n).map(|id| (id, Vec::new())),
            ExprKind::Field { recv, name } => {
                if self.ty(recv).is_some_and(|t| self.decls.is_shared(t)) {
                    return None;
                }
                let (id, mut path) = self.place(recv)?;
                path.push(Step::Field(name.name.clone()));
                Some((id, path))
            }
            ExprKind::Index { base, index } => {
                let (id, mut path) = self.place(base)?;
                path.push(Step::Elem(match index.kind {
                    ExprKind::Int(k) => Some(k),
                    _ => None,
                }));
                Some((id, path))
            }
            _ => None,
        }
    }

    // `x = v` (`whole`) or a change inside `x`.
    fn write_var(&mut self, id: usize, at: Span, whole: bool) {
        let (kind, name) = (self.locals[id].kind, self.locals[id].name.clone());
        match kind {
            Kind::Owned => {
                if whole {
                    self.locals[id].state = State::Live;
                    self.locals[id].copy_of = None;
                } else {
                    self.used(id, at);
                    if let Some((from, src)) = self.locals[id].copy_of.take() {
                        let mut d = Diagnostic::new(
                            Code::CopyChanged,
                            at,
                            msg::vs_copy_changed(&name, &src),
                        );
                        d.notes.push(note(from, msg::vs_note_copied_here()));
                        d.help = Some(msg::vs_help_copy_changed(&name, &src));
                        self.warnings.push(d);
                    }
                }
            }
            Kind::Inout | Kind::Collection => self.used(id, at),
            Kind::Borrowed { fix_at, this } => {
                let mut d = Diagnostic::new(Code::BorrowedWrite, at, msg::vs_borrowed_write(&name));
                if this {
                    d.help = Some(msg::vs_help_mutating());
                    d.fixes.push(Fix::new(
                        msg::vs_fix_mutating(),
                        Span::new(fix_at, fix_at),
                        "inout ",
                    ));
                } else {
                    d.help = Some(msg::vs_help_borrowed_write(&name));
                    d.fixes.push(Fix::new(
                        msg::vs_fix_param("inout", &name),
                        Span::new(fix_at, fix_at),
                        "inout ",
                    ));
                    d.fixes.push(Fix::new(
                        msg::vs_fix_param("consume", &name),
                        Span::new(fix_at, fix_at),
                        "consume ",
                    ));
                }
                self.errors.push(d);
            }
            Kind::ForEach | Kind::Bound => {
                let mut d = Diagnostic::new(Code::BorrowedWrite, at, msg::vs_view_write(&name));
                d.help = Some(if kind == Kind::ForEach {
                    msg::vs_help_foreach_write()
                } else {
                    msg::vs_help_bound_write()
                });
                self.errors.push(d);
            }
        }
    }

    // The target of `p.f = v`, `a[i] = v`, an `inout` argument or the receiver of a method that
    // changes it.
    fn write_place(&mut self, e: &Expr) {
        // Indices are read; a place reached through a temporary or a handle is not checked.
        self.read_indices(e);
        if let Some((id, path)) = self.place(e) {
            let _ = path;
            self.write_var(id, e.span, false);
        } else if let ExprKind::Field { recv, .. } = &e.kind {
            self.read(recv);
        }
    }

    fn read_indices(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Field { recv, .. } => self.read_indices(recv),
            ExprKind::Index { base, index } => {
                self.read_indices(base);
                self.read(index);
            }
            ExprKind::Var(_) => {}
            _ => self.read(e),
        }
    }

    fn method(&mut self, recv: &Expr, name: &Ident, args: &[Expr]) {
        let Some(t) = self.ty(recv) else {
            self.read(recv);
            for a in args {
                self.read(a);
            }
            return;
        };
        if let Some(modes) = self.decls.method_modes(t, &name.name) {
            return self.call_args(Some(&modes[1..]), Some((recv, modes[0])), args);
        }
        if let Some((kind, _)) = native::kind(t) {
            let n = name.name.as_str();
            if native::is_mutator(n) && kind != "Random" {
                self.write_place(recv);
            } else {
                self.read(recv);
            }
            let storing = STORING.contains(&n);
            for (i, a) in args.iter().enumerate() {
                // `m.put(k, v)` keeps both; `xs.set(i, v)` and `xs.insert(i, v)` keep `v`.
                let kept = storing && (n == "add" || n == "put" || i == 1);
                if kept {
                    self.value(a);
                } else {
                    self.read(a);
                }
            }
            return;
        }
        self.read(recv);
        for a in args {
            self.read(a);
        }
    }

    // Arguments by parameter mode (`None`: a built-in function, which only reads), with the
    // receiver of a method call and the mode of its `this`.
    fn call_args(&mut self, modes: Option<&[Mode]>, recv: Option<(&Expr, Mode)>, args: &[Expr]) {
        // The places this call lends, for the overlap rule: (argument, place, `inout`).
        let mut lent: Vec<(Span, (usize, Vec<Step>), bool)> = Vec::new();
        if let Some((r, mode)) = recv {
            if mode == Mode::Inout {
                self.write_place(r);
            } else {
                self.read(r);
            }
            if let Some(p) = self.place(r) {
                lent.push((r.span, p, mode == Mode::Inout));
            }
        }
        for (i, a) in args.iter().enumerate() {
            let mode = modes
                .and_then(|m| m.get(i))
                .copied()
                .unwrap_or(Mode::Borrow);
            let (inner, marked) = match &a.kind {
                ExprKind::Inout { expr } => (expr.as_ref(), true),
                _ => (a, false),
            };
            // Qubits (the checker records no type for a qubit lent to a call): the type
            // checker's rules (`Q0301`-`Q0315`).
            if self.ty(inner).is_none_or(|t| !Self::classical(t)) {
                if marked {
                    let d = Diagnostic::new(Code::InoutArgument, a.span, msg::vs_inout_qubit_arg());
                    self.errors.push(remove_word(
                        d,
                        Span::new(a.span.start, inner.span.start - 1),
                    ));
                }
                continue;
            }
            match mode {
                Mode::Inout => {
                    if !marked {
                        let mut d =
                            Diagnostic::new(Code::InoutArgument, a.span, msg::vs_inout_unmarked());
                        d.fixes.push(Fix::new(
                            msg::vs_fix_add_inout(),
                            Span::new(a.span.start, a.span.start),
                            "inout ",
                        ));
                        self.errors.push(d);
                    }
                    match self.place(inner) {
                        Some(p) => {
                            self.write_place(inner);
                            lent.push((inner.span, p, true));
                        }
                        None => {
                            self.read(inner);
                            let d = Diagnostic::new(
                                Code::InoutArgument,
                                inner.span,
                                msg::vs_inout_place(),
                            );
                            self.errors.push(d);
                        }
                    }
                }
                Mode::Borrow | Mode::Consume => {
                    if marked {
                        let d = Diagnostic::new(Code::InoutArgument, a.span, msg::vs_inout_extra());
                        self.errors.push(remove_word(
                            d,
                            Span::new(a.span.start, inner.span.start - 1),
                        ));
                    }
                    if mode == Mode::Consume {
                        self.value(inner);
                    } else {
                        self.read(inner);
                        if let Some(p) = self.place(inner) {
                            lent.push((inner.span, p, false));
                        }
                    }
                }
            }
        }
        // `Q0323`: a place lent as `inout` overlaps another place of the same call.
        for j in 0..lent.len() {
            for i in 0..j {
                let ((ai, pi, wi), (aj, pj, wj)) = (&lent[i], &lent[j]);
                if (*wi || *wj) && pi.0 == pj.0 && overlaps(&pi.1, &pj.1) {
                    let name = self.locals[pi.0].name.clone();
                    let mut d = Diagnostic::new(
                        Code::OverlappingInout,
                        *aj,
                        msg::vs_overlapping_inout(&name),
                    );
                    d.notes.push(note(*ai, msg::note_first_use()));
                    self.errors.push(d);
                }
            }
        }
    }
}

// Two paths below the same variable overlap unless they part at different fields or different
// literal indices.
fn overlaps(a: &[Step], b: &[Step]) -> bool {
    for (x, y) in a.iter().zip(b) {
        match (x, y) {
            (Step::Field(f), Step::Field(g)) if f != g => return false,
            (Step::Elem(Some(i)), Step::Elem(Some(j))) if i != j => return false,
            _ => {}
        }
    }
    true
}

fn note(span: Span, message: String) -> Note {
    Note {
        span: Some(span),
        message,
    }
}

// A short source-like rendering of a place for fix titles (`a.b[i]`).
fn text_of(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Var(n) => n.clone(),
        ExprKind::Field { recv, name } => format!("{}.{}", text_of(recv), name.name),
        ExprKind::Index { base, index } => match &index.kind {
            ExprKind::Var(_) | ExprKind::Field { .. } => {
                format!("{}[{}]", text_of(base), text_of(index))
            }
            ExprKind::Int(k) => format!("{}[{k}]", text_of(base)),
            _ => format!("{}[..]", text_of(base)),
        },
        ExprKind::Method { recv, name, .. } => format!("{}.{}(..)", text_of(recv), name.name),
        ExprKind::Try { expr, .. } => format!("{}?", text_of(expr)),
        _ => "the value".to_string(),
    }
}
