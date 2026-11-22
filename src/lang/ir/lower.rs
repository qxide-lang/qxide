// Lowering of a checked AST to the typed IR. Everything the tree-walking interpreter does at run
// time happens in the same order here: one step per expression node charged before its operands
// (pre-order), the number of variables a name lookup scans, one step per loop iteration, and
// every runtime error at the same span. Lowering never fails: code the checker would reject
// becomes a `Term::Trap` with the diagnostic the interpreter would report when reaching it.

use std::collections::HashMap;
use std::rc::Rc;

use super::super::ast::{
    Arm, BinOp, Block as AstBlock, Else, Expr, ExprKind, FmtPiece, FnDecl, Ident, IfStmt, PatKind,
    Program, Stmt, Ty, UnOp,
};
use super::super::check;
use super::super::diag::{Code, Diagnostic, Span};
use super::super::machine::{internal, Gate1, Rot};
use super::super::msg;
use super::super::pure;
use super::*;

struct Sig {
    params: Vec<Ty>,
    ret: Ty,
}

pub fn lower(p: &Program) -> Module {
    let mut index = HashMap::new();
    let mut sigs = HashMap::new();
    // Like the interpreter's name map, a later duplicate replaces an earlier one.
    for (i, f) in p.fns.iter().enumerate() {
        index.insert(f.name.name.clone(), i as FuncId);
        sigs.insert(
            f.name.name.as_str(),
            Sig {
                params: f.params.iter().map(|p| p.ty).collect(),
                ret: f.ret,
            },
        );
    }
    let layouts = super::super::record::Layouts::of(p);
    let funcs = p
        .fns
        .iter()
        .map(|f| FnLower::new(f, &index, &sigs, &layouts).lower(f))
        .collect();
    Module {
        funcs,
        index,
        layouts,
    }
}

struct FnLower<'p, 'a> {
    index: &'a HashMap<String, FuncId>,
    sigs: &'a HashMap<&'p str, Sig>,
    func: Func,
    blocks: Vec<(Vec<Inst>, Option<Term>)>,
    cur: usize,
    // Mirrors the interpreter's frame: (name, slot) per scope, innermost last.
    scopes: Vec<Vec<(&'p str, SlotId)>>,
    // Inside an `if` on a measured bit: `return` ends the region like in the interpreter.
    regions: usize,
    // Slots named after each base name so far, for unique dump names (`i`, `i.1`, ...).
    slot_names: HashMap<String, u32>,
    // Enclosing loops: (`continue` target, `break` target, `regions` at the loop).
    loops: Vec<(BlockId, BlockId, usize)>,
    layouts: &'a super::super::record::Layouts,
}

impl<'p, 'a> FnLower<'p, 'a> {
    fn new(
        f: &'p FnDecl,
        index: &'a HashMap<String, FuncId>,
        sigs: &'a HashMap<&'p str, Sig>,
        layouts: &'a super::super::record::Layouts,
    ) -> Self {
        let params = f
            .params
            .iter()
            .map(|p| Param {
                ty: IrTy::of(p.ty),
                mode: match (p.ty.is_quantum(), p.consume.is_some()) {
                    (false, _) => Mode::Copy,
                    (true, false) => Mode::Borrow,
                    (true, true) => Mode::Consume,
                },
            })
            .collect();
        FnLower {
            index,
            sigs,
            func: Func {
                name: f.name.name.clone(),
                span: f.name.span,
                params,
                ret: IrTy::of(f.ret),
                param_tys: f.params.iter().map(|p| p.ty).collect(),
                slots: Vec::new(),
                regs: Vec::new(),
                blocks: Vec::new(),
                charges: Vec::new(),
                formats: Vec::new(),
                host: f.name.name.starts_with("std."),
            },
            blocks: vec![(Vec::new(), None)],
            cur: 0,
            scopes: Vec::new(),
            regions: 0,
            slot_names: HashMap::new(),
            loops: Vec::new(),
            layouts,
        }
    }

    fn lower(mut self, f: &'p FnDecl) -> Func {
        let mut first = Vec::new();
        for p in &f.params {
            let s = self.slot(&p.name.name, IrTy::of(p.ty));
            first.push((p.name.name.as_str(), s));
        }
        self.scopes.push(first);
        if self.func.host {
            // Std calls go to the host; the interpreter never runs a stub body.
            self.terminate(Term::Trap(Box::new(internal(f.name.span))));
        } else {
            self.block(&f.body);
            if f.ret == Ty::Void {
                self.terminate(Term::Ret(None));
            } else {
                self.terminate(Term::FallOff { span: f.body.close });
            }
        }
        self.finish()
    }

