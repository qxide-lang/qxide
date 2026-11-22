// Cranelift JIT for the build-time classical functions of a lowered module (docs/design/qx-jit.md).
// A function is compiled when it only handles `int`, `double` and `boolean` values and only calls
// such functions; everything else stays in the IR interpreter. Native code charges the same steps
// at the same points as the interpreter (a fast inline check, else `Machine::charge_all` through
// `qx_charge`), checks the call depth like `Machine::enter_call`, and reports the interpreter's
// diagnostics, so results, errors and budget usage are identical.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{self as cl, types, AbiParam, InstBuilder, MemFlagsData};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId as ClId, Linkage, Module as _};

use super::ast::{BinOp, Ty, UnOp};
use super::diag::{Code, Diagnostic, Span};
use super::interp::Value;
use super::ir::exec::Native;
use super::ir::{Const, Func, FuncId, Inst, IrTy, Module, Term};
use super::machine::{coerce, internal, Machine, MAX_CALL_DEPTH, R};
use super::msg;

#[cfg(test)]
mod tests;

// What native code shares with the runtime. Field offsets are used by the generated code.
#[repr(C)]
struct Ctx {
    steps: u64,
    next_poll: u64,
    max_steps: u64,
    depth: u64,
    // Return value of the last call: `i64`, `f64` bits, or a `boolean` in the low byte.
    ret: u64,
    rt: *mut Rt,
}

const STEPS: i32 = 0;
const NEXT_POLL: i32 = 8;
const MAX_STEPS: i32 = 16;
const DEPTH: i32 = 24;
const RET: i32 = 32;

// Runtime state for the helpers, valid during one native call.
struct Rt {
    machine: *mut Machine<'static>,
    charges: *const Vec<ChargeSite>,
    fails: *const Vec<Diagnostic>,
    err: Option<Diagnostic>,
}

// One `charge` of native code: the interpreter's `Machine::charge` calls it stands for.
struct ChargeSite {
    total: u64,
    parts: Vec<(u64, Span)>,
}

// The slow path of a charge: replays it on the machine (limit and cancellation poll included).
extern "C" fn qx_charge(ctx: *mut Ctx, site: u32) -> u8 {
    // SAFETY: native code passes the `Ctx` of the running `Jit::call`, whose `rt` points at a
    // live `Rt` and whose machine is not otherwise borrowed while native code runs.
    let ctx = unsafe { &mut *ctx };
    let rt = unsafe { &mut *ctx.rt };
    let m = unsafe { &mut *rt.machine };
    let s = unsafe { &(&*rt.charges)[site as usize] };
    m.set_budget(ctx.steps, ctx.next_poll);
    let r = m.charge_all(s.total, &s.parts);
    let (steps, next_poll, _) = m.budget();
    ctx.steps = steps;
    ctx.next_poll = next_poll;
    match r {
        Ok(()) => 0,
        Err(d) => {
            rt.err = Some(d);
            1
        }
    }
}

// A runtime error at a known site (overflow, division by zero, call depth, a trap).
extern "C" fn qx_fail(ctx: *mut Ctx, site: u32) -> u8 {
    // SAFETY: as in `qx_charge`.
    let rt = unsafe { &mut *(*ctx).rt };
    rt.err = Some(unsafe { &(&*rt.fails)[site as usize] }.clone());
    1
}

// `double` `%`, which Cranelift has no instruction for (Rust's `%` is C's `fmod`).
extern "C" fn qx_fmod(a: f64, b: f64) -> f64 {
    a % b
}

fn cl_ty(t: IrTy) -> Option<cl::Type> {
    match t {
        IrTy::I64 => Some(types::I64),
        IrTy::F64 => Some(types::F64),
        IrTy::Bool => Some(types::I8),
        _ => None,
    }
}

// Larger functions stay interpreted: with at most this many slots and registers a native frame
// stays a few tens of KiB, so 200 nested calls fit easily in the interpreter's thread stack.
const MAX_VALUES: usize = 4096;

