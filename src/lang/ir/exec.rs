// IR interpreter: runs a function of a lowered module once and records its circuit. All circuit,
// output and budget work goes through `Machine`, the same code the tree-walking interpreter
// uses, so both engines produce the same `Elaborated` value and the same diagnostics.

use qxide::Circuit;

use super::super::ast::Ty;
use super::super::diag::{Code, Diagnostic, Span};
use super::super::interp::{Elaborated, Value};
use super::super::machine::{self, coerce, internal, Machine, R};
use super::super::stdlib::Host;
use super::super::{msg, Limits};
use super::*;

/// Runs the function `name` of a lowered module once with `args`.
pub fn run(
    module: &Module,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
) -> R<(Elaborated, Value)> {
    run_with(module, name, args, host, limits, None)
}

/// Native code for some functions of a module (the Cranelift JIT, src/lang/jit/).
pub trait Native {
    /// Whether function `id` runs natively; may compile it (and the functions it calls) first.
    fn has(&mut self, id: FuncId) -> bool;

    /// Runs function `id` with `args` after the caller's `Machine::enter_call`. Steps, call
    /// depth and runtime errors are the IR interpreter's, charged at the same points.
    fn call(&mut self, id: FuncId, args: Vec<Value>, m: &mut Machine) -> R<Value>;
}

/// `run`, with the functions `native` has compiled running as native code.
pub fn run_with(
    module: &Module,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
    native: Option<&mut dyn Native>,
) -> R<(Elaborated, Value)> {
    let id = *module
        .index
        .get(name)
        .ok_or_else(|| internal(Span::default()))?;
    let mut x = Exec {
        module,
        m: Machine::new(host, limits, module.layouts.clone()),
        free: vec![Vec::new(); module.funcs.len()],
        native,
    };
    let mut c = Circuit::new();
    let span = module.funcs[id as usize].span;
    let ret = x.call(&mut c, id, args, span)?;
    Ok((x.m.finish(c), ret))
}

struct Exec<'m, 'h, 'n> {
    module: &'m Module,
    m: Machine<'h>,
    native: Option<&'n mut dyn Native>,
    // Frames of returned calls, per function, reused as they are: lowering writes every register
    // and slot before reading it, so a call costs no work in proportion to the function's size.
    free: Vec<Vec<Frame>>,
}

#[derive(Clone)]
struct Frame {
    slots: Vec<Value>,
    regs: Vec<Value>,
}

enum Exit {
    Ret(Value),
    RegionEnd,
}

impl Frame {
    fn get(&self, r: Reg) -> Value {
        self.regs[r as usize].clone()
    }

    fn set(&mut self, r: Reg, v: Value) {
        self.regs[r as usize] = v;
    }

    fn int(&self, r: Reg, at: Span) -> R<i64> {
        match self.regs[r as usize] {
            Value::Int(n) => Ok(n),
            _ => Err(internal(at)),
        }
    }

    fn double(&self, r: Reg, at: Span) -> R<f64> {
        match self.regs[r as usize] {
            Value::Double(v) => Ok(v),
            _ => Err(internal(at)),
        }
    }

    fn qubit(&self, r: Reg, at: Span) -> R<usize> {
        match self.regs[r as usize] {
            Value::Qubit(i) => Ok(i),
            _ => Err(internal(at)),
        }
    }
}

impl<'m> Exec<'m, '_, '_> {
    // The interpreter's `call_fn`: a step and the depth limit, then the body.
    fn call(&mut self, c: &mut Circuit, id: FuncId, args: Vec<Value>, at: Span) -> R<Value> {
        let module: &'m Module = self.module;
        let f = &module.funcs[id as usize];
        self.m.enter_call(at)?;
        if let Some(n) = self.native.as_deref_mut() {
            if n.has(id) {
                let r = n.call(id, args, &mut self.m);
                self.m.leave_call();
                return r;
            }
        }
        let mut fr = self.free[id as usize].pop().unwrap_or_else(|| Frame {
            slots: vec![Value::Unit; f.slots.len()],
            regs: vec![Value::Unit; f.regs.len()],
        });
        for (i, (v, ty)) in args.into_iter().zip(&f.param_tys).enumerate() {
            fr.slots[i] = coerce(v, *ty);
        }
        let exit = if f.host {
            Err(internal(at))
        } else {
            self.run(c, f, &mut fr, 0)
        };
        self.free[id as usize].push(fr);
        self.m.leave_call();
        match exit? {
            Exit::Ret(v) if f.ret == IrTy::F64 => Ok(coerce(v, Ty::Double)),
            Exit::Ret(v) => Ok(v),
            Exit::RegionEnd => Err(internal(at)),
        }
    }

