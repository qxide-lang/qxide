// Typed IR of checked `.qx` programs (docs/design/qx-ir.md): functions of basic blocks over typed
// virtual registers and stack slots, lowered from the AST by `lower` and run by `exec`. It
// reproduces the tree-walking interpreter exactly, including the order of budget charges.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::ast::{BinOp, FmtPiece, Ty, UnOp};
use super::diag::{Diagnostic, Span};
use super::machine::{Gate1, Rot};

pub mod exec;
pub mod lower;
pub mod verify;

#[cfg(test)]
mod tests;

pub type Reg = u32;
pub type SlotId = u32;
pub type BlockId = u32;
pub type FuncId = u32;

/// Types of IR values. Build-time values (`i64`, `f64`, `bool`, `str`), shot-time values (`bit`,
/// `bits` and the printable views of a measured register) and quantum handles (`qubit`, `qreg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrTy {
    I64,
    F64,
    Bool,
    Str,
    Bit,
    Bits,
    // `r.as_int()` and `r.to_binary()`: printable only.
    ShotInt,
    ShotBinary,
    Qubit,
    QReg,
    Unit,
}

impl IrTy {
    pub fn of(t: Ty) -> IrTy {
        match t {
            Ty::Int => IrTy::I64,
            Ty::Double => IrTy::F64,
            Ty::Boolean => IrTy::Bool,
            Ty::String => IrTy::Str,
            Ty::Bit => IrTy::Bit,
            Ty::BitReg => IrTy::Bits,
            Ty::ShotInt => IrTy::ShotInt,
            Ty::ShotBinary => IrTy::ShotBinary,
            Ty::Qubit => IrTy::Qubit,
            Ty::QReg => IrTy::QReg,
            Ty::Void | Ty::Error => IrTy::Unit,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IrTy::I64 => "i64",
            IrTy::F64 => "f64",
            IrTy::Bool => "bool",
            IrTy::Str => "str",
            IrTy::Bit => "bit",
            IrTy::Bits => "bits",
            IrTy::ShotInt => "bits.int",
            IrTy::ShotBinary => "bits.bin",
            IrTy::Qubit => "qubit",
            IrTy::QReg => "qreg",
            IrTy::Unit => "unit",
        }
    }

    pub fn is_quantum(self) -> bool {
        matches!(self, IrTy::Qubit | IrTy::QReg)
    }
}

/// How a function receives a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    // Classical values are copied.
    Copy,
    // Quantum handles are borrowed by default: the callee may apply gates but not consume them.
    Borrow,
    // `consume`: the callee owns the qubits.
    Consume,
}

#[derive(Debug, Clone)]
pub enum Const {
    I64(i64),
    F64(f64),
    Bool(bool),
}