// Whether a function's own code can be compiled (its callees are checked by `jitable`).
fn supported(f: &Func) -> bool {
    let reg = |r: u32| f.regs[r as usize];
    !f.host
        && f.slots.len() + f.regs.len() <= MAX_VALUES
        && f.params.iter().all(|p| cl_ty(p.ty).is_some())
        && (f.ret == IrTy::Unit || cl_ty(f.ret).is_some())
        && f.slots.iter().all(|s| cl_ty(s.ty).is_some())
        && f.regs
            .iter()
            .all(|t| *t == IrTy::Unit || cl_ty(*t).is_some())
        && f.blocks.iter().all(|b| {
            !matches!(b.term, Term::RegionEnd)
                && b.insts.iter().all(|i| match i {
                    Inst::Charge { .. }
                    | Inst::Const { .. }
                    | Inst::Load { .. }
                    | Inst::Store { .. }
                    | Inst::Widen { .. }
                    | Inst::IntOp { .. }
                    | Inst::FloatOp { .. }
                    | Inst::BoolOp { .. }
                    | Inst::Call { .. } => true,
                    Inst::Unary { op, src, .. } => matches!(
                        (op, reg(*src)),
                        (UnOp::Neg | UnOp::BitNot, IrTy::I64)
                            | (UnOp::Neg, IrTy::F64)
                            | (UnOp::Not, IrTy::Bool)
                    ),
                    Inst::Cast { ty, src, .. } => {
                        matches!(ty, Ty::Int | Ty::Double)
                            && matches!(reg(*src), IrTy::I64 | IrTy::F64)
                    }
                    _ => false,
                })
        })
}

/// Functions native code can run: supported code that only calls such functions (fixpoint).
pub fn jitable(m: &Module) -> Vec<bool> {
    let mut ok: Vec<bool> = m.funcs.iter().map(supported).collect();
    loop {
        let mut changed = false;
        for (i, f) in m.funcs.iter().enumerate() {
            let calls_other = ok[i]
                && f.blocks
                    .iter()
                    .flat_map(|b| &b.insts)
                    .any(|inst| matches!(inst, Inst::Call { func, .. } if !ok[*func as usize]));
            if calls_other {
                ok[i] = false;
                changed = true;
            }
        }
        if !changed {
            return ok;
        }
    }
}

type Entry = unsafe extern "C" fn(*mut Ctx, *const u64) -> u8;

// Calls after which a function without loops or direct recursion is compiled; until then the
// IR interpreter runs it (compiling costs more than running a short function a few times).
const HOT_CALLS: u32 = 16;

thread_local! {
    // Tests compile on the first call, so `dual` runs as much native code as possible.
    static HOT: std::cell::Cell<u32> = const {
        std::cell::Cell::new(if cfg!(test) { 1 } else { HOT_CALLS })
    };
}

/// Sets this thread's compile threshold (the benchmarks use the real one).
#[cfg(test)]
pub fn set_hot_calls(n: u32) {
    HOT.with(|h| h.set(n));
}

#[cfg(test)]
pub const DEFAULT_HOT_CALLS: u32 = HOT_CALLS;

// A loop (a branch back to an earlier block; lowering numbers blocks in source order) or a call
// of itself: likely to run long, so it is compiled on its first call.
fn eager(f: &Func, id: FuncId) -> bool {
    f.blocks.iter().enumerate().any(|(k, b)| {
        let back = |t: &u32| (*t as usize) <= k;
        let loops = match &b.term {
            Term::Jump(t) => back(t),
            Term::Br { then, els, .. } => back(then) || back(els),
            _ => false,
        };
        loops
            || b.insts
                .iter()
                .any(|i| matches!(i, Inst::Call { func, .. } if *func == id))
    })
}

/// Native code for one lowered module, compiled function by function on first call.
pub struct Jit<'m> {
    ir: &'m Module,
    cl: Option<JITModule>,
    ok: Vec<bool>,
    // Cranelift ids of each compiled function's body and of its entry trampoline.
    ids: Vec<Option<(ClId, ClId)>>,
    entries: Vec<Option<Entry>>,
    helpers: [ClId; 3],
    charges: Vec<ChargeSite>,
    fails: Vec<Diagnostic>,
    fctx: FunctionBuilderContext,
    calls: Vec<u32>,
    hot: u32,
    // After a failed compilation nothing more runs natively (the module may hold declarations
    // without code).
    broken: bool,
    // Tests: make the next compilation fail.
    #[cfg(test)]
    inject_failure: bool,
}

impl Drop for Jit<'_> {
    fn drop(&mut self) {
        if let Some(m) = self.cl.take() {
            // SAFETY: entry pointers die with `self`; no native call is running (calls borrow
            // `self` mutably).
            unsafe { m.free_memory() };
        }
    }
}