    fn run(&mut self, c: &mut Circuit, f: &'m Func, fr: &mut Frame, entry: BlockId) -> R<Exit> {
        let mut b = entry as usize;
        loop {
            let blk = &f.blocks[b];
            for i in &blk.insts {
                self.inst(c, f, fr, i)?;
            }
            match &blk.term {
                Term::Jump(t) => b = *t as usize,
                Term::Br { cond, then, els } => {
                    b = match fr.regs[*cond as usize] {
                        Value::Bool(true) => *then as usize,
                        Value::Bool(false) => *els as usize,
                        _ => return Err(internal(f.span)),
                    }
                }
                Term::Ret(r) => return Ok(Exit::Ret(r.map_or(Value::Unit, |r| fr.get(r)))),
                Term::RegionEnd => return Ok(Exit::RegionEnd),
                Term::FallOff { span } => {
                    return Err(Diagnostic::new(
                        Code::NoReturn,
                        *span,
                        msg::no_return(&f.name),
                    ))
                }
                Term::Trap(d) => return Err((**d).clone()),
            }
        }
    }

    fn inst(&mut self, c: &mut Circuit, f: &'m Func, fr: &mut Frame, i: &'m Inst) -> R<()> {
        match i {
            Inst::Charge { total, first, len } => {
                let parts = &f.charges[*first as usize..(*first + *len) as usize];
                self.m.charge_all(*total, parts)?;
            }
            Inst::Const { dst, value } => fr.set(
                *dst,
                match value {
                    Const::I64(n) => Value::Int(*n),
                    Const::F64(v) => Value::Double(*v),
                    Const::Bool(b) => Value::Bool(*b),
                },
            ),
            Inst::Str { dst, text, span } => {
                self.m.charge_text(text.len(), *span)?;
                fr.set(*dst, Value::Str(text.clone()));
            }
            Inst::Load { dst, slot } => {
                let v = fr.slots[*slot as usize].clone();
                fr.set(*dst, v);
            }
            Inst::Store { slot, src } => fr.slots[*slot as usize] = fr.get(*src),
            Inst::Widen { dst, src } => {
                let v = coerce(fr.get(*src), Ty::Double);
                fr.set(*dst, v);
            }
            Inst::IntOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let v = machine::int_op(*op, fr.int(*a, *span)?, fr.int(*b, *span)?, *span)?;
                fr.set(*dst, v);
            }
            Inst::FloatOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let v =
                    machine::double_op(*op, fr.double(*a, *span)?, fr.double(*b, *span)?, *span)?;
                fr.set(*dst, v);
            }
            Inst::BoolOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let v = machine::binary(*op, fr.get(*a), fr.get(*b), *span)?;
                fr.set(*dst, v);
            }
            Inst::StrOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let v = self.m.str_binary(*op, fr.get(*a), fr.get(*b), *span)?;
                fr.set(*dst, v);
            }
            Inst::DynOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let (l, r) = (fr.get(*a), fr.get(*b));
                let v = if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) {
                    self.m.str_binary(*op, l, r, *span)?
                } else {
                    machine::binary(*op, l, r, *span)?
                };
                fr.set(*dst, v);
            }
            Inst::Unary { op, dst, src, span } => {
                let v = machine::unary(*op, fr.get(*src), *span)?;
                fr.set(*dst, v);
            }
            Inst::Cast { ty, dst, src, span } => {
                let v = machine::cast(*ty, fr.get(*src), *span)?;
                fr.set(*dst, v);
            }
            Inst::Index {
                dst,
                base,
                index,
                span,
            } => {
                let i = fr.int(*index, *span)?;
                let v = machine::index(fr.get(*base), i, *span)?;
                fr.set(*dst, v);
            }
            Inst::View {
                dst,
                src,
                name,
                span,
            } => {
                let v = machine::method(fr.get(*src), name, *span)?;
                fr.set(*dst, v);
            }
            Inst::NewArray {
                dst,
                ty,
                dims,
                span,
            } => {
                let mut lens = Vec::with_capacity(dims.len());
                for d in dims.iter() {
                    lens.push(fr.int(*d, *span)?);
                }
                let v = self.m.new_array(*ty, &lens, *span)?;
                fr.set(*dst, v);
            }
            Inst::ArrayLit {
                dst,
                ty,
                elems,
                span,
            } => {
                let vals = elems.iter().map(|a| fr.get(*a)).collect();
                let v = self.m.array_lit(*ty, vals, *span)?;
                fr.set(*dst, v);
            }
            Inst::NewStruct {
                dst,
                ty,
                args,
                span,
            } => {
                let vals = args.iter().map(|a| fr.get(*a)).collect();
                let v = self.m.new_struct(*ty, vals, *span)?;
                fr.set(*dst, v);
            }
            Inst::GetField {
                dst,
                recv,
                index,
                span,
            } => {
                let v = super::super::record::get_field(&fr.get(*recv), *index as usize, *span)?;
                fr.set(*dst, v);
            }
            Inst::SetField {
                base,
                index,
                op,
                value,
                span,
            } => {
                self.m
                    .set_field(fr.get(*base), *index as usize, *op, fr.get(*value), *span)?;
            }
            Inst::SetIndex {
                base,
                index,
                op,
                value,
                span,
            } => {
                let i = fr.int(*index, *span)?;
                self.m
                    .set_index(fr.get(*base), i, *op, fr.get(*value), *span)?;
            }
            Inst::StrMethod {
                name,
                dst,
                recv,
                args,
                span,
            } => {
                let vals = args.iter().map(|a| fr.get(*a)).collect();
                let v = self.m.str_method(name, fr.get(*recv), vals, *span)?;
                fr.set(*dst, v);
            }
            Inst::QAlloc { dst, span } => {
                let v = self.m.qubit(c, *span)?;
                fr.set(*dst, v);
            }
            Inst::QAllocReg {
                dst,
                size,
                size_span,
                span,
            } => {
                let k = fr.int(*size, *size_span)?;
                let v = self.m.qreg(c, k, *size_span, *span)?;
                fr.set(*dst, v);
            }
            Inst::Gate { gate, q, span } => {
                let q = fr.qubit(*q, *span)?;
                self.m.one_qubit(c, *gate, q, *span)?;
            }
            Inst::Rotate {
                rot,
                q,
                angle,
                angle_span,
                span,
            } => {
                let q = fr.qubit(*q, *span)?;
                self.m
                    .rotation(c, *rot, q, fr.get(*angle), *angle_span, *span)?;
            }
            Inst::Cx { a, b, span } => {
                let (a, b) = (fr.qubit(*a, *span)?, fr.qubit(*b, *span)?);
                self.m.cx(c, a, b, *span)?;
            }
            Inst::Ccx { a, b, t, span } => {
                let idx = [
                    fr.qubit(*a, *span)?,
                    fr.qubit(*b, *span)?,
                    fr.qubit(*t, *span)?,
                ];
                self.m.ccx(c, idx, *span)?;
            }
            Inst::Measure { dst, src, span } => {
                let v = self.m.measure(c, fr.get(*src), *span)?;
                fr.set(*dst, v);
            }
            Inst::Discard { src, span } => {
                self.m.discard(fr.get(*src), *span)?;
            }
            Inst::Len { dst, src, span } => {
                let v = self.m.len(fr.get(*src), *span)?;
                fr.set(*dst, v);
            }
            Inst::Print { fmt, args, span } => {
                let values = args.iter().map(|a| fr.get(*a)).collect();
                self.m.print(&f.formats[*fmt as usize], values, *span)?;
            }
            Inst::HostText { src, span } => {
                if let Value::Str(s) = &fr.regs[*src as usize] {
                    let n = s.len();
                    self.m.charge_text(n, *span)?;
                }
            }
            Inst::StdCall {
                name,
                dst,
                args,
                span,
            } => {
                let mut vals = Vec::with_capacity(args.len());
                for (a, at) in args.iter() {
                    vals.push(machine::host_value(fr.get(*a), *at)?);
                }
                let v = self.m.std_call(name, vals, *span)?;
                fr.set(*dst, v);
            }
            Inst::Call {
                func,
                dst,
                args,
                span,
            } => {
                let vals: Vec<Value> = args.iter().map(|a| fr.get(*a)).collect();
                let module: &'m Module = self.module;
                let callee = &module.funcs[*func as usize];
                self.m.check_aliasing(&vals, &callee.name, *span)?;
                let v = self.call(c, *func, vals, *span)?;
                fr.set(*dst, v);
            }
            Inst::IfBit {
                bit,
                then,
                els,
                span,
            } => self.if_bit(c, f, fr, *bit, *then, *els, *span)?,
        }
        Ok(())
    }

    // The interpreter's `if_bit`: each region is recorded as a conditional block of the circuit.
    #[allow(clippy::too_many_arguments)]
    fn if_bit(
        &mut self,
        c: &mut Circuit,
        f: &'m Func,
        fr: &mut Frame,
        bit: Reg,
        then: BlockId,
        els: Option<BlockId>,
        span: Span,
    ) -> R<()> {
        let Value::Bit(b) = fr.regs[bit as usize] else {
            return Err(internal(span));
        };
        self.m.enter_bit_if(span)?;
        let mut res = Ok(Exit::RegionEnd);
        c.if_one(b, |c| res = self.run(c, f, fr, then));
        self.m.leave_bit_if();
        res?;
        if let Some(e) = els {
            self.m.enter_bit_else(span)?;
            let mut res = Ok(Exit::RegionEnd);
            c.if_zero(b, |c| res = self.run(c, f, fr, e));
            self.m.leave_bit_if();
            res?;
        }
        Ok(())
    }
}