    // Drops unreachable blocks and renumbers the rest in creation order.
    fn finish(mut self) -> Func {
        let n = self.blocks.len();
        let mut seen = vec![false; n];
        let mut stack = vec![0usize];
        while let Some(b) = stack.pop() {
            if std::mem::replace(&mut seen[b], true) {
                continue;
            }
            let (insts, term) = &self.blocks[b];
            for i in insts {
                if let Inst::IfBit { then, els, .. } = i {
                    stack.push(*then as usize);
                    stack.extend(els.map(|e| e as usize));
                }
            }
            match term {
                Some(Term::Jump(t)) => stack.push(*t as usize),
                Some(Term::Br { then, els, .. }) => {
                    stack.push(*then as usize);
                    stack.push(*els as usize);
                }
                _ => {}
            }
        }
        let mut map = vec![u32::MAX; n];
        let mut next = 0;
        for (b, keep) in seen.iter().enumerate() {
            if *keep {
                map[b] = next;
                next += 1;
            }
        }
        let remap = |b: &mut BlockId| *b = map[*b as usize];
        for (b, (mut insts, term)) in self.blocks.into_iter().enumerate() {
            if !seen[b] {
                continue;
            }
            for i in &mut insts {
                if let Inst::IfBit { then, els, .. } = i {
                    remap(then);
                    if let Some(e) = els {
                        remap(e);
                    }
                }
            }
            let mut term = term.expect("reachable block is terminated");
            match &mut term {
                Term::Jump(t) => remap(t),
                Term::Br { then, els, .. } => {
                    remap(then);
                    remap(els);
                }
                _ => {}
            }
            self.func.blocks.push(Block { insts, term });
        }
        self.func
    }

    // --- builder -------------------------------------------------------------------------

    // Source names have no `.`, so `name.k` never collides with another base name.
    fn slot(&mut self, name: &str, ty: IrTy) -> SlotId {
        let k = self.slot_names.entry(name.to_string()).or_insert(0);
        let unique = match *k {
            0 => name.to_string(),
            k => format!("{name}.{k}"),
        };
        *k += 1;
        self.func.slots.push(Slot { name: unique, ty });
        (self.func.slots.len() - 1) as SlotId
    }

    fn reg(&mut self, ty: IrTy) -> Reg {
        self.func.regs.push(ty);
        (self.func.regs.len() - 1) as Reg
    }

    fn ty(&self, r: Reg) -> IrTy {
        self.func.regs[r as usize]
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push((Vec::new(), None));
        self.blocks.len() - 1
    }

    fn emit(&mut self, i: Inst) {
        self.blocks[self.cur].0.push(i);
    }

    // Ends the current block; anything lowered after it lands in a fresh unreachable block.
    fn terminate(&mut self, t: Term) {
        if self.blocks[self.cur].1.is_none() {
            self.blocks[self.cur].1 = Some(t);
        }
        self.cur = self.new_block();
    }

    fn switch(&mut self, b: usize) {
        self.cur = b;
    }

    fn trap(&mut self, d: Diagnostic) {
        self.terminate(Term::Trap(Box::new(d)));
    }

    // One `Machine::charge(n, at)` of the interpreter, merged with an immediately preceding one.
    fn charge(&mut self, n: u64, at: Span) {
        let end = self.func.charges.len() as u32;
        if let Some(Inst::Charge { total, first, len }) = self.blocks[self.cur].0.last_mut() {
            if *first + *len == end {
                *total = total.saturating_add(n);
                *len += 1;
                self.func.charges.push((n, at));
                return;
            }
        }
        self.func.charges.push((n, at));
        self.emit(Inst::Charge {
            total: n,
            first: end,
            len: 1,
        });
    }

    // Index and type of field `name` of the struct in register `r`.
    fn field_of(&self, r: Reg, name: &str) -> Option<(u32, Ty)> {
        let IrTy::Struct(id) = self.ty(r) else {
            return None;
        };
        self.layouts
            .field(Ty::Struct(id), name)
            .map(|(i, t)| (i as u32, t))
    }

    fn konst(&mut self, value: Const) -> Reg {
        let ty = match value {
            Const::I64(_) => IrTy::I64,
            Const::F64(_) => IrTy::F64,
            Const::Bool(_) => IrTy::Bool,
        };
        let dst = self.reg(ty);
        self.emit(Inst::Const { dst, value });
        dst
    }

    // Java widening of an `int` stored into a `double` slot, parameter or return value.
    fn widen_to(&mut self, r: Reg, want: IrTy) -> Reg {
        if want == IrTy::F64 && self.ty(r) == IrTy::I64 {
            let dst = self.reg(IrTy::F64);
            self.emit(Inst::Widen { dst, src: r });
            dst
        } else {
            r
        }
    }

    // `Frame::get`/`Frame::set`: innermost scope first, latest declaration first; the count of
    // variables compared is what the interpreter charges.
    fn lookup(&self, name: &str) -> (Option<SlotId>, u64) {
        let mut scanned = 0;
        for scope in self.scopes.iter().rev() {
            for (n, s) in scope.iter().rev() {
                scanned += 1;
                if *n == name {
                    return (Some(*s), scanned);
                }
            }
        }
        (None, scanned)
    }

    // --- statements ----------------------------------------------------------------------

