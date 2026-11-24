// Rewrites a monomorphized program that passed `values::check` into an ordinary program the
// engines run unchanged (`--experimental value-semantics`; see src/lang/values.rs):
//
// - `x.copy()` becomes a call of a generated function `#copyN` that builds a new value of the
//   type (fields, elements and entries copied in turn; `String`, enums and `Json` are shared,
//   since they never change).
// - A small struct (`Decls::implicit_copy`) read from a variable, field or element where a value
//   is kept is copied the same way, so changing the copy leaves the original alone.
// - An `inout` parameter the engines would pass by value (a number, `boolean`, `String`, enum,
//   or a parameter the function assigns as a whole) becomes a one-field struct `#RefN`: the
//   caller wraps the argument before the statement and writes the field back after it.
// - The `inout` and classical `consume` markers are removed.
//
// The result type-checks without the feature (a debug build checks it), and all three engines
// see the same program, so they agree on results and step counts.

use std::collections::{HashMap, HashSet};

use super::super::ast::*;
use super::super::diag::Span;
use super::{celled_params, is_loop_collection, Decls, Mode, Types, PROJECTING, STORING};

/// Rewrites `p` (monomorphized, checked with `types` recorded) for value semantics.
pub fn lower(p: &mut Program, types: &Types) {
    let snapshot = p.clone();
    let decls = Decls::new(&snapshot);
    let cells: HashMap<String, Vec<bool>> = snapshot
        .fns
        .iter()
        .map(|f| (f.name.name.clone(), celled_params(f)))
        .collect();
    let mut l = Lower {
        decls: &decls,
        types,
        fi: 0,
        cells,
        celled_here: HashSet::new(),
        copies: HashMap::new(),
        refs: HashMap::new(),
        new_fns: Vec::new(),
        new_structs: Vec::new(),
        fresh: 0,
        prelude: false,
    };
    for (fi, f) in p.fns.iter_mut().enumerate() {
        l.fi = fi;
        l.prelude = decls.prelude_fn(&snapshot.fns[fi]);
        l.celled_here.clear();
        let flags = l.cells.get(&f.name.name).cloned().unwrap_or_default();
        for (q, cell) in f.params.iter_mut().zip(flags) {
            if cell {
                l.celled_here.insert(q.name.name.clone());
                q.ty = l.ref_type(q.ty);
            }
        }
        l.block(&mut f.body);
        for q in &mut f.params {
            q.inout = None;
            if !q.ty.is_quantum() {
                q.consume = None;
            }
        }
        f.mutating = None;
    }
    for i in &mut p.ifaces {
        for m in &mut i.methods {
            for q in &mut m.params {
                q.inout = None;
                if !q.ty.is_quantum() {
                    q.consume = None;
                }
            }
        }
    }
    p.fns.append(&mut l.new_fns);
    p.structs.append(&mut l.new_structs);
}

// Statements to run before and after the statement being rewritten: the cells of its `inout`
// arguments and their write-backs.
#[derive(Default)]
struct Hoist {
    pre: Vec<Stmt>,
    post: Vec<Stmt>,
}

impl Hoist {
    fn is_empty(&self) -> bool {
        self.pre.is_empty() && self.post.is_empty()
    }
}

struct Lower<'a, 'p> {
    decls: &'a Decls<'p>,
    types: &'a Types,
    fi: usize,
    cells: HashMap<String, Vec<bool>>,
    // The parameters of the current function that are cells (`x` is `x.v`).
    celled_here: HashSet<String>,
    copies: HashMap<Ty, String>,
    refs: HashMap<Ty, Ty>,
    new_fns: Vec<FnDecl>,
    new_structs: Vec<StructDecl>,
    fresh: usize,
    // In a method of the prelude's `Result` or `Option`: every kept value is copied.
    prelude: bool,
}

const CELL_FIELD: &str = "v";

