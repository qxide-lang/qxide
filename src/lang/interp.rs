// Elaboration: a tree-walking interpreter that runs `main` once and records one qxide Circuit.
// Assumes the program passed `check`; anything the checker rules out is still reported as a
// diagnostic here instead of panicking.

use std::collections::HashMap;
use std::rc::Rc;

use qxide::{Bit, Circuit};

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::machine::{self, coerce, internal, Gate1, Machine, Rot, R};
use super::stdlib::Host;
use super::{msg, Limits};

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
    Bits(Rc<[Bit]>, BitsFmt),
}

/// Bytes `piece` prints in any shot (bits print as single digits).
pub fn piece_bytes(piece: &OutPiece) -> u64 {
    match piece {
        OutPiece::Text(t) => t.len() as u64,
        OutPiece::Bit(_) => 1,
        OutPiece::Bits(v, fmt) => bits_bytes(v.len(), *fmt),
    }
}

fn bits_bytes(n: usize, fmt: BitsFmt) -> u64 {
    let n = n as u64;
    match fmt {
        // `[0, 1, 1]`
        BitsFmt::Array => (3 * n).max(2),
        // `0b110`
        BitsFmt::Binary => n.max(1) + 2,
        // Registers wider than 64 bits are rejected, so at most 20 digits.
        BitsFmt::Int => 20,
    }
}

/// Operation counts of the recorded circuit, for the simulation-work estimate in `sim_work`.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpCounts {
    // Gates, measurements and resets (what a state-vector shot applies to every amplitude).
    pub sim_ops: u64,
    // Column passes over the stabilizer tableau: one per Clifford gate, three per `sdg`, one per reset.
    pub row_passes: u64,
    // Measurements and resets (row reductions on the tableau).
    pub measures: u64,
}

pub struct Elaborated {
    pub circuit: Circuit,
    pub output: Vec<OutPiece>,
    // Every measurement result in order, i.e. the circuit's classical register `c[0..]`.
    pub measured: Vec<Bit>,
    // Physical qubit slots of `circuit` (the library reuses slots freed by `measure`).
    pub slots: usize,
    pub counts: OpCounts,
    // Upper bound of the length of `circuit.to_qasm()`, so `qxide qasm` can refuse before building it.
    pub qasm_bytes: u64,
    // What the run used of `Limits`, for measuring the defaults.
    #[cfg_attr(not(test), allow(dead_code))]
    pub usage: Usage,
}

/// Budgets an elaboration used, for measuring the defaults (src/lang/app_limits_tests.rs).
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Usage {
    pub steps: u64,
    pub ops: u64,
    pub out_bytes: u64,
    pub cond_depth: usize,
}

// Registers share their storage, so reading a variable or indexing `r[i]` copies no qubit list (B-09).
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(Rc<str>),
    Bit(Bit),
    Bits(Rc<[Bit]>),
    ShotView(Rc<[Bit]>, BitsFmt),
    Qubit(usize),
    QReg(Rc<[usize]>),
    // Shared and mutable, like a Java array reference (src/lang/array.rs).
    Array(super::array::ArrayRef),
    Unit,
}

enum Flow {
    Next,
    Return(Value),
    Break,
    Continue,
}

// Names borrow from the AST, so binding a variable or starting a loop iteration allocates nothing.
struct Frame<'p> {
    // (name, value, declared type)
    scopes: Vec<Vec<(&'p str, Value, Ty)>>,
}

impl<'p> Frame<'p> {
    // Also returns how many variables were compared, which the step budget charges.
    fn get(&self, name: &str, at: Span) -> R<(Value, u64)> {
        let mut scanned = 0;
        for (n, v, _) in self.scopes.iter().rev().flat_map(|s| s.iter().rev()) {
            scanned += 1;
            if *n == name {
                return Ok((v.clone(), scanned));
            }
        }
        Err(Diagnostic::new(
            Code::UnknownVar,
            at,
            msg::unknown_var(name),
        ))
    }