impl<'m> Jit<'m> {
    /// `None` when Cranelift does not support this machine; the interpreter runs everything.
    pub fn new(ir: &'m Module) -> Option<Self> {
        // Arguments and return values are 64-bit words whose low byte holds a `boolean`.
        if cfg!(target_endian = "big") {
            return None;
        }
        cranelift_native::builder().ok()?;
        // Inline stack probes touch every page of a large frame (Windows grows the stack only
        // through its guard page).
        let flags = [
            ("opt_level", "speed"),
            ("enable_probestack", "true"),
            ("probestack_strategy", "inline"),
        ];
        let mut b = JITBuilder::with_flags(&flags, default_libcall_names()).ok()?;
        b.symbol("qx_charge", qx_charge as *const u8);
        b.symbol("qx_fail", qx_fail as *const u8);
        b.symbol("qx_fmod", qx_fmod as *const u8);
        let mut cl = JITModule::new(b);
        let ptr = cl.target_config().pointer_type();
        let mut sig = cl.make_signature();
        sig.params = vec![AbiParam::new(ptr), AbiParam::new(types::I32)];
        sig.returns = vec![AbiParam::new(types::I8)];
        let charge = cl
            .declare_function("qx_charge", Linkage::Import, &sig)
            .ok()?;
        let fail = cl.declare_function("qx_fail", Linkage::Import, &sig).ok()?;
        let mut fsig = cl.make_signature();
        fsig.params = vec![AbiParam::new(types::F64), AbiParam::new(types::F64)];
        fsig.returns = vec![AbiParam::new(types::F64)];
        let fmod = cl
            .declare_function("qx_fmod", Linkage::Import, &fsig)
            .ok()?;
        Some(Jit {
            ir,
            cl: Some(cl),
            ok: jitable(ir),
            ids: vec![None; ir.funcs.len()],
            entries: vec![None; ir.funcs.len()],
            helpers: [charge, fail, fmod],
            charges: Vec::new(),
            fails: Vec::new(),
            fctx: FunctionBuilderContext::new(),
            calls: vec![0; ir.funcs.len()],
            hot: HOT.with(std::cell::Cell::get),
            broken: false,
            #[cfg(test)]
            inject_failure: false,
        })
    }

