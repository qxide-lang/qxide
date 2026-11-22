// IR verifier, run on every lowered module in debug builds and tests: operand types, branch
// targets, charge ranges, the call graph, and the rules for `if.bit` regions (no allocation,
// measurement, output, host effects or `ret` inside, and calls only to functions that keep to
// the same rules). Qubit linearity is not re-checked here: the checker stays its only source.

use super::super::ast::{BinOp, FmtPiece};
use super::*;

type V = Result<(), String>;

pub fn verify(m: &Module) -> V {
    for (name, &id) in &m.index {
        if m.funcs.get(id as usize).map(|f| &f.name) != Some(name) {
            return Err(format!("index entry `{name}` points at function {id}"));
        }
    }
    let safe = conditional_safe(m);
    for f in m.funcs.iter().filter(|f| !f.host) {
        Checker { m, f, safe: &safe }
            .func()
            .map_err(|e| format!("fn {}: {e}", f.name))?;
    }
    Ok(())
}

// Instructions an `if.bit` region cannot contain: the circuit's conditional blocks only record
// gates and `reset` (`Q0314` in the checker).
fn unconditional(i: &Inst) -> bool {
    matches!(
        i,
        Inst::QAlloc { .. }
            | Inst::QAllocReg { .. }
            | Inst::Measure { .. }
            | Inst::Discard { .. }
            | Inst::Print { .. }
            | Inst::StdCall { .. }
            | Inst::HostText { .. }
    )
}

// Whether a call may run a function that is not region-safe: a dynamic call may run any method
// of its name.
fn calls_unsafe(m: &Module, i: &Inst, safe: &[bool]) -> Option<FuncId> {
    match i {
        Inst::Call { func, .. } if !safe[*func as usize] => Some(*func),
        Inst::CallDyn { method, .. } => m.methods_named(method).find(|f| !safe[*f as usize]),
        _ => None,
    }
}

// Functions that may run inside a region: no `consume` parameter, no quantum return value, no
// unconditional instruction, and calls only to such functions (greatest fixpoint).
fn conditional_safe(m: &Module) -> Vec<bool> {
    let mut safe: Vec<bool> = m
        .funcs
        .iter()
        .map(|f| {
            !f.host
                && !f.ret.is_quantum()
                && f.params.iter().all(|p| p.mode != Mode::Consume)
                && f.blocks.iter().all(|b| !b.insts.iter().any(unconditional))
        })
        .collect();
    loop {
        let mut changed = false;
        for (i, f) in m.funcs.iter().enumerate() {
            if !safe[i] {
                continue;
            }
            let unsafe_call = f
                .blocks
                .iter()
                .flat_map(|b| &b.insts)
                .any(|inst| calls_unsafe(m, inst, &safe).is_some());
            if unsafe_call {
                safe[i] = false;
                changed = true;
            }
        }
        if !changed {
            return safe;
        }
    }
}

struct Checker<'a> {
    m: &'a Module,
    f: &'a Func,
    safe: &'a [bool],
}