    fn set(&mut self, name: &str, value: Value) -> u64 {
        let mut scanned = 0;
        for slot in self
            .scopes
            .iter_mut()
            .rev()
            .flat_map(|s| s.iter_mut().rev())
        {
            scanned += 1;
            if slot.0 == name {
                slot.1 = coerce(value, slot.2);
                break;
            }
        }
        scanned
    }

    fn bind(&mut self, name: &'p str, value: Value, ty: Ty) {
        self.scopes
            .last_mut()
            .expect("frame has a scope")
            .push((name, coerce(value, ty), ty));
    }
}

struct Interp<'p, 'h> {
    fns: HashMap<&'p str, &'p FnDecl>,
    m: Machine<'h>,
}

/// Runs `main` once, recording its circuit. Std calls go to `host`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn elaborate(p: &Program, host: &mut dyn Host, limits: &Limits) -> R<Elaborated> {
    elaborate_call(p, "main", Vec::new(), host, limits).map(|(e, _)| e)
}

/// Runs the function `name` once with `args` and returns its circuit and return value, whose
/// `Bit`s refer to the circuit's measurements.
pub fn elaborate_call(
    p: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
) -> R<(Elaborated, Value)> {
    let mut it = Interp {
        fns: p.fns.iter().map(|f| (f.name.name.as_str(), f)).collect(),
        m: Machine::new(host, limits),
    };
    let f = *it.fns.get(name).ok_or_else(|| internal(Span::default()))?;
    let mut c = Circuit::new();
    let ret = it.call_fn(&mut c, f, args, f.name.span)?;
    Ok((it.m.finish(c), ret))
}