    /// Whether function `id` is native code once called.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn compiles(&self, id: FuncId) -> bool {
        self.ok[id as usize]
    }

    // `id` and the functions it calls that are not compiled yet.
    fn closure(&self, id: FuncId) -> Vec<FuncId> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(f) = stack.pop() {
            if !seen.insert(f) || self.ids[f as usize].is_some() {
                continue;
            }
            out.push(f);
            for i in self.ir.funcs[f as usize]
                .blocks
                .iter()
                .flat_map(|b| &b.insts)
            {
                if let Inst::Call { func, .. } = i {
                    stack.push(*func);
                }
            }
        }
        out
    }

    fn compile(&mut self, id: FuncId) -> Result<(), String> {
        #[cfg(test)]
        if std::mem::take(&mut self.inject_failure) {
            return Err("injected failure".into());
        }
        let batch = self.closure(id);
        if batch.iter().any(|&f| !self.ok[f as usize]) {
            return Err("calls a function that is not compiled".into());
        }
        let mut cl = self.cl.take().ok_or("no module")?;
        let r = self.compile_batch(&mut cl, &batch);
        self.cl = Some(cl);
        r
    }

    fn compile_batch(&mut self, cl: &mut JITModule, batch: &[FuncId]) -> Result<(), String> {
        let ptr = cl.target_config().pointer_type();
        for &f in batch {
            let func = &self.ir.funcs[f as usize];
            let mut sig = cl.make_signature();
            sig.params.push(AbiParam::new(ptr));
            for p in &func.params {
                sig.params
                    .push(AbiParam::new(cl_ty(p.ty).ok_or("param type")?));
            }
            sig.returns.push(AbiParam::new(types::I8));
            let body = cl
                .declare_function(&format!("f{f}"), Linkage::Local, &sig)
                .map_err(|e| e.to_string())?;
            let mut tsig = cl.make_signature();
            tsig.params = vec![AbiParam::new(ptr), AbiParam::new(ptr)];
            tsig.returns = vec![AbiParam::new(types::I8)];
            let entry = cl
                .declare_function(&format!("t{f}"), Linkage::Local, &tsig)
                .map_err(|e| e.to_string())?;
            self.ids[f as usize] = Some((body, entry));
        }
        let mut ctx = cl.make_context();
        for &f in batch {
            let (body, entry) = self.ids[f as usize].ok_or("undeclared")?;
            ctx.func.signature = cl.declarations().get_function_decl(body).signature.clone();
            self.body(cl, &mut ctx.func, f)?;
            cl.define_function(body, &mut ctx)
                .map_err(|e| format!("fn {}: {e:?}", self.ir.funcs[f as usize].name))?;
            cl.clear_context(&mut ctx);
            ctx.func.signature = cl.declarations().get_function_decl(entry).signature.clone();
            self.trampoline(cl, &mut ctx.func, f, body);
            cl.define_function(entry, &mut ctx)
                .map_err(|e| format!("entry {f}: {e:?}"))?;
            cl.clear_context(&mut ctx);
        }
        cl.finalize_definitions().map_err(|e| e.to_string())?;
        for &f in batch {
            let (_, entry) = self.ids[f as usize].ok_or("undeclared")?;
            let p = cl.get_finalized_function(entry);
            // SAFETY: `t{f}` was defined with the `Entry` signature in the default call
            // convention, which is `extern "C"` on this target.
            self.entries[f as usize] = Some(unsafe { std::mem::transmute::<*const u8, Entry>(p) });
        }
        Ok(())
    }

    // `t{f}(ctx, args)`: loads the arguments from the array and calls the body.
    fn trampoline(&mut self, cl: &mut JITModule, func: &mut cl::Function, f: FuncId, body: ClId) {
        let fref = cl.declare_func_in_func(body, func);
        let mut b = FunctionBuilder::new(func, &mut self.fctx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let (ctx, args) = (b.block_params(entry)[0], b.block_params(entry)[1]);
        let mut vals = vec![ctx];
        for (i, p) in self.ir.funcs[f as usize].params.iter().enumerate() {
            let t = cl_ty(p.ty).expect("checked by `supported`");
            vals.push(b.ins().load(t, MemFlagsData::trusted(), args, 8 * i as i32));
        }
        let call = b.ins().call(fref, &vals);
        let st = b.inst_results(call)[0];
        b.ins().return_(&[st]);
        b.seal_all_blocks();
        b.finalize(cl.target_config());
    }

    fn body(
        &mut self,
        cl: &mut JITModule,
        func: &mut cl::Function,
        f: FuncId,
    ) -> Result<(), String> {
        let ir = self.ir;
        let src = &ir.funcs[f as usize];
        let mut callees = std::collections::HashMap::new();
        for inst in src.blocks.iter().flat_map(|b| &b.insts) {
            if let Inst::Call { func: callee, .. } = inst {
                if let (Some((body, _)), false) =
                    (self.ids[*callee as usize], callees.contains_key(callee))
                {
                    callees.insert(*callee, cl.declare_func_in_func(body, func));
                }
            }
        }
        let helpers = self.helpers.map(|h| cl.declare_func_in_func(h, func));
        let g = Gen {
            config: cl.target_config(),
            b: FunctionBuilder::new(func, &mut self.fctx),
            src,
            callees,
            charge: helpers[0],
            fail: helpers[1],
            fmod: helpers[2],
            charges: &mut self.charges,
            fails: &mut self.fails,
            ctx: cl::Value::from_u32(0),
            err: cl::Block::from_u32(0),
            slots: Vec::new(),
            regs: Vec::new(),
        };
        g.function()
    }
}

