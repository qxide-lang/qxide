// Elaboration: a tree-walking interpreter that runs `main` once and records one qxide Circuit.
// Assumes the program passed `check`; anything the checker rules out is still reported as a
// diagnostic here instead of panicking.

use std::collections::HashMap;

use qxide::{Bit, Circuit, Qubit};

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::msg;

pub const MAX_CALL_DEPTH: usize = 200;
// Caps allocations so a huge `qreg(n)` fails with a diagnostic instead of aborting on OOM.
pub const MAX_QUBITS: usize = 1 << 20;

// How a `BitReg` is printed: `{}` gives an array, `.to_binary()` and `.as_int()` give numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitsFmt {
    Array,
    Binary,
    Int,
}

#[derive(Debug, Clone)]
pub enum OutPiece {
    Text(String),
    Bit(Bit),
    Bits(Vec<Bit>, BitsFmt),
}

pub struct Elaborated {
    pub circuit: Circuit,
    pub output: Vec<OutPiece>,
    // Every measurement result in order, i.e. the circuit's classical register `c[0..]`.
    pub measured: Vec<Bit>,
    // Physical qubit slots of `circuit` (the library reuses slots freed by `measure`).
    pub slots: usize,
}

#[derive(Debug, Clone)]
enum Value {
    Int(i64),
    Double(f64),
    Bool(bool),
    Bit(Bit),
    Bits(Vec<Bit>),
    ShotView(Vec<Bit>, BitsFmt),
    Qubit(usize),
    QReg(Vec<usize>),
    Unit,
}

enum Flow {
    Next,
    Return(Value),
}

type R<T> = Result<T, Diagnostic>;

// Java-style widening of an `int` into a `double` slot.
fn coerce(v: Value, ty: Ty) -> Value {
    match (v, ty) {
        (Value::Int(n), Ty::Double) => Value::Double(n as f64),
        (v, _) => v,
    }
}

struct Frame {
    // (name, value, declared type)
    scopes: Vec<Vec<(String, Value, Ty)>>,
}

impl Frame {
    fn get(&self, name: &str, at: Span) -> R<Value> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(n, _, _)| n == name)
            .map(|(_, v, _)| v.clone())
            .ok_or_else(|| Diagnostic::new(Code::UnknownVar, at, msg::unknown_var(name)))
    }

    fn set(&mut self, name: &str, value: Value) {
        if let Some(slot) = self
            .scopes
            .iter_mut()
            .rev()
            .flat_map(|s| s.iter_mut().rev())
            .find(|(n, _, _)| n == name)
        {
            slot.1 = coerce(value, slot.2);
        }
    }

    fn bind(&mut self, name: &str, value: Value, ty: Ty) {
        self.scopes.last_mut().expect("frame has a scope").push((
            name.to_string(),
            coerce(value, ty),
            ty,
        ));
    }
}

struct Interp<'p> {
    fns: HashMap<&'p str, &'p FnDecl>,
    pool: Vec<Option<Qubit>>,
    output: Vec<OutPiece>,
    measured: Vec<Bit>,
    depth: usize,
    // Mirrors Circuit's slot reuse: `measure` frees a slot, the next `qubit()` takes it.
    slots: usize,
    free_slots: usize,
}

pub fn elaborate(p: &Program) -> R<Elaborated> {
    let mut it = Interp {
        fns: p.fns.iter().map(|f| (f.name.name.as_str(), f)).collect(),
        pool: Vec::new(),
        output: Vec::new(),
        measured: Vec::new(),
        depth: 0,
        slots: 0,
        free_slots: 0,
    };
    let main = *it
        .fns
        .get("main")
        .ok_or_else(|| internal(Span::default()))?;
    let mut c = Circuit::new();
    it.call_fn(&mut c, main, Vec::new(), main.name.span)?;
    Ok(Elaborated {
        circuit: c,
        output: it.output,
        measured: it.measured,
        slots: it.slots,
    })
}

fn internal(at: Span) -> Diagnostic {
    // Only reachable if the checker and interpreter disagree.
    Diagnostic::new(Code::Internal, at, msg::internal_error())
}

