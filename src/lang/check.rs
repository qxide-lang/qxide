// Static checker: names, types, linear qubit ownership, and `if`-on-`Bit` restrictions.
// Flow-sensitive over the AST; reports every error it finds instead of stopping at the first.

use std::collections::HashMap;

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::msg;
use super::pure;

pub const BUILTINS: &[&str] = &[
    "qubit", "qreg", "h", "x", "z", "s", "sdg", "t", "tdg", "reset", "rx", "ry", "rz", "cx", "ccx",
    "measure", "discard", "len", "println",
];

pub const ONE_QUBIT_OPS: &[&str] = &["h", "x", "z", "s", "sdg", "t", "tdg", "reset"];

pub const ROTATIONS: &[&str] = &["rx", "ry", "rz"];

// Built-in `double` constants; their names cannot be declared or assigned.
pub const CONSTANTS: &[(&str, f64)] = &[("PI", std::f64::consts::PI)];

pub fn constant(name: &str) -> Option<f64> {
    CONSTANTS.iter().find(|(n, _)| *n == name).map(|&(_, v)| v)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Live,
    Consumed(Span),
}

struct Var {
    name: String,
    ty: Ty,
    mutable: bool,
    decl: Span,
    state: State,
    // Number of enclosing `if`-on-`Bit` bodies at the declaration.
    bit_if_depth: usize,
    // A quantum parameter without `consume`: usable by gates, never consumed by the callee.
    borrowed: Option<Span>,
}

struct Sig {
    // (type, `consume`)
    params: Vec<(Ty, bool)>,
    ret: Ty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Idx {
    Whole,
    Lit(i64),
    Dynamic,
}

// Why a function cannot be called inside an `if` on a `Bit`: its body would run conditionally,
// and qxide's conditional blocks only record gates and `reset`.
#[derive(Clone, Debug, PartialEq)]
enum Unconditional {
    Consume(String),
    ReturnsQuantum(Ty),
    Op(String),
    Calls(String),
}

impl Unconditional {
    fn note(&self) -> String {
        match self {
            Unconditional::Consume(param) => msg::note_cond_consume(param),
            Unconditional::ReturnsQuantum(ty) => msg::note_cond_returns(ty.name()),
            Unconditional::Op(op) => msg::note_cond_op(op),
            Unconditional::Calls(callee) => msg::note_cond_calls(callee),
        }
    }
}

// Built-ins that cannot run inside a conditional block.
const NOT_CONDITIONAL: &[&str] = &["qubit", "qreg", "measure", "discard", "println"];

#[derive(Default)]
struct Scan {
    op: Option<(Span, String)>,
    calls: Vec<(String, Span)>,
}

fn scan_block(b: &Block, out: &mut Scan) {
    for s in &b.stmts {
        match s {
            Stmt::Decl { init, .. } => scan_expr(init, out),
            Stmt::Assign { value, .. } => scan_expr(value, out),
            Stmt::If(i) => scan_if(i, out),
            Stmt::For {
                start, end, body, ..
            } => {
                scan_expr(start, out);
                scan_expr(end, out);
                scan_block(body, out);
            }
            Stmt::While { cond, body, .. } => {
                scan_expr(cond, out);
                scan_block(body, out);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    scan_expr(e, out);
                }
            }
            // Arrays are shared references, so a store may change the caller's data: a function
            // that stores into an array cannot run inside an `if` on a measured bit.
            Stmt::SetIndex {
                base,
                index,
                value,
                span,
                ..
            } => {
                if out.op.is_none() {
                    out.op = Some((*span, ARRAY_STORE.to_string()));
                }
                scan_expr(base, out);
                scan_expr(index, out);
                scan_expr(value, out);
            }
            // Structs are shared references too.
            Stmt::SetField {
                base, value, span, ..
            } => {
                if out.op.is_none() {
                    out.op = Some((*span, FIELD_STORE.to_string()));
                }
                scan_expr(base, out);
                scan_expr(value, out);
            }
            Stmt::Expr(e) => scan_expr(e, out),
        }
    }
}

fn scan_if(i: &IfStmt, out: &mut Scan) {
    scan_expr(&i.cond, out);
    scan_block(&i.then, out);
    match &i.els {
        Some(Else::Block(b)) => scan_block(b, out),
        Some(Else::If(inner)) => scan_if(inner, out),
        None => {}
    }
}