impl Native for Jit<'_> {
    fn has(&mut self, id: FuncId) -> bool {
        if self.broken || !self.ok[id as usize] {
            return false;
        }
        if self.entries[id as usize].is_some() {
            return true;
        }
        let calls = &mut self.calls[id as usize];
        *calls = calls.saturating_add(1);
        if *calls < self.hot && !eager(&self.ir.funcs[id as usize], id) {
            return false;
        }
        if let Err(e) = self.compile(id) {
            // A Cranelift failure is a bug: tests and debug builds stop; release builds interpret
            // everything from here on.
            #[cfg(test)]
            let injected = e == "injected failure";
            #[cfg(not(test))]
            let injected = false;
            debug_assert!(injected, "JIT of {}: {e}", self.ir.funcs[id as usize].name);
            self.broken = true;
            return false;
        }
        self.entries[id as usize].is_some()
    }

    fn call(&mut self, id: FuncId, args: Vec<Value>, m: &mut Machine) -> R<Value> {
        let f = &self.ir.funcs[id as usize];
        let entry = self.entries[id as usize].ok_or_else(|| internal(f.span))?;
        let raw: Vec<u64> = args
            .into_iter()
            .zip(&f.param_tys)
            .map(|(v, ty)| match coerce(v, *ty) {
                Value::Int(n) => n as u64,
                Value::Double(x) => x.to_bits(),
                Value::Bool(b) => u64::from(b),
                _ => 0,
            })
            .collect();
        if raw.len() != f.params.len() {
            return Err(internal(f.span));
        }
        let (steps, next_poll, max_steps) = m.budget();
        let mut rt = Rt {
            machine: (m as *mut Machine<'_>).cast::<Machine<'static>>(),
            charges: &self.charges,
            fails: &self.fails,
            err: None,
        };
        let mut ctx = Ctx {
            steps,
            next_poll,
            max_steps,
            depth: m.depth() as u64,
            ret: 0,
            rt: &mut rt,
        };
        // SAFETY: `entry` is finalized code of this module with the `Entry` signature; `raw`
        // holds one word per parameter; `m` is only used through `rt` until the call returns.
        let status = unsafe { entry(&mut ctx, raw.as_ptr()) };
        m.set_budget(ctx.steps, ctx.next_poll);
        m.set_depth(ctx.depth as usize);
        if status != 0 {
            return Err(rt.err.take().unwrap_or_else(|| internal(f.span)));
        }
        Ok(match f.ret {
            IrTy::I64 => Value::Int(ctx.ret as i64),
            IrTy::F64 => Value::Double(f64::from_bits(ctx.ret)),
            IrTy::Bool => Value::Bool(ctx.ret & 0xff != 0),
            _ => Value::Unit,
        })
    }
}

// Translation of one IR function.
struct Gen<'a, 'b> {
    b: FunctionBuilder<'b>,
    config: cranelift_codegen::isa::TargetFrontendConfig,
    src: &'a Func,
    callees: std::collections::HashMap<FuncId, cl::FuncRef>,
    charge: cl::FuncRef,
    fail: cl::FuncRef,
    fmod: cl::FuncRef,
    charges: &'a mut Vec<ChargeSite>,
    fails: &'a mut Vec<Diagnostic>,
    ctx: cl::Value,
    // Returns status 1: a helper stored the diagnostic.
    err: cl::Block,
    slots: Vec<Variable>,
    regs: Vec<Option<Variable>>,
}

impl Gen<'_, '_> {
    fn function(mut self) -> Result<(), String> {
        let src = self.src;
        let entry = self.b.create_block();
        self.b.append_block_params_for_function_params(entry);
        let blocks: Vec<cl::Block> = src.blocks.iter().map(|_| self.b.create_block()).collect();
        self.err = self.b.create_block();
        self.b.switch_to_block(entry);
        self.ctx = self.b.block_params(entry)[0];
        for s in &src.slots {
            let t = cl_ty(s.ty).ok_or("slot type")?;
            let v = self.b.declare_var(t);
            let zero = self.zero(t);
            self.b.def_var(v, zero);
            self.slots.push(v);
        }
        for r in &src.regs {
            self.regs.push(cl_ty(*r).map(|t| self.b.declare_var(t)));
        }
        for i in 0..src.params.len() {
            let p = self.b.block_params(entry)[i + 1];
            self.b.def_var(self.slots[i], p);
        }
        self.b.ins().jump(blocks[0], &[]);
        for (k, blk) in src.blocks.iter().enumerate() {
            self.b.switch_to_block(blocks[k]);
            for i in &blk.insts {
                self.inst(i)?;
            }
            self.term(&blk.term, &blocks)?;
        }
        self.b.switch_to_block(self.err);
        let one = self.b.ins().iconst(types::I8, 1);
        self.b.ins().return_(&[one]);
        self.b.seal_all_blocks();
        self.b.finalize(self.config);
        Ok(())
    }

    fn zero(&mut self, t: cl::Type) -> cl::Value {
        if t == types::F64 {
            self.b.ins().f64const(0.0)
        } else {
            self.b.ins().iconst(t, 0)
        }
    }