impl<'p> Interp<'p, '_> {
    fn var(&mut self, f: &Frame<'p>, name: &str, at: Span) -> R<Value> {
        if let Some(v) = super::check::constant(name) {
            return Ok(Value::Double(v));
        }
        let (v, scanned) = f.get(name, at)?;
        self.m.charge(scanned, at)?;
        Ok(v)
    }

    fn call_fn(&mut self, c: &mut Circuit, f: &'p FnDecl, args: Vec<Value>, at: Span) -> R<Value> {
        self.m.enter_call(at)?;
        let mut frame = Frame {
            scopes: vec![f
                .params
                .iter()
                .zip(args)
                .map(|(p, v)| (p.name.name.as_str(), coerce(v, p.ty), p.ty))
                .collect()],
        };
        let flow = self.block(c, &mut frame, &f.body);
        self.m.leave_call();
        match flow? {
            Flow::Return(v) => Ok(coerce(v, f.ret)),
            Flow::Next if f.ret == Ty::Void => Ok(Value::Unit),
            Flow::Next => Err(Diagnostic::new(
                Code::NoReturn,
                f.body.close,
                msg::no_return(&f.name.name),
            )),
            // The checker only accepts `break` and `continue` inside a loop.
            Flow::Break | Flow::Continue => Err(internal(f.body.close)),
        }
    }

    fn block(&mut self, c: &mut Circuit, f: &mut Frame<'p>, b: &'p Block) -> R<Flow> {
        f.scopes.push(Vec::new());
        let r = self.block_inner(c, f, b);
        f.scopes.pop();
        r
    }

    fn block_inner(&mut self, c: &mut Circuit, f: &mut Frame<'p>, b: &'p Block) -> R<Flow> {
        for s in &b.stmts {
            match self.stmt(c, f, s)? {
                Flow::Next => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Next)
    }

    // Stack: a dispatcher like `expr`, so nested blocks only pay for the statement kind they use.
    fn stmt(&mut self, c: &mut Circuit, f: &mut Frame<'p>, s: &'p Stmt) -> R<Flow> {
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
            Stmt::While { cond, body, span } => self.while_stmt(c, f, cond, body, *span),
            Stmt::SetIndex {
                base,
                index,
                op,
                value,
                span,
            } => self.set_index(c, f, base, index, *op, value, *span),
            Stmt::Break { .. } => Ok(Flow::Break),
            Stmt::Continue { .. } => Ok(Flow::Continue),
            Stmt::Return { value, .. } => self.return_stmt(c, f, value.as_ref()),
            Stmt::Expr(e) => self.expr(c, f, e).map(|_| Flow::Next),
        }
    }

    fn decl(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        name: &'p Ident,
        init: &'p Expr,
        ty: Option<Ty>,
    ) -> R<Flow> {
        let v = self.expr(c, f, init)?;
        // `var` is classical only, so the runtime value determines the slot type.
        let ty = ty.unwrap_or(match v {
            Value::Double(_) => Ty::Double,
            Value::Bool(_) => Ty::Boolean,
            Value::Str(_) => Ty::String,
            _ => Ty::Int,
        });
        f.bind(&name.name, v, ty);
        Ok(Flow::Next)
    }

    fn assign(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        name: &'p Ident,
        value: &'p Expr,
    ) -> R<Flow> {
        let v = self.expr(c, f, value)?;
        let scanned = f.set(&name.name, v);
        self.m.charge(scanned, name.span)?;
        Ok(Flow::Next)
    }

    fn for_stmt(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        var: &'p Ident,
        start: &'p Expr,
        end: &'p Expr,
        body: &'p Block,
    ) -> R<Flow> {
        let a = self.int(c, f, start)?;
        let b = self.int(c, f, end)?;
        for k in a..b {
            // An empty body still costs a step per iteration (B-01).
            self.m.charge(1, var.span)?;
            f.scopes
                .push(vec![(var.name.as_str(), Value::Int(k), Ty::Int)]);
            let r = self.block(c, f, body);
            f.scopes.pop();
            match r? {
                Flow::Next | Flow::Continue => {}
                Flow::Break => break,
                Flow::Return(v) => return Ok(Flow::Return(v)),
            }
        }
        Ok(Flow::Next)
    }

    // `while cond { body }`: the condition is evaluated before every iteration; each iteration
    // that runs the body costs one step at the `while` keyword, like a `for` iteration.
    fn while_stmt(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        cond: &'p Expr,
        body: &'p Block,
        span: Span,
    ) -> R<Flow> {
        loop {
            match self.expr(c, f, cond)? {
                Value::Bool(true) => {}
                Value::Bool(false) => return Ok(Flow::Next),
                _ => return Err(internal(cond.span)),
            }
            self.m.charge(1, span)?;
            match self.block(c, f, body)? {
                Flow::Next | Flow::Continue => {}
                Flow::Break => return Ok(Flow::Next),
                Flow::Return(v) => return Ok(Flow::Return(v)),
            }
        }
    }

    fn return_stmt(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        value: Option<&'p Expr>,
    ) -> R<Flow> {
        match value {
            Some(e) => self.expr(c, f, e).map(Flow::Return),
            None => Ok(Flow::Return(Value::Unit)),
        }
    }

    fn if_stmt(&mut self, c: &mut Circuit, f: &mut Frame<'p>, i: &'p IfStmt) -> R<Flow> {
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

    fn if_bit(&mut self, c: &mut Circuit, f: &mut Frame<'p>, i: &'p IfStmt, bit: Bit) -> R<Flow> {
        // Checker forbids `return` here, so the bodies always fall through.
        self.m.enter_bit_if(i.cond.span)?;
        let mut res = Ok(Flow::Next);
        c.if_one(bit, |c| res = self.block(c, f, &i.then));
        self.m.leave_bit_if();
        res?;
        if let Some(els) = &i.els {
            self.m.enter_bit_else(i.cond.span)?;
            let mut res = Ok(Flow::Next);
            c.if_zero(bit, |c| {
                res = match els {
                    Else::Block(b) => self.block(c, f, b),
                    Else::If(inner) => self.if_stmt(c, f, inner),
                }
            });
            self.m.leave_bit_if();
            res?;
        }
        Ok(Flow::Next)
    }

    fn int(&mut self, c: &mut Circuit, f: &mut Frame<'p>, e: &'p Expr) -> R<i64> {
        match self.expr(c, f, e)? {
            Value::Int(n) => Ok(n),
            _ => Err(internal(e.span)),
        }
    }

    // Stack: one small frame per nesting level (arms live in helpers) so 200 calls x 128 levels
    // fit in `STACK_SIZE` even unoptimized; an inline match here cost ~7 KiB per level at
    // opt-level 0.
    fn expr(&mut self, c: &mut Circuit, f: &mut Frame<'p>, e: &'p Expr) -> R<Value> {
        self.m.charge(1, e.span)?;
        match &e.kind {
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Double(v) => Ok(Value::Double(*v)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Str(pieces) => self.m.str_lit(pieces, e.span),
            ExprKind::Var(name) => self.var(f, name, e.span),
            ExprKind::Call { name, args } => self.call(c, f, name, args, e.span),
            ExprKind::Index { base, index } => self.index(c, f, base, index, e.span),
            ExprKind::Method { recv, name, args } => self.method(c, f, recv, name, args, e.span),
            ExprKind::Cast { ty, expr } => self.cast(c, f, ty, expr, e.span),
            ExprKind::Unary { op, expr } => self.unary(c, f, op, expr, e.span),
            ExprKind::Binary { op, lhs, rhs } => self.binary_expr(c, f, op, lhs, rhs, e.span),
            ExprKind::NewArray { ty, dims } => self.new_array(c, f, *ty, dims, e.span),
            ExprKind::ArrayLit { ty, elems } => self.array_lit(c, f, *ty, elems, e.span),
        }
    }

    fn new_array(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        ty: Ty,
        dims: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let mut lens = Vec::with_capacity(dims.len());
        for d in dims {
            lens.push(self.int(c, f, d)?);
        }
        self.m.new_array(ty, &lens, span)
    }

    fn array_lit(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        ty: Option<Ty>,
        elems: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let ty = ty.ok_or_else(|| internal(span))?;
        let mut vals = Vec::with_capacity(elems.len());
        for e in elems {
            vals.push(self.expr(c, f, e)?);
        }
        self.m.array_lit(ty, vals, span)
    }

    // `a[i] = v`: the array, the index and the value are evaluated in that order (like Java).
    #[allow(clippy::too_many_arguments)]
    fn set_index(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        base: &'p Expr,
        index: &'p Expr,
        op: Option<BinOp>,
        value: &'p Expr,
        span: Span,
    ) -> R<Flow> {
        let a = self.expr(c, f, base)?;
        let i = self.int(c, f, index)?;
        let v = self.expr(c, f, value)?;
        self.m.set_index(a, i, op, v, span)?;
        Ok(Flow::Next)
    }

    fn index(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        base: &'p Expr,
        index: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let b = self.expr(c, f, base)?;
        let i = self.int(c, f, index)?;
        machine::index(b, i, span)
    }

    fn method(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        recv: &'p Expr,
        name: &'p Ident,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let v = self.expr(c, f, recv)?;
        if let Value::Str(_) = v {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(self.expr(c, f, a)?);
            }
            return self.m.str_method(&name.name, v, vals, span);
        }
        machine::method(v, &name.name, span)
    }

    fn cast(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        ty: &Ty,
        expr: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let v = self.expr(c, f, expr)?;
        machine::cast(*ty, v, span)
    }

    fn unary(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        op: &UnOp,
        expr: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let v = self.expr(c, f, expr)?;
        machine::unary(*op, v, span)
    }

    fn binary_expr(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
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
        if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) {
            return self.m.str_binary(*op, l, r, span);
        }
        machine::binary(*op, l, r, span)
    }

    fn call(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
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

    fn args(&mut self, c: &mut Circuit, f: &mut Frame<'p>, args: &'p [Expr]) -> R<Vec<Value>> {
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
        self.m.check_aliasing(&vals, &func.name.name, span)?;
        self.call_fn(c, func, vals, span)
    }

    // Stack: a dispatcher like `expr`; builtins nest (`len(measure(qreg(..)))`), so each one's
    // temporaries live in its own helper instead of one large frame per nesting level.
    #[inline(never)]
    fn builtin(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        name: &'p Ident,
        args: &'p [Expr],
        span: Span,
    ) -> R<Option<Value>> {
        let n = name.name.as_str();
        let v = match n {
            "qubit" => self.m.qubit(c, span),
            "qreg" => self.qreg(c, f, &args[0], span),
            "cx" => self.cx(c, f, args, span),
            "ccx" => self.ccx(c, f, args, span),
            "measure" => self.measure(c, f, &args[0], span),
            "discard" => self.discard(c, f, &args[0], span),
            "len" => self.len(c, f, &args[0], span),
            "println" => self.println(c, f, args, span),
            _ if super::check::ROTATIONS.contains(&n) => self.rotation(c, f, n, args, span),
            _ if super::check::ONE_QUBIT_OPS.contains(&n) => self.one_qubit(c, f, n, args, span),
            _ if n.starts_with("std.") => self.std_call(c, f, n, args, span),
            _ => return Ok(None),
        };
        v.map(Some)
    }

    fn qreg(&mut self, c: &mut Circuit, f: &mut Frame<'p>, size: &'p Expr, span: Span) -> R<Value> {
        let k = self.int(c, f, size)?;
        self.m.qreg(c, k, size.span, span)
    }

    fn cx(&mut self, c: &mut Circuit, f: &mut Frame<'p>, args: &'p [Expr], span: Span) -> R<Value> {
        let [a, b] = [self.place(c, f, &args[0])?, self.place(c, f, &args[1])?];
        self.m.cx(c, a, b, span)
    }

    fn ccx(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let idx = [
            self.place(c, f, &args[0])?,
            self.place(c, f, &args[1])?,
            self.place(c, f, &args[2])?,
        ];
        self.m.ccx(c, idx, span)
    }

    fn measure(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        arg: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let v = self.expr(c, f, arg)?;
        self.m.measure(c, v, span)
    }

    fn discard(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        arg: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let v = self.expr(c, f, arg)?;
        self.m.discard(v, span)
    }

    fn len(&mut self, c: &mut Circuit, f: &mut Frame<'p>, arg: &'p Expr, span: Span) -> R<Value> {
        let v = self.expr(c, f, arg)?;
        self.m.len(v, span)
    }

    fn println(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let ExprKind::Str(pieces) = &args[0].kind else {
            return Err(internal(span));
        };
        let values = self.args(c, f, &args[1..])?;
        self.m.print(pieces, values, span)
    }

    fn rotation(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        n: &str,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let i = self.place(c, f, &args[0])?;
        let angle = self.expr(c, f, &args[1])?;
        let rot = Rot::from_name(n).ok_or_else(|| internal(span))?;
        self.m.rotation(c, rot, i, angle, args[1].span, span)
    }

    fn one_qubit(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        n: &str,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let i = self.place(c, f, &args[0])?;
        let g = Gate1::from_name(n).ok_or_else(|| internal(span))?;
        self.m.one_qubit(c, g, i, span)
    }

    // `std.fs.read_text(p)`: evaluates the arguments and lets the host run the effect.
    fn std_call(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        n: &str,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            let v = self.expr(c, f, a)?;
            vals.push(self.m.std_arg(v, a.span)?);
        }
        self.m.std_call(n, vals, span)
    }

    fn place(&mut self, c: &mut Circuit, f: &mut Frame<'p>, e: &'p Expr) -> R<usize> {
        match self.expr(c, f, e)? {
            Value::Qubit(i) => Ok(i),
            _ => Err(internal(e.span)),
        }
    }
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