fn ident(name: &str, span: Span) -> Ident {
    Ident {
        name: name.to_string(),
        span,
    }
}

fn var(name: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Var(name.to_string()),
        span,
    }
}

fn field(recv: Expr, name: &str) -> Expr {
    let span = recv.span;
    Expr {
        kind: ExprKind::Field {
            recv: Box::new(recv),
            name: ident(name, span),
        },
        span,
    }
}

fn index(base: Expr, i: Expr) -> Expr {
    let span = base.span;
    Expr {
        kind: ExprKind::Index {
            base: Box::new(base),
            index: Box::new(i),
        },
        span,
    }
}

fn call(name: &str, args: Vec<Expr>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            name: ident(name, span),
            args,
        },
        span,
    }
}

fn method(recv: Expr, name: &str, args: Vec<Expr>) -> Expr {
    let span = recv.span;
    Expr {
        kind: ExprKind::Method {
            recv: Box::new(recv),
            name: ident(name, span),
            args,
        },
        span,
    }
}

fn decl(name: &str, ty: Option<Ty>, init: Expr) -> Stmt {
    let span = init.span;
    Stmt::Decl {
        is_final: false,
        ty,
        ty_span: span,
        name: ident(name, span),
        init,
    }
}

fn int(n: i64, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Int(n),
        span,
    }
}

impl Lower<'_, '_> {
    fn ty(&self, e: &Expr) -> Option<Ty> {
        self.types.expr(self.fi, e)
    }

    fn fresh(&mut self, what: &str) -> String {
        self.fresh += 1;
        format!("#{what}{}", self.fresh)
    }

    // The cell struct `#RefN { T v; }` for `t`.
    fn ref_type(&mut self, t: Ty) -> Ty {
        if let Some(r) = self.refs.get(&t) {
            return *r;
        }
        let name = self.fresh("Ref");
        let span = Span::default();
        self.new_structs.push(StructDecl {
            name: ident(&name, span),
            params: Vec::new(),
            implements: Vec::new(),
            fields: vec![Field {
                ty: t,
                ty_span: span,
                name: ident(CELL_FIELD, span),
            }],
            methods: Vec::new(),
            prelude: false,
        });
        let r = Ty::struct_named(&name);
        self.refs.insert(t, r);
        r
    }

    // `e` copied: itself for values that never change, else a call of the copy function of its
    // type.
    fn copy_of(&mut self, e: Expr, t: Ty) -> Expr {
        if self.decls.frozen(t) || self.decls.is_shared(t) {
            return e;
        }
        let span = e.span;
        let f = self.copy_fn(t);
        call(&f, vec![e], span)
    }