/// One instruction. Instructions that can fail carry the span the diagnostic points at.
#[derive(Debug, Clone)]
pub enum Inst {
    /// Interpreter steps charged in source order, `(amount, span)` each; consecutive charges
    /// are merged into one instruction (`Func::charges[first..first + len]`).
    Charge {
        total: u64,
        first: u32,
        len: u32,
    },
    Const {
        dst: Reg,
        value: Const,
    },
    /// A `String` literal used as a value: charged per byte, then shared.
    Str {
        dst: Reg,
        text: Rc<str>,
        span: Span,
    },
    Load {
        dst: Reg,
        slot: SlotId,
    },
    Store {
        slot: SlotId,
        src: Reg,
    },
    /// `int` to `double` (Java widening at declarations, assignments, calls and returns).
    Widen {
        dst: Reg,
        src: Reg,
    },
    /// Checked `int` arithmetic and comparisons (overflow `Q0401`, division by zero `Q0402`).
    IntOp {
        op: BinOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        span: Span,
    },
    /// IEEE 754 `double` arithmetic and comparisons.
    FloatOp {
        op: BinOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        span: Span,
    },
    /// `boolean` `==` and `!=` (`&&` and `||` are lowered to branches).
    BoolOp {
        op: BinOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        span: Span,
    },
    /// `String` `+`, `==` and `!=`, charged per byte.
    StrOp {
        op: BinOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        span: Span,
    },
    /// Any other operand types: checked at run time like the tree-walking interpreter.
    DynOp {
        op: BinOp,
        dst: Reg,
        a: Reg,
        b: Reg,
        span: Span,
    },
    Unary {
        op: UnOp,
        dst: Reg,
        src: Reg,
        span: Span,
    },
    Cast {
        ty: Ty,
        dst: Reg,
        src: Reg,
        span: Span,
    },
    /// `r[i]` on a `QReg` or `BitReg` (out of bounds `Q0403`).
    Index {
        dst: Reg,
        base: Reg,
        index: Reg,
        span: Span,
    },
    /// `r.as_int()` or `r.to_binary()`.
    View {
        dst: Reg,
        src: Reg,
        name: &'static str,
        span: Span,
    },
    /// A method of a `String` value (src/lang/pure.rs), charged per byte.
    StrMethod {
        name: &'static str,
        dst: Reg,
        recv: Reg,
        args: Box<[Reg]>,
        span: Span,
    },
    QAlloc {
        dst: Reg,
        span: Span,
    },
    QAllocReg {
        dst: Reg,
        size: Reg,
        size_span: Span,
        span: Span,
    },
    Gate {
        gate: Gate1,
        q: Reg,
        span: Span,
    },
    Rotate {
        rot: Rot,
        q: Reg,
        angle: Reg,
        angle_span: Span,
        span: Span,
    },
    Cx {
        a: Reg,
        b: Reg,
        span: Span,
    },
    Ccx {
        a: Reg,
        b: Reg,
        t: Reg,
        span: Span,
    },
    Measure {
        dst: Reg,
        src: Reg,
        span: Span,
    },
    Discard {
        src: Reg,
        span: Span,
    },
    Len {
        dst: Reg,
        src: Reg,
        span: Span,
    },
    /// `println`: format pieces `Func::formats[fmt]`, one register per `{}`.
    Print {
        fmt: u32,
        args: Box<[Reg]>,
        span: Span,
    },
    /// Charges a `String` argument of a std call as it is handed to the host.
    HostText {
        src: Reg,
        span: Span,
    },
    /// `std.module.function(args)`: a build-time effect run by the host.
    StdCall {
        name: Rc<str>,
        dst: Reg,
        args: Box<[(Reg, Span)]>,
        span: Span,
    },
    Call {
        func: FuncId,
        dst: Reg,
        args: Box<[Reg]>,
        span: Span,
    },
    /// `if` on a measured bit: records the regions as conditional blocks of the circuit.
    /// Each region is a sub-graph entered at its block and left by `Term::RegionEnd`.
    IfBit {
        bit: Reg,
        then: BlockId,
        els: Option<BlockId>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Term {
    Jump(BlockId),
    Br {
        cond: Reg,
        then: BlockId,
        els: BlockId,
    },
    Ret(Option<Reg>),
    /// End of an `IfBit` region.
    RegionEnd,
    /// The end of a non-`void` function without `return` (`Q0409`).
    FallOff {
        span: Span,
    },
    /// A runtime error the tree-walking interpreter reports at this point.
    Trap(Box<Diagnostic>),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: IrTy,
    pub mode: Mode,
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub name: String,
    pub ty: IrTy,
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub span: Span,
    /// Parameters occupy the first slots, in order.
    pub params: Vec<Param>,
    pub ret: IrTy,
    // `Ty` of the parameters, for widening arguments passed in from outside (desktop commands).
    pub param_tys: Vec<Ty>,
    pub slots: Vec<Slot>,
    pub regs: Vec<IrTy>,
    pub blocks: Vec<Block>,
    pub charges: Vec<(u64, Span)>,
    pub formats: Vec<Vec<FmtPiece>>,
    /// A `std` declaration: calls go to the host, the body is never run.
    pub host: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub funcs: Vec<Func>,
    pub index: HashMap<String, FuncId>,
}

/// Lowers a checked program and, in debug builds and tests, verifies the result.
pub fn build(p: &super::ast::Program) -> Module {
    let m = lower::lower(p);
    // `cargo test` builds with debug assertions too; release benchmarks skip the verifier.
    if cfg!(debug_assertions) {
        if let Err(e) = verify::verify(&m) {
            panic!("IR verifier: {e}\n{m}");
        }
    }
    m
}

// Text dump in the notation of docs/design/qx-ir.md, for snapshot tests.

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for func in self.funcs.iter().filter(|g| !g.host) {
            if !first {
                writeln!(f)?;
            }
            first = false;
            func.dump(self, f)?;
        }
        Ok(())
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Copy => "copy",
        Mode::Borrow => "borrow",
        Mode::Consume => "consume",
    }
}