impl Checker<'_> {
    fn ty(&self, r: Reg) -> Result<IrTy, String> {
        self.f
            .regs
            .get(r as usize)
            .copied()
            .ok_or_else(|| format!("register %{r} out of range"))
    }

    fn want(&self, r: Reg, t: IrTy) -> V {
        let got = self.ty(r)?;
        if got == t {
            Ok(())
        } else {
            Err(format!("%{r} is {}, expected {}", got.name(), t.name()))
        }
    }

    fn want_enum(&self, r: Reg) -> V {
        match self.ty(r)? {
            IrTy::Enum(_) => Ok(()),
            got => Err(format!("%{r} is {}, expected an enum", got.name())),
        }
    }

    fn block(&self, b: BlockId) -> V {
        if (b as usize) < self.f.blocks.len() {
            Ok(())
        } else {
            Err(format!("block bb{b} out of range"))
        }
    }

    fn slot(&self, s: SlotId) -> Result<IrTy, String> {
        self.f
            .slots
            .get(s as usize)
            .map(|s| s.ty)
            .ok_or_else(|| format!("slot {s} out of range"))
    }

    fn func(&self) -> V {
        let f = self.f;
        if f.blocks.is_empty() {
            return Err("no blocks".into());
        }
        if f.params.len() != f.param_tys.len() || f.slots.len() < f.params.len() {
            return Err("parameters do not match their slots".into());
        }
        for (i, p) in f.params.iter().enumerate() {
            if f.slots[i].ty != p.ty {
                return Err(format!("parameter slot {i} has the wrong type"));
            }
        }
        for (k, b) in f.blocks.iter().enumerate() {
            for i in &b.insts {
                self.inst(i).map_err(|e| format!("bb{k}: {e}"))?;
            }
            self.term(&b.term).map_err(|e| format!("bb{k}: {e}"))?;
        }
        self.regions()
    }

    fn numeric_op(op: BinOp) -> bool {
        matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
        )
    }

    fn compare_op(op: BinOp) -> bool {
        matches!(
            op,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    fn inst(&self, i: &Inst) -> V {
        match i {
            Inst::Charge { total, first, len } => {
                let parts = self
                    .f
                    .charges
                    .get(*first as usize..(*first + *len) as usize)
                    .ok_or("charge range out of bounds")?;
                let sum = parts.iter().fold(0u64, |s, p| s.saturating_add(p.0));
                if sum != *total || parts.is_empty() {
                    return Err("charge total does not match its parts".into());
                }
            }
            Inst::Const { dst, value } => {
                let t = match value {
                    Const::I64(_) => IrTy::I64,
                    Const::F64(_) => IrTy::F64,
                    Const::Bool(_) => IrTy::Bool,
                };
                self.want(*dst, t)?;
            }
            Inst::Str { dst, .. } => self.want(*dst, IrTy::Str)?,
            Inst::Load { dst, slot } => self.want(*dst, self.slot(*slot)?)?,
            Inst::Store { slot, src } => self.want(*src, self.slot(*slot)?)?,
            Inst::Widen { dst, src } => {
                self.want(*src, IrTy::I64)?;
                self.want(*dst, IrTy::F64)?;
            }
            Inst::Upcast { dst, src } => {
                if !matches!(self.ty(*src)?, IrTy::Struct(_) | IrTy::Enum(_)) {
                    return Err(format!("%{src} is not a struct or enum"));
                }
                if !matches!(self.ty(*dst)?, IrTy::Iface(_)) {
                    return Err(format!("%{dst} is not an interface"));
                }
            }
            Inst::CallDyn {
                method, dst, args, ..
            } => {
                let Some(&recv) = args.first() else {
                    return Err(format!("call.dyn {method} without a receiver"));
                };
                if !matches!(self.ty(recv)?, IrTy::Iface(_)) {
                    return Err(format!("%{recv} is not an interface"));
                }
                for a in args.iter() {
                    self.ty(*a)?;
                }
                self.ty(*dst)?;
            }
            Inst::IntOp { op, dst, a, b, .. } => {
                self.want(*a, IrTy::I64)?;
                self.want(*b, IrTy::I64)?;
                if Self::numeric_op(*op) {
                    self.want(*dst, IrTy::I64)?;
                } else if Self::compare_op(*op) {
                    self.want(*dst, IrTy::Bool)?;
                } else {
                    return Err("logical operator on int".into());
                }
            }
            Inst::FloatOp { op, dst, a, b, .. } => {
                self.want(*a, IrTy::F64)?;
                self.want(*b, IrTy::F64)?;
                if Self::numeric_op(*op) {
                    self.want(*dst, IrTy::F64)?;
                } else if Self::compare_op(*op) {
                    self.want(*dst, IrTy::Bool)?;
                } else {
                    return Err("logical operator on double".into());
                }
            }
            Inst::BoolOp { op, dst, a, b, .. } => {
                self.want(*a, IrTy::Bool)?;
                self.want(*b, IrTy::Bool)?;
                self.want(*dst, IrTy::Bool)?;
                if !matches!(op, BinOp::Eq | BinOp::Ne) {
                    return Err("boolean op other than == or !=".into());
                }
            }
            Inst::StrOp { a, b, .. } => {
                if self.ty(*a)? != IrTy::Str && self.ty(*b)? != IrTy::Str {
                    return Err("string op without a string operand".into());
                }
            }
            Inst::DynOp { a, b, .. } => {
                self.ty(*a)?;
                self.ty(*b)?;
            }
            Inst::Unary { dst, src, .. } | Inst::Cast { dst, src, .. } => {
                self.ty(*dst)?;
                self.ty(*src)?;
            }
            Inst::Index { index, base, .. } => {
                self.want(*index, IrTy::I64)?;
                self.ty(*base)?;
            }
            Inst::View { src, .. } => self.want(*src, IrTy::Bits)?,
            Inst::NewArray { dst, dims, .. } => {
                for d in dims.iter() {
                    self.want(*d, IrTy::I64)?;
                }
                self.ty(*dst)?;
            }
            Inst::ArrayLit { dst, elems, .. } => {
                for a in elems.iter() {
                    self.ty(*a)?;
                }
                self.ty(*dst)?;
            }
            Inst::NewStruct { dst, args, .. } => {
                for a in args.iter() {
                    self.ty(*a)?;
                }
                self.ty(*dst)?;
            }
            Inst::NewEnum { dst, args, .. } => {
                for a in args.iter() {
                    self.ty(*a)?;
                }
                self.want_enum(*dst)?;
            }
            Inst::EnumTag { dst, src, .. } => {
                self.want_enum(*src)?;
                self.want(*dst, IrTy::I64)?;
            }
            Inst::EnumField { dst, src, .. } => {
                self.want_enum(*src)?;
                self.ty(*dst)?;
            }
            Inst::GetField { dst, recv, .. } => {
                self.ty(*recv)?;
                self.ty(*dst)?;
            }
            Inst::SetField { base, value, .. } => {
                self.ty(*base)?;
                self.ty(*value)?;
            }
            Inst::SetIndex {
                base, index, value, ..
            } => {
                self.ty(*base)?;
                self.want(*index, IrTy::I64)?;
                self.ty(*value)?;
            }
            Inst::StrMethod {
                dst, recv, args, ..
            } => {
                self.want(*recv, IrTy::Str)?;
                for a in args.iter() {
                    self.ty(*a)?;
                }
                self.ty(*dst)?;
            }
            Inst::QAlloc { dst, .. } => self.want(*dst, IrTy::Qubit)?,
            Inst::QAllocReg { dst, size, .. } => {
                self.want(*size, IrTy::I64)?;
                self.want(*dst, IrTy::QReg)?;
            }
            Inst::Gate { q, .. } => self.want(*q, IrTy::Qubit)?,
            Inst::Rotate { q, angle, .. } => {
                self.want(*q, IrTy::Qubit)?;
                if !matches!(self.ty(*angle)?, IrTy::I64 | IrTy::F64) {
                    return Err("rotation angle is not a number".into());
                }
            }
            Inst::Cx { a, b, .. } => {
                self.want(*a, IrTy::Qubit)?;
                self.want(*b, IrTy::Qubit)?;
            }
            Inst::Ccx { a, b, t, .. } => {
                self.want(*a, IrTy::Qubit)?;
                self.want(*b, IrTy::Qubit)?;
                self.want(*t, IrTy::Qubit)?;
            }
            Inst::Measure { dst, src, .. } => match self.ty(*src)? {
                IrTy::Qubit => self.want(*dst, IrTy::Bit)?,
                IrTy::QReg => self.want(*dst, IrTy::Bits)?,
                t => return Err(format!("measure of {}", t.name())),
            },
            Inst::Discard { src, .. } => {
                if !self.ty(*src)?.is_quantum() {
                    return Err("discard of a classical value".into());
                }
            }
            Inst::Len { dst, src, .. } => {
                self.want(*dst, IrTy::I64)?;
                if !matches!(
                    self.ty(*src)?,
                    IrTy::Str | IrTy::QReg | IrTy::Bits | IrTy::Array(_)
                ) {
                    return Err("len of a value without a length".into());
                }
            }
            Inst::Print { fmt, args, .. } => {
                let pieces = self
                    .f
                    .formats
                    .get(*fmt as usize)
                    .ok_or("format out of range")?;
                let holes = pieces.iter().filter(|p| **p == FmtPiece::Hole).count();
                if holes != args.len() {
                    return Err("print argument count".into());
                }
                for a in args.iter() {
                    if matches!(self.ty(*a)?, IrTy::Qubit | IrTy::QReg | IrTy::Unit) {
                        return Err(format!("%{a} is not printable"));
                    }
                }
            }
            Inst::HostText { src, .. } => self.want(*src, IrTy::Str)?,
            Inst::StdCall { args, .. } => {
                for (a, _) in args.iter() {
                    if !matches!(self.ty(*a)?, IrTy::I64 | IrTy::F64 | IrTy::Bool | IrTy::Str) {
                        return Err(format!("%{a} cannot be passed to the host"));
                    }
                }
            }
            Inst::Call {
                func, dst, args, ..
            } => {
                let callee = self
                    .m
                    .funcs
                    .get(*func as usize)
                    .ok_or("call of an unknown function")?;
                if callee.host {
                    return Err(format!("call of host function {}", callee.name));
                }
                if callee.params.len() != args.len() {
                    return Err(format!(
                        "call of {} with {} arguments",
                        callee.name,
                        args.len()
                    ));
                }
                for (a, p) in args.iter().zip(&callee.params) {
                    self.want(*a, p.ty)?;
                }
                self.want(*dst, callee.ret)?;
            }
            Inst::IfBit { bit, then, els, .. } => {
                self.want(*bit, IrTy::Bit)?;
                self.block(*then)?;
                if let Some(e) = els {
                    self.block(*e)?;
                }
            }
        }
        Ok(())
    }

    fn term(&self, t: &Term) -> V {
        match t {
            Term::Jump(b) => self.block(*b),
            Term::Br { cond, then, els } => {
                self.want(*cond, IrTy::Bool)?;
                self.block(*then)?;
                self.block(*els)
            }
            Term::Ret(Some(r)) => self.want(*r, self.f.ret),
            Term::Ret(None) if self.f.ret == IrTy::Unit => Ok(()),
            Term::Ret(None) => Err("`ret` without a value in a non-void function".into()),
            Term::FallOff { .. } if self.f.ret == IrTy::Unit => {
                Err("fall-off trap in a void function".into())
            }
            Term::RegionEnd | Term::FallOff { .. } | Term::Trap(_) => Ok(()),
        }
    }

    // Blocks reached from the entry stay outside regions; blocks reached from a region entry
    // are inside one (and never both). Regions end with `end.region` and never return.
    fn regions(&self) -> V {
        let n = self.f.blocks.len();
        let succ = |b: usize| -> Vec<usize> {
            match &self.f.blocks[b].term {
                Term::Jump(t) => vec![*t as usize],
                Term::Br { then, els, .. } => vec![*then as usize, *els as usize],
                _ => Vec::new(),
            }
        };
        // 0 = unseen, 1 = main flow, 2 = region.
        let mut kind = vec![0u8; n];
        let mut stack = vec![(0usize, 1u8)];
        while let Some((b, k)) = stack.pop() {
            if kind[b] == k {
                continue;
            }
            if kind[b] != 0 {
                return Err(format!("bb{b} is both inside and outside a region"));
            }
            kind[b] = k;
            for i in &self.f.blocks[b].insts {
                if let Inst::IfBit { then, els, .. } = i {
                    stack.push((*then as usize, 2));
                    if let Some(e) = els {
                        stack.push((*e as usize, 2));
                    }
                }
            }
            for s in succ(b) {
                stack.push((s, k));
            }
        }
        for (b, blk) in self.f.blocks.iter().enumerate() {
            match (kind[b], &blk.term) {
                (0, _) => return Err(format!("bb{b} is unreachable")),
                (1, Term::RegionEnd) => return Err(format!("bb{b}: end.region outside a region")),
                (2, Term::Ret(_) | Term::FallOff { .. }) => {
                    return Err(format!("bb{b}: return inside a region"))
                }
                _ => {}
            }
            if kind[b] == 2 {
                for i in &blk.insts {
                    if unconditional(i) {
                        return Err(format!("bb{b}: {i:?} inside a region"));
                    }
                    if let Some(func) = calls_unsafe(self.m, i, self.safe) {
                        let name = &self.m.funcs[func as usize].name;
                        return Err(format!("bb{b}: call of {name} inside a region"));
                    }
                }
            }
        }
        Ok(())
    }
}
