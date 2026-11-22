// Static checker: names, types, linear qubit ownership, and `if`-on-`Bit` restrictions.
// Flow-sensitive over the AST; reports every error it finds instead of stopping at the first.

use std::collections::{HashMap, HashSet};

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
    // Type parameters of a generic function (inferred from the arguments at each call).
    tparams: Vec<Ty>,
    // The interface each type parameter must implement (`<T extends Shape>`).
    bounds: Vec<Option<Ty>>,
}

// A method an interface requires: name, parameter types with `consume`, return type.
#[derive(Clone)]
struct IfaceMethod {
    name: String,
    params: Vec<(Ty, bool)>,
    ret: Ty,
}

impl IfaceMethod {
    // `double area(int)`, for diagnostics.
    fn text(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|&(t, c)| {
                if c {
                    format!("consume {}", t.name())
                } else {
                    t.name().to_string()
                }
            })
            .collect();
        format!("{} {}({})", self.ret.name(), self.name, params.join(", "))
    }
}

/// How the checker resolved a method call `v.m(...)` on a struct, enum, interface or bounded
/// type parameter, for monomorphization.
#[derive(Debug, Clone)]
pub enum MethodCall {
    /// The method function `Type.m` of a struct or enum (a generic one takes the receiver's type
    /// arguments, recorded in `Inferred::calls`).
    Static(String),
    /// On a value of this bounded type parameter: static or dynamic once the type is known.
    Bound(Ty),
    /// On an interface value: dispatched on the run-time type.
    Dynamic,
}

/// What type inference chose for generic code, keyed by expression span (`start`, `end`), for
/// monomorphization (src/lang/mono.rs): the instance a `new`/variant expression creates, and the
/// type arguments of each call of a generic function, in the callee's parameter order. Types may
/// mention the type parameters of the enclosing generic function.
#[derive(Debug, Default)]
pub struct Inferred {
    pub types: HashMap<(usize, usize), Ty>,
    pub calls: HashMap<(usize, usize), Vec<Ty>>,
    pub methods: HashMap<(usize, usize), MethodCall>,
}

fn key(s: Span) -> (usize, usize) {
    (s.start, s.end)
}

// A classical type, possibly inside arrays (what a type argument must be).
fn shape_ok(mut t: Ty) -> bool {
    while let Some(e) = t.elem() {
        t = e;
    }
    t.is_element()
}

// Does type parameter `p` occur in `t`?
fn mentions(t: Ty, p: Ty) -> bool {
    if t == p {
        return true;
    }
    if let Some(e) = t.elem() {
        return mentions(e, p);
    }
    t.generic_args()
        .is_some_and(|(_, args)| args.iter().any(|&a| mentions(a, p)))
}