    // The generated function that copies a value of type `t`.
    fn copy_fn(&mut self, t: Ty) -> String {
        if let Some(n) = self.copies.get(&t) {
            return n.clone();
        }
        let name = self.fresh("copy");
        self.copies.insert(t, name.clone());
        let span = Span::default();
        let x = || var("#x", span);
        let mut stmts = Vec::new();
        let result = if let Some(fields) = self.decls.fields(t) {
            let args = fields
                .into_iter()
                .map(|(f, ft)| self.copy_of(field(x(), &f), ft))
                .collect();
            Expr {
                kind: ExprKind::NewStruct { ty: t, args },
                span,
            }
        } else {
            // Arrays, `List` and `Map`: a new collection filled in a loop.
            let r = || var("#r", span);
            let i = || var("#i", span);
            let (init, src, put) = match t {
                Ty::Array(_) => {
                    let elem = t.elem().unwrap_or(Ty::Error);
                    let len = call("len", vec![x()], span);
                    let init = Expr {
                        kind: ExprKind::NewArray {
                            ty: t,
                            dims: vec![len],
                        },
                        span,
                    };
                    let value = self.copy_of(index(x(), i()), elem);
                    let put = Stmt::SetIndex {
                        base: r(),
                        index: i(),
                        op: None,
                        value,
                        span,
                    };
                    (init, x(), put)
                }
                _ => {
                    let (kind, args) = super::super::native::kind(t).unwrap_or(("", Vec::new()));
                    let init = Expr {
                        kind: ExprKind::NewStruct {
                            ty: t,
                            args: Vec::new(),
                        },
                        span,
                    };
                    if kind == "Map" {
                        // `keys()` and `values()` list the entries in the same order.
                        stmts.push(decl("#k", None, method(x(), "keys", Vec::new())));
                        stmts.push(decl("#v", None, method(x(), "values", Vec::new())));
                        let value = self.copy_of(index(var("#v", span), i()), args[1]);
                        let put = Stmt::Expr(method(
                            r(),
                            "put",
                            vec![index(var("#k", span), i()), value],
                        ));
                        (init, var("#k", span), put)
                    } else {
                        let value = self.copy_of(index(x(), i()), args[0]);
                        (init, x(), Stmt::Expr(method(r(), "add", vec![value])))
                    }
                }
            };
            stmts.insert(0, decl("#r", Some(t), init));
            stmts.push(Stmt::For {
                var: ident("#i", span),
                start: int(0, span),
                end: call("len", vec![src], span),
                body: Block {
                    stmts: vec![put],
                    close: span,
                },
            });
            r()
        };
        stmts.push(Stmt::Return {
            value: Some(result),
            span,
        });
        self.new_fns.push(FnDecl {
            public: None,
            tparams: Vec::new(),
            bounds: Vec::new(),
            name: ident(&name, span),
            params: vec![Param {
                consume: None,
                inout: None,
                name: ident("#x", span),
                ty: t,
                ty_span: span,
            }],
            ret: t,
            ret_span: span,
            body: Block { stmts, close: span },
            method: None,
            mutating: None,
        });
        name
    }