impl<'p> Interp<'p> {
    fn call_fn(&mut self, c: &mut Circuit, f: &'p FnDecl, args: Vec<Value>, at: Span) -> R<Value> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(Diagnostic::new(
                Code::RecursionLimit,
                at,
                msg::recursion_limit(MAX_CALL_DEPTH),
            ));
        }
        self.depth += 1;
        let mut frame = Frame {
            scopes: vec![f
                .params
                .iter()
                .zip(args)
                .map(|(p, v)| (p.name.name.clone(), coerce(v, p.ty), p.ty))
                .collect()],
        };
        let flow = self.block(c, &mut frame, &f.body);
        self.depth -= 1;
        match flow? {
            Flow::Return(v) => Ok(coerce(v, f.ret)),
            Flow::Next if f.ret == Ty::Void => Ok(Value::Unit),
            Flow::Next => Err(Diagnostic::new(
                Code::NoReturn,
                f.body.close,
                msg::no_return(&f.name.name),
            )),
        }
    }

    fn block(&mut self, c: &mut Circuit, f: &mut Frame, b: &'p Block) -> R<Flow> {
        f.scopes.push(Vec::new());
        let r = self.block_inner(c, f, b);
        f.scopes.pop();
        r
    }

    fn block_inner(&mut self, c: &mut Circuit, f: &mut Frame, b: &'p Block) -> R<Flow> {
        for s in &b.stmts {
            if let Flow::Return(v) = self.stmt(c, f, s)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Next)
    }

    // Stack: a dispatcher like `expr`, so nested blocks only pay for the statement kind they use.
    fn stmt(&mut self, c: &mut Circuit, f: &mut Frame, s: &'p Stmt) -> R<Flow> {
        match s {
            Stmt::Decl { name, init, ty, .. } => self.decl(c, f, name, init, *ty),
            Stmt::Assign { name, value } => self.assign(c, f, name, value),
            Stmt::If(i) => self.if_stmt(c, f, i),
            Stmt::For {
                var,
                start,
                end,
                body,
            } => self.for_stmt(c, f, var, start, end, body),
            Stmt::Return { value, .. } => self.return_stmt(c, f, value.as_ref()),
            Stmt::Expr(e) => self.expr(c, f, e).map(|_| Flow::Next),
        }
    }

    fn decl(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        name: &'p Ident,
        init: &'p Expr,
        ty: Option<Ty>,
    ) -> R<Flow> {
        let v = self.expr(c, f, init)?;
        // `var` is classical only, so the runtime value determines the slot type.
        let ty = ty.unwrap_or(match v {
            Value::Double(_) => Ty::Double,
            Value::Bool(_) => Ty::Boolean,
            _ => Ty::Int,
        });
        f.bind(&name.name, v, ty);
        Ok(Flow::Next)
    }

    fn assign(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        name: &'p Ident,
        value: &'p Expr,
    ) -> R<Flow> {
        let v = self.expr(c, f, value)?;
        f.set(&name.name, v);
        Ok(Flow::Next)
    }

    fn for_stmt(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        var: &'p Ident,
        start: &'p Expr,
        end: &'p Expr,
        body: &'p Block,
    ) -> R<Flow> {
        let a = self.int(c, f, start)?;
        let b = self.int(c, f, end)?;
        for k in a..b {
            f.scopes
                .push(vec![(var.name.clone(), Value::Int(k), Ty::Int)]);
            let r = self.block(c, f, body);
            f.scopes.pop();
            if let Flow::Return(v) = r? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Next)
    }

    fn return_stmt(&mut self, c: &mut Circuit, f: &mut Frame, value: Option<&'p Expr>) -> R<Flow> {
        match value {
            Some(e) => self.expr(c, f, e).map(Flow::Return),
            None => Ok(Flow::Return(Value::Unit)),
        }
    }

    fn if_stmt(&mut self, c: &mut Circuit, f: &mut Frame, i: &'p IfStmt) -> R<Flow> {
        match self.expr(c, f, &i.cond)? {
            Value::Bool(true) => self.block(c, f, &i.then),
            Value::Bool(false) => match &i.els {
                Some(Else::Block(b)) => self.block(c, f, b),
                Some(Else::If(inner)) => self.if_stmt(c, f, inner),
                None => Ok(Flow::Next),
            },
            Value::Bit(bit) => self.if_bit(c, f, i, bit),
            _ => Err(internal(i.cond.span)),
        }
    }

    fn if_bit(&mut self, c: &mut Circuit, f: &mut Frame, i: &'p IfStmt, bit: Bit) -> R<Flow> {
        // Checker forbids `return` here, so the bodies always fall through.
        let mut res = Ok(Flow::Next);
        c.if_one(bit, |c| res = self.block(c, f, &i.then));
        res?;
        if let Some(els) = &i.els {
            let mut res = Ok(Flow::Next);
            c.if_zero(bit, |c| {
                res = match els {
                    Else::Block(b) => self.block(c, f, b),
                    Else::If(inner) => self.if_stmt(c, f, inner),
                }
            });
            res?;
        }
        Ok(Flow::Next)
    }

    fn int(&mut self, c: &mut Circuit, f: &mut Frame, e: &'p Expr) -> R<i64> {
        match self.expr(c, f, e)? {
            Value::Int(n) => Ok(n),
            _ => Err(internal(e.span)),
        }
    }

    // Stack: one small frame per nesting level (arms live in helpers) so 200 x 128 levels fit in
    // `STACK_SIZE` even unoptimized; an inline match here cost ~7 KiB per level at opt-level 0.
    fn expr(&mut self, c: &mut Circuit, f: &mut Frame, e: &'p Expr) -> R<Value> {
        match &e.kind {
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Double(v) => Ok(Value::Double(*v)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Str(_) => Err(internal(e.span)),
            ExprKind::Var(name) => var(f, name, e.span),
            ExprKind::Call { name, args } => self.call(c, f, name, args, e.span),
            ExprKind::Index { base, index } => self.index(c, f, base, index, e.span),
            ExprKind::Method { recv, name } => self.method(c, f, recv, name, e.span),
            ExprKind::Cast { ty, expr } => self.cast(c, f, ty, expr, e.span),
            ExprKind::Unary { op, expr } => self.unary(c, f, op, expr, e.span),
            ExprKind::Binary { op, lhs, rhs } => self.binary_expr(c, f, op, lhs, rhs, e.span),
        }
    }

    fn index(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        base: &'p Expr,
        index: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let b = self.expr(c, f, base)?;
        let i = self.int(c, f, index)?;
        match b {
            Value::QReg(v) => Ok(Value::Qubit(v[checked_index(i, v.len(), span)?])),
            Value::Bits(v) => Ok(Value::Bit(v[checked_index(i, v.len(), span)?])),
            _ => Err(internal(span)),
        }
    }

    fn method(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        recv: &'p Expr,
        name: &'p Ident,
        span: Span,
    ) -> R<Value> {
        let Value::Bits(v) = self.expr(c, f, recv)? else {
            return Err(internal(span));
        };
        match name.name.as_str() {
            // Printed as an unsigned integer with r[0] as the least significant bit.
            "as_int" if v.len() > 64 => Err(Diagnostic::new(
                Code::AsIntTooWide,
                span,
                msg::as_int_too_wide(v.len()),
            )),
            "as_int" => Ok(Value::ShotView(v, BitsFmt::Int)),
            "to_binary" => Ok(Value::ShotView(v, BitsFmt::Binary)),
            _ => Err(internal(span)),
        }
    }

    fn cast(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        ty: &Ty,
        expr: &'p Expr,
        span: Span,
    ) -> R<Value> {
        match (ty, self.expr(c, f, expr)?) {
            (Ty::Int, Value::Int(n)) => Ok(Value::Int(n)),
            // Java's `(long)` rule, which `as` matches: toward zero, NaN -> 0, saturating.
            (Ty::Int, Value::Double(v)) => Ok(Value::Int(v as i64)),
            (Ty::Double, Value::Int(n)) => Ok(Value::Double(n as f64)),
            (Ty::Double, Value::Double(v)) => Ok(Value::Double(v)),
            _ => Err(internal(span)),
        }
    }

    fn unary(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        op: &UnOp,
        expr: &'p Expr,
        span: Span,
    ) -> R<Value> {
        match (op, self.expr(c, f, expr)?) {
            (UnOp::Neg, Value::Int(n)) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| Diagnostic::new(Code::Overflow, span, msg::overflow())),
            (UnOp::Neg, Value::Double(v)) => Ok(Value::Double(-v)),
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err(internal(span)),
        }
    }

    fn binary_expr(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        op: &BinOp,
        lhs: &'p Expr,
        rhs: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let l = self.expr(c, f, lhs)?;
        // Short-circuit like Rust.
        match (op, &l) {
            (BinOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
            (BinOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
            _ => {}
        }
        let r = self.expr(c, f, rhs)?;
        binary(*op, l, r, span)
    }

    fn call(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        name: &'p Ident,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        if let Some(v) = self.builtin(c, f, name, args, span)? {
            return Ok(v);
        }
        let func = self.user_fn(name)?;
        let vals = self.args(c, f, args)?;
        self.call_user(c, func, vals, span)
    }

    fn user_fn(&self, name: &'p Ident) -> R<&'p FnDecl> {
        let n = name.name.as_str();
        self.fns
            .get(n)
            .copied()
            .ok_or_else(|| Diagnostic::new(Code::UnknownFn, name.span, msg::unknown_fn(n)))
    }

    fn args(&mut self, c: &mut Circuit, f: &mut Frame, args: &'p [Expr]) -> R<Vec<Value>> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(c, f, a)?);
        }
        Ok(vals)
    }

    fn call_user(
        &mut self,
        c: &mut Circuit,
        func: &'p FnDecl,
        vals: Vec<Value>,
        span: Span,
    ) -> R<Value> {
        let n = func.name.name.as_str();
        // Register elements with run-time indices may alias; the checker catches the rest.
        let mut qubits: Vec<usize> = Vec::new();
        for v in &vals {
            match v {
                Value::Qubit(i) => qubits.push(*i),
                Value::QReg(r) => qubits.extend_from_slice(r),
                _ => {}
            }
        }
        qubits.sort_unstable();
        if qubits.windows(2).any(|w| w[0] == w[1]) {
            return Err(Diagnostic::new(
                Code::DuplicateQubit,
                span,
                msg::duplicate_qubit(n),
            ));
        }
        self.call_fn(c, func, vals, span)
    }

    // Stack: kept out of line so its large frame is not part of every user-function call level.
    #[inline(never)]
    fn builtin(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        name: &'p Ident,
        args: &'p [Expr],
        span: Span,
    ) -> R<Option<Value>> {
        let n = name.name.as_str();
        let v = match n {
            "qubit" => Ok(Value::Qubit(self.alloc(c, 1, span)?[0])),
            "qreg" => {
                let k = self.int(c, f, &args[0])?;
                if k < 0 {
                    return Err(Diagnostic::new(
                        Code::NegativeSize,
                        args[0].span,
                        msg::negative_size(k),
                    ));
                }
                // `as usize` would wrap sizes of 2^32 and above on 32-bit targets (wasm).
                let n = usize::try_from(k).unwrap_or(usize::MAX);
                Ok(Value::QReg(self.alloc(c, n, span)?))
            }
            "cx" => {
                let [a, b] = [self.place(c, f, &args[0])?, self.place(c, f, &args[1])?];
                let [a, b] = self.qubits([a, b], span, n)?;
                c.cx(a, b);
                Ok(Value::Unit)
            }
            "ccx" => {
                let idx = [
                    self.place(c, f, &args[0])?,
                    self.place(c, f, &args[1])?,
                    self.place(c, f, &args[2])?,
                ];
                let [a, b, t] = self.qubits(idx, span, n)?;
                c.ccx(a, b, t);
                Ok(Value::Unit)
            }
            "measure" => match self.expr(c, f, &args[0])? {
                Value::Qubit(i) => {
                    self.free_slots += 1;
                    let b = c.measure(self.take(i, span)?);
                    self.measured.push(b);
                    Ok(Value::Bit(b))
                }
                Value::QReg(v) => {
                    let mut bits = Vec::with_capacity(v.len());
                    for i in v {
                        self.free_slots += 1;
                        bits.push(c.measure(self.take(i, span)?));
                    }
                    self.measured.extend_from_slice(&bits);
                    Ok(Value::Bits(bits))
                }
                _ => Err(internal(span)),
            },
            "discard" => {
                let idx = match self.expr(c, f, &args[0])? {
                    Value::Qubit(i) => vec![i],
                    Value::QReg(v) => v,
                    _ => return Err(internal(span)),
                };
                for i in idx {
                    self.take(i, span)?;
                }
                Ok(Value::Unit)
            }
            "len" => match self.expr(c, f, &args[0])? {
                Value::QReg(v) => Ok(Value::Int(v.len() as i64)),
                Value::Bits(v) => Ok(Value::Int(v.len() as i64)),
                _ => Err(internal(span)),
            },
            "println" => {
                let ExprKind::Str(pieces) = &args[0].kind else {
                    return Err(internal(span));
                };
                let mut values = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    values.push(self.expr(c, f, a)?);
                }
                let mut values = values.into_iter();
                for p in pieces {
                    match p {
                        FmtPiece::Lit(s) => self.text(s),
                        FmtPiece::Hole => match values.next() {
                            Some(Value::Int(n)) => self.text(&n.to_string()),
                            Some(Value::Double(v)) => self.text(&format!("{v:?}")),
                            Some(Value::Bool(b)) => self.text(&b.to_string()),
                            Some(Value::Bit(b)) => self.output.push(OutPiece::Bit(b)),
                            Some(Value::Bits(v)) => {
                                self.output.push(OutPiece::Bits(v, BitsFmt::Array))
                            }
                            Some(Value::ShotView(v, fmt)) => {
                                self.output.push(OutPiece::Bits(v, fmt))
                            }
                            _ => return Err(internal(span)),
                        },
                    }
                }
                self.text("\n");
                Ok(Value::Unit)
            }
            _ if super::check::ROTATIONS.contains(&n) => {
                let i = self.place(c, f, &args[0])?;
                let theta = match coerce(self.expr(c, f, &args[1])?, Ty::Double) {
                    Value::Double(v) => v,
                    _ => return Err(internal(args[1].span)),
                };
                if !theta.is_finite() {
                    return Err(Diagnostic::new(
                        Code::NonFiniteAngle,
                        args[1].span,
                        msg::non_finite_angle(n, theta),
                    ));
                }
                let [q] = self.qubits([i], span, n)?;
                match n {
                    "rx" => c.rx(q, theta),
                    "ry" => c.ry(q, theta),
                    _ => c.rz(q, theta),
                }
                Ok(Value::Unit)
            }
            _ if super::check::ONE_QUBIT_OPS.contains(&n) => {
                let i = self.place(c, f, &args[0])?;
                let [q] = self.qubits([i], span, n)?;
                match n {
                    "h" => c.h(q),
                    "x" => c.x(q),
                    "z" => c.z(q),
                    "s" => c.s(q),
                    "sdg" => c.sdg(q),
                    "t" => c.t(q),
                    "tdg" => c.tdg(q),
                    _ => c.reset(q),
                }
                Ok(Value::Unit)
            }
            _ => return Ok(None),
        };
        v.map(Some)
    }

    fn text(&mut self, s: &str) {
        if let Some(OutPiece::Text(t)) = self.output.last_mut() {
            t.push_str(s);
        } else {
            self.output.push(OutPiece::Text(s.to_string()));
        }
    }

    fn alloc(&mut self, c: &mut Circuit, n: usize, at: Span) -> R<Vec<usize>> {
        if n > MAX_QUBITS - self.pool.len() {
            return Err(Diagnostic::new(
                Code::TooManyQubits,
                at,
                msg::too_many_qubits(MAX_QUBITS),
            ));
        }
        Ok((0..n)
            .map(|_| {
                if self.free_slots > 0 {
                    self.free_slots -= 1;
                } else {
                    self.slots += 1;
                }
                self.pool.push(Some(c.qubit()));
                self.pool.len() - 1
            })
            .collect())
    }

    fn place(&mut self, c: &mut Circuit, f: &mut Frame, e: &'p Expr) -> R<usize> {
        match self.expr(c, f, e)? {
            Value::Qubit(i) => Ok(i),
            _ => Err(internal(e.span)),
        }
    }

    fn take(&mut self, i: usize, at: Span) -> R<Qubit> {
        self.pool
            .get_mut(i)
            .and_then(Option::take)
            .ok_or_else(|| internal(at))
    }

    fn qubits<const N: usize>(
        &mut self,
        idx: [usize; N],
        at: Span,
        gate: &str,
    ) -> R<[&mut Qubit; N]> {
        let slots = self
            .pool
            .get_disjoint_mut(idx)
            .map_err(|_| Diagnostic::new(Code::DuplicateQubit, at, msg::duplicate_qubit(gate)))?;
        let mut out = Vec::with_capacity(N);
        for s in slots {
            out.push(s.as_mut().ok_or_else(|| internal(at))?);
        }
        out.try_into().map_err(|_| internal(at))
    }
}