// Binds the type parameters in `pat` (a callee's or a generic type's field type) by matching it
// against the type `act` of a value. The first binding wins, except that `int` widens to
// `double` (`pick(1, 2.5)` picks `double`).
fn unify(pat: Ty, act: Ty, map: &mut HashMap<Ty, Ty>) {
    match pat {
        Ty::Param(_) if act != Ty::Error => {
            let slot = map.entry(pat).or_insert(act);
            if *slot == Ty::Int && act == Ty::Double {
                *slot = Ty::Double;
            }
        }
        Ty::Array(_) => {
            if let (Some(p), Some(a)) = (pat.elem(), act.elem()) {
                unify(p, a, map);
            }
        }
        _ => {
            if let (Some((pb, pa)), Some((ab, aa))) = (pat.generic_args(), act.generic_args()) {
                if pb == ab {
                    for (p, a) in pa.into_iter().zip(aa) {
                        unify(p, a, map);
                    }
                }
            }
        }
    }
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

// What `call_sig` needs to know about the function being called.
struct Callee {
    params: Vec<(Ty, bool)>,
    ret: Ty,
    tparams: Vec<Ty>,
    bounds: Vec<Option<Ty>>,
    // Why it cannot run inside an `if` on a `Bit`, if it cannot.
    uncond: Option<(Span, Unconditional)>,
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
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                scan_expr(scrutinee, out);
                for a in arms {
                    scan_block(&a.body, out);
                }
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
        ExprKind::Method { recv, args, name } => {
            // Resolved per method name by `conditional_safety`: the call may run any method
            // of that name (its receiver's type is not known here).
            out.calls.push((format!(".{}", name.name), e.span));
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
        | ExprKind::NewStruct { args: items, .. }
        | ExprKind::Variant { args: items, .. } => {
            for a in items {
                scan_expr(a, out);
            }
        }
        ExprKind::Field { recv, .. } | ExprKind::Try { expr: recv, .. } => scan_expr(recv, out),
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
    // Fields of every struct: types in declaration order, and by name.
    structs: HashMap<Ty, (Vec<Ty>, HashMap<String, Ty>)>,
    // Variants of every enum in declaration order: (name, payload types).
    enums: HashMap<Ty, Vec<(String, Vec<Ty>)>>,
    // Type parameters of each generic struct and enum, keyed by the generic (base) type; the
    // tables above hold their fields with these parameters, substituted per instance.
    gparams: HashMap<Ty, Vec<Ty>>,
    // Every declared struct and enum type.
    declared: HashSet<Ty>,
    // The methods of every interface (keyed by the generic base; its parameters are in
    // `gparams`).
    ifaces: HashMap<Ty, Vec<IfaceMethod>>,
    // The interfaces each struct and enum implements (keyed by the generic base; they may
    // mention its type parameters).
    impls: HashMap<Ty, Vec<Ty>>,
    // Bounds of the type parameters of the function being checked.
    bounds: HashMap<Ty, Ty>,
    // Method functions by method name: `len` -> [`Vec2.len`, ...].
    method_fns: HashMap<String, Vec<String>>,
    // The type the next expression is expected to have (a declaration, assignment, return,
    // argument or store), so `Option.None` can take its type arguments from it.
    hint: Option<Ty>,
    inferred: Inferred,
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
    check_infer(p, require_main).0
}

/// The recursion rules for declared types (`Q0227`) on a monomorphized program, whose instances
/// are ordinary declarations: `Wrap<Shape>` may contain itself where `Wrap<T>` does not.
pub fn type_cycles(p: &Program) -> Vec<Diagnostic> {
    let mut c = Checker::new();
    c.struct_decls(p);
    c.errors.retain(|d| d.code == Code::BadFieldType);
    c.errors.sort_by_key(|d| (d.span.start, d.code.as_str()));
    c.errors
}

/// Checks the program and returns what inference chose for its generic code.
pub fn check_infer(p: &Program, require_main: bool) -> (Vec<Diagnostic>, Inferred) {
    let mut c = Checker::new();
    c.struct_decls(p);
    c.signatures(p, require_main);
    c.conformance(p);
    c.conditional_safety(p);
    for f in &p.fns {
        c.function(f);
    }
    c.errors.sort_by_key(|d| (d.span.start, d.code.as_str()));
    (c.errors, c.inferred)
}

impl Checker {
    fn new() -> Self {
        Checker {
            fns: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            gparams: HashMap::new(),
            declared: HashSet::new(),
            ifaces: HashMap::new(),
            impls: HashMap::new(),
            bounds: HashMap::new(),
            method_fns: HashMap::new(),
            hint: None,
            inferred: Inferred::default(),
            unconditional: HashMap::new(),
            vars: Vec::new(),
            scopes: Vec::new(),
            bit_ifs: Vec::new(),
            held: Vec::new(),
            loops: Vec::new(),
            ret: Ty::Void,
            errors: Vec::new(),
        }
    }

    fn err(&mut self, code: Code, span: Span, message: String) -> &mut Diagnostic {
        self.errors.push(Diagnostic::new(code, span, message));
        self.errors.last_mut().unwrap()
    }

    // Struct names are unique, fields are unique and of build-time classical types, and no
    // struct contains itself (directly or through arrays): there is no `null` to end a chain.
    fn struct_decls(&mut self, p: &Program) {
        let types: HashSet<&str> = p
            .structs
            .iter()
            .map(|s| s.name.name.as_str())
            .chain(p.enums.iter().map(|e| e.name.name.as_str()))
            .chain(p.ifaces.iter().map(|i| i.name.name.as_str()))
            .collect();
        let decls = p
            .structs
            .iter()
            .map(|s| (Ty::struct_named(&s.name.name), &s.params))
            .chain(
                p.enums
                    .iter()
                    .map(|e| (Ty::enum_named(&e.name.name), &e.params)),
            )
            .chain(
                p.ifaces
                    .iter()
                    .map(|i| (Ty::iface_named(&i.name.name), &i.params)),
            );
        for (ty, params) in decls {
            self.declared.insert(ty);
            if !params.is_empty() {
                let tys = self.type_params(params, &types);
                self.gparams.entry(ty).or_insert(tys);
            }
        }
        // Structs and enums share one namespace of type names.
        let mut names = HashSet::new();
        for s in &p.structs {
            let ty = Ty::struct_named(&s.name.name);
            if !names.insert(s.name.name.as_str()) {
                self.err(
                    Code::Duplicate,
                    s.name.span,
                    msg::duplicate_def(&s.name.name),
                );
                continue;
            }
            let mut order = Vec::new();
            let mut by_name: HashMap<String, Ty> = HashMap::new();
            for f in &s.fields {
                if by_name.contains_key(&f.name.name) {
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
                let ty = if !inner.is_element() {
                    self.err(
                        Code::BadFieldType,
                        f.ty_span,
                        msg::bad_field_type(inner.name()),
                    );
                    Ty::Error
                } else if self.generic_ok(f.ty, f.ty_span) {
                    f.ty
                } else {
                    Ty::Error
                };
                order.push(ty);
                by_name.insert(f.name.name.clone(), ty);
            }
            self.structs.insert(ty, (order, by_name));
        }
        for e in &p.enums {
            if !names.insert(e.name.name.as_str()) {
                self.err(
                    Code::Duplicate,
                    e.name.span,
                    msg::duplicate_def(&e.name.name),
                );
                continue;
            }
            let mut variants: Vec<(String, Vec<Ty>)> = Vec::new();
            for v in &e.variants {
                if variants.iter().any(|(n, _)| *n == v.name.name) {
                    self.err(
                        Code::Duplicate,
                        v.name.span,
                        msg::duplicate_def(&v.name.name),
                    );
                    continue;
                }
                let mut seen = HashSet::new();
                let mut tys = Vec::new();
                for f in &v.fields {
                    if !seen.insert(f.name.name.as_str()) {
                        self.err(
                            Code::Duplicate,
                            f.name.span,
                            msg::duplicate_def(&f.name.name),
                        );
                    }
                    let mut inner = f.ty;
                    while let Some(e) = inner.elem() {
                        inner = e;
                    }
                    tys.push(if !inner.is_element() {
                        self.err(
                            Code::BadFieldType,
                            f.ty_span,
                            msg::bad_payload_type(inner.name()),
                        );
                        Ty::Error
                    } else if self.generic_ok(f.ty, f.ty_span) {
                        f.ty
                    } else {
                        Ty::Error
                    });
                }
                variants.push((v.name.name.clone(), tys));
            }
            self.enums.insert(Ty::enum_named(&e.name.name), variants);
        }
        self.iface_decls(p, names);
        self.struct_cycles(p);
    }

    // Interface names share the namespace of struct and enum names; each interface's methods
    // are distinct and of valid types. Then the interfaces each struct and enum claims to
    // implement (`Q0232` for a type that is not an interface).
    fn iface_decls<'p>(&mut self, p: &'p Program, mut names: HashSet<&'p str>) {
        for d in &p.ifaces {
            if !names.insert(d.name.name.as_str()) {
                self.err(
                    Code::Duplicate,
                    d.name.span,
                    msg::duplicate_def(&d.name.name),
                );
                continue;
            }
            let mut methods: Vec<IfaceMethod> = Vec::new();
            for m in &d.methods {
                if methods.iter().any(|x| x.name == m.name.name) {
                    self.err(
                        Code::Duplicate,
                        m.name.span,
                        msg::duplicate_def(&m.name.name),
                    );
                    continue;
                }
                let mut seen = HashSet::new();
                for q in &m.params {
                    if !seen.insert(q.name.name.as_str()) {
                        self.err(
                            Code::Duplicate,
                            q.name.span,
                            msg::duplicate_def(&q.name.name),
                        );
                    }
                    if q.ty == Ty::Void {
                        self.err(Code::TypeMismatch, q.ty_span, msg::void_variable());
                    } else {
                        self.array_ty_ok(q.ty, q.ty_span);
                    }
                    if let Some(at) = q.consume {
                        if !q.ty.is_quantum() {
                            self.err(
                                Code::ConsumeNonQuantum,
                                at,
                                msg::consume_non_quantum(q.ty.name()),
                            );
                        }
                    }
                }
                self.array_ty_ok(m.ret, m.ret_span);
                methods.push(IfaceMethod {
                    name: m.name.name.clone(),
                    params: m
                        .params
                        .iter()
                        .map(|q| (q.ty, q.consume.is_some() && q.ty.is_quantum()))
                        .collect(),
                    ret: m.ret,
                });
            }
            self.ifaces.insert(Ty::iface_named(&d.name.name), methods);
        }
        let owners = p
            .structs
            .iter()
            .map(|s| (Ty::struct_named(&s.name.name), &s.implements))
            .chain(
                p.enums
                    .iter()
                    .map(|e| (Ty::enum_named(&e.name.name), &e.implements)),
            );
        for (ty, list) in owners {
            let mut out: Vec<Ty> = Vec::new();
            for &(i, at) in list {
                if !matches!(i, Ty::Iface(_)) {
                    self.err(Code::Interface, at, msg::not_an_interface(i.name()));
                    continue;
                }
                if !self.generic_ok(i, at) {
                    continue;
                }
                if out.contains(&i) {
                    self.err(Code::Duplicate, at, msg::duplicate_def(i.name()));
                    continue;
                }
                out.push(i);
            }
            self.impls.entry(ty).or_insert(out);
        }
    }

    // The methods an interface (or interface instance) requires, its type arguments substituted.
    fn iface_methods(&self, t: Ty) -> Option<Vec<IfaceMethod>> {
        if let Some(ms) = self.ifaces.get(&t) {
            return Some(ms.clone());
        }
        let (base, map) = self.inst(t)?;
        let sub = |t: Ty| t.subst(&|p| map.get(&p).copied());
        Some(
            self.ifaces
                .get(&base)?
                .iter()
                .map(|m| IfaceMethod {
                    name: m.name.clone(),
                    params: m.params.iter().map(|&(t, c)| (sub(t), c)).collect(),
                    ret: sub(m.ret),
                })
                .collect(),
        )
    }

    // Whether a value of type `t` may be used where the interface `want` is expected: `t` is
    // that interface, a struct or enum that implements it, or a type parameter bounded by it.
    fn implements(&self, t: Ty, want: Ty) -> bool {
        if t == want {
            return true;
        }
        match t {
            Ty::Param(_) => self.bounds.get(&t) == Some(&want),
            Ty::Struct(_) | Ty::Enum(_) => {
                if let Some(list) = self.impls.get(&t) {
                    return list.contains(&want);
                }
                let Some((base, map)) = self.inst(t) else {
                    return false;
                };
                self.impls.get(&base).is_some_and(|list| {
                    list.iter()
                        .any(|i| i.subst(&|p| map.get(&p).copied()) == want)
                })
            }
            _ => false,
        }
    }

    // Whether `new T[n]` can fill elements of type `t`: interface values have no default, and
    // neither has a struct with such a field or an enum whose first variant holds one.
    fn has_default(&self, t: Ty, depth: usize) -> bool {
        // Defaults never form cycles (`Q0227`); the cap only bounds the search after errors.
        if depth > 256 {
            return true;
        }
        match t {
            Ty::Iface(_) => false,
            Ty::Struct(_) => self
                .field_types(t)
                .is_none_or(|fs| fs.iter().all(|&f| self.has_default(f, depth + 1))),
            Ty::Enum(_) => self.variants_of(t).is_none_or(|vs| {
                vs.first()
                    .is_none_or(|(_, fs)| fs.iter().all(|&f| self.has_default(f, depth + 1)))
            }),
            _ => true,
        }
    }

    // Every struct and enum has each method of the interfaces it implements, with the same
    // parameter types, `consume` markers and return type (`Q0232`).
    fn conformance(&mut self, p: &Program) {
        let owners = p
            .structs
            .iter()
            .map(|s| (Ty::struct_named(&s.name.name), &s.name.name, &s.implements))
            .chain(
                p.enums
                    .iter()
                    .map(|e| (Ty::enum_named(&e.name.name), &e.name.name, &e.implements)),
            );
        for (ty, name, list) in owners {
            let valid = self.impls.get(&ty).cloned().unwrap_or_default();
            for &(i, at) in list {
                if !valid.contains(&i) {
                    continue;
                }
                for m in self.iface_methods(i).unwrap_or_default() {
                    // A monomorphized instance `Box<int>` has the method `Box.get<int>`.
                    let fname = match ty.generic_args() {
                        Some((base, args)) => {
                            let args: Vec<&str> = args.iter().map(|a| a.name()).collect();
                            format!("{}.{}<{}>", base.name(), m.name, args.join(", "))
                        }
                        None => format!("{name}.{}", m.name),
                    };
                    let Some(sig) = self.fns.get(&fname) else {
                        self.err(
                            Code::Interface,
                            at,
                            msg::missing_iface_method(name, i.name(), &m.name),
                        );
                        continue;
                    };
                    if sig.params.get(1..) != Some(&m.params[..]) || sig.ret != m.ret {
                        let decl = p
                            .fns
                            .iter()
                            .find(|f| f.name.name == fname)
                            .map_or(at, |f| f.name.span);
                        self.err(
                            Code::Interface,
                            decl,
                            msg::iface_method_mismatch(&m.name, i.name(), &m.text()),
                        );
                    }
                }
            }
        }
    }

    // Two checks over "contains" edges between the structs and enums (nodes: the structs, then
    // the enums), each edge a field, through arrays or not:
    // - default values: struct fields and the payload of each enum's first variant form no cycle
    //   (one iterative depth-first search; each edge that closes a cycle is reported);
    // - shared values: a cycle through any variant may only link enums directly. Structs and
    //   arrays are mutable, so a cycle through one would let a value refer to itself (a reference
    //   cycle never freed). One error per such strongly connected component.
    fn struct_cycles(&mut self, p: &Program) {
        let ns = p.structs.len();
        // Interfaces are nodes too (after the structs and enums), linked to every type that
        // implements them: a struct can reach itself through an interface-typed field.
        let typed = ns + p.enums.len();
        let ids: HashMap<Ty, usize> = p
            .structs
            .iter()
            .map(|s| Ty::struct_named(&s.name.name))
            .chain(p.enums.iter().map(|e| Ty::enum_named(&e.name.name)))
            .chain(p.ifaces.iter().map(|i| Ty::iface_named(&i.name.name)))
            .enumerate()
            .map(|(i, t)| (t, i))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        // (target, field span, through an array). An instance `Box<S>` links to its generic type
        // and, conservatively, to each type argument as if through an array (a mutable path).
        let edge = |f: &Field| {
            let mut out = Vec::new();
            let mut stack = vec![(f.ty, false)];
            while let Some((mut t, mut array)) = stack.pop() {
                while let Some(e) = t.elem() {
                    t = e;
                    array = true;
                }
                // A monomorphized program declares its instances: they are nodes themselves.
                if let Some(&v) = ids.get(&t) {
                    out.push((v, f.ty_span, array));
                    continue;
                }
                if let Some((base, args)) = t.generic_args() {
                    t = base;
                    stack.extend(args.into_iter().map(|a| (a, true)));
                }
                if let Some(&v) = ids.get(&t) {
                    out.push((v, f.ty_span, array));
                }
            }
            out
        };
        let defaults = p.structs.iter().map(|s| s.fields.as_slice()).chain(
            p.enums
                .iter()
                .map(|e| e.variants.first().map_or(&[][..], |v| v.fields.as_slice())),
        );
        // Interface values have no default, so defaults never lead through one.
        let edges: Vec<Vec<(usize, Span)>> = defaults
            .map(|fs| {
                fs.iter()
                    .flat_map(edge)
                    .filter(|&(v, _, _)| v < typed)
                    .map(|(v, s, _)| (v, s))
                    .collect()
            })
            .collect();
        let reported = self.default_cycles(p, &edges);
        let all: Vec<Vec<(usize, Span, bool)>> = p
            .structs
            .iter()
            .map(|s| {
                s.fields
                    .iter()
                    .flat_map(edge)
                    .map(|(v, s, _)| (v, s, true))
                    .collect()
            })
            .chain(p.enums.iter().map(|e| {
                e.variants
                    .iter()
                    .flat_map(|v| v.fields.iter().flat_map(edge))
                    .collect()
            }))
            .chain(p.ifaces.iter().map(|d| {
                let iface = Ty::iface_named(&d.name.name);
                let owners = p
                    .structs
                    .iter()
                    .map(|s| &s.implements)
                    .chain(p.enums.iter().map(|e| &e.implements));
                let mut out = Vec::new();
                for (u, list) in owners.enumerate() {
                    for &(i, at) in list {
                        if i.generic_args().map_or(i, |(b, _)| b) == iface {
                            out.push((u, at, false));
                        }
                    }
                }
                out
            }))
            .collect();
        let comp = components(&all);
        let done: HashSet<usize> = reported.iter().map(|&u| comp[u]).collect();
        // The last mutable edge inside each component, in declaration order.
        let mut last: HashMap<usize, (usize, Span)> = HashMap::new();
        for (u, es) in all.iter().enumerate() {
            for &(v, span, mutable) in es {
                if mutable && comp[u] == comp[v] && !done.contains(&comp[u]) {
                    last.insert(comp[u], (u, span));
                }
            }
        }
        let mut found: Vec<(usize, Span)> = last.into_values().collect();
        found.sort_by_key(|&(_, s)| s.start);
        for (u, span) in found {
            let message = match p.structs.get(u) {
                Some(s) => msg::recursive_struct(&s.name.name),
                None => msg::recursive_shared(&p.enums[u - ns].name.name),
            };
            self.err(Code::BadFieldType, span, message);
        }
    }

    // Returns the nodes whose field closed a reported cycle.
    fn default_cycles(&mut self, p: &Program, edges: &[Vec<(usize, Span)>]) -> Vec<usize> {
        let ns = p.structs.len();
        let mut reported = Vec::new();
        // 0: not visited, 1: on the current path, 2: done.
        let mut color = vec![0u8; edges.len()];
        for start in 0..edges.len() {
            if color[start] != 0 {
                continue;
            }
            color[start] = 1;
            let mut stack = vec![(start, 0usize)];
            while let Some(top) = stack.last_mut() {
                let (u, k) = *top;
                let Some(&(v, span)) = edges[u].get(k) else {
                    color[u] = 2;
                    stack.pop();
                    continue;
                };
                top.1 += 1;
                match color[v] {
                    0 => {
                        color[v] = 1;
                        stack.push((v, 0));
                    }
                    1 => {
                        let message = match p.structs.get(u) {
                            Some(s) => msg::recursive_struct(&s.name.name),
                            None => msg::recursive_enum(&p.enums[u - ns].name.name),
                        };
                        self.err(Code::BadFieldType, span, message);
                        reported.push(u);
                    }
                    _ => {}
                }
            }
        }
        reported
    }

    fn signatures(&mut self, p: &Program, require_main: bool) {
        let type_names: HashSet<&str> = p
            .structs
            .iter()
            .map(|s| s.name.name.as_str())
            .chain(p.enums.iter().map(|e| e.name.name.as_str()))
            .chain(p.ifaces.iter().map(|i| i.name.name.as_str()))
            .collect();
        let mut main = None;
        for f in &p.fns {
            let name = &f.name.name;
            // Functions of imported modules carry qualified names such as `bell_utils.prepare`.
            let base = name.rsplit('.').next().unwrap_or(name);
            // Methods are named through their receiver, so any method name is free.
            let method = f.method.is_some();
            if !method && BUILTINS.contains(&base) {
                self.err(
                    Code::BuiltinRedefined,
                    f.name.span,
                    msg::builtin_redefined(base),
                );
                continue;
            }
            if !method && constant(base).is_some() {
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
            if let (Some(m), Some(this)) = (&f.method, f.params.first()) {
                // A method and a field of one struct cannot share a name.
                if self.field_named(this.ty, m).is_some() {
                    self.err(Code::Duplicate, f.name.span, msg::duplicate_def(m));
                }
                self.method_fns
                    .entry(m.clone())
                    .or_default()
                    .push(name.clone());
            }
            let mut bounds = Vec::with_capacity(f.tparams.len());
            for b in &f.bounds {
                bounds.push(match *b {
                    Some((t, at)) if !matches!(t, Ty::Iface(_)) => {
                        self.err(Code::Interface, at, msg::not_an_interface(t.name()));
                        None
                    }
                    Some((t, at)) => self.generic_ok(t, at).then_some(t),
                    None => None,
                });
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
            let tparams = self.type_params(&f.tparams, &type_names);
            // Type arguments are inferred from the arguments only.
            for (p, t) in f.tparams.iter().zip(&tparams) {
                if !f.params.iter().any(|q| mentions(q.ty, *t)) {
                    self.err(
                        Code::TypeArgs,
                        p.span,
                        msg::unused_type_param(&p.name, base),
                    );
                }
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
                    tparams,
                    bounds,
                },
            );
        }
        match main {
            None if !require_main => {}
            None => {
                let at = p.fns.first().map_or(Span::default(), |f| f.name.span);
                self.err(Code::BadMain, at, msg::missing_main());
            }
            // `void main()`, `int main()` (the exit code), optionally with `String[] args`.
            Some(f)
                if !matches!(f.ret, Ty::Void | Ty::Int)
                    || f.params.len() > 1
                    || f.params.first().is_some_and(|p| {
                        p.ty != Ty::array_of(Ty::String) || p.consume.is_some()
                    }) =>
            {
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
        // A method call `.m` stands for a call of every method named `m`.
        let mut by_method: HashMap<&str, Vec<&str>> = HashMap::new();
        for f in &p.fns {
            if let Some(m) = &f.method {
                by_method.entry(m).or_default().push(&f.name.name);
            }
        }
        for (_, calls) in &mut edges {
            let all = std::mem::take(calls);
            for (callee, at) in all {
                match callee.strip_prefix('.') {
                    Some(m) => {
                        for f in by_method.get(m).into_iter().flatten() {
                            calls.push((f.to_string(), at));
                        }
                    }
                    None => calls.push((callee, at)),
                }
            }
        }
        propagate(&edges, &mut self.unconditional);
    }

    fn function(&mut self, f: &FnDecl) {
        self.vars.clear();
        self.loops.clear();
        self.scopes = vec![Vec::new()];
        self.ret = f.ret;
        self.bounds.clear();
        if let Some(sig) = self.fns.get(&f.name.name) {
            for (p, b) in sig.tparams.iter().zip(&sig.bounds) {
                if let Some(b) = b {
                    self.bounds.insert(*p, *b);
                }
            }
        }
        for (i, p) in f.params.iter().enumerate() {
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
            // A method's `this` cannot be reassigned.
            self.declare(&p.name, ty, i > 0 || f.method.is_none());
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
                self.hint = *ty;
                let found = self.let_init(init);
                self.hint = None;
                let var_ty = match *ty {
                    // `var` infers classical types only; quantum and per-shot values need a type.
                    None => match found {
                        Ty::Int
                        | Ty::Double
                        | Ty::Boolean
                        | Ty::String
                        | Ty::Array(_)
                        | Ty::Struct(_)
                        | Ty::Enum(_)
                        | Ty::Param(_)
                        | Ty::Iface(_)
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
                self.hint = self.lookup(&name.name).map(|id| self.vars[id].ty);
                let found = self.expr(value);
                self.hint = None;
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
            Stmt::Match {
                scrutinee,
                arms,
                span,
            } => self.match_stmt(scrutinee, arms, *span),
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
                    Some(e) => self.expr_hint(e, want),
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
            || (matches!(want, Ty::Iface(_)) && self.implements(found, want))
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

    // `expr` with an expected type (only the outermost expression sees it).
    fn expr_hint(&mut self, e: &Expr, hint: Ty) -> Ty {
        self.hint = Some(hint);
        let t = self.expr(e);
        self.hint = None;
        t
    }

    // A value of a type parameter can only be moved; any other operation is `Q0230`.
    fn param_op(&mut self, at: Span, tys: &[Ty]) -> bool {
        let Some(p) = tys.iter().find(|t| matches!(t, Ty::Param(_))) else {
            return false;
        };
        self.err(Code::TypeArgs, at, msg::type_param_op(p.name()));
        true
    }

    fn expr(&mut self, e: &Expr) -> Ty {
        let hint = self.hint.take();
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
                if !self.array_ty_ok(*ty, e.span) {
                    return Ty::Error;
                }
                let mut elem = *ty;
                for _ in 0..dims.len() {
                    elem = elem.elem().unwrap_or(Ty::Error);
                }
                if !self.has_default(elem, 0) {
                    self.err(Code::BadArrayElement, e.span, msg::no_default(elem.name()))
                        .help = Some(msg::help_no_default());
                    return Ty::Error;
                }
                *ty
            }
            ExprKind::ArrayLit { ty, elems } => self.array_lit(*ty, elems, e.span),
            ExprKind::NewStruct { ty, args } => self.new_struct(*ty, args, e.span, hint),
            ExprKind::Variant { ty, name, args } => self.variant(*ty, name, args, e.span, hint),
            ExprKind::Field { recv, name } => {
                let t = self.expr(recv);
                self.field_ty(t, name)
            }
            ExprKind::Try { expr, op } => self.try_expr(expr, *op),
            ExprKind::Method { recv, name, args } => {
                let t = self.expr(recv);
                match t {
                    Ty::Struct(_) | Ty::Enum(_) => {
                        return self.user_method(t, recv.span, name, args, e.span)
                    }
                    Ty::Iface(_) => {
                        let how = MethodCall::Dynamic;
                        return self.iface_method(t, t, recv.span, name, args, e.span, how);
                    }
                    Ty::Param(_) => {
                        if let Some(&b) = self.bounds.get(&t) {
                            let how = MethodCall::Bound(t);
                            return self.iface_method(t, b, recv.span, name, args, e.span, how);
                        }
                    }
                    _ => {}
                }
                if self.param_op(recv.span, &[t]) {
                    for a in args {
                        self.expr(a);
                    }
                    return Ty::Error;
                }
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
                if self.param_op(expr.span, &[t]) {
                    return Ty::Error;
                }
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
                if self.param_op(expr.span, &[t]) {
                    return Ty::Error;
                }
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
                if self.param_op(e.span, &[l, r]) {
                    return Ty::Error;
                }
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
        self.generic_ok(t, span)
    }

    // Type arguments of a generic type (the right number, of classical types), and no generic
    // type without them; reported once at the type (`Q0230`).
    fn generic_ok(&mut self, t: Ty, span: Span) -> bool {
        let mut t = t;
        while let Some(e) = t.elem() {
            t = e;
        }
        // A monomorphized program declares its instances as ordinary types.
        if self.declared.contains(&t) && !self.gparams.contains_key(&t) {
            return true;
        }
        match t.generic_args() {
            Some((base, args)) => {
                let want = self.gparams.get(&base).map_or(0, Vec::len);
                if want != args.len() {
                    self.err(
                        Code::TypeArgs,
                        span,
                        msg::type_args_count(base.name(), want, args.len()),
                    );
                    return false;
                }
                args.into_iter().all(|a| self.type_arg_ok(a, span))
            }
            None if self.gparams.contains_key(&t) => {
                self.err(Code::TypeArgs, span, msg::needs_type_args(t.name()))
                    .help = Some(msg::help_needs_type_args(t.name()));
                false
            }
            None => true,
        }
    }

    fn type_arg_ok(&mut self, a: Ty, span: Span) -> bool {
        let mut inner = a;
        while let Some(e) = inner.elem() {
            inner = e;
        }
        if !inner.is_element() {
            self.err(Code::TypeArgs, span, msg::bad_type_arg(a.name()));
            return false;
        }
        self.generic_ok(a, span)
    }

    // The type parameters of a generic declaration: distinct, and not named like a type.
    fn type_params(&mut self, params: &[Ident], types: &HashSet<&str>) -> Vec<Ty> {
        let mut seen = HashSet::new();
        for p in params {
            if !seen.insert(p.name.as_str()) || types.contains(p.name.as_str()) {
                self.err(Code::Duplicate, p.span, msg::duplicate_def(&p.name));
            }
        }
        params.iter().map(|p| Ty::param_named(&p.name)).collect()
    }

    // An instance's generic type and the substitution of its type arguments.
    fn inst(&self, t: Ty) -> Option<(Ty, HashMap<Ty, Ty>)> {
        let (base, args) = t.generic_args()?;
        let params = self.gparams.get(&base)?;
        Some((base, params.iter().copied().zip(args).collect()))
    }

    // Field types of a struct (or struct instance) in declaration order.
    fn field_types(&self, t: Ty) -> Option<Vec<Ty>> {
        if let Some((order, _)) = self.structs.get(&t) {
            return Some(order.clone());
        }
        let (base, map) = self.inst(t)?;
        let (order, _) = self.structs.get(&base)?;
        Some(
            order
                .iter()
                .map(|f| f.subst(&|p| map.get(&p).copied()))
                .collect(),
        )
    }

    fn field_named(&self, t: Ty, name: &str) -> Option<Ty> {
        if let Some((_, by_name)) = self.structs.get(&t) {
            return by_name.get(name).copied();
        }
        let (base, map) = self.inst(t)?;
        let f = *self.structs.get(&base)?.1.get(name)?;
        Some(f.subst(&|p| map.get(&p).copied()))
    }

    // Variants of an enum (or enum instance): (name, payload types).
    fn variants_of(&self, t: Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        if let Some(vs) = self.enums.get(&t) {
            return Some(vs.clone());
        }
        let (base, map) = self.inst(t)?;
        let vs = self.enums.get(&base)?;
        Some(
            vs.iter()
                .map(|(n, tys)| {
                    let tys = tys.iter().map(|f| f.subst(&|p| map.get(&p).copied()));
                    (n.clone(), tys.collect())
                })
                .collect(),
        )
    }

    // `new Pair(1, "a")` or `Option.Some(5)` of a generic type `base`: the type arguments come
    // from the expected type, then from the values in order (`fields` mention the parameters).
    // A value whose field type is already known gets it as its own expected type, so
    // `Tree.Node(Tree.Leaf, 1, Tree.Leaf)` works inside out.
    fn infer(
        &mut self,
        base: Ty,
        fields: &[Ty],
        args: &[Expr],
        span: Span,
        hint: Option<Ty>,
    ) -> Ty {
        let params = self.gparams.get(&base).cloned().unwrap_or_default();
        let mut map = HashMap::new();
        if let Some((hb, ha)) = hint.and_then(Ty::generic_args) {
            if hb == base {
                // The declared type is reported where it is written.
                if !ha.iter().all(|&a| shape_ok(a)) {
                    for a in args {
                        self.expr(a);
                    }
                    return Ty::Error;
                }
                map.extend(params.iter().copied().zip(ha));
            }
        }
        let mut found = Vec::with_capacity(args.len());
        for (f, a) in fields.iter().zip(args) {
            let known = f.subst(&|p| map.get(&p).copied());
            let t = if known.has_param() {
                self.expr(a)
            } else {
                self.expr_hint(a, known)
            };
            unify(*f, t, &mut map);
            found.push(t);
        }
        let targs: Option<Vec<Ty>> = params.iter().map(|p| map.get(p).copied()).collect();
        let Some(targs) = targs else {
            if !found.contains(&Ty::Error) {
                self.err(Code::TypeArgs, span, msg::cannot_infer(base.name()))
                    .help = Some(msg::help_cannot_infer(base.name()));
            }
            return Ty::Error;
        };
        if !targs.iter().all(|&a| self.type_arg_ok(a, span)) {
            return Ty::Error;
        }
        for ((f, t), a) in fields.iter().zip(found).zip(args) {
            self.expect_ty(a.span, f.subst(&|p| map.get(&p).copied()), t);
        }
        let inst = Ty::instance(base, &targs);
        self.inferred.types.insert(key(span), inst);
        inst
    }

    // `new Point(1.0, 2.0)`: the field values in declaration order.
    fn new_struct(&mut self, ty: Ty, args: &[Expr], span: Span, hint: Option<Ty>) -> Ty {
        let generic = self.gparams.contains_key(&ty);
        let fields = if generic {
            self.structs.get(&ty).map(|(order, _)| order.clone())
        } else if ty.generic_args().is_some() && !self.generic_ok(ty, span) {
            None
        } else {
            self.field_types(ty)
        };
        let Some(fields) = fields.clone().filter(|f| f.len() == args.len()) else {
            if let Some(n) = fields.map(|f| f.len()) {
                self.err(Code::Arity, span, msg::arity(ty.name(), n, args.len()));
            }
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        if generic {
            return self.infer(ty, &fields, args, span, hint);
        }
        for (a, want) in args.iter().zip(fields) {
            let found = self.expr_hint(a, want);
            self.expect_ty(a.span, want, found);
        }
        ty
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
            if let Some(want) = elem {
                let found = self.expr_hint(e, want);
                self.expect_ty(e.span, want, found);
            } else {
                self.expr(e);
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
        self.hint = b.elem();
        let v = self.expr(value);
        self.hint = None;
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
        let found = self.field_named(t, &name.name);
        found.unwrap_or_else(|| {
            self.err(
                Code::UnknownField,
                name.span,
                msg::unknown_field(&name.name, t.name()),
            );
            Ty::Error
        })
    }

    // `Shape.Circle(r)`: the payload values in declaration order.
    fn variant(&mut self, ty: Ty, name: &Ident, args: &[Expr], span: Span, hint: Option<Ty>) -> Ty {
        let generic = self.gparams.contains_key(&ty);
        let variants = if !generic && ty.generic_args().is_some() && !self.generic_ok(ty, span) {
            None
        } else {
            self.variants_of(ty)
        };
        let found = variants
            .as_ref()
            .and_then(|vs| vs.iter().find(|(n, _)| *n == name.name))
            .map(|(_, tys)| tys.clone());
        let Some(tys) = found else {
            if variants.is_some() {
                self.err(
                    Code::BadPattern,
                    name.span,
                    msg::unknown_variant(&name.name, ty.name()),
                );
            }
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        if tys.len() != args.len() {
            self.err(
                Code::Arity,
                span,
                msg::variant_args(&name.name, tys.len(), args.len()),
            );
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        }
        if generic {
            return self.infer(ty, &tys, args, span, hint);
        }
        for (a, want) in args.iter().zip(tys) {
            let found = self.expr_hint(a, want);
            self.expect_ty(a.span, want, found);
        }
        ty
    }

    // `e?`: the payload of an `Ok` or `Some`, or an early return of the `Err` or `None`. The
    // function must return a `Result` with the same error type (or an `Option`). Like `return`,
    // it is rejected inside an `if` on a measured bit and may not leave an owned qubit live.
    fn try_expr(&mut self, inner: &Expr, op: Span) -> Ty {
        let t = self.expr(inner);
        if t == Ty::Error {
            return Ty::Error;
        }
        let Some((kind, ok, err)) = self.try_shape(t) else {
            self.err(Code::BadTry, op, msg::try_operand(t.name()));
            return Ty::Error;
        };
        if let Some(&at) = self.bit_ifs.last() {
            self.err(Code::ForbiddenInBitIf, op, msg::forbidden_in_bit_if("?"))
                .notes
                .push(note(at, msg::note_bit_if_here()));
            return ok;
        }
        let ret = self.ret;
        let fits = match self.try_shape(ret) {
            Some((k, _, e)) => k == kind && e == err,
            None => false,
        };
        if ret != Ty::Error && !fits {
            self.err(Code::BadTry, op, msg::try_return(t.name(), ret.name()));
            return ok;
        }
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
                .push(note(op, msg::note_try_returns_here()));
        }
        ok
    }

    // The prelude shapes `?` works on: `Result<T, E>` gives ("Result", T, Some(E)) and
    // `Option<T>` gives ("Option", T, None).
    fn try_shape(&self, t: Ty) -> Option<(&'static str, Ty, Option<Ty>)> {
        let (base, _) = t.generic_args()?;
        let vs = self.variants_of(t)?;
        let shape: Vec<(&str, usize)> = vs.iter().map(|(n, f)| (n.as_str(), f.len())).collect();
        match (base.name(), shape.as_slice()) {
            ("Result", [("Ok", 1), ("Err", 1)]) => Some(("Result", vs[0].1[0], Some(vs[1].1[0]))),
            ("Option", [("None", 0), ("Some", 1)]) => Some(("Option", vs[1].1[0], None)),
            _ => None,
        }
    }

    // Arms run like the branches of an `if`/`else if` chain: qubit states merge the same way,
    // and the statement diverges only if it is exhaustive and every arm diverges.
    fn match_stmt(&mut self, scrutinee: &Expr, arms: &[Arm], span: Span) -> bool {
        let t = self.expr(scrutinee);
        let kind_ok = matches!(
            t,
            Ty::Enum(_) | Ty::Int | Ty::String | Ty::Boolean | Ty::Error
        );
        if !kind_ok {
            self.err(
                Code::BadPattern,
                scrutinee.span,
                msg::bad_scrutinee(t.name()),
            );
        }
        let variants = self.variants_of(t).unwrap_or_default();
        // A pattern may name the enum: `Option.Some(x)` for an `Option<int>`.
        let enum_name = t.generic_args().map_or(t, |(b, _)| b).name();
        let mut covered = vec![false; variants.len()];
        let mut ints = HashSet::new();
        let mut texts = HashSet::new();
        let mut bools = [false; 2];
        let mut wild = false;
        let before = self.states();
        // States after each arm that falls through.
        let mut ends: Vec<Vec<State>> = Vec::new();
        let mut first_end: Option<Vec<State>> = None;
        for arm in arms {
            let p = &arm.pat;
            let unreachable = wild;
            let mut binds: Vec<(&Ident, Ty)> = Vec::new();
            let mut fits = kind_ok && t != Ty::Error;
            let dup = match &p.kind {
                PatKind::Wild => {
                    wild = true;
                    false
                }
                PatKind::Int(n) if matches!(t, Ty::Int | Ty::Error) => !ints.insert(*n),
                PatKind::Bool(b) if matches!(t, Ty::Boolean | Ty::Error) => {
                    std::mem::replace(&mut bools[usize::from(*b)], true)
                }
                PatKind::Str(pieces) if matches!(t, Ty::String | Ty::Error) => {
                    if pieces.contains(&FmtPiece::Hole) {
                        self.err(Code::StringNotAllowed, p.span, msg::string_not_allowed());
                        false
                    } else {
                        !texts.insert(format!("{pieces:?}"))
                    }
                }
                PatKind::Variant {
                    qual,
                    name,
                    binds: names,
                } if matches!(t, Ty::Enum(_)) => {
                    let qual_ok = qual.as_ref().is_none_or(|q| q.name == enum_name);
                    match variants.iter().position(|(n, _)| *n == name.name) {
                        Some(tag) if qual_ok => {
                            let tys = &variants[tag].1;
                            if tys.len() != names.len() {
                                self.err(
                                    Code::BadPattern,
                                    p.span,
                                    msg::pattern_arity(&name.name, tys.len(), names.len()),
                                );
                            }
                            for (i, b) in names.iter().enumerate() {
                                binds.push((b, tys.get(i).copied().unwrap_or(Ty::Error)));
                            }
                            std::mem::replace(&mut covered[tag], true)
                        }
                        Some(_) => {
                            self.err(Code::BadPattern, p.span, msg::pattern_mismatch(t.name()));
                            fits = false;
                            false
                        }
                        None => {
                            if qual_ok {
                                self.err(
                                    Code::BadPattern,
                                    name.span,
                                    msg::unknown_variant(&name.name, t.name()),
                                );
                            } else {
                                self.err(Code::BadPattern, p.span, msg::pattern_mismatch(t.name()));
                            }
                            fits = false;
                            false
                        }
                    }
                }
                PatKind::Variant { binds: names, .. } => {
                    if fits {
                        self.err(Code::BadPattern, p.span, msg::pattern_mismatch(t.name()));
                    }
                    for b in names {
                        binds.push((b, Ty::Error));
                    }
                    fits = false;
                    false
                }
                _ => {
                    if fits {
                        self.err(Code::BadPattern, p.span, msg::pattern_mismatch(t.name()));
                    }
                    fits = false;
                    false
                }
            };
            if fits && (unreachable || dup) {
                self.err(Code::BadPattern, p.span, msg::unreachable_arm());
            }
            self.restore(&before);
            self.scopes.push(Vec::new());
            for (b, ty) in binds {
                if b.name == "_" {
                    continue;
                }
                if self
                    .scopes
                    .last()
                    .unwrap()
                    .iter()
                    .any(|&id| self.vars[id].name == b.name)
                {
                    self.err(Code::Duplicate, b.span, msg::duplicate_def(&b.name));
                }
                self.declare(b, ty, false);
            }
            let diverges = self.block(&arm.body);
            self.scopes.pop();
            let end = self.states();
            if first_end.is_none() {
                first_end = Some(end.clone());
            }
            if !diverges {
                ends.push(end);
            }
        }
        let missing: Vec<String> = match t {
            _ if wild => Vec::new(),
            Ty::Enum(_) => variants
                .iter()
                .zip(&covered)
                .filter(|(_, c)| !**c)
                .map(|((n, _), _)| format!("`{n}`"))
                .collect(),
            Ty::Boolean => [false, true]
                .into_iter()
                .filter(|b| !bools[usize::from(*b)])
                .map(|b| format!("`{b}`"))
                .collect(),
            Ty::Int | Ty::String => vec!["`_`".to_string()],
            _ => Vec::new(),
        };
        let exhaustive = missing.is_empty();
        if !exhaustive {
            self.err(
                Code::NonExhaustive,
                span,
                msg::non_exhaustive(&missing.join(", ")),
            )
            .help = Some(msg::help_non_exhaustive());
            // Values no arm matches fall through unchanged.
            ends.push(before.clone());
        }
        match ends.split_first() {
            None => {
                self.restore(&first_end.unwrap_or(before));
                exhaustive
            }
            Some((first, rest)) => {
                self.restore(first);
                for id in 0..before.len() {
                    if !self.vars[id].ty.is_quantum() {
                        continue;
                    }
                    let a = first[id];
                    for s in rest {
                        let b = s[id];
                        if a == b {
                            continue;
                        }
                        let (at, merged) = match (a, b) {
                            (State::Consumed(x), State::Live) => (x, a),
                            (State::Live, State::Consumed(y)) => (y, b),
                            // Consumed in both arms at different places.
                            _ => continue,
                        };
                        self.vars[id].state = merged;
                        let name = self.vars[id].name.clone();
                        self.err(Code::BranchMismatch, at, msg::branch_mismatch(&name));
                        break;
                    }
                }
                false
            }
        }
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
        self.hint = self.field_named(b, &name.name);
        let v = self.expr(value);
        self.hint = None;
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
        // `try_parse_int` returns the prelude `Result`; a program's own `Result` replaces it.
        if let Some(ok) = pure::try_parse_ok(&name.name) {
            let shaped = self
                .try_shape(ret)
                .is_some_and(|(_, t, e)| t == ok && e == Some(Ty::String));
            if !shaped {
                self.err(
                    Code::BadTry,
                    name.span,
                    msg::try_parse_needs_result(&name.name),
                );
                return Ty::Error;
            }
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

    // `v.m(args)` on a struct or enum: a call of its method function `Type.m` with `v` first.
    fn user_method(&mut self, t: Ty, at: Span, name: &Ident, args: &[Expr], span: Span) -> Ty {
        let base = t.generic_args().map_or(t, |(b, _)| b);
        let fname = format!("{}.{}", base.name(), name.name);
        let Some(sig) = self.fns.get(&fname) else {
            self.err(
                Code::UnknownMethod,
                name.span,
                msg::unknown_method(&name.name, t.name()),
            );
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        let call = Callee {
            params: sig.params.clone(),
            ret: sig.ret,
            tparams: sig.tparams.clone(),
            bounds: sig.bounds.clone(),
            uncond: self.unconditional.get(&fname).cloned(),
        };
        self.inferred
            .methods
            .insert(key(span), MethodCall::Static(fname.clone()));
        self.call_sig(&fname, name, call, Some((t, at)), args, span)
    }

    // `v.m(args)` on an interface value or a value of a type parameter bounded by `iface`.
    #[allow(clippy::too_many_arguments)]
    fn iface_method(
        &mut self,
        t: Ty,
        iface: Ty,
        at: Span,
        name: &Ident,
        args: &[Expr],
        span: Span,
        how: MethodCall,
    ) -> Ty {
        let found = self
            .iface_methods(iface)
            .and_then(|ms| ms.into_iter().find(|m| m.name == name.name));
        let Some(m) = found else {
            self.err(
                Code::UnknownMethod,
                name.span,
                msg::unknown_method(&name.name, t.name()),
            );
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        self.inferred.methods.insert(key(span), how);
        // The call may run any method of that name.
        let uncond = self
            .method_fns
            .get(&name.name)
            .and_then(|fs| fs.iter().find_map(|f| self.unconditional.get(f).cloned()));
        let mut params = vec![(t, false)];
        params.extend(m.params);
        let call = Callee {
            params,
            ret: m.ret,
            tparams: Vec::new(),
            bounds: Vec::new(),
            uncond,
        };
        let label = format!("{}.{}", iface.name(), name.name);
        self.call_sig(&label, name, call, Some((t, at)), args, span)
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
        let call = Callee {
            params: sig.params.clone(),
            ret: sig.ret,
            tparams: sig.tparams.clone(),
            bounds: sig.bounds.clone(),
            uncond: self.unconditional.get(n).cloned(),
        };
        self.call_sig(n, name, call, None, args, span)
    }

    // A call of a function, method or interface method `n`; `recv` is the type and span of an
    // already checked receiver, passed as the first parameter.
    fn call_sig(
        &mut self,
        n: &str,
        name: &Ident,
        call: Callee,
        recv: Option<(Ty, Span)>,
        args: &[Expr],
        span: Span,
    ) -> Ty {
        let Callee {
            params,
            mut ret,
            tparams,
            bounds,
            uncond,
        } = call;
        // Generic calls: type parameters bound from the arguments, then their types checked.
        let mut map = HashMap::new();
        let mut deferred = Vec::new();
        if let Some(&cond) = self.bit_ifs.last() {
            if let Some((at, why)) = uncond {
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
        let skip = usize::from(recv.is_some());
        if let (Some((t, at)), Some(&(want, _))) = (recv, params.first()) {
            if want.has_param() {
                unify(want, t, &mut map);
                deferred.push((at, want, t));
            } else {
                self.expect_ty(at, want, t);
            }
        }
        let want_args = params.len().saturating_sub(skip);
        if self.arity(name, args, want_args, span) {
            // Quantum arguments already passed, as (argument index, place).
            let mut passed: Vec<(usize, (usize, Idx))> = Vec::new();
            for (j, (a, (want, consume))) in args
                .iter()
                .zip(params.iter().skip(skip).copied())
                .enumerate()
            {
                if !want.is_quantum() {
                    if want.has_param() {
                        let t = self.expr(a);
                        unify(want, t, &mut map);
                        deferred.push((a.span, want, t));
                    } else {
                        let t = self.expr_hint(a, want);
                        self.expect_ty(a.span, want, t);
                    }
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
        if tparams.is_empty() {
            return ret;
        }
        let targs: Option<Vec<Ty>> = tparams.iter().map(|p| map.get(p).copied()).collect();
        let ok = targs
            .as_ref()
            .is_some_and(|ts| ts.iter().all(|&a| self.type_arg_ok(a, span)));
        for (at, want, t) in deferred {
            self.expect_ty(at, want.subst(&|p| map.get(&p).copied()), t);
        }
        match targs {
            Some(ts) if ok => {
                for ((p, b), a) in tparams.iter().zip(&bounds).zip(&ts) {
                    let Some(b) = b else { continue };
                    let want = b.subst(&|q| map.get(&q).copied());
                    if !self.implements(*a, want) {
                        self.err(
                            Code::Interface,
                            span,
                            msg::bound_not_satisfied(a.name(), want.name(), p.name()),
                        );
                    }
                }
                self.inferred.calls.insert(key(span), ts);
                ret.subst(&|p| map.get(&p).copied())
            }
            _ if ret.has_param() => Ty::Error,
            _ => ret,
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
                    if self.param_op(a.span, &[t]) {
                        continue;
                    }
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
// Strongly connected component of every node (Kosaraju, iterative: chains may be long).
fn components(edges: &[Vec<(usize, Span, bool)>]) -> Vec<usize> {
    let n = edges.len();
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![(start, 0usize)];
        while let Some(top) = stack.last_mut() {
            let (u, k) = *top;
            match edges[u].get(k) {
                Some(&(v, _, _)) => {
                    top.1 += 1;
                    if !seen[v] {
                        seen[v] = true;
                        stack.push((v, 0));
                    }
                }
                None => {
                    order.push(u);
                    stack.pop();
                }
            }
        }
    }
    let mut rev = vec![Vec::new(); n];
    for (u, es) in edges.iter().enumerate() {
        for &(v, _, _) in es {
            rev[v].push(u);
        }
    }
    let mut comp = vec![usize::MAX; n];
    for (c, &start) in order.iter().rev().enumerate() {
        if comp[start] != usize::MAX {
            continue;
        }
        comp[start] = c;
        let mut stack = vec![start];
        while let Some(u) = stack.pop() {
            for &v in &rev[u] {
                if comp[v] == usize::MAX {
                    comp[v] = c;
                    stack.push(v);
                }
            }
        }
    }
    comp
}

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