    // Is `e` a new value (a call result, a constructor, a literal) rather than one kept
    // elsewhere?
    fn fresh_value(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Var(_) | ExprKind::Field { .. } | ExprKind::Index { .. } => false,
            ExprKind::Try { expr, .. } => self.fresh_value(expr),
            ExprKind::Method { recv, name, .. } => {
                !(PROJECTING.contains(&name.name.as_str())
                    && self.ty(recv).is_some_and(|t| {
                        super::super::native::kind(t)
                            .is_some_and(|(k, _)| k == "List" || k == "Map")
                    }))
            }
            _ => true,
        }
    }

    // `e` where a value is kept: a small struct read from a place is copied.
    fn value(&mut self, e: &mut Expr, h: &mut Hoist) {
        let t = self.ty(e);
        let fresh = self.fresh_value(e);
        self.expr(e, h);
        if let Some(t) = t {
            let small_struct = matches!(t, Ty::Struct(_))
                && self.decls.implicit_copy(t)
                && !self.decls.is_shared(t);
            let library = self.prelude && self.decls.copyable(t);
            if (small_struct || library) && !fresh {
                let inner = std::mem::replace(e, int(0, e.span));
                *e = self.copy_of(inner, t);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr, h: &mut Hoist) {
        let t = self.ty(e);
        match &mut e.kind {
            ExprKind::Var(n) => {
                if self.celled_here.contains(n.as_str()) {
                    let inner = var(n, e.span);
                    *e = field(inner, CELL_FIELD);
                }
            }
            ExprKind::Inout { expr } => {
                let mut inner = std::mem::replace(expr.as_mut(), int(0, e.span));
                self.expr(&mut inner, h);
                *e = inner;
            }
            ExprKind::Method { recv, name, args } => {
                let rt = self.ty(recv);
                let is_copy = name.name == "copy"
                    && args.is_empty()
                    && rt.is_some_and(|rt| {
                        let base = rt.generic_args().map_or(rt, |(b, _)| b);
                        self.decls.fn_decl(&format!("{}.copy", rt.name())).is_none()
                            && self
                                .decls
                                .fn_decl(&format!("{}.copy", base.name()))
                                .is_none()
                    });
                if is_copy {
                    let mut inner = std::mem::replace(recv.as_mut(), int(0, e.span));
                    self.expr(&mut inner, h);
                    *e = self.copy_of(inner, t.or(rt).unwrap_or(Ty::Error));
                    return;
                }
                let (fname, modes) = match rt {
                    Some(rt) => self.method_target(rt, &name.name),
                    None => (None, None),
                };
                self.expr(recv, h);
                match modes {
                    Some(modes) => self.args(fname.as_deref(), &modes[1..], 1, args, h),
                    None => {
                        let storing = STORING.contains(&name.name.as_str())
                            && rt.is_some_and(|rt| super::super::native::kind(rt).is_some());
                        for (i, a) in args.iter_mut().enumerate() {
                            let n = name.name.as_str();
                            if storing && (n == "add" || n == "put" || i == 1) {
                                self.value(a, h);
                            } else {
                                self.expr(a, h);
                            }
                        }
                    }
                }
            }
            ExprKind::Call { name, args } => {
                let f = self.decls.fn_decl(&name.name);
                match f {
                    Some(f) => {
                        let modes: Vec<Mode> = f.params.iter().map(Mode::of).collect();
                        let n = name.name.clone();
                        self.args(Some(&n), &modes, 0, args, h);
                    }
                    None => {
                        for a in args {
                            self.expr(a, h);
                        }
                    }
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base, h);
                self.expr(index, h);
            }
            ExprKind::Field { recv, .. } | ExprKind::Try { expr: recv, .. } => self.expr(recv, h),
            ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => self.expr(expr, h),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, h);
                self.expr(rhs, h);
            }
            ExprKind::NewStruct { args: items, .. }
            | ExprKind::Variant { args: items, .. }
            | ExprKind::ArrayLit { elems: items, .. } => {
                for a in items {
                    self.value(a, h);
                }
            }
            ExprKind::NewArray { dims, .. } => {
                for d in dims {
                    self.expr(d, h);
                }
            }
            ExprKind::Int(_) | ExprKind::Double(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        }
    }

    // The function a method call runs and its parameter modes, `this` first.
    fn method_target(&self, t: Ty, name: &str) -> (Option<String>, Option<Vec<Mode>>) {
        if matches!(t, Ty::Struct(_) | Ty::Enum(_)) {
            let base = t.generic_args().map_or(t, |(b, _)| b);
            for owner in [t.name(), base.name()] {
                let n = format!("{owner}.{name}");
                if let Some(f) = self.decls.fn_decl(&n) {
                    return (Some(n), Some(f.params.iter().map(Mode::of).collect()));
                }
            }
            return (None, None);
        }
        (None, self.decls.method_modes(t, name))
    }

    // Arguments of a call of `callee` (`skip`: parameters before the first argument, 1 for the
    // receiver of a method).
    fn args(
        &mut self,
        callee: Option<&str>,
        modes: &[Mode],
        skip: usize,
        args: &mut [Expr],
        h: &mut Hoist,
    ) {
        let flags = callee
            .and_then(|c| self.cells.get(c))
            .cloned()
            .unwrap_or_default();
        for (i, a) in args.iter_mut().enumerate() {
            if let ExprKind::Inout { expr } = &mut a.kind {
                let inner = std::mem::replace(expr.as_mut(), int(0, a.span));
                *a = inner;
            }
            let cell = flags.get(i + skip).copied().unwrap_or(false);
            let mode = modes.get(i).copied().unwrap_or(Mode::Borrow);
            if cell {
                self.cell_arg(a, h);
            } else if mode == Mode::Consume {
                self.value(a, h);
            } else {
                self.expr(a, h);
            }
        }
    }

    // An argument of a cell parameter: a cell of the current function is passed as it is;
    // any other place is wrapped before the statement and written back after it.
    fn cell_arg(&mut self, a: &mut Expr, h: &mut Hoist) {
        if let ExprKind::Var(n) = &a.kind {
            if self.celled_here.contains(n.as_str()) {
                return;
            }
        }
        let Some(t) = self.ty(a) else {
            return self.expr(a, h);
        };
        self.expr(a, h);
        let span = a.span;
        let r = self.ref_type(t);
        let tmp = self.fresh("cell");
        let place = std::mem::replace(a, var(&tmp, span));
        let init = Expr {
            kind: ExprKind::NewStruct {
                ty: r,
                args: vec![place.clone()],
            },
            span,
        };
        h.pre.push(decl(&tmp, Some(r), init));
        let back = field(var(&tmp, span), CELL_FIELD);
        h.post.push(match place.kind {
            ExprKind::Var(n) => Stmt::Assign {
                name: ident(&n, span),
                value: back,
            },
            ExprKind::Field { recv, name } => Stmt::SetField {
                base: *recv,
                name,
                op: None,
                value: back,
                span,
            },
            ExprKind::Index { base, index } => Stmt::SetIndex {
                base: *base,
                index: *index,
                op: None,
                value: back,
                span,
            },
            // `values::check` admits only places as `inout` arguments.
            _ => Stmt::Expr(back),
        });
    }

    fn block(&mut self, b: &mut Block) {
        // A local of the same name hides a cell parameter until the end of its block.
        let cells = self.celled_here.clone();
        let stmts = std::mem::take(&mut b.stmts);
        for s in stmts {
            let out = self.stmt(s);
            for d in &out {
                if let Stmt::Decl { name, .. } = d {
                    self.celled_here.remove(&name.name);
                }
            }
            b.stmts.extend(out);
        }
        self.celled_here = cells;
    }

    // A statement, with the cells its calls need around it.
    fn stmt(&mut self, mut s: Stmt) -> Vec<Stmt> {
        let mut h = Hoist::default();
        match &mut s {
            Stmt::Decl { name, init, .. } => {
                let view = is_loop_collection(&name.name)
                    || matches!(&init.kind, ExprKind::Index { base, .. }
                        if matches!(&base.kind, ExprKind::Var(n) if is_loop_collection(n)));
                if view {
                    self.expr(init, &mut h);
                } else {
                    self.value(init, &mut h);
                }
            }
            Stmt::Assign { name, value } => {
                self.value(value, &mut h);
                let celled = self.celled_here.contains(&name.name);
                if !h.is_empty() {
                    // The call's write-backs come before the assignment.
                    let tmp = self.fresh("t");
                    let v = std::mem::replace(value, var(&tmp, value.span));
                    h.pre.push(decl(&tmp, None, v));
                    h.pre.append(&mut h.post);
                }
                if celled {
                    let span = name.span;
                    let value = std::mem::replace(value, int(0, span));
                    s = Stmt::SetField {
                        base: var(&name.name, span),
                        name: ident(CELL_FIELD, span),
                        op: None,
                        value,
                        span,
                    };
                }
            }
            Stmt::SetField {
                base, op, value, ..
            }
            | Stmt::SetIndex {
                base, op, value, ..
            } => {
                if op.is_some() {
                    self.expr(value, &mut h);
                } else {
                    self.value(value, &mut h);
                }
                self.expr(base, &mut h);
                if let Stmt::SetIndex { index, .. } = &mut s {
                    self.expr(index, &mut h);
                }
            }
            Stmt::If(i) => {
                let out = self.if_stmt(std::mem::replace(i, empty_if()));
                return out;
            }
            Stmt::For {
                var: index_var,
                start,
                end,
                body,
            } => {
                self.expr(start, &mut h);
                self.expr(end, &mut h);
                if !h.is_empty() {
                    for (e, n) in [(&mut *start, "s"), (&mut *end, "e")] {
                        let tmp = self.fresh(n);
                        let v = std::mem::replace(e, var(&tmp, e.span));
                        h.pre.push(decl(&tmp, None, v));
                    }
                    h.pre.append(&mut h.post);
                }
                let cells = self.celled_here.clone();
                self.celled_here.remove(&index_var.name);
                self.block(body);
                self.celled_here = cells;
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond, &mut h);
                self.block(body);
                if !h.is_empty() {
                    // `while true { cells; var c = cond; write-backs; if !c { break; } body }`
                    let span = cond.span;
                    let tmp = self.fresh("c");
                    let c = std::mem::replace(
                        cond,
                        Expr {
                            kind: ExprKind::Bool(true),
                            span,
                        },
                    );
                    let mut head = std::mem::take(&mut h.pre);
                    head.push(decl(&tmp, None, c));
                    head.append(&mut h.post);
                    head.push(Stmt::If(IfStmt {
                        cond: Expr {
                            kind: ExprKind::Unary {
                                op: UnOp::Not,
                                expr: Box::new(var(&tmp, span)),
                            },
                            span,
                        },
                        then: Block {
                            stmts: vec![Stmt::Break { span }],
                            close: span,
                        },
                        els: None,
                    }));
                    head.append(&mut body.stmts);
                    body.stmts = head;
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.value(v, &mut h);
                    if !h.is_empty() {
                        let tmp = self.fresh("t");
                        let e = std::mem::replace(v, var(&tmp, v.span));
                        h.pre.push(decl(&tmp, None, e));
                        h.pre.append(&mut h.post);
                    }
                }
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, &mut h);
                if !h.is_empty() {
                    let tmp = self.fresh("m");
                    let e = std::mem::replace(scrutinee, var(&tmp, scrutinee.span));
                    h.pre.push(decl(&tmp, None, e));
                    h.pre.append(&mut h.post);
                }
                for a in arms {
                    let cells = self.celled_here.clone();
                    if let PatKind::Variant { binds, .. } = &a.pat.kind {
                        for b in binds {
                            self.celled_here.remove(&b.name);
                        }
                    }
                    self.block(&mut a.body);
                    self.celled_here = cells;
                }
            }
            Stmt::Expr(e) => self.expr(e, &mut h),
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
        let mut out = std::mem::take(&mut h.pre);
        out.push(s);
        out.append(&mut h.post);
        out
    }

    fn if_stmt(&mut self, mut i: IfStmt) -> Vec<Stmt> {
        let mut h = Hoist::default();
        self.expr(&mut i.cond, &mut h);
        if !h.is_empty() {
            let tmp = self.fresh("c");
            let span = i.cond.span;
            let c = std::mem::replace(&mut i.cond, var(&tmp, span));
            h.pre.push(decl(&tmp, None, c));
            h.pre.append(&mut h.post);
        }
        self.block(&mut i.then);
        i.els = match i.els.take() {
            Some(Else::Block(mut b)) => {
                self.block(&mut b);
                Some(Else::Block(b))
            }
            Some(Else::If(inner)) => {
                let mut out = self.if_stmt(*inner);
                if out.len() == 1 {
                    match out.pop() {
                        Some(Stmt::If(x)) => Some(Else::If(Box::new(x))),
                        other => Some(Else::Block(Block {
                            stmts: other.into_iter().collect(),
                            close: i.then.close,
                        })),
                    }
                } else {
                    Some(Else::Block(Block {
                        stmts: out,
                        close: i.then.close,
                    }))
                }
            }
            None => None,
        };
        let mut out = h.pre;
        out.push(Stmt::If(i));
        out
    }
}

fn empty_if() -> IfStmt {
    let span = Span::default();
    IfStmt {
        cond: Expr {
            kind: ExprKind::Bool(false),
            span,
        },
        then: Block {
            stmts: Vec::new(),
            close: span,
        },
        els: None,
    }
}