fn op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Rem => "rem",
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => "lt",
        BinOp::Le => "le",
        BinOp::Gt => "gt",
        BinOp::Ge => "ge",
        BinOp::And => "and",
        BinOp::Or => "or",
    }
}

impl Func {
    fn slot(&self, s: SlotId) -> String {
        format!("${}", self.slots[s as usize].name)
    }

    fn dump(&self, m: &Module, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params: Vec<String> = self
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "{} {}: {}",
                    mode_name(p.mode),
                    self.slot(i as SlotId),
                    p.ty.name()
                )
            })
            .collect();
        writeln!(
            f,
            "fn {}({}) -> {} {{",
            self.name,
            params.join(", "),
            self.ret.name()
        )?;
        for s in &self.slots[self.params.len()..] {
            writeln!(f, "    slot ${}: {}", s.name, s.ty.name())?;
        }
        for (i, b) in self.blocks.iter().enumerate() {
            writeln!(f, "bb{i}:")?;
            for inst in &b.insts {
                writeln!(f, "    {}", self.inst(m, inst))?;
            }
            writeln!(f, "    {}", self.term(&b.term))?;
        }
        writeln!(f, "}}")
    }

    fn r(&self, r: Reg) -> String {
        format!("%{r}")
    }

    fn inst(&self, m: &Module, i: &Inst) -> String {
        let r = |x: Reg| self.r(x);
        let ty = |x: Reg| self.regs[x as usize].name();
        match i {
            Inst::Charge { total, .. } => format!("charge {total}"),
            Inst::Const { dst, value } => {
                let v = match value {
                    Const::I64(n) => n.to_string(),
                    Const::F64(v) => format!("{v:?}"),
                    Const::Bool(b) => b.to_string(),
                };
                format!("{} = const.{} {v}", r(*dst), ty(*dst))
            }
            Inst::Str { dst, text, .. } => format!("{} = str {text:?}", r(*dst)),
            Inst::Load { dst, slot } => format!("{} = load {}", r(*dst), self.slot(*slot)),
            Inst::Store { slot, src } => format!("store {}, {}", self.slot(*slot), r(*src)),
            Inst::Widen { dst, src } => format!("{} = widen {}", r(*dst), r(*src)),
            Inst::IntOp { op, dst, a, b, .. } => {
                format!("{} = {}.i64 {}, {}", r(*dst), op_name(*op), r(*a), r(*b))
            }
            Inst::FloatOp { op, dst, a, b, .. } => {
                format!("{} = {}.f64 {}, {}", r(*dst), op_name(*op), r(*a), r(*b))
            }
            Inst::BoolOp { op, dst, a, b, .. } => {
                format!("{} = {}.bool {}, {}", r(*dst), op_name(*op), r(*a), r(*b))
            }
            Inst::StrOp { op, dst, a, b, .. } => {
                format!("{} = {}.str {}, {}", r(*dst), op_name(*op), r(*a), r(*b))
            }
            Inst::DynOp { op, dst, a, b, .. } => {
                format!("{} = {}.dyn {}, {}", r(*dst), op_name(*op), r(*a), r(*b))
            }
            Inst::Unary { op, dst, src, .. } => {
                let n = match op {
                    UnOp::Neg => "neg",
                    UnOp::Not => "not",
                };
                format!("{} = {n}.{} {}", r(*dst), ty(*dst), r(*src))
            }
            Inst::Cast { dst, src, .. } => format!("{} = cast.{} {}", r(*dst), ty(*dst), r(*src)),
            Inst::Index {
                dst, base, index, ..
            } => {
                format!("{} = index {}, {}", r(*dst), r(*base), r(*index))
            }
            Inst::View { dst, src, name, .. } => format!("{} = {name} {}", r(*dst), r(*src)),
            Inst::QAlloc { dst, .. } => format!("{} = qalloc", r(*dst)),
            Inst::QAllocReg { dst, size, .. } => format!("{} = qalloc.reg {}", r(*dst), r(*size)),
            Inst::Gate { gate, q, .. } => format!("{} {}", gate.name(), r(*q)),
            Inst::Rotate { rot, q, angle, .. } => {
                format!("{} {}, {}", rot.name(), r(*q), r(*angle))
            }
            Inst::Cx { a, b, .. } => format!("cx {}, {}", r(*a), r(*b)),
            Inst::Ccx { a, b, t, .. } => format!("ccx {}, {}, {}", r(*a), r(*b), r(*t)),
            Inst::Measure { dst, src, .. } => {
                let suffix = if self.regs[*src as usize] == IrTy::QReg {
                    ".reg"
                } else {
                    ""
                };
                format!("{} = measure{suffix} {}", r(*dst), r(*src))
            }
            Inst::Discard { src, .. } => format!("discard {}", r(*src)),
            Inst::Len { dst, src, .. } => format!("{} = len {}", r(*dst), r(*src)),
            Inst::Print { fmt, args, .. } => {
                let mut text = String::new();
                for p in &self.formats[*fmt as usize] {
                    match p {
                        FmtPiece::Lit(s) => text.push_str(&s.replace('{', "{{").replace('}', "}}")),
                        FmtPiece::Hole => text.push_str("{}"),
                    }
                }
                let mut s = format!("print {text:?}");
                for a in args.iter() {
                    s.push_str(", ");
                    s.push_str(&r(*a));
                }
                s
            }
            Inst::HostText { src, .. } => format!("host.text {}", r(*src)),
            Inst::StdCall {
                name, dst, args, ..
            } => {
                let args: Vec<String> = args.iter().map(|(a, _)| r(*a)).collect();
                format!("{} = call.host {name}({})", r(*dst), args.join(", "))
            }
            Inst::StrMethod {
                name,
                dst,
                recv,
                args,
                ..
            } => {
                let args: Vec<String> = std::iter::once(recv)
                    .chain(args.iter())
                    .map(|a| r(*a))
                    .collect();
                format!("{} = str.{name} {}", r(*dst), args.join(", "))
            }
            Inst::Call {
                func, dst, args, ..
            } => {
                let args: Vec<String> = args.iter().map(|a| r(*a)).collect();
                let name = &m.funcs[*func as usize].name;
                if self.regs[*dst as usize] == IrTy::Unit {
                    format!("call {name}({})", args.join(", "))
                } else {
                    format!("{} = call {name}({})", r(*dst), args.join(", "))
                }
            }
            Inst::IfBit { bit, then, els, .. } => match els {
                Some(e) => format!("if.bit {} then bb{then} else bb{e}", r(*bit)),
                None => format!("if.bit {} then bb{then}", r(*bit)),
            },
        }
    }

    fn term(&self, t: &Term) -> String {
        match t {
            Term::Jump(b) => format!("jump bb{b}"),
            Term::Br { cond, then, els } => format!("br {}, bb{then}, bb{els}", self.r(*cond)),
            Term::Ret(Some(r)) => format!("ret {}", self.r(*r)),
            Term::Ret(None) => "ret".into(),
            Term::RegionEnd => "end.region".into(),
            Term::FallOff { .. } => "trap.no_return".into(),
            Term::Trap(d) => format!("trap {}", d.code.as_str()),
        }
    }
}