    fn get(&mut self, r: u32) -> Result<cl::Value, String> {
        let v = self.regs[r as usize].ok_or("unit register used")?;
        Ok(self.b.use_var(v))
    }

    fn set(&mut self, r: u32, v: cl::Value) -> Result<(), String> {
        let var = self.regs[r as usize].ok_or("unit register defined")?;
        self.b.def_var(var, v);
        Ok(())
    }

    // Continues in a fresh block when `cond` is zero; otherwise reports `d`.
    fn fail_if(&mut self, cond: cl::Value, d: Diagnostic) {
        let fail = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(cond, fail, &[], cont, &[]);
        self.b.switch_to_block(fail);
        self.fail_here(d);
        self.b.switch_to_block(cont);
    }

    // Reports `d` and returns status 1.
    fn fail_here(&mut self, d: Diagnostic) {
        self.fails.push(d);
        let site = self
            .b
            .ins()
            .iconst(types::I32, (self.fails.len() - 1) as i64);
        self.b.ins().call(self.fail, &[self.ctx, site]);
        self.b.ins().jump(self.err, &[]);
    }

    // `Machine::charge_all(total, parts)`: inline when no limit or poll is in reach.
    fn charge(&mut self, total: u64, parts: Vec<(u64, Span)>) {
        self.charges.push(ChargeSite { total, parts });
        let site = (self.charges.len() - 1) as i64;
        let flags = MemFlagsData::trusted();
        let ctx = self.ctx;
        let steps = self.b.ins().load(types::I64, flags, ctx, STEPS);
        let poll = self.b.ins().load(types::I64, flags, ctx, NEXT_POLL);
        let max = self.b.ins().load(types::I64, flags, ctx, MAX_STEPS);
        let after = self.b.ins().iadd_imm_s(steps, total as i64);
        let below_poll = self.b.ins().icmp(IntCC::UnsignedLessThan, after, poll);
        let within = self
            .b
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, after, max);
        let fast = self.b.ins().band(below_poll, within);
        let (fast_bb, slow_bb, cont) = (
            self.b.create_block(),
            self.b.create_block(),
            self.b.create_block(),
        );
        self.b.ins().brif(fast, fast_bb, &[], slow_bb, &[]);
        self.b.switch_to_block(fast_bb);
        self.b.ins().store(flags, after, ctx, STEPS);
        self.b.ins().jump(cont, &[]);
        self.b.switch_to_block(slow_bb);
        let site = self.b.ins().iconst(types::I32, site);
        let call = self.b.ins().call(self.charge, &[ctx, site]);
        let st = self.b.inst_results(call)[0];
        self.b.ins().brif(st, self.err, &[], cont, &[]);
        self.b.switch_to_block(cont);
    }

    fn inst(&mut self, i: &Inst) -> Result<(), String> {
        let src = self.src;
        match i {
            Inst::Charge { total, first, len } => {
                let parts = src.charges[*first as usize..(*first + *len) as usize].to_vec();
                self.charge(*total, parts);
            }
            Inst::Const { dst, value } => {
                let v = match value {
                    Const::I64(n) => self.b.ins().iconst(types::I64, *n),
                    Const::F64(x) => self.b.ins().f64const(*x),
                    Const::Bool(b) => self.b.ins().iconst(types::I8, i64::from(*b)),
                };
                self.set(*dst, v)?;
            }
            Inst::Load { dst, slot } => {
                let v = self.b.use_var(self.slots[*slot as usize]);
                self.set(*dst, v)?;
            }
            Inst::Store { slot, src: r } => {
                let v = self.get(*r)?;
                self.b.def_var(self.slots[*slot as usize], v);
            }
            Inst::Widen { dst, src: r } => {
                let x = self.get(*r)?;
                let v = self.b.ins().fcvt_from_sint(types::F64, x);
                self.set(*dst, v)?;
            }
            Inst::IntOp {
                op,
                dst,
                a,
                b,
                span,
            } => {
                let (x, y) = (self.get(*a)?, self.get(*b)?);
                let v = self.int_op(*op, x, y, *span)?;
                self.set(*dst, v)?;
            }
            Inst::FloatOp { op, dst, a, b, .. } => {
                let (x, y) = (self.get(*a)?, self.get(*b)?);
                let v = self.float_op(*op, x, y)?;
                self.set(*dst, v)?;
            }
            Inst::BoolOp { op, dst, a, b, .. } => {
                let (x, y) = (self.get(*a)?, self.get(*b)?);
                let cc = match op {
                    BinOp::Eq => IntCC::Equal,
                    BinOp::Ne => IntCC::NotEqual,
                    _ => return Err("boolean op".into()),
                };
                let v = self.b.ins().icmp(cc, x, y);
                self.set(*dst, v)?;
            }
            Inst::Unary {
                op,
                dst,
                src: r,
                span,
            } => {
                let x = self.get(*r)?;
                let v = match (op, src.regs[*r as usize]) {
                    (UnOp::Neg, IrTy::I64) => {
                        let min = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                        self.fail_if(min, overflow(*span));
                        self.b.ins().ineg(x)
                    }
                    (UnOp::Neg, IrTy::F64) => self.b.ins().fneg(x),
                    (UnOp::Not, IrTy::Bool) => self.b.ins().bxor_imm_s(x, 1),
                    (UnOp::BitNot, IrTy::I64) => self.b.ins().bnot(x),
                    _ => return Err("unary operand".into()),
                };
                self.set(*dst, v)?;
            }
            Inst::Cast {
                ty, dst, src: r, ..
            } => {
                let x = self.get(*r)?;
                let v = match (ty, src.regs[*r as usize]) {
                    (Ty::Int, IrTy::I64) | (Ty::Double, IrTy::F64) => x,
                    // Java's `(long)` rule, like Rust's `as`: toward zero, NaN -> 0, saturating.
                    (Ty::Int, IrTy::F64) => self.b.ins().fcvt_to_sint_sat(types::I64, x),
                    (Ty::Double, IrTy::I64) => self.b.ins().fcvt_from_sint(types::F64, x),
                    _ => return Err("cast operand".into()),
                };
                self.set(*dst, v)?;
            }
            Inst::Call {
                func,
                dst,
                args,
                span,
            } => self.call(*func, *dst, args, *span)?,
            _ => return Err(format!("unsupported instruction {i:?}")),
        }
        Ok(())
    }

    fn int_op(
        &mut self,
        op: BinOp,
        x: cl::Value,
        y: cl::Value,
        span: Span,
    ) -> Result<cl::Value, String> {
        Ok(match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let (v, of) = match op {
                    BinOp::Add => self.b.ins().sadd_overflow(x, y),
                    BinOp::Sub => self.b.ins().ssub_overflow(x, y),
                    _ => self.b.ins().smul_overflow(x, y),
                };
                self.fail_if(of, overflow(span));
                v
            }
            BinOp::Div | BinOp::Rem => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.fail_if(
                    zero,
                    Diagnostic::new(Code::DivByZero, span, msg::div_by_zero()),
                );
                let minus_one = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                if op == BinOp::Div {
                    let min = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                    let of = self.b.ins().band(min, minus_one);
                    self.fail_if(of, overflow(span));
                    self.b.ins().sdiv(x, y)
                } else {
                    // `int.MIN % -1` is 0 (Java, `wrapping_rem`); avoid the hardware trap.
                    let one = self.b.ins().iconst(types::I64, 1);
                    let d = self.b.ins().select(minus_one, one, y);
                    let r = self.b.ins().srem(x, d);
                    let zero = self.b.ins().iconst(types::I64, 0);
                    self.b.ins().select(minus_one, zero, r)
                }
            }
            BinOp::BitAnd => self.b.ins().band(x, y),
            BinOp::BitOr => self.b.ins().bor(x, y),
            BinOp::BitXor => self.b.ins().bxor(x, y),
            BinOp::Shl | BinOp::Shr | BinOp::UShr => {
                // The interpreters' rule: an amount outside 0..=63 is `Q0420`.
                let bad = self.b.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, y, 63);
                self.fail_if(
                    bad,
                    Diagnostic::new(Code::ShiftRange, span, msg::shift_range()),
                );
                match op {
                    BinOp::Shl => self.b.ins().ishl(x, y),
                    BinOp::Shr => self.b.ins().sshr(x, y),
                    _ => self.b.ins().ushr(x, y),
                }
            }
            _ => {
                let cc = match op {
                    BinOp::Eq => IntCC::Equal,
                    BinOp::Ne => IntCC::NotEqual,
                    BinOp::Lt => IntCC::SignedLessThan,
                    BinOp::Le => IntCC::SignedLessThanOrEqual,
                    BinOp::Gt => IntCC::SignedGreaterThan,
                    BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
                    _ => return Err("logical op on int".into()),
                };
                self.b.ins().icmp(cc, x, y)
            }
        })
    }

    fn float_op(&mut self, op: BinOp, x: cl::Value, y: cl::Value) -> Result<cl::Value, String> {
        Ok(match op {
            BinOp::Add => self.b.ins().fadd(x, y),
            BinOp::Sub => self.b.ins().fsub(x, y),
            BinOp::Mul => self.b.ins().fmul(x, y),
            BinOp::Div => self.b.ins().fdiv(x, y),
            BinOp::Rem => {
                let call = self.b.ins().call(self.fmod, &[x, y]);
                self.b.inst_results(call)[0]
            }
            _ => {
                // Rust's float comparisons: `!=` is true for NaN, the others false.
                let cc = match op {
                    BinOp::Eq => FloatCC::Equal,
                    BinOp::Ne => FloatCC::NotEqual,
                    BinOp::Lt => FloatCC::LessThan,
                    BinOp::Le => FloatCC::LessThanOrEqual,
                    BinOp::Gt => FloatCC::GreaterThan,
                    BinOp::Ge => FloatCC::GreaterThanOrEqual,
                    _ => return Err("logical op on double".into()),
                };
                self.b.ins().fcmp(cc, x, y)
            }
        })
    }

    // The IR interpreter's `Call`: aliasing is moot without qubits, then `Machine::enter_call`
    // (one step, then the depth limit), the call, `leave_call`.
    fn call(&mut self, func: FuncId, dst: u32, args: &[u32], span: Span) -> Result<(), String> {
        self.charge(1, vec![(1, span)]);
        let flags = MemFlagsData::trusted();
        let ctx = self.ctx;
        let depth = self.b.ins().load(types::I64, flags, ctx, DEPTH);
        let deep = self.b.ins().icmp_imm_s(
            IntCC::UnsignedGreaterThanOrEqual,
            depth,
            MAX_CALL_DEPTH as i64,
        );
        self.fail_if(
            deep,
            Diagnostic::new(
                Code::RecursionLimit,
                span,
                msg::recursion_limit(MAX_CALL_DEPTH),
            ),
        );
        let deeper = self.b.ins().iadd_imm_s(depth, 1);
        self.b.ins().store(flags, deeper, ctx, DEPTH);
        let mut vals = vec![ctx];
        for a in args {
            vals.push(self.get(*a)?);
        }
        let callee = *self.callees.get(&func).ok_or("callee not declared")?;
        let call = self.b.ins().call(callee, &vals);
        let st = self.b.inst_results(call)[0];
        let depth = self.b.ins().load(types::I64, flags, ctx, DEPTH);
        let back = self.b.ins().iadd_imm_s(depth, -1);
        self.b.ins().store(flags, back, ctx, DEPTH);
        let cont = self.b.create_block();
        self.b.ins().brif(st, self.err, &[], cont, &[]);
        self.b.switch_to_block(cont);
        if let Some(t) = cl_ty(self.src.regs[dst as usize]) {
            let v = self.b.ins().load(t, flags, ctx, RET);
            self.set(dst, v)?;
        }
        Ok(())
    }

    fn term(&mut self, t: &Term, blocks: &[cl::Block]) -> Result<(), String> {
        match t {
            Term::Jump(b) => {
                self.b.ins().jump(blocks[*b as usize], &[]);
            }
            Term::Br { cond, then, els } => {
                let c = self.get(*cond)?;
                self.b
                    .ins()
                    .brif(c, blocks[*then as usize], &[], blocks[*els as usize], &[]);
            }
            Term::Ret(r) => {
                if let Some(r) = r {
                    let v = self.get(*r)?;
                    self.b
                        .ins()
                        .store(MemFlagsData::trusted(), v, self.ctx, RET);
                }
                let zero = self.b.ins().iconst(types::I8, 0);
                self.b.ins().return_(&[zero]);
            }
            Term::FallOff { span } => self.fail_here(Diagnostic::new(
                Code::NoReturn,
                *span,
                msg::no_return(&self.src.name),
            )),
            Term::Trap(d) => self.fail_here((**d).clone()),
            Term::RegionEnd => return Err("region in a compiled function".into()),
        }
        Ok(())
    }
}

fn overflow(at: Span) -> Diagnostic {
    Diagnostic::new(Code::Overflow, at, msg::overflow())
}
