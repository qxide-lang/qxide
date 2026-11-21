// Elaboration: a tree-walking interpreter that runs `main` once and records one qxide Circuit.
// Assumes the program passed `check`; anything the checker rules out is still reported as a
// diagnostic here instead of panicking.

use std::collections::HashMap;
use std::rc::Rc;

use qxide::{Bit, Circuit, Qubit};

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::stdlib::{Host, HostValue, StdErrorKind};
use super::{msg, Limits};

pub const MAX_CALL_DEPTH: usize = 200;
// Caps allocations so a huge `qreg(n)` fails with a diagnostic instead of aborting on OOM.
pub const MAX_QUBITS: usize = 1 << 20;
// How often (in budget steps) elaboration asks the host whether the call was cancelled.
const CANCEL_POLL: u64 = 4096;

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
    host: &'h mut dyn Host,
    // `steps` at which the host is next asked whether the call was cancelled.
    next_poll: u64,
    pool: Vec<Option<Qubit>>,
    output: Vec<OutPiece>,
    measured: Vec<Bit>,
    depth: usize,
    // Mirrors Circuit's slot reuse: `measure` frees a slot, the next `qubit()` takes it.
    slots: usize,
    free_slots: usize,
    limits: Limits,
    // Work done so far, checked against `limits` (B-01, B-02, M-10).
    steps: u64,
    ops: u64,
    out_bytes: u64,
    qasm_bytes: u64,
    counts: OpCounts,
    // `if` blocks on measured bits that enclose the operations being recorded, across calls.
    cond_depth: usize,
    peak_cond_depth: usize,
}

// `OPENQASM 3.0;`, `include`, `qubit[n] q;` and `bit[n] c;` lines.
const QASM_HEADER_BYTES: u64 = 96;

// Steps per byte of `String` text built, compared, measured or passed to or from the host. Every
// live byte was charged when it was built, so the step budget also bounds string memory:
// `max_steps / 8` bytes (128 MiB for the CLI, 256 MiB per desktop-app call).
const STR_BYTE_STEPS: u64 = 8;

/// Runs `main` once, recording its circuit. Std calls go to `host`.
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
        host,
        next_poll: CANCEL_POLL,
        pool: Vec::new(),
        output: Vec::new(),
        measured: Vec::new(),
        depth: 0,
        slots: 0,
        free_slots: 0,
        limits: *limits,
        steps: 0,
        ops: 0,
        out_bytes: 0,
        qasm_bytes: QASM_HEADER_BYTES,
        counts: OpCounts::default(),
        cond_depth: 0,
        peak_cond_depth: 0,
    };
    let f = *it.fns.get(name).ok_or_else(|| internal(Span::default()))?;
    let mut c = Circuit::new();
    let ret = it.call_fn(&mut c, f, args, f.name.span)?;
    Ok((
        Elaborated {
            circuit: c,
            output: it.output,
            measured: it.measured,
            slots: it.slots,
            counts: it.counts,
            qasm_bytes: it.qasm_bytes,
            usage: Usage {
                steps: it.steps,
                ops: it.ops,
                out_bytes: it.out_bytes,
                cond_depth: it.peak_cond_depth,
            },
        },
        ret,
    ))
}

// What a recorded operation is, for the operation budget, the QASM size and the work estimate.
#[derive(Clone, Copy)]
enum Rec {
    // A gate of `rows` tableau passes printed as `name q[i];` with an argument text of `arg` bytes.
    Gate { name: usize, arg: usize, rows: u64 },
    Cx,
    Ccx,
    Measure,
    Reset,
    If,
}

fn digits(n: usize) -> u64 {
    n.checked_ilog10().map_or(1, |d| u64::from(d) + 1)
}

// Length of `{v:?}` for an `f64`, as `Gate::qasm` prints rotation angles.
fn debug_len(v: f64) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0 += s.len();
            Ok(())
        }
    }
    let mut n = Count(0);
    let _ = std::fmt::write(&mut n, format_args!("{v:?}"));
    n.0
}

fn internal(at: Span) -> Diagnostic {
    // Only reachable if the checker and interpreter disagree.
    Diagnostic::new(Code::Internal, at, msg::internal_error())
}