    fn block(&mut self, b: &'p AstBlock) {
        self.scopes.push(Vec::new());
        for s in &b.stmts {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &'p Stmt) {
        match s {
            Stmt::Decl { name, init, ty, .. } => self.decl(name, init, *ty),
            Stmt::Assign { name, value } => self.assign(name, value),
            Stmt::If(i) => self.if_stmt(i),
            Stmt::For {
                var,
                start,
                end,
                body,
            } => self.for_stmt(var, start, end, body),
            Stmt::While { cond, body, span } => self.while_stmt(cond, body, *span),
            Stmt::SetIndex {
                base,
                index,
                op,
                value,
                span,
            } => {
                let base = self.expr(base);
                let index = self.expr(index);
                let value = self.expr(value);
                self.emit(Inst::SetIndex {
                    base,
                    index,
                    op: *op,
                    value,
                    span: *span,
                });
            }
            Stmt::SetField {
                base,
                name,
                op,
                value,
                span,
            } => {
                let base = self.expr(base);
                let value = self.expr(value);
                match self.field_of(base, &name.name) {
                    Some((index, _)) => self.emit(Inst::SetField {
                        base,
                        index,
                        op: *op,
                        value,
                        span: *span,
                    }),
                    None => self.trap(internal(*span)),
                }
            }
            Stmt::Match {
                scrutinee,
                arms,
                span,
            } => self.match_stmt(scrutinee, arms, *span),
            Stmt::Break { span } => self.loop_exit(false, *span),
            Stmt::Continue { span } => self.loop_exit(true, *span),
            Stmt::Return { value, .. } => self.return_stmt(value.as_ref()),
            Stmt::Expr(e) => {
                self.expr(e);
            }
        }
    }

    fn decl(&mut self, name: &'p Ident, init: &'p Expr, ty: Option<Ty>) {
        let v = self.expr(init);
        // `var` takes the type of its (classical) initializer.
        let slot_ty = match ty {
            Some(t) => IrTy::of(t),
            None => match self.ty(v) {
                IrTy::F64 => IrTy::F64,
                IrTy::Bool => IrTy::Bool,
                IrTy::Str => IrTy::Str,
                t @ (IrTy::Array(_) | IrTy::Struct(_) | IrTy::Enum(_)) => t,
                _ => IrTy::I64,
            },
        };
        let v = self.widen_to(v, slot_ty);
        let slot = self.slot(&name.name, slot_ty);
        self.emit(Inst::Store { slot, src: v });
        self.scopes
            .last_mut()
            .expect("function scope")
            .push((&name.name, slot));
    }

    fn assign(&mut self, name: &'p Ident, value: &'p Expr) {
        let v = self.expr(value);
        let (slot, scanned) = self.lookup(&name.name);
        if let Some(slot) = slot {
            let v = self.widen_to(v, self.func.slots[slot as usize].ty);
            self.emit(Inst::Store { slot, src: v });
        }
        self.charge(scanned, name.span);
    }

    // `for v in a..b`: bounds evaluated once; a hidden counter drives the loop, so assigning
    // `v` in the body does not change the iteration (as with the interpreter's Rust range).
    fn for_stmt(&mut self, var: &'p Ident, start: &'p Expr, end: &'p Expr, body: &'p AstBlock) {
        let a = self.expr(start);
        if self.ty(a) != IrTy::I64 {
            self.trap(internal(start.span));
            return;
        }
        let b = self.expr(end);
        if self.ty(b) != IrTy::I64 {
            self.trap(internal(end.span));
            return;
        }
        let counter = self.slot(&format!("{}.next", var.name), IrTy::I64);
        self.emit(Inst::Store {
            slot: counter,
            src: a,
        });
        let head = self.new_block();
        let body_bb = self.new_block();
        let latch = self.new_block();
        let exit = self.new_block();
        self.terminate(Term::Jump(head as BlockId));
        self.switch(head);
        let k = self.reg(IrTy::I64);
        self.emit(Inst::Load {
            dst: k,
            slot: counter,
        });
        let more = self.reg(IrTy::Bool);
        self.emit(Inst::IntOp {
            op: BinOp::Lt,
            dst: more,
            a: k,
            b,
            span: var.span,
        });
        self.blocks[head].1 = Some(Term::Br {
            cond: more,
            then: body_bb as BlockId,
            els: exit as BlockId,
        });
        self.switch(body_bb);
        // An empty body still costs a step per iteration (B-01).
        self.charge(1, var.span);
        let v = self.slot(&var.name, IrTy::I64);
        self.emit(Inst::Store { slot: v, src: k });
        self.scopes.push(vec![(&var.name, v)]);
        self.loops
            .push((latch as BlockId, exit as BlockId, self.regions));
        self.block(body);
        self.loops.pop();
        self.scopes.pop();
        self.terminate(Term::Jump(latch as BlockId));
        self.switch(latch);
        // `k < b <= i64::MAX`, so the increment cannot overflow.
        let one = self.konst(Const::I64(1));
        let next = self.reg(IrTy::I64);
        self.emit(Inst::IntOp {
            op: BinOp::Add,
            dst: next,
            a: k,
            b: one,
            span: var.span,
        });
        self.emit(Inst::Store {
            slot: counter,
            src: next,
        });
        self.terminate(Term::Jump(head as BlockId));
        self.switch(exit);
    }

    // `while cond { body }`: the condition runs at the head of every iteration, then one step
    // per iteration at the `while` keyword, as in the interpreter.
    fn while_stmt(&mut self, cond: &'p Expr, body: &'p AstBlock, span: Span) {
        let head = self.new_block();
        let body_bb = self.new_block();
        let exit = self.new_block();
        self.terminate(Term::Jump(head as BlockId));
        self.switch(head);
        let c = self.expr(cond);
        if self.ty(c) != IrTy::Bool {
            self.trap(internal(cond.span));
            self.switch(exit);
            return;
        }
        self.terminate(Term::Br {
            cond: c,
            then: body_bb as BlockId,
            els: exit as BlockId,
        });
        self.switch(body_bb);
        self.charge(1, span);
        self.loops
            .push((head as BlockId, exit as BlockId, self.regions));
        self.block(body);
        self.loops.pop();
        self.terminate(Term::Jump(head as BlockId));
        self.switch(exit);
    }

    // `match`: the value once, then per arm one step at its pattern and a test that branches to
    // the arm or to the next test, like the interpreter trying arms in order. Enum arms compare
    // the variant tag; bindings load payload fields into slots of a scope around the arm.
    fn match_stmt(&mut self, scrutinee: &'p Expr, arms: &'p [Arm], span: Span) {
        let v = self.expr(scrutinee);
        let tag = match self.ty(v) {
            IrTy::Enum(_) => {
                let d = self.reg(IrTy::I64);
                self.emit(Inst::EnumTag {
                    dst: d,
                    src: v,
                    span: scrutinee.span,
                });
                Some(d)
            }
            _ => None,
        };
        let exit = self.new_block();
        for arm in arms {
            let p = &arm.pat;
            self.charge(1, p.span);
            let cond = match (&p.kind, self.ty(v), tag) {
                (PatKind::Wild, _, _) => None,
                (PatKind::Int(n), IrTy::I64, _) => {
                    let k = self.konst(Const::I64(*n));
                    let d = self.reg(IrTy::Bool);
                    self.emit(Inst::IntOp {
                        op: BinOp::Eq,
                        dst: d,
                        a: v,
                        b: k,
                        span: p.span,
                    });
                    Some(d)
                }
                (PatKind::Bool(b), IrTy::Bool, _) => {
                    let k = self.konst(Const::Bool(*b));
                    let d = self.reg(IrTy::Bool);
                    self.emit(Inst::BoolOp {
                        op: BinOp::Eq,
                        dst: d,
                        a: v,
                        b: k,
                        span: p.span,
                    });
                    Some(d)
                }
                (PatKind::Str(pieces), IrTy::Str, _) => {
                    let k = self.str_lit(pieces, p.span);
                    let d = self.reg(IrTy::Bool);
                    self.emit(Inst::StrOp {
                        op: BinOp::Eq,
                        dst: d,
                        a: v,
                        b: k,
                        span: p.span,
                    });
                    Some(d)
                }
                (PatKind::Variant { name, .. }, IrTy::Enum(id), Some(tag)) => {
                    let Some((t, _)) = self.layouts.variant(Ty::Enum(id), &name.name) else {
                        self.trap(internal(p.span));
                        break;
                    };
                    let k = self.konst(Const::I64(i64::from(t)));
                    let d = self.reg(IrTy::Bool);
                    self.emit(Inst::IntOp {
                        op: BinOp::Eq,
                        dst: d,
                        a: tag,
                        b: k,
                        span: p.span,
                    });
                    Some(d)
                }
                _ => {
                    self.trap(internal(p.span));
                    break;
                }
            };
            let body = self.new_block();
            let next = self.new_block();
            match cond {
                Some(c) => self.terminate(Term::Br {
                    cond: c,
                    then: body as BlockId,
                    els: next as BlockId,
                }),
                None => self.terminate(Term::Jump(body as BlockId)),
            }
            self.switch(body);
            let mut scope = Vec::new();
            if let (PatKind::Variant { name, binds, .. }, IrTy::Enum(id)) = (&p.kind, self.ty(v)) {
                let tys = self
                    .layouts
                    .variant(Ty::Enum(id), &name.name)
                    .map(|(_, t)| t)
                    .unwrap_or_default();
                for (i, b) in binds.iter().enumerate() {
                    if b.name == "_" {
                        continue;
                    }
                    let t = IrTy::of(tys.get(i).copied().unwrap_or(Ty::Error));
                    let r = self.reg(t);
                    self.emit(Inst::EnumField {
                        dst: r,
                        src: v,
                        index: i as u32,
                        span: b.span,
                    });
                    let slot = self.slot(&b.name, t);
                    self.emit(Inst::Store { slot, src: r });
                    scope.push((b.name.as_str(), slot));
                }
            }
            self.scopes.push(scope);
            self.block(&arm.body);
            self.scopes.pop();
            self.terminate(Term::Jump(exit as BlockId));
            self.switch(next);
        }
        // The checker only accepts exhaustive matches.
        self.trap(internal(span));
        self.switch(exit);
    }

    // `break` jumps to the loop exit, `continue` to the next iteration. The checker rejects
    // both outside a loop and inside an `if` on a measured bit within the loop.
    fn loop_exit(&mut self, cont: bool, span: Span) {
        match self.loops.last() {
            Some(&(next, exit, regions)) if regions == self.regions => {
                self.terminate(Term::Jump(if cont { next } else { exit }));
            }
            _ => self.trap(internal(span)),
        }
    }

    // `e?`: branch on the tag; the failing side builds the function's `Err`/`None` and returns.
    fn try_expr(&mut self, inner: &'p Expr, span: Span) -> Reg {
        let v = self.expr(inner);
        let (IrTy::Enum(id), IrTy::Enum(ret)) = (self.ty(v), self.func.ret) else {
            self.trap(internal(span));
            return self.reg(IrTy::Unit);
        };
        let (ty, ret) = (Ty::Enum(id), Ty::Enum(ret));
        let found = super::super::record::try_tags(self.layouts, ty, ret);
        let payload = self.layouts.payload(ty, found.map_or(0, |(ok, _)| ok));
        let (Some((ok, fail)), Some(payload), 0) = (found, payload, self.regions) else {
            self.trap(internal(span));
            return self.reg(IrTy::Unit);
        };
        let tag = self.reg(IrTy::I64);
        self.emit(Inst::EnumTag {
            dst: tag,
            src: v,
            span,
        });
        let k = self.konst(Const::I64(i64::from(ok)));
        let cond = self.reg(IrTy::Bool);
        self.emit(Inst::IntOp {
            op: BinOp::Eq,
            dst: cond,
            a: tag,
            b: k,
            span,
        });
        let (yes, no) = (self.new_block(), self.new_block());
        self.terminate(Term::Br {
            cond,
            then: yes as BlockId,
            els: no as BlockId,
        });
        self.switch(no);
        let carried = self.layouts.payload(ret, fail).unwrap_or_default();
        let mut args = Vec::new();
        if let Some(&t) = carried.first() {
            let r = self.reg(IrTy::of(t));
            self.emit(Inst::EnumField {
                dst: r,
                src: v,
                index: 0,
                span,
            });
            args.push(r);
        }
        let out = self.reg(IrTy::Enum(match ret {
            Ty::Enum(r) => r,
            _ => id,
        }));
        self.emit(Inst::NewEnum {
            dst: out,
            ty: ret,
            tag: fail,
            args: args.into(),
            span,
        });
        self.terminate(Term::Ret(Some(out)));
        self.switch(yes);
        let dst = self.reg(IrTy::of(payload.first().copied().unwrap_or(Ty::Error)));
        self.emit(Inst::EnumField {
            dst,
            src: v,
            index: 0,
            span,
        });
        dst
    }

    fn return_stmt(&mut self, value: Option<&'p Expr>) {
        let v = value.map(|e| self.expr(e));
        if self.regions > 0 {
            // The interpreter drops a `return` inside an `if` on a bit: the region just ends.
            self.terminate(Term::RegionEnd);
            return;
        }
        let v = v.map(|r| self.widen_to(r, self.func.ret));
        self.terminate(Term::Ret(v));
    }

    fn if_stmt(&mut self, i: &'p IfStmt) {
        let c = self.expr(&i.cond);
        match self.ty(c) {
            IrTy::Bool => {
                let then = self.new_block();
                let els = self.new_block();
                let join = self.new_block();
                self.terminate(Term::Br {
                    cond: c,
                    then: then as BlockId,
                    els: els as BlockId,
                });
                self.switch(then);
                self.block(&i.then);
                self.terminate(Term::Jump(join as BlockId));
                self.switch(els);
                match &i.els {
                    Some(Else::Block(b)) => self.block(b),
                    Some(Else::If(inner)) => self.if_stmt(inner),
                    None => {}
                }
                self.terminate(Term::Jump(join as BlockId));
                self.switch(join);
            }
            IrTy::Bit => {
                let back = self.cur;
                let then = self.region(|l| l.block(&i.then));
                let els = i.els.as_ref().map(|e| {
                    self.region(|l| match e {
                        Else::Block(b) => l.block(b),
                        Else::If(inner) => l.if_stmt(inner),
                    })
                });
                self.switch(back);
                self.emit(Inst::IfBit {
                    bit: c,
                    then,
                    els,
                    span: i.cond.span,
                });
            }
            _ => self.trap(internal(i.cond.span)),
        }
    }

    fn region(&mut self, body: impl FnOnce(&mut Self)) -> BlockId {
        let entry = self.new_block();
        self.switch(entry);
        self.regions += 1;
        body(self);
        self.regions -= 1;
        self.terminate(Term::RegionEnd);
        entry as BlockId
    }

    // --- expressions ---------------------------------------------------------------------

    fn expr(&mut self, e: &'p Expr) -> Reg {
        self.charge(1, e.span);
        match &e.kind {
            ExprKind::Int(n) => self.konst(Const::I64(*n)),
            ExprKind::Double(v) => self.konst(Const::F64(*v)),
            ExprKind::Bool(b) => self.konst(Const::Bool(*b)),
            ExprKind::Str(pieces) => self.str_lit(pieces, e.span),
            ExprKind::Var(name) => self.var(name, e.span),
            ExprKind::Call { name, args } => self.call(name, args, e.span),
            ExprKind::Index { base, index } => {
                let b = self.expr(base);
                let i = self.expr(index);
                if self.ty(i) != IrTy::I64 {
                    self.trap(internal(index.span));
                    return self.reg(IrTy::Unit);
                }
                let ty = match self.ty(b) {
                    IrTy::QReg => IrTy::Qubit,
                    IrTy::Bits => IrTy::Bit,
                    t @ IrTy::Array(_) => t.elem().unwrap_or(IrTy::Unit),
                    _ => IrTy::Unit,
                };
                let dst = self.reg(ty);
                self.emit(Inst::Index {
                    dst,
                    base: b,
                    index: i,
                    span: e.span,
                });
                dst
            }
            ExprKind::NewArray { ty, dims } => {
                let dims: Box<[Reg]> = dims.iter().map(|d| self.expr(d)).collect();
                let dst = self.reg(IrTy::of(*ty));
                self.emit(Inst::NewArray {
                    dst,
                    ty: *ty,
                    dims,
                    span: e.span,
                });
                dst
            }
            ExprKind::NewStruct { ty, args } => {
                let tys: Vec<Ty> = self
                    .layouts
                    .fields(*ty)
                    .map(|f| f.iter().map(|(_, t)| *t).collect())
                    .unwrap_or_default();
                let mut regs = Vec::with_capacity(args.len());
                for (a, t) in args
                    .iter()
                    .zip(tys.iter().chain(std::iter::repeat(&Ty::Error)))
                {
                    let r = self.expr(a);
                    regs.push(self.widen_to(r, IrTy::of(*t)));
                }
                let dst = self.reg(IrTy::of(*ty));
                self.emit(Inst::NewStruct {
                    dst,
                    ty: *ty,
                    args: regs.into(),
                    span: e.span,
                });
                dst
            }
            ExprKind::Variant { ty, name, args } => {
                let found = self.layouts.variant(*ty, &name.name);
                let tys = found.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
                let mut regs = Vec::with_capacity(args.len());
                for (a, t) in args
                    .iter()
                    .zip(tys.iter().chain(std::iter::repeat(&Ty::Error)))
                {
                    let r = self.expr(a);
                    regs.push(self.widen_to(r, IrTy::of(*t)));
                }
                let Some((tag, _)) = found else {
                    self.trap(internal(e.span));
                    return self.reg(IrTy::Unit);
                };
                let dst = self.reg(IrTy::of(*ty));
                self.emit(Inst::NewEnum {
                    dst,
                    ty: *ty,
                    tag,
                    args: regs.into(),
                    span: e.span,
                });
                dst
            }
            ExprKind::Field { recv, name } => {
                let r = self.expr(recv);
                let Some((index, ty)) = self.field_of(r, &name.name) else {
                    self.trap(internal(e.span));
                    return self.reg(IrTy::Unit);
                };
                let dst = self.reg(IrTy::of(ty));
                self.emit(Inst::GetField {
                    dst,
                    recv: r,
                    index,
                    span: e.span,
                });
                dst
            }
            ExprKind::Try { expr, .. } => self.try_expr(expr, e.span),
            ExprKind::ArrayLit { ty, elems } => {
                let elems: Box<[Reg]> = elems.iter().map(|x| self.expr(x)).collect();
                let Some(ty) = *ty else {
                    self.trap(internal(e.span));
                    return self.reg(IrTy::Unit);
                };
                let dst = self.reg(IrTy::of(ty));
                self.emit(Inst::ArrayLit {
                    dst,
                    ty,
                    elems,
                    span: e.span,
                });
                dst
            }
            ExprKind::Method { recv, name, args } => {
                let v = self.expr(recv);
                if self.ty(v) == IrTy::Str {
                    let Some((name, _, ret)) = pure::string_method(&name.name) else {
                        self.trap(internal(e.span));
                        return self.reg(IrTy::Unit);
                    };
                    let args: Box<[Reg]> = args.iter().map(|a| self.expr(a)).collect();
                    let dst = self.reg(IrTy::of(ret));
                    self.emit(Inst::StrMethod {
                        name,
                        dst,
                        recv: v,
                        args,
                        span: e.span,
                    });
                    return dst;
                }
                let (name, ty) = match name.name.as_str() {
                    "as_int" => ("as_int", IrTy::ShotInt),
                    "to_binary" => ("to_binary", IrTy::ShotBinary),
                    _ => {
                        self.trap(internal(e.span));
                        return self.reg(IrTy::Unit);
                    }
                };
                let dst = self.reg(ty);
                self.emit(Inst::View {
                    dst,
                    src: v,
                    name,
                    span: e.span,
                });
                dst
            }
            ExprKind::Cast { ty, expr } => {
                let v = self.expr(expr);
                let dst = self.reg(IrTy::of(*ty));
                self.emit(Inst::Cast {
                    ty: *ty,
                    dst,
                    src: v,
                    span: e.span,
                });
                dst
            }
            ExprKind::Unary { op, expr } => {
                let v = self.expr(expr);
                let ty = match op {
                    UnOp::Neg => self.ty(v),
                    UnOp::Not => IrTy::Bool,
                };
                let dst = self.reg(ty);
                self.emit(Inst::Unary {
                    op: *op,
                    dst,
                    src: v,
                    span: e.span,
                });
                dst
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span),
        }
    }

    fn str_lit(&mut self, pieces: &[FmtPiece], at: Span) -> Reg {
        let mut text = String::new();
        for p in pieces {
            match p {
                FmtPiece::Lit(t) => text.push_str(t),
                FmtPiece::Hole => {
                    self.trap(internal(at));
                    return self.reg(IrTy::Str);
                }
            }
        }
        let dst = self.reg(IrTy::Str);
        self.emit(Inst::Str {
            dst,
            text: text.into(),
            span: at,
        });
        dst
    }

    fn var(&mut self, name: &str, at: Span) -> Reg {
        if let Some(v) = check::constant(name) {
            return self.konst(Const::F64(v));
        }
        let (slot, scanned) = self.lookup(name);
        let Some(slot) = slot else {
            self.trap(Diagnostic::new(
                Code::UnknownVar,
                at,
                msg::unknown_var(name),
            ));
            return self.reg(IrTy::Unit);
        };
        self.charge(scanned, at);
        let dst = self.reg(self.func.slots[slot as usize].ty);
        self.emit(Inst::Load { dst, slot });
        dst
    }

    fn binary(&mut self, op: BinOp, lhs: &'p Expr, rhs: &'p Expr, span: Span) -> Reg {
        let l = self.expr(lhs);
        if matches!(op, BinOp::And | BinOp::Or) && self.ty(l) == IrTy::Bool {
            return self.short_circuit(op, l, rhs, span);
        }
        let r = self.expr(rhs);
        self.binary_op(op, l, r, span)
    }

    // The interpreter's `binary_expr` after both operands are evaluated.
    fn binary_op(&mut self, op: BinOp, l: Reg, r: Reg, span: Span) -> Reg {
        use BinOp::*;
        enum Kind {
            Str,
            Int,
            Float,
            Bool,
            Dyn,
        }
        let (lt, rt) = (self.ty(l), self.ty(r));
        let cmp = matches!(op, Eq | Ne | Lt | Le | Gt | Ge);
        let numeric = |t: IrTy| matches!(t, IrTy::I64 | IrTy::F64);
        let (kind, ty) = if lt == IrTy::Str || rt == IrTy::Str {
            (Kind::Str, if op == Add { IrTy::Str } else { IrTy::Bool })
        } else if matches!(op, And | Or) {
            (Kind::Dyn, IrTy::Bool)
        } else if lt == IrTy::I64 && rt == IrTy::I64 {
            (Kind::Int, if cmp { IrTy::Bool } else { IrTy::I64 })
        } else if numeric(lt) && numeric(rt) {
            (Kind::Float, if cmp { IrTy::Bool } else { IrTy::F64 })
        } else if lt == IrTy::Bool && rt == IrTy::Bool && matches!(op, Eq | Ne) {
            (Kind::Bool, IrTy::Bool)
        } else {
            (Kind::Dyn, IrTy::Unit)
        };
        let (a, b) = match kind {
            // Mixed `int` and `double` operands compute in `double`, as in Java.
            Kind::Float => (self.widen_to(l, IrTy::F64), self.widen_to(r, IrTy::F64)),
            _ => (l, r),
        };
        let dst = self.reg(ty);
        self.emit(match kind {
            Kind::Str => Inst::StrOp {
                op,
                dst,
                a,
                b,
                span,
            },
            Kind::Int => Inst::IntOp {
                op,
                dst,
                a,
                b,
                span,
            },
            Kind::Float => Inst::FloatOp {
                op,
                dst,
                a,
                b,
                span,
            },
            Kind::Bool => Inst::BoolOp {
                op,
                dst,
                a,
                b,
                span,
            },
            Kind::Dyn => Inst::DynOp {
                op,
                dst,
                a,
                b,
                span,
            },
        });
        dst
    }

    // `a && b` / `a || b` on a `boolean` left operand: the right one runs only when needed.
    fn short_circuit(&mut self, op: BinOp, l: Reg, rhs: &'p Expr, span: Span) -> Reg {
        let tmp = self.slot(if op == BinOp::And { "and" } else { "or" }, IrTy::Bool);
        let eval = self.new_block();
        let short = self.new_block();
        let join = self.new_block();
        let (then, els) = if op == BinOp::And {
            (eval, short)
        } else {
            (short, eval)
        };
        self.terminate(Term::Br {
            cond: l,
            then: then as BlockId,
            els: els as BlockId,
        });
        self.switch(short);
        let k = self.konst(Const::Bool(op == BinOp::Or));
        self.emit(Inst::Store { slot: tmp, src: k });
        self.terminate(Term::Jump(join as BlockId));
        self.switch(eval);
        let r = self.expr(rhs);
        let v = if self.ty(r) == IrTy::Bool {
            r
        } else {
            // A non-boolean right operand fails like the interpreter's `binary`.
            let dst = self.reg(IrTy::Bool);
            self.emit(Inst::DynOp {
                op,
                dst,
                a: l,
                b: r,
                span,
            });
            dst
        };
        self.emit(Inst::Store { slot: tmp, src: v });
        self.terminate(Term::Jump(join as BlockId));
        self.switch(join);
        let dst = self.reg(IrTy::Bool);
        self.emit(Inst::Load { dst, slot: tmp });
        dst
    }

    // --- calls ---------------------------------------------------------------------------

    fn call(&mut self, name: &'p Ident, args: &'p [Expr], span: Span) -> Reg {
        let n = name.name.as_str();
        if let Some(r) = self.builtin(n, args, span) {
            return r;
        }
        let (Some(&func), Some(sig)) = (self.index.get(n), self.sigs.get(n)) else {
            self.trap(Diagnostic::new(
                Code::UnknownFn,
                name.span,
                msg::unknown_fn(n),
            ));
            return self.reg(IrTy::Unit);
        };
        let params = sig.params.clone();
        let ret = sig.ret;
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(a));
        }
        let vals: Vec<Reg> = vals
            .into_iter()
            .enumerate()
            .map(|(i, v)| match params.get(i) {
                Some(t) => self.widen_to(v, IrTy::of(*t)),
                None => v,
            })
            .collect();
        let dst = self.reg(IrTy::of(ret));
        self.emit(Inst::Call {
            func,
            dst,
            args: vals.into(),
            span,
        });
        dst
    }

