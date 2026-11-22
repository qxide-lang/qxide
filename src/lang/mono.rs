// Monomorphization: after checking, every generic function and type is replaced by one copy per
// set of type arguments it is used with, so the interpreters, the IR and the JIT only ever see
// concrete code. Instances are named by their source text: `first<int>` for a function,
// `Pair<int, String>` for a type (the same interned type the checker used).
//
// Instantiation is transitive (a generic body may use other generics, or itself) and bounded:
// a generic function calling itself with an ever larger type argument stops at a size or count
// limit with `Q0230` at the call or construction that asked for it.

use std::collections::{HashMap, HashSet, VecDeque};

use super::ast::*;
use super::check::{Inferred, MethodCall};
use super::diag::{Code, Diagnostic, Span};
use super::msg;

/// Most instances (functions and types together) one program may create.
pub const MAX_INSTANCES: usize = 1000;
/// Most type constructors in one instance type (`Box<Box<int>>` has 3).
pub const MAX_TYPE_SIZE: usize = 64;

type R<T> = Result<T, Diagnostic>;
type Subst = HashMap<Ty, Ty>;

struct Mono<'a> {
    inferred: &'a Inferred,
    gen_fns: HashMap<String, FnDecl>,
    gen_structs: HashMap<Ty, StructDecl>,
    gen_enums: HashMap<Ty, EnumDecl>,
    fns_done: HashSet<String>,
    types_done: HashSet<Ty>,
    // (instance name, generic function, type arguments)
    queue: VecDeque<(String, String, Vec<Ty>)>,
    structs: Vec<StructDecl>,
    enums: Vec<EnumDecl>,
    count: usize,
    // Methods of each generic struct and enum (by generic base): instances of a type that
    // implements an interface get all of them, since an interface call may reach any.
    methods_of: HashMap<Ty, Vec<String>>,
    // `new T[n]` in generic code, as (span, element type after substitution): checked once all
    // instances exist, since an interface element type has no default value.
    defaults: Vec<(Span, Ty)>,
}

pub fn monomorphize(p: Program, inferred: &Inferred) -> R<Program> {
    let generic = p.fns.iter().any(|f| !f.tparams.is_empty())
        || p.structs.iter().any(|s| !s.params.is_empty())
        || p.enums.iter().any(|e| !e.params.is_empty());
    // Method calls become calls of their method functions.
    if !generic && !p.fns.iter().any(|f| f.method.is_some()) {
        return Ok(p);
    }
    let mut methods_of: HashMap<Ty, Vec<String>> = HashMap::new();
    for f in p.fns.iter().filter(|f| !f.tparams.is_empty()) {
        if let (Some(_), Some(this)) = (&f.method, f.params.first()) {
            if let Some((base, _)) = this.ty.generic_args() {
                methods_of
                    .entry(base)
                    .or_default()
                    .push(f.name.name.clone());
            }
        }
    }
    let (gen_fns, fns): (Vec<FnDecl>, Vec<FnDecl>) =
        p.fns.into_iter().partition(|f| !f.tparams.is_empty());
    let (gen_structs, structs): (Vec<StructDecl>, Vec<StructDecl>) =
        p.structs.into_iter().partition(|s| !s.params.is_empty());
    let (gen_enums, enums): (Vec<EnumDecl>, Vec<EnumDecl>) =
        p.enums.into_iter().partition(|e| !e.params.is_empty());
    let mut m = Mono {
        inferred,
        gen_fns: gen_fns
            .into_iter()
            .map(|f| (f.name.name.clone(), f))
            .collect(),
        gen_structs: gen_structs
            .into_iter()
            .map(|s| (Ty::struct_named(&s.name.name), s))
            .collect(),
        gen_enums: gen_enums
            .into_iter()
            .map(|e| (Ty::enum_named(&e.name.name), e))
            .collect(),
        fns_done: HashSet::new(),
        types_done: HashSet::new(),
        queue: VecDeque::new(),
        structs,
        enums,
        count: 0,
        methods_of,
        defaults: Vec::new(),
    };
    // Concrete declarations may use instances too (`struct S { Pair<int, int> p; }`).
    let none = Subst::new();
    let concrete: Vec<(Ty, Span)> = m
        .structs
        .iter()
        .flat_map(|s| s.fields.iter())
        .chain(
            m.enums
                .iter()
                .flat_map(|e| e.variants.iter().flat_map(|v| &v.fields)),
        )
        .map(|f| (f.ty, f.ty_span))
        .collect();
    for (t, at) in concrete {
        m.need(t, at)?;
    }
    let mut out = Vec::with_capacity(fns.len());
    for mut f in fns {
        m.func(&mut f, &none)?;
        out.push(f);
    }
    while let Some((name, base, targs)) = m.queue.pop_front() {
        let g = &m.gen_fns[&base];
        let mut f = g.clone();
        let map: Subst = g
            .tparams
            .iter()
            .map(|p| Ty::param_named(&p.name))
            .zip(targs)
            .collect();
        f.tparams.clear();
        f.name.name = name;
        // An instance is not itself an entry point (an app command, for example).
        f.public = None;
        m.func(&mut f, &map)?;
        out.push(f);
    }
    m.check_defaults()?;
    Ok(Program {
        imports: p.imports,
        structs: m.structs,
        enums: m.enums,
        ifaces: p.ifaces,
        fns: out,
    })
}