fn scan_expr(e: &Expr, out: &mut Scan) {
    match &e.kind {
        ExprKind::Call { name, args } => {
            let n = name.name.as_str();
            if NOT_CONDITIONAL.contains(&n) {
                if out.op.is_none() {
                    out.op = Some((e.span, n.to_string()));
                }
            } else if !BUILTINS.contains(&n) {
                out.calls.push((n.to_string(), e.span));
            }
            for a in args {
                scan_expr(a, out);
            }
        }
        ExprKind::Index { base, index } => {
            scan_expr(base, out);
            scan_expr(index, out);
        }
        ExprKind::Method { recv, args, .. } => {
            scan_expr(recv, out);
            for a in args {
                scan_expr(a, out);
            }
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => scan_expr(expr, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            scan_expr(lhs, out);
            scan_expr(rhs, out);
        }
        ExprKind::NewArray { dims: items, .. }
        | ExprKind::ArrayLit { elems: items, .. }
        | ExprKind::NewStruct { args: items, .. } => {
            for a in items {
                scan_expr(a, out);
            }
        }
        ExprKind::Field { recv, .. } => scan_expr(recv, out),
        ExprKind::Int(_)
        | ExprKind::Double(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Var(_) => {}
    }
}

// How array element and struct field stores are named in diagnostics.
const ARRAY_STORE: &str = "a[i] = v";
const FIELD_STORE: &str = "p.f = v";

// An enclosing `for` or `while`, for `break` and `continue`.
struct Loop {
    // Variable states when the loop was entered.
    before: Vec<State>,
    // `bit_ifs.len()` and `scopes.len()` at the loop.
    bit_ifs: usize,
    scopes: usize,
    // Some `break` leaves this loop.
    breaks: bool,
}

struct Checker {
    fns: HashMap<String, Sig>,
    // Fields of every struct, in declaration order.
    structs: HashMap<Ty, Vec<(String, Ty)>>,
    // Functions that cannot run inside an `if` on a `Bit`, with the first reason found.
    unconditional: HashMap<String, (Span, Unconditional)>,
    vars: Vec<Var>,
    scopes: Vec<Vec<usize>>,
    // Condition spans of the enclosing `if`-on-`Bit` statements.
    bit_ifs: Vec<Span>,
    // Qubits lent to calls whose arguments are still being checked, as (variable, use site).
    held: Vec<(usize, Span)>,
    loops: Vec<Loop>,
    ret: Ty,
    errors: Vec<Diagnostic>,
}

/// `require_main: false` checks a library module, which has no `main`.
pub fn check_with(p: &Program, require_main: bool) -> Vec<Diagnostic> {
    let mut c = Checker {
        fns: HashMap::new(),
        structs: HashMap::new(),
        unconditional: HashMap::new(),
        vars: Vec::new(),
        scopes: Vec::new(),
        bit_ifs: Vec::new(),
        held: Vec::new(),
        loops: Vec::new(),
        ret: Ty::Void,
        errors: Vec::new(),
    };
    c.struct_decls(p);
    c.signatures(p, require_main);
    c.conditional_safety(p);
    for f in &p.fns {
        c.function(f);
    }
    c.errors.sort_by_key(|d| (d.span.start, d.code.as_str()));
    c.errors
}

impl Checker {
    fn err(&mut self, code: Code, span: Span, message: String) -> &mut Diagnostic {
        self.errors.push(Diagnostic::new(code, span, message));
        self.errors.last_mut().unwrap()
    }

    // Struct names are unique, fields are unique and of build-time classical types, and no
    // struct contains itself (directly or through arrays): there is no `null` to end a chain.
    fn struct_decls(&mut self, p: &Program) {
        for s in &p.structs {
            let ty = Ty::struct_named(&s.name.name);
            if self.structs.contains_key(&ty) {
                self.err(
                    Code::Duplicate,
                    s.name.span,
                    msg::duplicate_def(&s.name.name),
                );
                continue;
            }
            let mut fields: Vec<(String, Ty)> = Vec::new();
            for f in &s.fields {
                if fields.iter().any(|(n, _)| *n == f.name.name) {
                    self.err(
                        Code::Duplicate,
                        f.name.span,
                        msg::duplicate_def(&f.name.name),
                    );
                    continue;
                }
                let mut inner = f.ty;
                while let Some(e) = inner.elem() {
                    inner = e;
                }
                let ty = if inner.is_element() {
                    f.ty
                } else {
                    self.err(
                        Code::BadFieldType,
                        f.ty_span,
                        msg::bad_field_type(inner.name()),
                    );
                    Ty::Error
                };
                fields.push((f.name.name.clone(), ty));
            }
            self.structs.insert(ty, fields);
        }
        let strip = |mut t: Ty| {
            while let Some(e) = t.elem() {
                t = e;
            }
            t
        };
        for s in &p.structs {
            let me = Ty::struct_named(&s.name.name);
            for f in &s.fields {
                // Depth-first over the struct types reachable from this field.
                let mut stack = vec![strip(f.ty)];
                let mut seen = Vec::new();
                let mut cycle = false;
                while let Some(t) = stack.pop() {
                    if t == me {
                        cycle = true;
                        break;
                    }
                    if !matches!(t, Ty::Struct(_)) || seen.contains(&t) {
                        continue;
                    }
                    seen.push(t);
                    if let Some(fs) = self.structs.get(&t) {
                        stack.extend(fs.iter().map(|(_, ft)| strip(*ft)));
                    }
                }
                if cycle {
                    self.err(
                        Code::BadFieldType,
                        f.ty_span,
                        msg::recursive_struct(&s.name.name),
                    );
                }
            }
        }
    }

    fn signatures(&mut self, p: &Program, require_main: bool) {
        let mut main = None;
        for f in &p.fns {
            let name = &f.name.name;
            // Functions of imported modules carry qualified names such as `bell_utils.prepare`.
            let base = name.rsplit('.').next().unwrap_or(name);
            if BUILTINS.contains(&base) {
                self.err(
                    Code::BuiltinRedefined,
                    f.name.span,
                    msg::builtin_redefined(base),
                );
                continue;
            }
            if constant(base).is_some() {
                self.err(
                    Code::BuiltinRedefined,
                    f.name.span,
                    msg::constant_redefined(base),
                )
                .help = Some(msg::help_constant_redefined());
                continue;
            }
            if self.fns.contains_key(name) {
                self.err(Code::Duplicate, f.name.span, msg::duplicate_def(name));
                continue;
            }
            for p in &f.params {
                if let Some(at) = p.consume {
                    if !p.ty.is_quantum() {
                        self.err(
                            Code::ConsumeNonQuantum,
                            at,
                            msg::consume_non_quantum(p.ty.name()),
                        );
                    }
                }
            }
            if name == "main" {
                main = Some(f);
            }
            self.fns.insert(
                name.clone(),
                Sig {
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.ty, p.consume.is_some() && p.ty.is_quantum()))
                        .collect(),
                    ret: f.ret,
                },
            );
        }
        match main {
            None if !require_main => {}
            None => {
                let at = p.fns.first().map_or(Span::default(), |f| f.name.span);
                self.err(Code::BadMain, at, msg::missing_main());
            }
            Some(f) if !f.params.is_empty() || f.ret != Ty::Void => {
                self.err(Code::BadMain, f.name.span, msg::main_signature());
            }
            Some(_) => {}
        }
    }

    // A greatest fixpoint over the call graph: a function is unconditional if it has a direct
    // reason or calls an unconditional function.
    fn conditional_safety(&mut self, p: &Program) {
        let mut edges = Vec::new();
        for f in &p.fns {
            let mut scan = Scan::default();
            scan_block(&f.body, &mut scan);
            let consume = f
                .params
                .iter()
                .find(|p| p.ty.is_quantum() && p.consume.is_some());
            // Pure std functions (`std.math`) are plain build-time arithmetic.
            let direct = if f.name.name.starts_with("std.") && !pure::is_pure_fn(&f.name.name) {
                Some((f.name.span, Unconditional::Op(f.name.name.clone())))
            } else if let Some(p) = consume {
                Some((p.name.span, Unconditional::Consume(p.name.name.clone())))
            } else if f.ret.is_quantum() {
                Some((f.ret_span, Unconditional::ReturnsQuantum(f.ret)))
            } else {
                scan.op.map(|(at, op)| (at, Unconditional::Op(op)))
            };
            if let Some(d) = direct {
                self.unconditional.entry(f.name.name.clone()).or_insert(d);
            }
            edges.push((f.name.name.clone(), scan.calls));
        }
        propagate(&edges, &mut self.unconditional);
    }

    fn function(&mut self, f: &FnDecl) {
        self.vars.clear();
        self.loops.clear();
        self.scopes = vec![Vec::new()];
        self.ret = f.ret;
        for p in &f.params {
            if self.scopes[0]
                .iter()
                .any(|&id| self.vars[id].name == p.name.name)
            {
                self.err(
                    Code::Duplicate,
                    p.name.span,
                    msg::duplicate_def(&p.name.name),
                );
            }
            if p.ty == Ty::Void {
                self.err(Code::TypeMismatch, p.ty_span, msg::void_variable());
            }
            let ty = if p.ty == Ty::Void || !self.array_ty_ok(p.ty, p.ty_span) {
                Ty::Error
            } else {
                p.ty
            };
            self.declare(&p.name, ty, true);
            if ty.is_quantum() && p.consume.is_none() {
                self.vars.last_mut().unwrap().borrowed = Some(p.name.span);
            }
        }
        self.array_ty_ok(f.ret, f.ret_span);
        let diverges = self.block(&f.body);
        if !diverges {
            // `consume` parameters live in the outermost scope, which `block` does not check.
            let leaked: Vec<usize> = self.scopes[0]
                .iter()
                .copied()
                .filter(|&id| self.owned_live(id))
                .collect();
            for id in leaked {
                let (decl, name) = (self.vars[id].decl, self.vars[id].name.clone());
                self.err(Code::NeverConsumed, decl, msg::never_consumed(&name))
                    .notes
                    .push(note(f.body.close, msg::note_scope_ends()));
            }
        }
        if !diverges && !matches!(f.ret, Ty::Void | Ty::Error) {
            self.err(
                Code::MissingReturn,
                f.body.close,
                msg::missing_return(&f.name.name, f.ret.name()),
            );
        }
    }

    // A quantum variable this function owns and has not consumed yet.
    fn owned_live(&self, id: usize) -> bool {
        let v = &self.vars[id];
        v.ty.is_quantum() && v.state == State::Live && v.borrowed.is_none()
    }

    // Reports consuming a borrowed parameter; the variable stays live so errors do not cascade.
    fn borrowed_consumed(&mut self, id: usize, at: Span) -> bool {
        let Some(param) = self.vars[id].borrowed else {
            return false;
        };
        let name = self.vars[id].name.clone();
        self.err(Code::BorrowedConsume, at, msg::borrowed_consume(&name))
            .notes
            .push(note(param, msg::note_borrowed_here()));
        true
    }

    fn declare(&mut self, name: &Ident, ty: Ty, mutable: bool) {
        if constant(&name.name).is_some() {
            self.err(
                Code::BuiltinRedefined,
                name.span,
                msg::constant_redefined(&name.name),
            )
            .help = Some(msg::help_constant_redefined());
        }
        self.vars.push(Var {
            name: name.name.clone(),
            ty,
            mutable,
            decl: name.span,
            state: State::Live,
            bit_if_depth: self.bit_ifs.len(),
            borrowed: None,
        });
        let id = self.vars.len() - 1;
        self.scopes.last_mut().unwrap().push(id);
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .copied()
            .find(|&id| self.vars[id].name == name)
    }

    fn states(&self) -> Vec<State> {
        self.vars.iter().map(|v| v.state).collect()
    }

    fn restore(&mut self, states: &[State]) {
        for (v, s) in self.vars.iter_mut().zip(states) {
            v.state = *s;
        }
    }

    /// Returns true when every path through the block ends in `return`.
    fn block(&mut self, b: &Block) -> bool {
        self.scopes.push(Vec::new());
        let mut diverges = false;
        for s in &b.stmts {
            diverges |= self.stmt(s);
        }
        let scope = self.scopes.pop().unwrap();
        if !diverges {
            for id in scope {
                if self.owned_live(id) {
                    let (decl, name) = (self.vars[id].decl, self.vars[id].name.clone());
                    self.err(Code::NeverConsumed, decl, msg::never_consumed(&name))
                        .notes
                        .push(note(b.close, msg::note_scope_ends()));
                }
            }
        }
        diverges
    }

    fn stmt(&mut self, s: &Stmt) -> bool {
        match s {
            Stmt::Decl {
                is_final,
                ty,
                ty_span,
                name,
                init,
            } => {
                let found = self.let_init(init);
                let var_ty = match *ty {
                    // `var` infers classical types only; quantum and per-shot values need a type.
                    None => match found {
                        Ty::Int
                        | Ty::Double
                        | Ty::Boolean
                        | Ty::String
                        | Ty::Array(_)
                        | Ty::Struct(_)
                        | Ty::Error => found,
                        other => {
                            self.err(Code::VarInfer, *ty_span, msg::var_infer(other.name()));
                            Ty::Error
                        }
                    },
                    Some(Ty::Void) => {
                        self.err(Code::TypeMismatch, *ty_span, msg::void_variable());
                        Ty::Error
                    }
                    // The initializer was already rejected; do not also report a leak.
                    Some(t) if t.is_quantum() && found == Ty::Error => Ty::Error,
                    Some(t) if !self.array_ty_ok(t, *ty_span) => Ty::Error,
                    Some(t) => {
                        self.expect_ty(init.span, t, found);
                        t
                    }
                };
                let call_reported = matches!(init.kind, ExprKind::Call { .. });
                if var_ty.is_quantum() && !self.bit_ifs.is_empty() && !call_reported {
                    let at = *self.bit_ifs.last().unwrap();
                    self.err(
                        Code::ForbiddenInBitIf,
                        name.span,
                        msg::forbidden_in_bit_if(var_ty.name()),
                    )
                    .notes
                    .push(note(at, msg::note_bit_if_here()));
                }
                let shadowed = self.scopes.last().unwrap().iter().copied().find(|&id| {
                    let v = &self.vars[id];
                    v.name == name.name && v.ty.is_quantum() && v.state == State::Live
                });
                if let Some(id) = shadowed {
                    let decl = self.vars[id].decl;
                    self.err(Code::ShadowLive, name.span, msg::shadow_live(&name.name))
                        .notes
                        .push(note(decl, msg::note_declared_here()));
                    // Already reported; do not report the leak again at scope end.
                    self.vars[id].state = State::Consumed(name.span);
                }
                self.declare(name, var_ty, !is_final);
                false
            }
            Stmt::Assign { name, value } => {
                let found = self.expr(value);
                let Some(id) = self.lookup(&name.name) else {
                    if constant(&name.name).is_some() {
                        self.err(
                            Code::ImmutableAssign,
                            name.span,
                            msg::constant_assign(&name.name),
                        )
                        .help = Some(msg::help_constant_assign());
                    } else {
                        self.err(Code::UnknownVar, name.span, msg::unknown_var(&name.name));
                    }
                    return false;
                };
                let (ty, mutable, depth) = {
                    let v = &self.vars[id];
                    (v.ty, v.mutable, v.bit_if_depth)
                };
                if ty.is_quantum() {
                    self.err(
                        Code::QuantumAssign,
                        name.span,
                        msg::quantum_assign(&name.name),
                    );
                } else if !mutable {
                    let decl = self.vars[id].decl;
                    self.err(
                        Code::ImmutableAssign,
                        name.span,
                        msg::immutable_assign(&name.name),
                    )
                    .notes
                    .push(note(decl, msg::note_declared_here()));
                } else if depth < self.bit_ifs.len() {
                    let at = self.bit_ifs[depth];
                    self.err(
                        Code::OuterAssignInBitIf,
                        name.span,
                        msg::outer_assign_in_bit_if(&name.name),
                    )
                    .notes
                    .push(note(at, msg::note_bit_if_here()));
                } else {
                    self.expect_ty(value.span, ty, found);
                }
                false
            }
            Stmt::If(i) => self.if_stmt(i),
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                for e in [start, end] {
                    let t = self.expr(e);
                    self.expect_ty(e.span, Ty::Int, t);
                }
                let before = self.states();
                self.enter_loop(before.clone());
                self.scopes.push(Vec::new());
                self.declare(var, Ty::Int, false);
                let diverges = self.block(body);
                self.scopes.pop();
                self.loops.pop();
                if !diverges {
                    self.consumed_in_loop(&before);
                }
                // The loop may run zero times.
                self.restore(&before);
                false
            }
            Stmt::While { cond, body, .. } => {
                // The condition runs on every iteration, so it belongs to the loop.
                let before = self.states();
                let t = self.expr(cond);
                if !matches!(t, Ty::Boolean | Ty::Error) {
                    self.err(
                        Code::BadCondition,
                        cond.span,
                        msg::while_condition(t.name()),
                    );
                }
                self.enter_loop(before.clone());
                let diverges = self.block(body);
                let breaks = self.loops.pop().is_some_and(|l| l.breaks);
                if !diverges {
                    self.consumed_in_loop(&before);
                }
                self.restore(&before);
                // Like Java, `while true` without `break` never completes normally.
                matches!(cond.kind, ExprKind::Bool(true)) && !breaks
            }
            Stmt::SetIndex {
                base,
                index,
                op,
                value,
                span,
            } => {
                self.set_index(base, index, *op, value, *span);
                false
            }
            Stmt::SetField {
                base,
                name,
                op,
                value,
                span,
            } => {
                self.set_field(base, name, *op, value, *span);
                false
            }
            Stmt::Break { span } => self.loop_exit("break", *span),
            Stmt::Continue { span } => self.loop_exit("continue", *span),
            Stmt::Return { value, span } => {
                if let Some(&at) = self.bit_ifs.last() {
                    self.err(
                        Code::ForbiddenInBitIf,
                        *span,
                        msg::forbidden_in_bit_if("return"),
                    )
                    .notes
                    .push(note(at, msg::note_bit_if_here()));
                    // Already rejected; skip the leak analysis so it does not cascade.
                    return false;
                }
                let want = self.ret;
                // Returning a qubit moves it to the caller, which becomes responsible for it.
                let found = match value {
                    Some(e) if want.is_quantum() => self.consume(e, *span, want),
                    Some(e) => self.expr(e),
                    None => Ty::Void,
                };
                self.expect_ty(value.as_ref().map_or(*span, |e| e.span), want, found);
                let live: Vec<usize> = self
                    .scopes
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|&id| self.owned_live(id))
                    .collect();
                for id in live {
                    let (decl, name) = (self.vars[id].decl, self.vars[id].name.clone());
                    self.err(Code::NeverConsumed, decl, msg::never_consumed(&name))
                        .notes
                        .push(note(*span, msg::note_returns_here()));
                    self.vars[id].state = State::Consumed(*span);
                }
                true
            }
            Stmt::Expr(e) => {
                // Naming an existing qubit (`q;`, `r[0];`) is a no-op; only a new allocation leaks.
                if self.expr(e).is_quantum() && self.place_key(e).is_none() {
                    self.err(Code::TempDropped, e.span, msg::temp_dropped());
                }
                false
            }
        }
    }

    fn enter_loop(&mut self, before: Vec<State>) {
        self.loops.push(Loop {
            before,
            bit_ifs: self.bit_ifs.len(),
            scopes: self.scopes.len(),
            breaks: false,
        });
    }

    // Outer qubits consumed since the loop began (`Q0305`); reported once per use site.
    fn consumed_in_loop(&mut self, before: &[State]) {
        let mut moved = Vec::new();
        for (id, s) in before.iter().enumerate() {
            let v = &self.vars[id];
            if let (State::Live, State::Consumed(at)) = (s, v.state) {
                if v.ty.is_quantum() {
                    moved.push((at, v.name.clone(), v.decl));
                }
            }
        }
        for (at, name, decl) in moved {
            let seen = self
                .errors
                .iter()
                .any(|d| d.code == Code::ConsumedInLoop && d.span == at);
            if !seen {
                self.err(Code::ConsumedInLoop, at, msg::consumed_in_loop(&name))
                    .notes
                    .push(note(decl, msg::note_declared_here()));
            }
        }
    }

    // `break` or `continue` leaves the loop body like `return` leaves a function, so qubits
    // declared inside the loop must be consumed by then. Returns true: the rest is unreachable.
    fn loop_exit(&mut self, word: &str, span: Span) -> bool {
        let Some(l) = self.loops.last_mut() else {
            self.err(Code::LoopControl, span, msg::loop_control_outside(word));
            return false;
        };
        if self.bit_ifs.len() > l.bit_ifs {
            let at = *self.bit_ifs.last().unwrap();
            self.err(Code::ForbiddenInBitIf, span, msg::forbidden_in_bit_if(word))
                .notes
                .push(note(at, msg::note_bit_if_here()));
            return false;
        }
        if word == "break" {
            l.breaks = true;
        }
        let (before, depth) = (l.before.clone(), l.scopes);
        self.consumed_in_loop(&before);
        let live: Vec<usize> = self.scopes[depth..]
            .iter()
            .flatten()
            .copied()
            .filter(|&id| self.owned_live(id))
            .collect();
        for id in live {
            let (decl, name) = (self.vars[id].decl, self.vars[id].name.clone());
            self.err(Code::NeverConsumed, decl, msg::never_consumed(&name))
                .notes
                .push(note(span, msg::note_loop_exits_here(word)));
            self.vars[id].state = State::Consumed(span);
        }
        true
    }

    fn if_stmt(&mut self, i: &IfStmt) -> bool {
        let cond = self.expr(&i.cond);
        match cond {
            Ty::Bit => {
                self.bit_ifs.push(i.cond.span);
                self.block(&i.then);
                match &i.els {
                    Some(Else::Block(b)) => {
                        self.block(b);
                    }
                    Some(Else::If(inner)) => {
                        self.if_stmt(inner);
                    }
                    None => {}
                }
                self.bit_ifs.pop();
                false
            }
            _ => {
                if !matches!(cond, Ty::Boolean | Ty::Error) {
                    self.err(
                        Code::BadCondition,
                        i.cond.span,
                        msg::bad_condition(cond.name()),
                    );
                }
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
                    (false, true) | (true, true) => self.restore(&s1),
                    (false, false) => {
                        for id in 0..before.len() {
                            let (a, b) = (s1[id], s2[id]);
                            if !self.vars[id].ty.is_quantum() || a == b {
                                continue;
                            }
                            let (at, merged) = match (a, b) {
                                (State::Consumed(x), State::Live) => (x, a),
                                (State::Live, State::Consumed(y)) => (y, b),
                                // Consumed in both branches at different places.
                                _ => {
                                    self.vars[id].state = a;
                                    continue;
                                }
                            };
                            self.vars[id].state = merged;
                            let name = self.vars[id].name.clone();
                            self.err(Code::BranchMismatch, at, msg::branch_mismatch(&name));
                        }
                    }
                }
                d1 && d2
            }
        }
    }

    // `Qubit p = q;` moves a qubit; `Qubit q = r[i];` would move a register element out.
    fn let_init(&mut self, init: &Expr) -> Ty {
        match &init.kind {
            ExprKind::Var(name) => {
                if let Some(id) = self.lookup(name) {
                    if self.vars[id].ty.is_quantum() {
                        self.use_var(id, init.span);
                        if self.borrowed_consumed(id, init.span) {
                            return Ty::Error;
                        }
                        self.vars[id].state = State::Consumed(init.span);
                        return self.vars[id].ty;
                    }
                }
                self.expr(init)
            }
            ExprKind::Index { base, .. } if self.is_qreg_var(base) => {
                self.expr(init);
                self.err(Code::PartialMove, init.span, msg::partial_move());
                Ty::Error
            }
            _ => self.expr(init),
        }
    }

    fn is_qreg_var(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Var(n) => self
                .lookup(n)
                .is_some_and(|id| self.vars[id].ty == Ty::QReg),
            _ => false,
        }
    }

    fn use_var(&mut self, id: usize, at: Span) {
        if let State::Consumed(where_) = self.vars[id].state {
            let name = self.vars[id].name.clone();
            self.err(Code::UseAfterConsume, at, msg::use_after_consume(&name))
                .notes
                .push(note(where_, msg::note_consumed_here()));
        }
    }

    // A gate or call operand stays in use until every argument has been evaluated.
    fn hold(&mut self, id: usize, at: Span) {
        if self.vars[id].state == State::Live {
            self.held.push((id, at));
        }
    }

    // Ends the holds taken since `mark`; an operand consumed meanwhile (by a later argument or
    // its own index) would reach the gate already measured.
    fn release(&mut self, mark: usize) {
        let held: Vec<(usize, Span)> = self.held.drain(mark..).collect();
        let mut reported = Vec::new();
        for (id, at) in held {
            if let State::Consumed(where_) = self.vars[id].state {
                if reported.contains(&id) {
                    continue;
                }
                reported.push(id);
                let name = self.vars[id].name.clone();
                self.err(Code::UseAfterConsume, at, msg::use_after_consume(&name))
                    .notes
                    .push(note(where_, msg::note_consumed_here()));
            }
        }
    }

    fn expect_ty(&mut self, at: Span, want: Ty, found: Ty) {
        // Java-style widening: an `int` is accepted where a `double` is expected.
        if want == found
            || want == Ty::Error
            || found == Ty::Error
            || (want == Ty::Double && found == Ty::Int)
        {
            return;
        }
        if found.is_shot_time() && !want.is_shot_time() {
            self.err(
                Code::BitAtBuildTime,
                at,
                msg::bit_at_build_time(found.name()),
            );
        } else {
            self.err(
                Code::TypeMismatch,
                at,
                msg::type_mismatch(want.name(), found.name()),
            );
        }
    }

    // A quantum value that is only read (indexed, `len`) must live in a variable, or it is lost.
    fn forbid_temp(&mut self, e: &Expr, ty: Ty) {
        if ty.is_quantum() && !matches!(e.kind, ExprKind::Var(_)) {
            self.err(Code::TempDropped, e.span, msg::temp_dropped());
        }
    }

    // Operand of arithmetic or an ordering comparison; reports and returns false otherwise.
    fn numeric(&mut self, at: Span, t: Ty) -> bool {
        if t.is_numeric() || t == Ty::Error {
            return true;
        }
        self.expect_ty(at, Ty::Int, t);
        false
    }

    fn forbid_in_bit_if(&mut self, at: Span, op: &str) {
        if let Some(&cond) = self.bit_ifs.last() {
            self.err(Code::ForbiddenInBitIf, at, msg::forbidden_in_bit_if(op))
                .notes
                .push(note(cond, msg::note_bit_if_here()));
        }
    }

    fn expr(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Double(_) => Ty::Double,
            ExprKind::Bool(_) => Ty::Boolean,
            // A `String` value; `{}` holes only make sense in `println`'s format string.
            ExprKind::Str(pieces) => {
                if pieces.contains(&FmtPiece::Hole) {
                    self.err(Code::StringNotAllowed, e.span, msg::string_not_allowed());
                    return Ty::Error;
                }
                Ty::String
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(id) => {
                    if self.vars[id].ty.is_quantum() {
                        self.use_var(id, e.span);
                    }
                    self.vars[id].ty
                }
                None if constant(name).is_some() => Ty::Double,
                None => {
                    self.err(Code::UnknownVar, e.span, msg::unknown_var(name));
                    Ty::Error
                }
            },
            ExprKind::Call { name, args } => self.call(name, args, e.span),
            ExprKind::Index { base, index } => {
                let b = self.expr(base);
                self.forbid_temp(base, b);
                let i = self.expr(index);
                self.expect_ty(index.span, Ty::Int, i);
                match b {
                    Ty::QReg => Ty::Qubit,
                    Ty::BitReg => Ty::Bit,
                    Ty::Array(_) => b.elem().unwrap_or(Ty::Error),
                    Ty::Error => Ty::Error,
                    other => {
                        self.err(
                            Code::NotIndexable,
                            base.span,
                            msg::not_indexable(other.name()),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::NewArray { ty, dims } => {
                for d in dims {
                    let t = self.expr(d);
                    self.expect_ty(d.span, Ty::Int, t);
                }
                if self.array_ty_ok(*ty, e.span) {
                    *ty
                } else {
                    Ty::Error
                }
            }
            ExprKind::ArrayLit { ty, elems } => self.array_lit(*ty, elems, e.span),
            ExprKind::NewStruct { ty, args } => {
                let fields = self.structs.get(ty).cloned().unwrap_or_default();
                if fields.len() != args.len() {
                    self.err(
                        Code::Arity,
                        e.span,
                        msg::arity(ty.name(), fields.len(), args.len()),
                    );
                    for a in args {
                        self.expr(a);
                    }
                    return Ty::Error;
                }
                for (a, (_, want)) in args.iter().zip(fields) {
                    let found = self.expr(a);
                    self.expect_ty(a.span, want, found);
                }
                *ty
            }
            ExprKind::Field { recv, name } => {
                let t = self.expr(recv);
                self.field_ty(t, name)
            }
            ExprKind::Method { recv, name, args } => {
                let t = self.expr(recv);
                if t == Ty::String {
                    return self.string_method(name, args, e.span);
                }
                if !args.is_empty() {
                    self.err(Code::Arity, e.span, msg::arity(&name.name, 0, args.len()));
                    for a in args {
                        self.expr(a);
                    }
                    return Ty::Error;
                }
                match (t, name.name.as_str()) {
                    (Ty::BitReg, "as_int") => Ty::ShotInt,
                    (Ty::BitReg, "to_binary") => Ty::ShotBinary,
                    (Ty::Error, _) => Ty::Error,
                    _ => {
                        self.err(
                            Code::UnknownMethod,
                            name.span,
                            msg::unknown_method(&name.name, t.name()),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::Cast { ty, expr } => {
                let t = self.expr(expr);
                match (t, ty) {
                    (_, Ty::Int | Ty::Double) if t.is_numeric() || t == Ty::Error => *ty,
                    (Ty::Error, _) => Ty::Error,
                    _ if t.is_shot_time() => {
                        self.err(
                            Code::BitAtBuildTime,
                            expr.span,
                            msg::bit_at_build_time(t.name()),
                        );
                        Ty::Error
                    }
                    _ => {
                        self.err(
                            Code::InvalidCast,
                            e.span,
                            msg::invalid_cast(t.name(), ty.name()),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::Unary { op, expr } => {
                let t = self.expr(expr);
                match op {
                    UnOp::Neg => {
                        self.numeric(expr.span, t);
                        if t == Ty::Double {
                            Ty::Double
                        } else {
                            Ty::Int
                        }
                    }
                    UnOp::Not => {
                        self.expect_ty(expr.span, Ty::Boolean, t);
                        Ty::Boolean
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.expr(lhs);
                let r = if matches!(op, BinOp::And | BinOp::Or) {
                    self.short_circuit_rhs(*op, rhs)
                } else {
                    self.expr(rhs)
                };
                use BinOp::*;
                // `+` with a `String` on either side concatenates any build-time value.
                if *op == Add && (l == Ty::String || r == Ty::String) {
                    for (side, t) in [(lhs, l), (rhs, r)] {
                        if !matches!(
                            t,
                            Ty::Int | Ty::Double | Ty::Boolean | Ty::String | Ty::Error
                        ) {
                            self.expect_ty(side.span, Ty::String, t);
                        }
                    }
                    return Ty::String;
                }
                match op {
                    Add | Sub | Mul | Div | Rem | Lt | Le | Gt | Ge => {
                        self.numeric(lhs.span, l);
                        self.numeric(rhs.span, r);
                        if matches!(op, Lt | Le | Gt | Ge) {
                            Ty::Boolean
                        } else if l == Ty::Double || r == Ty::Double {
                            Ty::Double
                        } else {
                            Ty::Int
                        }
                    }
                    And | Or => {
                        self.expect_ty(lhs.span, Ty::Boolean, l);
                        self.expect_ty(rhs.span, Ty::Boolean, r);
                        Ty::Boolean
                    }
                    Eq | Ne => {
                        if matches!(l, Ty::Boolean | Ty::String) {
                            self.expect_ty(rhs.span, l, r);
                        } else if r == Ty::String {
                            self.expect_ty(lhs.span, Ty::String, l);
                        } else if l == Ty::Boolean {
                            self.expect_ty(rhs.span, Ty::Boolean, r);
                        } else {
                            self.numeric(lhs.span, l);
                            self.numeric(rhs.span, r);
                        }
                        Ty::Boolean
                    }
                }
            }
        }
    }

    // The right operand of `&&`/`||` may be skipped at run time, like a one-armed `if`.
    // Arrays hold build-time classical values only (`Q0225`); reported once at the type.
    fn array_ty_ok(&mut self, t: Ty, span: Span) -> bool {
        let mut e = t;
        while let Some(inner) = e.elem() {
            e = inner;
        }
        if t.elem().is_some() && !e.is_element() {
            self.err(
                Code::BadArrayElement,
                span,
                msg::bad_array_element(e.name()),
            );
            return false;
        }
        true
    }

    // `{1, 2}` of a declaration or `new int[]{1, 2}`; the parser passed the array type down.
    fn array_lit(&mut self, ty: Option<Ty>, elems: &[Expr], span: Span) -> Ty {
        let t = match ty {
            Some(t) if self.array_ty_ok(t, span) => Some(t),
            Some(_) => None,
            None => {
                self.err(Code::TypeMismatch, span, msg::array_lit_needs_type());
                None
            }
        };
        let elem = t.and_then(Ty::elem);
        for e in elems {
            let found = self.expr(e);
            if let Some(want) = elem {
                self.expect_ty(e.span, want, found);
            }
        }
        t.unwrap_or(Ty::Error)
    }

    // `a[i] = v` and `a[i] op= v`: like assigning `a[i]` as a variable of the element type.
    fn set_index(
        &mut self,
        base: &Expr,
        index: &Expr,
        op: Option<BinOp>,
        value: &Expr,
        span: Span,
    ) {
        let b = self.expr(base);
        let i = self.expr(index);
        self.expect_ty(index.span, Ty::Int, i);
        let v = self.expr(value);
        if let Some(&at) = self.bit_ifs.last() {
            self.err(
                Code::ForbiddenInBitIf,
                span,
                msg::forbidden_in_bit_if(ARRAY_STORE),
            )
            .notes
            .push(note(at, msg::note_bit_if_here()));
            return;
        }
        let elem = match b {
            Ty::Array(_) => b.elem().unwrap_or(Ty::Error),
            Ty::Error => return,
            other => {
                self.err(
                    Code::TypeMismatch,
                    base.span,
                    msg::element_not_assignable(other.name()),
                );
                return;
            }
        };
        self.store(elem, op, v, value);
    }

    // The type `p.f` has, or `Q0226`.
    fn field_ty(&mut self, t: Ty, name: &Ident) -> Ty {
        if t == Ty::Error {
            return Ty::Error;
        }
        let found = self
            .structs
            .get(&t)
            .and_then(|fs| fs.iter().find(|(n, _)| *n == name.name))
            .map(|(_, ft)| *ft);
        found.unwrap_or_else(|| {
            self.err(
                Code::UnknownField,
                name.span,
                msg::unknown_field(&name.name, t.name()),
            );
            Ty::Error
        })
    }

    // `p.f = v` and `p.f op= v`: like `a[i] = v` with the field's type.
    fn set_field(
        &mut self,
        base: &Expr,
        name: &Ident,
        op: Option<BinOp>,
        value: &Expr,
        span: Span,
    ) {
        let b = self.expr(base);
        let v = self.expr(value);
        if let Some(&at) = self.bit_ifs.last() {
            self.err(
                Code::ForbiddenInBitIf,
                span,
                msg::forbidden_in_bit_if(FIELD_STORE),
            )
            .notes
            .push(note(at, msg::note_bit_if_here()));
            return;
        }
        let ft = self.field_ty(b, name);
        self.store(ft, op, v, value);
    }

    // Storing a value of type `v` (or `slot op v`) into a slot of type `slot`.
    fn store(&mut self, elem: Ty, op: Option<BinOp>, v: Ty, value: &Expr) {
        if elem == Ty::Error {
            return;
        }
        let found = match op {
            None => v,
            Some(_) if v == Ty::Error || elem == Ty::Error => Ty::Error,
            Some(BinOp::Add) if elem == Ty::String => Ty::String,
            Some(_) if elem.is_numeric() && v.is_numeric() => {
                if elem == Ty::Double || v == Ty::Double {
                    Ty::Double
                } else {
                    Ty::Int
                }
            }
            Some(_) => {
                self.err(
                    Code::TypeMismatch,
                    value.span,
                    msg::type_mismatch(elem.name(), v.name()),
                );
                return;
            }
        };
        self.expect_ty(value.span, elem, found);
    }

    // `s.substring(0, 3)` and the other methods of `String` (src/lang/pure.rs).
    fn string_method(&mut self, name: &Ident, args: &[Expr], span: Span) -> Ty {
        let sig = pure::string_method(&name.name);
        let Some((_, params, ret)) = sig.filter(|(_, p, _)| p.len() == args.len()) else {
            match sig {
                Some((_, params, _)) => {
                    self.err(
                        Code::Arity,
                        span,
                        msg::arity(&name.name, params.len(), args.len()),
                    );
                }
                None => {
                    self.err(
                        Code::UnknownMethod,
                        name.span,
                        msg::unknown_method(&name.name, Ty::String.name()),
                    );
                }
            }
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        for (a, want) in args.iter().zip(params) {
            let found = self.expr(a);
            self.expect_ty(a.span, *want, found);
        }
        ret
    }

    fn short_circuit_rhs(&mut self, op: BinOp, rhs: &Expr) -> Ty {
        let before = self.states();
        let t = self.expr(rhs);
        for (id, was) in before.into_iter().enumerate() {
            if let (State::Live, State::Consumed(at)) = (was, self.vars[id].state) {
                if self.vars[id].ty.is_quantum() {
                    let name = self.vars[id].name.clone();
                    let op = if op == BinOp::And { "&&" } else { "||" };
                    self.err(
                        Code::ShortCircuitConsume,
                        at,
                        msg::short_circuit_consume(&name, op),
                    );
                }
            }
        }
        t
    }

    fn arity(&mut self, name: &Ident, args: &[Expr], want: usize, span: Span) -> bool {
        if args.len() != want {
            self.err(Code::Arity, span, msg::arity(&name.name, want, args.len()));
            for a in args {
                self.expr(a);
            }
            return false;
        }
        true
    }

    fn call(&mut self, name: &Ident, args: &[Expr], span: Span) -> Ty {
        let n = name.name.as_str();
        if BUILTINS.contains(&n) {
            return self.builtin(name, args, span);
        }
        let Some(sig) = self.fns.get(n) else {
            self.err(Code::UnknownFn, name.span, msg::unknown_fn(n));
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        let (params, mut ret) = (sig.params.clone(), sig.ret);
        if let Some(&cond) = self.bit_ifs.last() {
            if let Some((at, why)) = self.unconditional.get(n).cloned() {
                let d = self.err(Code::ConditionalCall, span, msg::conditional_call(n));
                d.notes.push(note(at, why.note()));
                d.notes.push(note(cond, msg::note_bit_if_here()));
                // Already rejected; a returned qubit should not also be reported as leaked.
                if ret.is_quantum() {
                    ret = Ty::Error;
                }
            }
        }
        let mark = self.held.len();
        if self.arity(name, args, params.len(), span) {
            // Quantum arguments already passed, as (argument index, place).
            let mut passed: Vec<(usize, (usize, Idx))> = Vec::new();
            for (j, (a, (want, consume))) in args.iter().zip(params).enumerate() {
                if !want.is_quantum() {
                    let t = self.expr(a);
                    self.expect_ty(a.span, want, t);
                    continue;
                }
                if let Some(key) = self.place_key(a) {
                    if let Some(&(i, _)) = passed.iter().find(|(_, k)| overlaps(*k, key)) {
                        self.err(Code::DuplicateQubit, a.span, msg::duplicate_qubit(n))
                            .notes
                            .push(note(args[i].span, msg::note_first_use()));
                        // Already reported; treat a handed-over variable as moved so it does not
                        // also leak.
                        if consume && key.1 == Idx::Whole && self.vars[key.0].borrowed.is_none() {
                            self.vars[key.0].state = State::Consumed(span);
                            self.held.retain(|&(h, _)| h != key.0);
                        }
                        continue;
                    }
                    passed.push((j, key));
                }
                if consume {
                    let t = self.consume(a, span, want);
                    self.expect_ty(a.span, want, t);
                } else {
                    self.borrow(a, want);
                }
            }
        }
        self.release(mark);
        ret
    }

    fn builtin(&mut self, name: &Ident, args: &[Expr], span: Span) -> Ty {
        let n = name.name.as_str();
        match n {
            "qubit" => {
                self.forbid_in_bit_if(span, "qubit()");
                self.arity(name, args, 0, span);
                Ty::Qubit
            }
            "qreg" => {
                self.forbid_in_bit_if(span, "qreg()");
                if self.arity(name, args, 1, span) {
                    let t = self.expr(&args[0]);
                    self.expect_ty(args[0].span, Ty::Int, t);
                }
                Ty::QReg
            }
            "cx" | "ccx" => {
                let want = if n == "cx" { 2 } else { 3 };
                let mark = self.held.len();
                if self.arity(name, args, want, span) {
                    let places: Vec<_> = args.iter().map(|a| self.place(a)).collect();
                    for j in 0..places.len() {
                        for i in 0..j {
                            if let (Some(a), Some(b)) = (places[i], places[j]) {
                                if overlaps(a, b) {
                                    self.err(
                                        Code::DuplicateQubit,
                                        args[j].span,
                                        msg::duplicate_qubit(n),
                                    )
                                    .notes
                                    .push(note(args[i].span, msg::note_first_use()));
                                }
                            }
                        }
                    }
                }
                self.release(mark);
                Ty::Void
            }
            "measure" | "discard" => {
                self.forbid_in_bit_if(span, n);
                if !self.arity(name, args, 1, span) {
                    return if n == "measure" { Ty::Error } else { Ty::Void };
                }
                let t = self.consume(&args[0], span, Ty::Qubit);
                match (n, t) {
                    ("discard", _) => Ty::Void,
                    (_, Ty::Qubit) => Ty::Bit,
                    (_, Ty::QReg) => Ty::BitReg,
                    _ => Ty::Error,
                }
            }
            "len" => {
                if self.arity(name, args, 1, span) {
                    let t = self.expr(&args[0]);
                    self.forbid_temp(&args[0], t);
                    if !matches!(
                        t,
                        Ty::QReg | Ty::BitReg | Ty::String | Ty::Array(_) | Ty::Error
                    ) {
                        self.err(
                            Code::TypeMismatch,
                            args[0].span,
                            msg::type_mismatch("QReg", t.name()),
                        );
                    }
                }
                Ty::Int
            }
            "println" => {
                self.forbid_in_bit_if(span, "println");
                let Some(first) = args.first() else {
                    self.err(Code::PrintlnFormat, span, msg::println_format());
                    return Ty::Void;
                };
                let ExprKind::Str(pieces) = &first.kind else {
                    self.err(Code::PrintlnFormat, first.span, msg::println_format());
                    for a in args {
                        self.expr(a);
                    }
                    return Ty::Void;
                };
                let holes = pieces.iter().filter(|p| **p == FmtPiece::Hole).count();
                if holes != args.len() - 1 {
                    self.err(
                        Code::FormatArgCount,
                        first.span,
                        msg::format_arg_count(holes, args.len() - 1),
                    );
                }
                for a in &args[1..] {
                    let t = self.expr(a);
                    if !matches!(
                        t,
                        Ty::Int
                            | Ty::Double
                            | Ty::Boolean
                            | Ty::String
                            | Ty::Bit
                            | Ty::BitReg
                            | Ty::ShotInt
                            | Ty::ShotBinary
                            | Ty::Error
                    ) {
                        self.err(Code::NotPrintable, a.span, msg::not_printable(t.name()));
                    }
                }
                Ty::Void
            }
            _ if ROTATIONS.contains(&n) => {
                let mark = self.held.len();
                if self.arity(name, args, 2, span) {
                    self.place(&args[0]);
                    let t = self.expr(&args[1]);
                    self.numeric(args[1].span, t);
                }
                self.release(mark);
                Ty::Void
            }
            _ => {
                debug_assert!(ONE_QUBIT_OPS.contains(&n));
                let mark = self.held.len();
                if self.arity(name, args, 1, span) {
                    self.place(&args[0]);
                }
                self.release(mark);
                Ty::Void
            }
        }
    }

    // A borrowed gate operand: a `Qubit` variable or an element of a `QReg` variable.
    fn place(&mut self, e: &Expr) -> Option<(usize, Idx)> {
        match &e.kind {
            ExprKind::Var(name) => {
                if let Some(id) = self.lookup(name) {
                    if self.vars[id].ty == Ty::Qubit {
                        self.use_var(id, e.span);
                        self.hold(id, e.span);
                        return Some((id, Idx::Whole));
                    }
                }
            }
            ExprKind::Index { base, index } if self.is_qreg_var(base) => {
                let ExprKind::Var(name) = &base.kind else {
                    unreachable!()
                };
                let id = self.lookup(name)?;
                self.use_var(id, base.span);
                self.hold(id, base.span);
                let t = self.expr(index);
                self.expect_ty(index.span, Ty::Int, t);
                let idx = match index.kind {
                    ExprKind::Int(k) => Idx::Lit(k),
                    _ => Idx::Dynamic,
                };
                return Some((id, idx));
            }
            _ => {}
        }
        let t = self.expr(e);
        if t.is_quantum() {
            self.err(Code::PlaceRequired, e.span, msg::place_required());
        } else {
            self.expect_ty(e.span, Ty::Qubit, t);
        }
        None
    }

    // Argument for a borrowed parameter: a `Qubit`/`QReg` variable or a register element.
    fn borrow(&mut self, e: &Expr, want: Ty) {
        match &e.kind {
            ExprKind::Var(name) => {
                if let Some(id) = self.lookup(name) {
                    let t = self.vars[id].ty;
                    if t.is_quantum() {
                        self.use_var(id, e.span);
                        self.hold(id, e.span);
                        self.expect_ty(e.span, want, t);
                        return;
                    }
                }
            }
            ExprKind::Index { base, .. } if want == Ty::Qubit && self.is_qreg_var(base) => {
                self.place(e);
                return;
            }
            _ => {}
        }
        let t = self.expr(e);
        if t == want {
            // A fresh qubit lent to a function would never be measured or discarded.
            self.err(Code::PlaceRequired, e.span, msg::borrow_place_required())
                .help = Some(msg::help_borrow_temp());
        } else {
            self.expect_ty(e.span, want, t);
        }
    }

    // The variable and element a quantum argument refers to, without side effects.
    fn place_key(&self, e: &Expr) -> Option<(usize, Idx)> {
        match &e.kind {
            ExprKind::Var(n) => self
                .lookup(n)
                .filter(|&id| self.vars[id].ty.is_quantum())
                .map(|id| (id, Idx::Whole)),
            ExprKind::Index { base, index } => {
                let ExprKind::Var(n) = &base.kind else {
                    return None;
                };
                let id = self.lookup(n).filter(|&id| self.vars[id].ty == Ty::QReg)?;
                let idx = match index.kind {
                    ExprKind::Int(k) => Idx::Lit(k),
                    _ => Idx::Dynamic,
                };
                Some((id, idx))
            }
            _ => None,
        }
    }

    // A consumed operand of `measure`/`discard` or a `consume` argument: a whole `Qubit`/`QReg`
    // variable or a fresh value. `want` is the type reported for a classical operand.
    fn consume(&mut self, e: &Expr, call: Span, want: Ty) -> Ty {
        match &e.kind {
            ExprKind::Var(name) => {
                if let Some(id) = self.lookup(name) {
                    if self.vars[id].ty.is_quantum() {
                        self.use_var(id, e.span);
                        if !self.borrowed_consumed(id, e.span) {
                            self.vars[id].state = State::Consumed(call);
                        }
                        return self.vars[id].ty;
                    }
                }
            }
            ExprKind::Index { base, .. } if self.is_qreg_var(base) => {
                self.expr(e);
                self.err(Code::PartialMove, e.span, msg::partial_move());
                return Ty::Error;
            }
            _ => {}
        }
        let t = self.expr(e);
        if !t.is_quantum() && t != Ty::Error {
            self.expect_ty(e.span, want, t);
            return Ty::Error;
        }
        t
    }
}

type Edges = [(String, Vec<(String, Span)>)];

// Marks every function that calls a marked one, recording the call that the sweep
// `until no change { for f in declaration order, if unmarked: mark at its first call to a
// marked function }` would record. Running that sweep takes one pass per link of a chain
// declared callers first, which is quadratic. Instead, each function's marking time
// (pass, declaration index) is computed with Dijkstra over reverse call edges: in pass q, the
// function at index e sees a callee marked at an earlier time, which is in pass q - 1 or
// before, or in pass q at a smaller index. Pass 0 holds the functions marked beforehand.
fn propagate(edges: &Edges, marked: &mut HashMap<String, (Span, Unconditional)>) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut callers: HashMap<&str, Vec<usize>> = HashMap::new();
    for (e, (_, calls)) in edges.iter().enumerate() {
        for (callee, _) in calls {
            callers.entry(callee.as_str()).or_default().push(e);
        }
    }
    let mut time: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let reached = |name: &str, (pass, at): (usize, usize), heap: &mut BinaryHeap<_>| {
        for &e in callers.get(name).into_iter().flatten() {
            let q = match pass {
                0 => 1,
                _ if at < e => pass,
                _ => pass + 1,
            };
            heap.push(Reverse((q, e)));
        }
    };
    for (name, _) in edges {
        if marked.contains_key(name) && !time.contains_key(name.as_str()) {
            time.insert(name, (0, 0));
            reached(name, (0, 0), &mut heap);
        }
    }
    while let Some(Reverse(t)) = heap.pop() {
        let (name, calls) = &edges[t.1];
        if time.contains_key(name.as_str()) {
            continue;
        }
        let hit = calls
            .iter()
            .find(|(callee, _)| time.get(callee.as_str()).is_some_and(|&c| c < t));
        // The pushing callee was marked before `t`, so some call is always found.
        let Some((callee, at)) = hit else { continue };
        marked.insert(name.clone(), (*at, Unconditional::Calls(callee.clone())));
        time.insert(name, t);
        reached(name, t, &mut heap);
    }
}

// Whether two operands can be the same qubit: a whole variable overlaps any of its elements;
// dynamic indices are left to the runtime check.
fn overlaps(a: (usize, Idx), b: (usize, Idx)) -> bool {
    a.0 == b.0
        && match (a.1, b.1) {
            (Idx::Whole, _) | (_, Idx::Whole) => true,
            (Idx::Lit(x), Idx::Lit(y)) => x == y,
            _ => false,
        }
}

fn note(span: Span, message: String) -> super::diag::Note {
    super::diag::Note {
        span: Some(span),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The sweep `propagate` replaces, kept as the reference for the differential test.
    fn sweep(edges: &Edges, marked: &mut HashMap<String, (Span, Unconditional)>) {
        loop {
            let mut changed = false;
            for (name, calls) in edges {
                if marked.contains_key(name) {
                    continue;
                }
                let hit = calls.iter().find(|(callee, _)| marked.contains_key(callee));
                if let Some((callee, at)) = hit {
                    let why = Unconditional::Calls(callee.clone());
                    marked.insert(name.clone(), (*at, why));
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    #[test]
    fn propagate_records_what_the_sweep_records() {
        // Random call graphs with several callees per function, cycles, self-calls, duplicate
        // names, calls to undefined names, and both declaration orders.
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |m: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % m) as usize
        };
        for round in 0..3000 {
            let names = 1 + next(12);
            let fns = 1 + next(16);
            let mut span = 0;
            let mut edges = Vec::new();
            for _ in 0..fns {
                let calls = (0..next(5))
                    .map(|_| {
                        span += 1;
                        // Names past `names` are never defined.
                        let at = Span {
                            start: span,
                            end: span,
                        };
                        (format!("f{}", next(names as u64 + 2)), at)
                    })
                    .collect();
                edges.push((format!("f{}", next(names as u64)), calls));
            }
            let mut direct = HashMap::new();
            for (name, _) in &edges {
                if next(4) == 0 {
                    let why = (Span::default(), Unconditional::Op("println".into()));
                    direct.entry(name.clone()).or_insert(why);
                }
            }
            let (mut want, mut got) = (direct.clone(), direct);
            sweep(&edges, &mut want);
            propagate(&edges, &mut got);
            assert_eq!(got, want, "round {round}: {edges:?}");
        }
    }
}