fn var(f: &Frame, name: &str, at: Span) -> R<Value> {
    match super::check::constant(name) {
        Some(v) => Ok(Value::Double(v)),
        None => f.get(name, at),
    }
}

fn checked_index(i: i64, len: usize, at: Span) -> R<usize> {
    usize::try_from(i).ok().filter(|&k| k < len).ok_or_else(|| {
        Diagnostic::new(Code::IndexOutOfBounds, at, msg::index_out_of_bounds(i, len))
    })
}

fn binary(op: BinOp, l: Value, r: Value, at: Span) -> R<Value> {
    use BinOp::*;
    let overflow = || Diagnostic::new(Code::Overflow, at, msg::overflow());
    Ok(match (l, r) {
        (Value::Int(a), Value::Int(b)) => match op {
            Add => Value::Int(a.checked_add(b).ok_or_else(overflow)?),
            Sub => Value::Int(a.checked_sub(b).ok_or_else(overflow)?),
            Mul => Value::Int(a.checked_mul(b).ok_or_else(overflow)?),
            Div | Rem if b == 0 => {
                return Err(Diagnostic::new(Code::DivByZero, at, msg::div_by_zero()))
            }
            Div => Value::Int(a.checked_div(b).ok_or_else(overflow)?),
            // `int.MIN % -1` is 0, as in Java; `checked_rem` would call it an overflow.
            Rem => Value::Int(a.wrapping_rem(b)),
            Eq => Value::Bool(a == b),
            Ne => Value::Bool(a != b),
            Lt => Value::Bool(a < b),
            Le => Value::Bool(a <= b),
            Gt => Value::Bool(a > b),
            Ge => Value::Bool(a >= b),
            And | Or => return Err(internal(at)),
        },
        (Value::Double(a), Value::Int(b)) => double_op(op, a, b as f64, at)?,
        (Value::Int(a), Value::Double(b)) => double_op(op, a as f64, b, at)?,
        (Value::Double(a), Value::Double(b)) => double_op(op, a, b, at)?,
        (Value::Bool(a), Value::Bool(b)) => match op {
            And => Value::Bool(a && b),
            Or => Value::Bool(a || b),
            Eq => Value::Bool(a == b),
            Ne => Value::Bool(a != b),
            _ => return Err(internal(at)),
        },
        _ => return Err(internal(at)),
    })
}

// IEEE 754 like Java: division by zero gives an infinity or NaN, not an error.
fn double_op(op: BinOp, a: f64, b: f64, at: Span) -> R<Value> {
    use BinOp::*;
    Ok(match op {
        Add => Value::Double(a + b),
        Sub => Value::Double(a - b),
        Mul => Value::Double(a * b),
        Div => Value::Double(a / b),
        Rem => Value::Double(a % b),
        Eq => Value::Bool(a == b),
        Ne => Value::Bool(a != b),
        Lt => Value::Bool(a < b),
        Le => Value::Bool(a <= b),
        Gt => Value::Bool(a > b),
        Ge => Value::Bool(a >= b),
        And | Or => return Err(internal(at)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A checker/interpreter mismatch must not masquerade as a user error such as Q0311.
    #[test]
    fn internal_errors_have_their_own_code() {
        let d = internal(Span::default());
        assert_eq!(d.code, Code::Internal);
        assert_eq!(d.code.as_str(), "Q0412");
    }
}