    fn arg(&mut self, args: &'p [Expr], i: usize, span: Span) -> Option<&'p Expr> {
        let a = args.get(i);
        if a.is_none() {
            self.trap(internal(span));
        }
        a
    }

    // A gate operand: must evaluate to a qubit, else the interpreter's `place` fails there.
    fn place(&mut self, e: &'p Expr) -> Option<Reg> {
        let r = self.expr(e);
        if self.ty(r) == IrTy::Qubit {
            Some(r)
        } else {
            self.trap(internal(e.span));
            None
        }
    }

    fn unit(&mut self) -> Reg {
        self.reg(IrTy::Unit)
    }

    // The interpreter's `builtin`, in the same order of names.
    fn builtin(&mut self, n: &str, args: &'p [Expr], span: Span) -> Option<Reg> {
        Some(match n {
            "qubit" => {
                let dst = self.reg(IrTy::Qubit);
                self.emit(Inst::QAlloc { dst, span });
                dst
            }
            "qreg" => {
                let Some(size) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let k = self.expr(size);
                if self.ty(k) != IrTy::I64 {
                    self.trap(internal(size.span));
                    return Some(self.unit());
                }
                let dst = self.reg(IrTy::QReg);
                self.emit(Inst::QAllocReg {
                    dst,
                    size: k,
                    size_span: size.span,
                    span,
                });
                dst
            }
            "cx" | "ccx" => {
                let want = if n == "cx" { 2 } else { 3 };
                let mut q = Vec::with_capacity(want);
                for i in 0..want {
                    let Some(a) = self.arg(args, i, span) else {
                        return Some(self.unit());
                    };
                    let Some(r) = self.place(a) else {
                        return Some(self.unit());
                    };
                    q.push(r);
                }
                self.emit(if want == 2 {
                    Inst::Cx {
                        a: q[0],
                        b: q[1],
                        span,
                    }
                } else {
                    Inst::Ccx {
                        a: q[0],
                        b: q[1],
                        t: q[2],
                        span,
                    }
                });
                self.unit()
            }
            "measure" => {
                let Some(a) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let v = self.expr(a);
                let ty = match self.ty(v) {
                    IrTy::Qubit => IrTy::Bit,
                    IrTy::QReg => IrTy::Bits,
                    _ => IrTy::Unit,
                };
                let dst = self.reg(ty);
                self.emit(Inst::Measure { dst, src: v, span });
                dst
            }
            "discard" => {
                let Some(a) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let v = self.expr(a);
                self.emit(Inst::Discard { src: v, span });
                self.unit()
            }
            "len" => {
                let Some(a) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let v = self.expr(a);
                let dst = self.reg(IrTy::I64);
                self.emit(Inst::Len { dst, src: v, span });
                dst
            }
            "println" => {
                let Some(ExprKind::Str(pieces)) = args.first().map(|a| &a.kind) else {
                    self.trap(internal(span));
                    return Some(self.unit());
                };
                let mut vals = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    vals.push(self.expr(a));
                }
                self.func.formats.push(pieces.clone());
                let fmt = (self.func.formats.len() - 1) as u32;
                self.emit(Inst::Print {
                    fmt,
                    args: vals.into(),
                    span,
                });
                self.unit()
            }
            _ if check::ROTATIONS.contains(&n) => {
                let Some(a) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let Some(q) = self.place(a) else {
                    return Some(self.unit());
                };
                let Some(b) = self.arg(args, 1, span) else {
                    return Some(self.unit());
                };
                let angle = self.expr(b);
                let rot = Rot::from_name(n).expect("rotation");
                self.emit(Inst::Rotate {
                    rot,
                    q,
                    angle,
                    angle_span: b.span,
                    span,
                });
                self.unit()
            }
            _ if check::ONE_QUBIT_OPS.contains(&n) => {
                let Some(a) = self.arg(args, 0, span) else {
                    return Some(self.unit());
                };
                let Some(q) = self.place(a) else {
                    return Some(self.unit());
                };
                let gate = Gate1::from_name(n).expect("one-qubit gate");
                self.emit(Inst::Gate { gate, q, span });
                self.unit()
            }
            _ if n.starts_with("std.") => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    let v = self.expr(a);
                    // `Machine::std_arg` converts (and charges) each argument right after it is
                    // evaluated; only build-time values reach the host.
                    match self.ty(v) {
                        IrTy::Str => self.emit(Inst::HostText {
                            src: v,
                            span: a.span,
                        }),
                        IrTy::I64 | IrTy::F64 | IrTy::Bool => {}
                        _ => {
                            self.trap(internal(a.span));
                            return Some(self.unit());
                        }
                    }
                    vals.push((v, a.span));
                }
                let ret = self.sigs.get(n).map_or(Ty::Void, |s| s.ret);
                let dst = self.reg(IrTy::of(ret));
                self.emit(Inst::StdCall {
                    name: Rc::from(n),
                    dst,
                    args: vals.into(),
                    span,
                });
                dst
            }
            _ => return None,
        })
    }
}