fn limit(at: Span, message: String) -> Diagnostic {
    Diagnostic::new(Code::TypeArgs, at, message)
}

impl Mono<'_> {
    fn count(&mut self, at: Span) -> R<()> {
        self.count += 1;
        if self.count > MAX_INSTANCES {
            return Err(limit(at, msg::instantiation_limit(MAX_INSTANCES)));
        }
        Ok(())
    }

    // `t` with the type arguments in `map`, its instances created.
    fn ty(&mut self, t: Ty, map: &Subst, at: Span) -> R<Ty> {
        let t = if map.is_empty() {
            t
        } else {
            t.subst(&|p| map.get(&p).copied())
        };
        self.need(t, at)?;
        Ok(t)
    }

    // Creates the declarations of the instance types `t` mentions.
    fn need(&mut self, t: Ty, at: Span) -> R<()> {
        let mut stack = vec![t];
        while let Some(mut t) = stack.pop() {
            while let Some(e) = t.elem() {
                t = e;
            }
            let Some((base, args)) = t.generic_args() else {
                continue;
            };
            if !self.types_done.insert(t) {
                continue;
            }
            if t.size() > MAX_TYPE_SIZE {
                return Err(limit(at, msg::type_too_large(MAX_TYPE_SIZE)));
            }
            self.count(at)?;
            let name = Ident {
                name: t.name().to_string(),
                span: Span::default(),
            };
            let sub = |params: &[Ident], ty: Ty| {
                let map: Subst = params
                    .iter()
                    .map(|p| Ty::param_named(&p.name))
                    .zip(args.iter().copied())
                    .collect();
                ty.subst(&|p| map.get(&p).copied())
            };
            let implements = if let Some(g) = self.gen_structs.get(&base) {
                !g.implements.is_empty()
            } else {
                self.gen_enums
                    .get(&base)
                    .is_some_and(|g| !g.implements.is_empty())
            };
            if implements {
                for f in self.methods_of.get(&base).cloned().unwrap_or_default() {
                    self.instance_fn(&f, args.clone(), at)?;
                }
            }
            if let Some(g) = self.gen_structs.get(&base) {
                let mut s = g.clone();
                for f in &mut s.fields {
                    f.ty = sub(&g.params, f.ty);
                    stack.push(f.ty);
                }
                for i in &mut s.implements {
                    i.0 = sub(&g.params, i.0);
                }
                s.params.clear();
                s.name = Ident {
                    span: g.name.span,
                    ..name
                };
                self.structs.push(s);
            } else if let Some(g) = self.gen_enums.get(&base) {
                let mut e = g.clone();
                for f in e.variants.iter_mut().flat_map(|v| &mut v.fields) {
                    f.ty = sub(&g.params, f.ty);
                    stack.push(f.ty);
                }
                for i in &mut e.implements {
                    i.0 = sub(&g.params, i.0);
                }
                e.params.clear();
                e.name = Ident {
                    span: g.name.span,
                    ..name
                };
                self.enums.push(e);
            }
        }
        Ok(())
    }

    // Names the instance `name<args>` of the generic function `name` and queues it once.
    fn instance_fn(&mut self, name: &str, tys: Vec<Ty>, at: Span) -> R<String> {
        let names: Vec<&str> = tys.iter().map(|t| t.name()).collect();
        let inst = format!("{name}<{}>", names.join(", "));
        if self.fns_done.insert(inst.clone()) {
            self.count(at)?;
            self.queue.push_back((inst.clone(), name.to_string(), tys));
        }
        Ok(inst)
    }

    // `new T[n]` with an element type that turned out to have no default value (`Q0225`).
    fn check_defaults(&self) -> R<()> {
        let structs: HashMap<Ty, &StructDecl> = self
            .structs
            .iter()
            .map(|s| (Ty::struct_named(&s.name.name), s))
            .collect();
        let enums: HashMap<Ty, &EnumDecl> = self
            .enums
            .iter()
            .map(|e| (Ty::enum_named(&e.name.name), e))
            .collect();
        fn ok(t: Ty, s: &HashMap<Ty, &StructDecl>, e: &HashMap<Ty, &EnumDecl>, d: usize) -> bool {
            if d > 256 {
                return true;
            }
            match t {
                Ty::Iface(_) => false,
                Ty::Struct(_) => s
                    .get(&t)
                    .is_none_or(|x| x.fields.iter().all(|f| ok(f.ty, s, e, d + 1))),
                Ty::Enum(_) => e.get(&t).is_none_or(|x| {
                    x.variants
                        .first()
                        .is_none_or(|v| v.fields.iter().all(|f| ok(f.ty, s, e, d + 1)))
                }),
                _ => true,
            }
        }
        for &(at, t) in &self.defaults {
            if !ok(t, &structs, &enums, 0) {
                let mut d = Diagnostic::new(Code::BadArrayElement, at, msg::no_default(t.name()));
                d.help = Some(msg::help_no_default());
                return Err(d);
            }
        }
        Ok(())
    }

    // `v.m(args)` resolved to a method function becomes the call `Type.m(v, args)`; a call on an
    // interface value stays a method call, dispatched at run time.
    fn method(&mut self, e: &mut Expr, map: &Subst) -> R<()> {
        let at = e.span;
        let key = (at.start, at.end);
        let ExprKind::Method { recv, name, args } = &mut e.kind else {
            return Ok(());
        };
        // `s.try_parse_int()` returns `Result<int, String>`, which no declaration names.
        if let Some(ok) = super::pure::try_parse_ok(&name.name) {
            self.need(super::pure::parse_result(ok), at)?;
        }
        self.expr(recv, map)?;
        self.exprs(args, map)?;
        let (fname, tys) = match self.inferred.methods.get(&key) {
            Some(MethodCall::Static(f)) => {
                let tys = match self.inferred.calls.get(&key) {
                    Some(targs) => {
                        let mut tys = Vec::with_capacity(targs.len());
                        for &t in targs {
                            tys.push(self.ty(t, map, at)?);
                        }
                        tys
                    }
                    None => Vec::new(),
                };
                (f.clone(), tys)
            }
            Some(MethodCall::Bound(p)) => {
                let t = self.ty(*p, map, at)?;
                if !matches!(t, Ty::Struct(_) | Ty::Enum(_)) {
                    return Ok(());
                }
                match t.generic_args() {
                    Some((base, args)) => (format!("{}.{}", base.name(), name.name), args),
                    None => (format!("{}.{}", t.name(), name.name), Vec::new()),
                }
            }
            _ => return Ok(()),
        };
        let fname = if tys.is_empty() {
            fname
        } else {
            self.instance_fn(&fname, tys, at)?
        };
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(std::mem::replace(
            recv.as_mut(),
            Expr {
                kind: ExprKind::Int(0),
                span: at,
            },
        ));
        all.append(args);
        e.kind = ExprKind::Call {
            name: Ident {
                name: fname,
                span: name.span,
            },
            args: all,
        };
        Ok(())
    }

    fn func(&mut self, f: &mut FnDecl, map: &Subst) -> R<()> {
        for p in &mut f.params {
            p.ty = self.ty(p.ty, map, p.ty_span)?;
        }
        f.ret = self.ty(f.ret, map, f.ret_span)?;
        self.block(&mut f.body, map)
    }

    fn block(&mut self, b: &mut Block, map: &Subst) -> R<()> {
        for s in &mut b.stmts {
            self.stmt(s, map)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &mut Stmt, map: &Subst) -> R<()> {
        match s {
            Stmt::Decl {
                ty, ty_span, init, ..
            } => {
                if let Some(t) = ty {
                    *t = self.ty(*t, map, *ty_span)?;
                }
                self.expr(init, map)
            }
            Stmt::Assign { value, .. } => self.expr(value, map),
            Stmt::If(i) => self.if_stmt(i, map),
            Stmt::For {
                start, end, body, ..
            } => {
                self.expr(start, map)?;
                self.expr(end, map)?;
                self.block(body, map)
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond, map)?;
                self.block(body, map)
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => Ok(()),
            Stmt::Return { value, .. } => match value {
                Some(e) => self.expr(e, map),
                None => Ok(()),
            },
            Stmt::SetField { base, value, .. } => {
                self.expr(base, map)?;
                self.expr(value, map)
            }
            Stmt::SetIndex {
                base, index, value, ..
            } => {
                self.expr(base, map)?;
                self.expr(index, map)?;
                self.expr(value, map)
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, map)?;
                for a in arms {
                    self.block(&mut a.body, map)?;
                }
                Ok(())
            }
            Stmt::Expr(e) => self.expr(e, map),
        }
    }

    fn if_stmt(&mut self, i: &mut IfStmt, map: &Subst) -> R<()> {
        self.expr(&mut i.cond, map)?;
        self.block(&mut i.then, map)?;
        match &mut i.els {
            Some(Else::Block(b)) => self.block(b, map),
            Some(Else::If(e)) => self.if_stmt(e, map),
            None => Ok(()),
        }
    }

    fn exprs(&mut self, es: &mut [Expr], map: &Subst) -> R<()> {
        for e in es {
            self.expr(e, map)?;
        }
        Ok(())
    }

    fn expr(&mut self, e: &mut Expr, map: &Subst) -> R<()> {
        let at = e.span;
        let key = (at.start, at.end);
        match &mut e.kind {
            ExprKind::Int(_)
            | ExprKind::Double(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Var(_) => Ok(()),
            ExprKind::Call { name, args } => {
                self.exprs(args, map)?;
                let Some(targs) = self.inferred.calls.get(&key) else {
                    return Ok(());
                };
                let mut tys = Vec::with_capacity(targs.len());
                for &t in targs {
                    tys.push(self.ty(t, map, at)?);
                }
                name.name = self.instance_fn(&name.name.clone(), tys, at)?;
                Ok(())
            }
            ExprKind::Index { base, index } => {
                self.expr(base, map)?;
                self.expr(index, map)
            }
            ExprKind::Method { .. } => self.method(e, map),
            ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => self.expr(expr, map),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, map)?;
                self.expr(rhs, map)
            }
            ExprKind::NewStruct { ty, args } | ExprKind::Variant { ty, args, .. } => {
                let t = self.inferred.types.get(&key).copied().unwrap_or(*ty);
                *ty = self.ty(t, map, at)?;
                self.exprs(args, map)
            }
            ExprKind::Field { recv, .. } | ExprKind::Try { expr: recv, .. } => self.expr(recv, map),
            ExprKind::NewArray { ty, dims } => {
                let generic = ty.has_param();
                *ty = self.ty(*ty, map, at)?;
                if generic {
                    let mut elem = *ty;
                    for _ in 0..dims.len() {
                        elem = elem.elem().unwrap_or(Ty::Error);
                    }
                    self.defaults.push((at, elem));
                }
                self.exprs(dims, map)
            }
            ExprKind::ArrayLit { ty, elems } => {
                if let Some(t) = ty {
                    *t = self.ty(*t, map, at)?;
                }
                self.exprs(elems, map)
            }
        }
    }
}
