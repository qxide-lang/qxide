// Static checker: names, types, linear qubit ownership, and `if`-on-`Bit` restrictions.
// Flow-sensitive over the AST; reports every error it finds instead of stopping at the first.

use std::collections::HashMap;

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::msg;

pub const BUILTINS: &[&str] = &[
    "qubit", "qreg", "h", "x", "z", "s", "sdg", "t", "tdg", "reset", "rx", "ry", "rz", "cx", "ccx",
    "measure", "discard", "len", "println",
];

pub const ONE_QUBIT_OPS: &[&str] = &["h", "x", "z", "s", "sdg", "t", "tdg", "reset"];

pub const ROTATIONS: &[&str] = &["rx", "ry", "rz"];

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
}

struct Sig {
    params: Vec<Ty>,
    ret: Ty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Idx {
    Whole,
    Lit(i64),
    Dynamic,
}

struct Checker {
    fns: HashMap<String, Sig>,
    vars: Vec<Var>,
    scopes: Vec<Vec<usize>>,
    // Condition spans of the enclosing `if`-on-`Bit` statements.
    bit_ifs: Vec<Span>,
    ret: Ty,
    errors: Vec<Diagnostic>,
}

pub fn check(p: &Program) -> Vec<Diagnostic> {
    let mut c = Checker {
        fns: HashMap::new(),
        vars: Vec::new(),
        scopes: Vec::new(),
        bit_ifs: Vec::new(),
        ret: Ty::Void,
        errors: Vec::new(),
    };
    c.signatures(p);
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

    fn signatures(&mut self, p: &Program) {
        let mut main = None;
        for f in &p.fns {
            let name = &f.name.name;
            if BUILTINS.contains(&name.as_str()) {
                self.err(
                    Code::BuiltinRedefined,
                    f.name.span,
                    msg::builtin_redefined(name),
                );
                continue;
            }
            if self.fns.contains_key(name) {
                self.err(Code::Duplicate, f.name.span, msg::duplicate_def(name));
                continue;
            }
            for p in &f.params {
                if p.ty.is_quantum() {
                    self.err(Code::QuantumSignature, p.ty_span, msg::quantum_signature());
                }
            }
            if f.ret.is_quantum() {
                self.err(Code::QuantumSignature, f.ret_span, msg::quantum_signature());
            }
            if name == "main" {
                main = Some(f);
            }
            self.fns.insert(
                name.clone(),
                Sig {
                    params: f.params.iter().map(|p| p.ty).collect(),
                    ret: f.ret,
                },
            );
        }
        match main {
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

    fn function(&mut self, f: &FnDecl) {
        self.vars.clear();
        self.scopes = vec![Vec::new()];
        self.ret = if f.ret.is_quantum() { Ty::Error } else { f.ret };
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
            let ty = if p.ty.is_quantum() || p.ty == Ty::Void {
                Ty::Error
            } else {
                p.ty
            };
            self.declare(&p.name, ty, true);
        }
        let diverges = self.block(&f.body);
        if !diverges && !matches!(f.ret, Ty::Void | Ty::Error) {
            self.err(
                Code::MissingReturn,
                f.body.close,
                msg::missing_return(&f.name.name, f.ret.name()),
            );
        }
    }

    fn declare(&mut self, name: &Ident, ty: Ty, mutable: bool) {
        self.vars.push(Var {
            name: name.name.clone(),
            ty,
            mutable,
            decl: name.span,
            state: State::Live,
            bit_if_depth: self.bit_ifs.len(),
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
                let v = &self.vars[id];
                if v.ty.is_quantum() && v.state == State::Live {
                    let (decl, name) = (v.decl, v.name.clone());
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
                let var_ty = if *ty == Ty::Void {
                    self.err(Code::TypeMismatch, *ty_span, msg::void_variable());
                    Ty::Error
                } else if ty.is_quantum() && found == Ty::Error {
                    // The initializer was already rejected; do not also report a leak.
                    Ty::Error
                } else {
                    self.expect_ty(init.span, *ty, found);
                    *ty
                };
                if var_ty.is_quantum() && !self.bit_ifs.is_empty() {
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
                    self.err(Code::UnknownVar, name.span, msg::unknown_var(&name.name));
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
                self.scopes.push(Vec::new());
                self.declare(var, Ty::Int, false);
                let diverges = self.block(body);
                self.scopes.pop();
                if !diverges {
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
                        self.err(Code::ConsumedInLoop, at, msg::consumed_in_loop(&name))
                            .notes
                            .push(note(decl, msg::note_declared_here()));
                    }
                }
                // The loop may run zero times.
                self.restore(&before);
                false
            }
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
                let found = match value {
                    Some(e) => self.expr(e),
                    None => Ty::Void,
                };
                let want = self.ret;
                self.expect_ty(value.as_ref().map_or(*span, |e| e.span), want, found);
                let live: Vec<usize> = self
                    .scopes
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|&id| {
                        let v = &self.vars[id];
                        v.ty.is_quantum() && v.state == State::Live
                    })
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
                if self.expr(e).is_quantum() {
                    self.err(Code::TempDropped, e.span, msg::temp_dropped());
                }
                false
            }
        }
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
            ExprKind::Str(_) => {
                self.err(Code::StringNotAllowed, e.span, msg::string_not_allowed());
                Ty::Error
            }
            ExprKind::Var(name) => match self.lookup(name) {
                Some(id) => {
                    if self.vars[id].ty.is_quantum() {
                        self.use_var(id, e.span);
                    }
                    self.vars[id].ty
                }
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
            ExprKind::Method { recv, name } => {
                let t = self.expr(recv);
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
                let (l, r) = (self.expr(lhs), self.expr(rhs));
                use BinOp::*;
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
                        if l == Ty::Boolean {
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
        let (params, ret) = (sig.params.clone(), sig.ret);
        self.forbid_in_bit_if(span, &format!("{n}()"));
        if self.arity(name, args, params.len(), span) {
            for (a, want) in args.iter().zip(params) {
                let t = self.expr(a);
                let want = if want.is_quantum() { Ty::Error } else { want };
                self.expect_ty(a.span, want, t);
            }
        }
        if ret.is_quantum() {
            Ty::Error
        } else {
            ret
        }
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
                if self.arity(name, args, want, span) {
                    let places: Vec<_> = args.iter().map(|a| self.place(a)).collect();
                    for j in 0..places.len() {
                        for i in 0..j {
                            if let (Some(a), Some(b)) = (places[i], places[j]) {
                                if same_place(a, b) {
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
                Ty::Void
            }
            "measure" | "discard" => {
                self.forbid_in_bit_if(span, n);
                if !self.arity(name, args, 1, span) {
                    return if n == "measure" { Ty::Error } else { Ty::Void };
                }
                let t = self.consume(&args[0], span);
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
                    if !matches!(t, Ty::QReg | Ty::BitReg | Ty::Error) {
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
                if self.arity(name, args, 2, span) {
                    self.place(&args[0]);
                    let t = self.expr(&args[1]);
                    self.numeric(args[1].span, t);
                }
                Ty::Void
            }
            _ => {
                debug_assert!(ONE_QUBIT_OPS.contains(&n));
                if self.arity(name, args, 1, span) {
                    self.place(&args[0]);
                }
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

    // A consumed operand of `measure`/`discard`: a whole `Qubit`/`QReg` variable or a fresh value.
    fn consume(&mut self, e: &Expr, call: Span) -> Ty {
        match &e.kind {
            ExprKind::Var(name) => {
                if let Some(id) = self.lookup(name) {
                    if self.vars[id].ty.is_quantum() {
                        self.use_var(id, e.span);
                        self.vars[id].state = State::Consumed(call);
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
            self.expect_ty(e.span, Ty::Qubit, t);
            return Ty::Error;
        }
        t
    }
}

fn same_place(a: (usize, Idx), b: (usize, Idx)) -> bool {
    a.0 == b.0
        && match (a.1, b.1) {
            (Idx::Whole, Idx::Whole) => true,
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