impl<'p> Interp<'p, '_> {
    // Text work in proportion to its length (`STR_BYTE_STEPS` per byte), charged before it is done.
    fn charge_text(&mut self, bytes: usize, at: Span) -> R<()> {
        self.charge((bytes as u64).saturating_mul(STR_BYTE_STEPS), at)
    }

    // A string literal used as a value; `println` reads its format string from the AST instead.
    // Each evaluation builds a new `String`, so a literal in a loop is charged every time.
    fn str_lit(&mut self, pieces: &[FmtPiece], at: Span) -> R<Value> {
        let mut len = 0;
        for p in pieces {
            match p {
                FmtPiece::Lit(t) => len += t.len(),
                FmtPiece::Hole => return Err(internal(at)),
            }
        }
        self.charge_text(len, at)?;
        let mut text = String::with_capacity(len);
        for p in pieces {
            if let FmtPiece::Lit(t) = p {
                text.push_str(t);
            }
        }
        Ok(Value::Str(text.into()))
    }

    // `String` `+` (either side a `String`), `==` and `!=`, charged by the text they touch.
    fn str_binary(&mut self, op: BinOp, l: Value, r: Value, at: Span) -> R<Value> {
        match op {
            BinOp::Add => {
                let (a, b) = (concat_text(&l, at)?, concat_text(&r, at)?);
                self.charge_text(a.len() + b.len(), at)?;
                let mut text = String::with_capacity(a.len() + b.len());
                text.push_str(&a);
                text.push_str(&b);
                Ok(Value::Str(text.into()))
            }
            BinOp::Eq | BinOp::Ne => {
                let (Value::Str(a), Value::Str(b)) = (&l, &r) else {
                    return Err(internal(at));
                };
                self.charge_text(a.len().min(b.len()), at)?;
                Ok(Value::Bool((a == b) == (op == BinOp::Eq)))
            }
            _ => Err(internal(at)),
        }
    }

    fn charge(&mut self, steps: u64, at: Span) -> R<()> {
        self.steps = self.steps.saturating_add(steps);
        if self.steps > self.limits.max_steps {
            return Err(Diagnostic::new(
                Code::StepLimit,
                at,
                msg::step_limit(self.limits.max_steps),
            ));
        }
        if self.steps >= self.next_poll {
            self.next_poll = self.steps.saturating_add(CANCEL_POLL);
            if self.host.cancelled() {
                return Err(Diagnostic::new(Code::Cancelled, at, msg::cancelled()));
            }
        }
        Ok(())
    }

    // Counts one circuit operation before it is recorded, with an upper bound of its QASM line.
    fn record(&mut self, op: Rec, at: Span) -> R<()> {
        self.ops += 1;
        if self.ops > self.limits.max_ops {
            return Err(Diagnostic::new(
                Code::OpLimit,
                at,
                msg::op_limit(self.limits.max_ops),
            ));
        }
        // Physical indices are below `slots`; the next classical bit is `c[measured.len()]`.
        let q = digits(self.slots);
        let b = digits(self.measured.len());
        let pad = 2 * self.cond_depth as u64;
        let line = match op {
            Rec::Gate { name, arg, rows } => {
                self.counts.sim_ops += 1;
                self.counts.row_passes += rows;
                // `rx(0.5) q[3];` is name + `(arg)` + ` q[` + index + `];` + newline.
                name as u64 + if arg > 0 { arg as u64 + 2 } else { 0 } + 6 + q
            }
            Rec::Cx => {
                self.counts.sim_ops += 1;
                self.counts.row_passes += 1;
                13 + 2 * q
            }
            Rec::Ccx => {
                self.counts.sim_ops += 1;
                19 + 3 * q
            }
            Rec::Measure => {
                self.counts.sim_ops += 1;
                self.counts.measures += 1;
                19 + b + q
            }
            Rec::Reset => {
                self.counts.sim_ops += 1;
                self.counts.measures += 1;
                self.counts.row_passes += 1;
                11 + q
            }
            // `if (c[i] == false) {` plus the closing `}` line at the same indentation.
            Rec::If => 23 + b + pad,
        };
        self.qasm_bytes = self.qasm_bytes.saturating_add(line + pad);
        Ok(())
    }

    // Counts bytes of `println` output as they are produced (B-02).
    fn out(&mut self, bytes: u64, at: Span) -> R<()> {
        self.out_bytes = self.out_bytes.saturating_add(bytes);
        if self.out_bytes > self.limits.max_output_bytes {
            return Err(Diagnostic::new(
                Code::OutputLimit,
                at,
                msg::output_limit(self.limits.max_output_bytes),
            ));
        }
        Ok(())
    }

    fn var(&mut self, f: &Frame<'p>, name: &str, at: Span) -> R<Value> {
        if let Some(v) = super::check::constant(name) {
            return Ok(Value::Double(v));
        }
        let (v, scanned) = f.get(name, at)?;
        self.charge(scanned, at)?;
        Ok(v)
    }

    fn call_fn(&mut self, c: &mut Circuit, f: &'p FnDecl, args: Vec<Value>, at: Span) -> R<Value> {
        self.charge(1, at)?;
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
                .map(|(p, v)| (p.name.name.as_str(), coerce(v, p.ty), p.ty))
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

    fn block(&mut self, c: &mut Circuit, f: &mut Frame<'p>, b: &'p Block) -> R<Flow> {
        f.scopes.push(Vec::new());
        let r = self.block_inner(c, f, b);
        f.scopes.pop();
        r
    }

    fn block_inner(&mut self, c: &mut Circuit, f: &mut Frame<'p>, b: &'p Block) -> R<Flow> {
        for s in &b.stmts {
            if let Flow::Return(v) = self.stmt(c, f, s)? {
                return Ok(Flow::Return(v));
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
        self.charge(scanned, name.span)?;
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
            self.charge(1, var.span)?;
            f.scopes
                .push(vec![(var.name.as_str(), Value::Int(k), Ty::Int)]);
            let r = self.block(c, f, body);
            f.scopes.pop();
            if let Flow::Return(v) = r? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Next)
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
        // Bodies of calls made inside count too: the circuit nests them all (M-10).
        if self.cond_depth >= self.limits.max_cond_depth {
            return Err(Diagnostic::new(
                Code::CondDepth,
                i.cond.span,
                msg::cond_depth_limit(self.limits.max_cond_depth),
            ));
        }
        // Checker forbids `return` here, so the bodies always fall through.
        self.record(Rec::If, i.cond.span)?;
        self.cond_depth += 1;
        self.peak_cond_depth = self.peak_cond_depth.max(self.cond_depth);
        let mut res = Ok(Flow::Next);
        c.if_one(bit, |c| res = self.block(c, f, &i.then));
        self.cond_depth -= 1;
        res?;
        if let Some(els) = &i.els {
            self.record(Rec::If, i.cond.span)?;
            self.cond_depth += 1;
            self.peak_cond_depth = self.peak_cond_depth.max(self.cond_depth);
            let mut res = Ok(Flow::Next);
            c.if_zero(bit, |c| {
                res = match els {
                    Else::Block(b) => self.block(c, f, b),
                    Else::If(inner) => self.if_stmt(c, f, inner),
                }
            });
            self.cond_depth -= 1;
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

    // Stack: one small frame per nesting level (arms live in helpers) so 200 x 128 levels fit in
    // `STACK_SIZE` even unoptimized; an inline match here cost ~7 KiB per level at opt-level 0.
    fn expr(&mut self, c: &mut Circuit, f: &mut Frame<'p>, e: &'p Expr) -> R<Value> {
        self.charge(1, e.span)?;
        match &e.kind {
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Double(v) => Ok(Value::Double(*v)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Str(pieces) => self.str_lit(pieces, e.span),
            ExprKind::Var(name) => self.var(f, name, e.span),
            ExprKind::Call { name, args } => self.call(c, f, name, args, e.span),
            ExprKind::Index { base, index } => self.index(c, f, base, index, e.span),
            ExprKind::Method { recv, name, .. } => self.method(c, f, recv, name, e.span),
            ExprKind::Cast { ty, expr } => self.cast(c, f, ty, expr, e.span),
            ExprKind::Unary { op, expr } => self.unary(c, f, op, expr, e.span),
            ExprKind::Binary { op, lhs, rhs } => self.binary_expr(c, f, op, lhs, rhs, e.span),
        }
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
        match b {
            Value::QReg(v) => Ok(Value::Qubit(v[checked_index(i, v.len(), span)?])),
            Value::Bits(v) => Ok(Value::Bit(v[checked_index(i, v.len(), span)?])),
            _ => Err(internal(span)),
        }
    }

    fn method(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
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
        f: &mut Frame<'p>,
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
        f: &mut Frame<'p>,
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
            return self.str_binary(*op, l, r, span);
        }
        binary(*op, l, r, span)
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
        let n = func.name.name.as_str();
        // Register elements with run-time indices may alias; the checker catches the rest.
        let mut qubits: Vec<usize> = Vec::new();
        for v in &vals {
            match v {
                Value::Qubit(i) => qubits.push(*i),
                Value::QReg(r) => {
                    // Collecting and sorting the register is work proportional to its size.
                    self.charge(r.len() as u64, span)?;
                    qubits.extend_from_slice(r);
                }
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
            "qubit" => self.alloc(c, 1, span).map(|q| Value::Qubit(q[0])),
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
        if k < 0 {
            return Err(Diagnostic::new(
                Code::NegativeSize,
                size.span,
                msg::negative_size(k),
            ));
        }
        // `as usize` would wrap sizes of 2^32 and above on 32-bit targets (wasm).
        let n = usize::try_from(k).unwrap_or(usize::MAX);
        Ok(Value::QReg(self.alloc(c, n, span)?.into()))
    }

    fn cx(&mut self, c: &mut Circuit, f: &mut Frame<'p>, args: &'p [Expr], span: Span) -> R<Value> {
        let [a, b] = [self.place(c, f, &args[0])?, self.place(c, f, &args[1])?];
        self.record(Rec::Cx, span)?;
        let [a, b] = self.qubits([a, b], span, "cx")?;
        c.cx(a, b);
        Ok(Value::Unit)
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
        self.record(Rec::Ccx, span)?;
        let [a, b, t] = self.qubits(idx, span, "ccx")?;
        c.ccx(a, b, t);
        Ok(Value::Unit)
    }

    fn measure(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        arg: &'p Expr,
        span: Span,
    ) -> R<Value> {
        match self.expr(c, f, arg)? {
            Value::Qubit(i) => {
                self.record(Rec::Measure, span)?;
                self.free_slots += 1;
                let b = c.measure(self.take(i, span)?);
                self.measured.push(b);
                Ok(Value::Bit(b))
            }
            Value::QReg(v) => {
                self.charge(v.len() as u64, span)?;
                let mut bits = Vec::with_capacity(v.len());
                for &i in v.iter() {
                    self.record(Rec::Measure, span)?;
                    self.free_slots += 1;
                    let b = c.measure(self.take(i, span)?);
                    self.measured.push(b);
                    bits.push(b);
                }
                Ok(Value::Bits(bits.into()))
            }
            _ => Err(internal(span)),
        }
    }

    fn discard(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame<'p>,
        arg: &'p Expr,
        span: Span,
    ) -> R<Value> {
        let idx = match self.expr(c, f, arg)? {
            Value::Qubit(i) => Rc::from([i]),
            Value::QReg(v) => v,
            _ => return Err(internal(span)),
        };
        self.charge(idx.len() as u64, span)?;
        for &i in idx.iter() {
            self.take(i, span)?;
        }
        Ok(Value::Unit)
    }

    fn len(&mut self, c: &mut Circuit, f: &mut Frame<'p>, arg: &'p Expr, span: Span) -> R<Value> {
        match self.expr(c, f, arg)? {
            Value::Str(s) => {
                self.charge_text(s.len(), span)?;
                Ok(Value::Int(s.chars().count() as i64))
            }
            Value::QReg(v) => Ok(Value::Int(v.len() as i64)),
            Value::Bits(v) => Ok(Value::Int(v.len() as i64)),
            _ => Err(internal(span)),
        }
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
        self.print(pieces, values, span)
    }

    fn print(&mut self, pieces: &[FmtPiece], values: Vec<Value>, span: Span) -> R<Value> {
        let mut values = values.into_iter();
        for p in pieces {
            let piece = match p {
                FmtPiece::Lit(s) => OutPiece::Text(s.clone()),
                FmtPiece::Hole => match values.next() {
                    Some(Value::Int(n)) => OutPiece::Text(n.to_string()),
                    Some(Value::Double(v)) => OutPiece::Text(format!("{v:?}")),
                    Some(Value::Bool(b)) => OutPiece::Text(b.to_string()),
                    // `emit` charges the copy to the output budget.
                    Some(Value::Str(s)) => OutPiece::Text(s.to_string()),
                    Some(Value::Bit(b)) => OutPiece::Bit(b),
                    Some(Value::Bits(v)) => OutPiece::Bits(v, BitsFmt::Array),
                    Some(Value::ShotView(v, fmt)) => OutPiece::Bits(v, fmt),
                    _ => return Err(internal(span)),
                },
            };
            self.emit(piece, span)?;
        }
        self.emit(OutPiece::Text("\n".into()), span)?;
        Ok(Value::Unit)
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
        // Rotations never run on the tableau, so they need no row passes.
        let op = Rec::Gate {
            name: n.len(),
            arg: debug_len(theta),
            rows: 0,
        };
        self.record(op, span)?;
        let [q] = self.qubits([i], span, n)?;
        match n {
            "rx" => c.rx(q, theta),
            "ry" => c.ry(q, theta),
            _ => c.rz(q, theta),
        }
        Ok(Value::Unit)
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
        let op = match n {
            "reset" => Rec::Reset,
            // S† runs as S three times on the tableau.
            "sdg" => Rec::Gate {
                name: 3,
                arg: 0,
                rows: 3,
            },
            _ => Rec::Gate {
                name: n.len(),
                arg: 0,
                rows: 1,
            },
        };
        self.record(op, span)?;
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
            vals.push(match self.expr(c, f, a)? {
                Value::Int(n) => HostValue::Int(n),
                Value::Double(v) => HostValue::Double(v),
                Value::Bool(b) => HostValue::Bool(b),
                Value::Str(s) => {
                    self.charge_text(s.len(), a.span)?;
                    HostValue::Str(s.to_string())
                }
                _ => return Err(internal(a.span)),
            });
        }
        let rest = &n["std.".len()..];
        let (module, function) = rest.split_once('.').ok_or_else(|| internal(span))?;
        match self.host.call(module, function, vals) {
            Ok(HostValue::Unit) => Ok(Value::Unit),
            Ok(HostValue::Int(n)) => Ok(Value::Int(n)),
            Ok(HostValue::Double(v)) => Ok(Value::Double(v)),
            Ok(HostValue::Bool(b)) => Ok(Value::Bool(b)),
            // Hosts cap what they read (16 MiB for `std.fs` and `std.http`); the program pays for
            // bringing it in like for text it builds itself.
            Ok(HostValue::Str(s)) => {
                self.charge_text(s.len(), span)?;
                Ok(Value::Str(s.into()))
            }
            Err(e) => {
                let code = match e.kind {
                    StdErrorKind::Unavailable => Code::StdUnavailable,
                    StdErrorKind::Io => Code::StdIo,
                    StdErrorKind::Path => Code::StdPath,
                    StdErrorKind::Argument => Code::StdArgument,
                    StdErrorKind::Http => Code::StdHttp,
                    StdErrorKind::Url => Code::StdUrl,
                    StdErrorKind::Cancelled => Code::Cancelled,
                };
                Err(Diagnostic::new(code, span, e.message))
            }
        }
    }

    fn emit(&mut self, piece: OutPiece, at: Span) -> R<()> {
        self.out(piece_bytes(&piece), at)?;
        if let OutPiece::Bits(v, _) = &piece {
            // Printing a register formats every bit in each shot.
            self.charge(v.len() as u64, at)?;
        }
        match (piece, self.output.last_mut()) {
            (OutPiece::Text(s), Some(OutPiece::Text(t))) => t.push_str(&s),
            (piece, _) => self.output.push(piece),
        }
        Ok(())
    }

    fn alloc(&mut self, c: &mut Circuit, n: usize, at: Span) -> R<Vec<usize>> {
        if n > MAX_QUBITS - self.pool.len() {
            return Err(Diagnostic::new(
                Code::TooManyQubits,
                at,
                msg::too_many_qubits(MAX_QUBITS),
            ));
        }
        self.charge(n as u64, at)?;
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

    fn place(&mut self, c: &mut Circuit, f: &mut Frame<'p>, e: &'p Expr) -> R<usize> {
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

// The text of a build-time value in `String` concatenation, spelled like `println` prints it.
fn concat_text(v: &Value, at: Span) -> R<std::borrow::Cow<'_, str>> {
    use std::borrow::Cow;
    Ok(match v {
        Value::Int(n) => Cow::Owned(n.to_string()),
        Value::Double(v) => Cow::Owned(format!("{v:?}")),
        Value::Bool(b) => Cow::Owned(b.to_string()),
        Value::Str(s) => Cow::Borrowed(s),
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
