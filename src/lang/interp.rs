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

#[derive(Debug, Clone)]
pub enum OutPiece {
    Text(String),
    Bit(Bit),
    Bits(Vec<Bit>),
}

pub struct Elaborated {
    pub circuit: Circuit,
    pub output: Vec<OutPiece>,
    // Physical qubit slots of `circuit` (the library reuses slots freed by `measure`).
    pub slots: usize,
}

#[derive(Debug, Clone)]
enum Value {
    Int(i64),
    Bool(bool),
    Bit(Bit),
    Bits(Vec<Bit>),
    Qubit(usize),
    QReg(Vec<usize>),
    Unit,
}

enum Flow {
    Next,
    Return(Value),
}

type R<T> = Result<T, Diagnostic>;

struct Frame {
    scopes: Vec<Vec<(String, Value)>>,
}

impl Frame {
    fn get(&self, name: &str, at: Span) -> R<Value> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| Diagnostic::new(Code::UnknownVar, at, msg::unknown_var(name)))
    }

    fn set(&mut self, name: &str, value: Value) {
        if let Some(slot) = self
            .scopes
            .iter_mut()
            .rev()
            .flat_map(|s| s.iter_mut().rev())
            .find(|(n, _)| n == name)
        {
            slot.1 = value;
        }
    }

    fn bind(&mut self, name: &str, value: Value) {
        self.scopes
            .last_mut()
            .expect("frame has a scope")
            .push((name.to_string(), value));
    }
}

struct Interp<'p> {
    fns: HashMap<&'p str, &'p FnDecl>,
    pool: Vec<Option<Qubit>>,
    output: Vec<OutPiece>,
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
        slots: it.slots,
    })
}

fn internal(at: Span) -> Diagnostic {
    // Only reachable if the checker and interpreter disagree.
    Diagnostic::new(Code::PlaceRequired, at, msg::place_required())
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
                .map(|(p, v)| (p.name.name.clone(), v))
                .collect()],
        };
        let flow = self.block(c, &mut frame, &f.body);
        self.depth -= 1;
        match flow? {
            Flow::Return(v) => Ok(v),
            Flow::Next if f.ret == Ty::Unit => Ok(Value::Unit),
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
        match &b.tail {
            Some(e) => Ok(Flow::Return(self.expr(c, f, e)?)),
            None => Ok(Flow::Next),
        }
    }

    fn stmt(&mut self, c: &mut Circuit, f: &mut Frame, s: &'p Stmt) -> R<Flow> {
        match s {
            Stmt::Let { name, init, .. } => {
                let v = self.expr(c, f, init)?;
                f.bind(&name.name, v);
            }
            Stmt::Assign { name, value } => {
                let v = self.expr(c, f, value)?;
                f.set(&name.name, v);
            }
            Stmt::If(i) => return self.if_stmt(c, f, i),
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                let a = self.int(c, f, start)?;
                let b = self.int(c, f, end)?;
                for k in a..b {
                    f.scopes.push(vec![(var.name.clone(), Value::Int(k))]);
                    let r = self.block(c, f, body);
                    f.scopes.pop();
                    if let Flow::Return(v) = r? {
                        return Ok(Flow::Return(v));
                    }
                }
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.expr(c, f, e)?,
                    None => Value::Unit,
                };
                return Ok(Flow::Return(v));
            }
            Stmt::Expr(e) => {
                self.expr(c, f, e)?;
            }
        }
        Ok(Flow::Next)
    }

    fn if_stmt(&mut self, c: &mut Circuit, f: &mut Frame, i: &'p IfStmt) -> R<Flow> {
        match self.expr(c, f, &i.cond)? {
            Value::Bool(true) => self.block(c, f, &i.then),
            Value::Bool(false) => match &i.els {
                Some(Else::Block(b)) => self.block(c, f, b),
                Some(Else::If(inner)) => self.if_stmt(c, f, inner),
                None => Ok(Flow::Next),
            },
            Value::Bit(bit) => {
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
            _ => Err(internal(i.cond.span)),
        }
    }

    fn int(&mut self, c: &mut Circuit, f: &mut Frame, e: &'p Expr) -> R<i64> {
        match self.expr(c, f, e)? {
            Value::Int(n) => Ok(n),
            _ => Err(internal(e.span)),
        }
    }

    fn expr(&mut self, c: &mut Circuit, f: &mut Frame, e: &'p Expr) -> R<Value> {
        Ok(match &e.kind {
            ExprKind::Int(n) => Value::Int(*n),
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Str(_) => return Err(internal(e.span)),
            ExprKind::Var(name) => f.get(name, e.span)?,
            ExprKind::Call { name, args } => return self.call(c, f, name, args, e.span),
            ExprKind::Index { base, index } => {
                let b = self.expr(c, f, base)?;
                let i = self.int(c, f, index)?;
                match b {
                    Value::QReg(v) => Value::Qubit(v[checked_index(i, v.len(), e.span)?]),
                    Value::Bits(v) => Value::Bit(v[checked_index(i, v.len(), e.span)?]),
                    _ => return Err(internal(e.span)),
                }
            }
            ExprKind::Unary { op, expr } => match (op, self.expr(c, f, expr)?) {
                (UnOp::Neg, Value::Int(n)) => Value::Int(
                    n.checked_neg()
                        .ok_or_else(|| Diagnostic::new(Code::Overflow, e.span, msg::overflow()))?,
                ),
                (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
                _ => return Err(internal(e.span)),
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.expr(c, f, lhs)?;
                // Short-circuit like Rust.
                match (op, &l) {
                    (BinOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
                    (BinOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
                    _ => {}
                }
                let r = self.expr(c, f, rhs)?;
                binary(*op, l, r, e.span)?
            }
        })
    }

    fn call(
        &mut self,
        c: &mut Circuit,
        f: &mut Frame,
        name: &'p Ident,
        args: &'p [Expr],
        span: Span,
    ) -> R<Value> {
        let n = name.name.as_str();
        match n {
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
                Ok(Value::QReg(self.alloc(c, k as usize, span)?))
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
                    Ok(Value::Bit(c.measure(self.take(i, span)?)))
                }
                Value::QReg(v) => {
                    let mut bits = Vec::with_capacity(v.len());
                    for i in v {
                        self.free_slots += 1;
                        bits.push(c.measure(self.take(i, span)?));
                    }
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
                            Some(Value::Bool(b)) => self.text(&b.to_string()),
                            Some(Value::Bit(b)) => self.output.push(OutPiece::Bit(b)),
                            Some(Value::Bits(v)) => self.output.push(OutPiece::Bits(v)),
                            _ => return Err(internal(span)),
                        },
                    }
                }
                self.text("\n");
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
            _ => {
                let func = *self.fns.get(n).ok_or_else(|| {
                    Diagnostic::new(Code::UnknownFn, name.span, msg::unknown_fn(n))
                })?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.expr(c, f, a)?);
                }
                self.call_fn(c, func, vals, span)
            }
        }
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
            Rem => Value::Int(a.checked_rem(b).ok_or_else(overflow)?),
            Eq => Value::Bool(a == b),
            Ne => Value::Bool(a != b),
            Lt => Value::Bool(a < b),
            Le => Value::Bool(a <= b),
            Gt => Value::Bool(a > b),
            Ge => Value::Bool(a >= b),
            And | Or => return Err(internal(at)),
        },
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
