// State shared by both elaborators (the tree-walking interpreter in interp.rs and the IR
// interpreter in ir/exec.rs): the qubit pool, the output template, the measurement record and
// the resource budgets. Every operation takes values that are already evaluated, so the two
// engines charge budgets, record operations and report runtime errors through the same code.

use std::rc::Rc;

use qxide::{Circuit, Qubit};

use super::ast::{BinOp, FmtPiece, Ty};
use super::diag::{Code, Diagnostic, Span};
use super::interp::{piece_bytes, BitsFmt, Elaborated, OpCounts, OutPiece, Usage, Value};
use super::record::Layouts;
use super::stdlib::{Host, HostValue, StdError, StdErrorKind};
use super::{msg, Limits};

pub const MAX_CALL_DEPTH: usize = 200;
// Caps allocations so a huge `qreg(n)` fails with a diagnostic instead of aborting on OOM.
pub const MAX_QUBITS: usize = 1 << 20;
// How often (in budget steps) elaboration asks the host whether the call was cancelled.
const CANCEL_POLL: u64 = 4096;

// `OPENQASM 3.0;`, `include`, `qubit[n] q;` and `bit[n] c;` lines.
const QASM_HEADER_BYTES: u64 = 96;

// Steps per byte of `String` text built, compared, measured or passed to or from the host. Every
// live byte was charged when it was built, so the step budget also bounds string memory:
// `max_steps / 8` bytes (128 MiB for the CLI, 256 MiB per desktop-app call).
pub const STR_BYTE_STEPS: u64 = 8;

pub type R<T> = Result<T, Diagnostic>;

// Java-style widening of an `int` into a `double` slot.
pub fn coerce(v: Value, ty: Ty) -> Value {
    match (v, ty) {
        (Value::Int(n), Ty::Double) => Value::Double(n as f64),
        (v, _) => v,
    }
}

pub fn internal(at: Span) -> Diagnostic {
    // Only reachable if the checker and interpreter disagree.
    Diagnostic::new(Code::Internal, at, msg::internal_error())
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

fn std_error(e: StdError, span: Span) -> Diagnostic {
    let code = match e.kind {
        StdErrorKind::Unavailable => Code::StdUnavailable,
        StdErrorKind::Io => Code::StdIo,
        StdErrorKind::Path => Code::StdPath,
        StdErrorKind::Argument => Code::StdArgument,
        StdErrorKind::Http => Code::StdHttp,
        StdErrorKind::Url => Code::StdUrl,
        StdErrorKind::Cancelled => Code::Cancelled,
    };
    Diagnostic::new(code, span, e.message)
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

/// A one-qubit gate or `reset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate1 {
    H,
    X,
    Z,
    S,
    Sdg,
    T,
    Tdg,
    Reset,
}

impl Gate1 {
    pub fn from_name(n: &str) -> Option<Gate1> {
        Some(match n {
            "h" => Gate1::H,
            "x" => Gate1::X,
            "z" => Gate1::Z,
            "s" => Gate1::S,
            "sdg" => Gate1::Sdg,
            "t" => Gate1::T,
            "tdg" => Gate1::Tdg,
            "reset" => Gate1::Reset,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Gate1::H => "h",
            Gate1::X => "x",
            Gate1::Z => "z",
            Gate1::S => "s",
            Gate1::Sdg => "sdg",
            Gate1::T => "t",
            Gate1::Tdg => "tdg",
            Gate1::Reset => "reset",
        }
    }
}

/// A rotation gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rot {
    Rx,
    Ry,
    Rz,
}

impl Rot {
    pub fn from_name(n: &str) -> Option<Rot> {
        Some(match n {
            "rx" => Rot::Rx,
            "ry" => Rot::Ry,
            "rz" => Rot::Rz,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Rot::Rx => "rx",
            Rot::Ry => "ry",
            Rot::Rz => "rz",
        }
    }
}

pub struct Machine<'h> {
    pub host: &'h mut dyn Host,
    // Struct field layouts of the program being elaborated.
    layouts: Rc<Layouts>,
    // `steps` at which the host is next asked whether the call was cancelled.
    next_poll: u64,
    pool: Vec<Option<Qubit>>,
    output: Vec<OutPiece>,
    measured: Vec<qxide::Bit>,
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

impl<'h> Machine<'h> {
    pub fn new(host: &'h mut dyn Host, limits: &Limits, layouts: Rc<Layouts>) -> Self {
        Machine {
            host,
            layouts,
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
        }
    }

    pub fn layouts(&self) -> Rc<Layouts> {
        self.layouts.clone()
    }

    pub fn finish(self, circuit: Circuit) -> Elaborated {
        Elaborated {
            circuit,
            exit_code: 0,
            output: self.output,
            measured: self.measured,
            slots: self.slots,
            counts: self.counts,
            qasm_bytes: self.qasm_bytes,
            usage: Usage {
                steps: self.steps,
                ops: self.ops,
                out_bytes: self.out_bytes,
                cond_depth: self.peak_cond_depth,
            },
        }
    }

    /// Steps used and the step count of the next cancellation poll, handed to native code.
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    pub fn budget(&self) -> (u64, u64, u64) {
        (self.steps, self.next_poll, self.limits.max_steps)
    }

    /// Takes back the counters native code advanced (see `budget`).
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    pub fn set_budget(&mut self, steps: u64, next_poll: u64) {
        self.steps = steps;
        self.next_poll = next_poll;
    }

    /// Calls in progress, for native code that checks `MAX_CALL_DEPTH` itself.
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    pub fn depth(&self) -> usize {
        self.depth
    }

    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    pub fn charge(&mut self, steps: u64, at: Span) -> R<()> {
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

    /// Several consecutive `charge` calls at once, with the same effect as making them one by
    /// one: when no partial sum can reach the limit or the next cancellation poll, they are
    /// added in one step; otherwise they are replayed in order.
    pub fn charge_all(&mut self, total: u64, parts: &[(u64, Span)]) -> R<()> {
        let after = self.steps.saturating_add(total);
        if after < self.next_poll && after <= self.limits.max_steps {
            self.steps = after;
            return Ok(());
        }
        for &(n, at) in parts {
            self.charge(n, at)?;
        }
        Ok(())
    }

    // Text work in proportion to its length (`STR_BYTE_STEPS` per byte), charged before it is done.
    pub fn charge_text(&mut self, bytes: usize, at: Span) -> R<()> {
        self.charge((bytes as u64).saturating_mul(STR_BYTE_STEPS), at)
    }

    // A string literal used as a value; `println` reads its format string from the AST instead.
    // Each evaluation builds a new `String`, so a literal in a loop is charged every time.
    pub fn str_lit(&mut self, pieces: &[FmtPiece], at: Span) -> R<Value> {
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
    pub fn str_binary(&mut self, op: BinOp, l: Value, r: Value, at: Span) -> R<Value> {
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

    /// Starts a call: one step, then the call-depth limit (`Q0405`).
    pub fn enter_call(&mut self, at: Span) -> R<()> {
        self.charge(1, at)?;
        if self.depth >= MAX_CALL_DEPTH {
            return Err(Diagnostic::new(
                Code::RecursionLimit,
                at,
                msg::recursion_limit(MAX_CALL_DEPTH),
            ));
        }
        self.depth += 1;
        Ok(())
    }

    pub fn leave_call(&mut self) {
        self.depth -= 1;
    }

    /// Register elements with run-time indices may alias; the checker catches the rest.
    pub fn check_aliasing(&mut self, vals: &[Value], callee: &str, span: Span) -> R<()> {
        let mut qubits: Vec<usize> = Vec::new();
        for v in vals {
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
                msg::duplicate_qubit(callee),
            ));
        }
        Ok(())
    }

    /// Opens the `if_one` block of an `if` on a measured bit.
    pub fn enter_bit_if(&mut self, at: Span) -> R<()> {
        // Bodies of calls made inside count too: the circuit nests them all (M-10).
        if self.cond_depth >= self.limits.max_cond_depth {
            return Err(Diagnostic::new(
                Code::CondDepth,
                at,
                msg::cond_depth_limit(self.limits.max_cond_depth),
            ));
        }
        self.enter_bit_else(at)
    }

    /// Opens the `if_zero` block of the `else` branch (the depth was checked by `enter_bit_if`).
    pub fn enter_bit_else(&mut self, at: Span) -> R<()> {
        self.record(Rec::If, at)?;
        self.cond_depth += 1;
        self.peak_cond_depth = self.peak_cond_depth.max(self.cond_depth);
        Ok(())
    }

    pub fn leave_bit_if(&mut self) {
        self.cond_depth -= 1;
    }

    pub fn qreg(&mut self, c: &mut Circuit, k: i64, size_span: Span, span: Span) -> R<Value> {
        if k < 0 {
            return Err(Diagnostic::new(
                Code::NegativeSize,
                size_span,
                msg::negative_size(k),
            ));
        }
        // `as usize` would wrap sizes of 2^32 and above on 32-bit targets (wasm).
        let n = usize::try_from(k).unwrap_or(usize::MAX);
        Ok(Value::QReg(self.alloc(c, n, span)?.into()))
    }

    pub fn qubit(&mut self, c: &mut Circuit, span: Span) -> R<Value> {
        self.alloc(c, 1, span).map(|q| Value::Qubit(q[0]))
    }

    pub fn cx(&mut self, c: &mut Circuit, a: usize, b: usize, span: Span) -> R<Value> {
        self.record(Rec::Cx, span)?;
        let [a, b] = self.qubits([a, b], span, "cx")?;
        c.cx(a, b);
        Ok(Value::Unit)
    }

    pub fn ccx(&mut self, c: &mut Circuit, idx: [usize; 3], span: Span) -> R<Value> {
        self.record(Rec::Ccx, span)?;
        let [a, b, t] = self.qubits(idx, span, "ccx")?;
        c.ccx(a, b, t);
        Ok(Value::Unit)
    }

    pub fn measure(&mut self, c: &mut Circuit, v: Value, span: Span) -> R<Value> {
        match v {
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

    pub fn discard(&mut self, v: Value, span: Span) -> R<Value> {
        let idx = match v {
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

    pub fn len(&mut self, v: Value, span: Span) -> R<Value> {
        match v {
            Value::Str(s) => {
                self.charge_text(s.len(), span)?;
                Ok(Value::Int(s.chars().count() as i64))
            }
            Value::QReg(v) => Ok(Value::Int(v.len() as i64)),
            Value::Bits(v) => Ok(Value::Int(v.len() as i64)),
            Value::Array(a) => Ok(Value::Int(a.borrow().len() as i64)),
            Value::Native(n) => Ok(Value::Int(super::native::len(&n, span)? as i64)),
            _ => Err(internal(span)),
        }
    }

    pub fn print(&mut self, pieces: &[FmtPiece], values: Vec<Value>, span: Span) -> R<Value> {
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

    pub fn rotation(
        &mut self,
        c: &mut Circuit,
        rot: Rot,
        q: usize,
        angle: Value,
        angle_span: Span,
        span: Span,
    ) -> R<Value> {
        let theta = match coerce(angle, Ty::Double) {
            Value::Double(v) => v,
            _ => return Err(internal(angle_span)),
        };
        if !theta.is_finite() {
            return Err(Diagnostic::new(
                Code::NonFiniteAngle,
                angle_span,
                msg::non_finite_angle(rot.name(), theta),
            ));
        }
        // Rotations never run on the tableau, so they need no row passes.
        let op = Rec::Gate {
            name: rot.name().len(),
            arg: debug_len(theta),
            rows: 0,
        };
        self.record(op, span)?;
        let [q] = self.qubits([q], span, rot.name())?;
        match rot {
            Rot::Rx => c.rx(q, theta),
            Rot::Ry => c.ry(q, theta),
            Rot::Rz => c.rz(q, theta),
        }
        Ok(Value::Unit)
    }

    pub fn one_qubit(&mut self, c: &mut Circuit, g: Gate1, q: usize, span: Span) -> R<Value> {
        let op = match g {
            Gate1::Reset => Rec::Reset,
            // S† runs as S three times on the tableau.
            Gate1::Sdg => Rec::Gate {
                name: 3,
                arg: 0,
                rows: 3,
            },
            _ => Rec::Gate {
                name: g.name().len(),
                arg: 0,
                rows: 1,
            },
        };
        self.record(op, span)?;
        let [q] = self.qubits([q], span, g.name())?;
        match g {
            Gate1::H => c.h(q),
            Gate1::X => c.x(q),
            Gate1::Z => c.z(q),
            Gate1::S => c.s(q),
            Gate1::Sdg => c.sdg(q),
            Gate1::T => c.t(q),
            Gate1::Tdg => c.tdg(q),
            Gate1::Reset => c.reset(q),
        }
        Ok(Value::Unit)
    }

    /// One evaluated argument of a std call, as the host receives it.
    pub fn std_arg(&mut self, v: Value, at: Span) -> R<HostValue> {
        if let Value::Str(s) = &v {
            self.charge_text(s.len(), at)?;
        }
        host_value(v, at)
    }

    // `std.fs.read_text(p)`: the arguments are evaluated; the host runs the effect.
    pub fn std_call(&mut self, n: &str, vals: Vec<HostValue>, span: Span) -> R<Value> {
        let rest = n.strip_prefix("std.").ok_or_else(|| internal(span))?;
        let (module, function) = rest.split_once('.').ok_or_else(|| internal(span))?;
        // Pure modules run here, never through the host.
        let result = if super::pure::is_pure_module(module) {
            Ok(super::pure::math(function, &vals, span)?)
        } else {
            // Text written to the terminal is charged per byte, like text the program builds.
            if module == "io" {
                if let Some(HostValue::Str(s)) = vals.first() {
                    self.charge_text(s.len(), span)?;
                }
            }
            self.host.call(module, function, vals)
        };
        // `Option<String>` results: the host answers with the text or `Unit` for `None`.
        if super::stdlib::returns_option(module, function) {
            let some = match result {
                Ok(HostValue::Unit) => None,
                Ok(HostValue::Str(s)) => {
                    self.charge_text(s.len(), span)?;
                    Some(Value::Str(s.into()))
                }
                Ok(_) => return Err(internal(span)),
                Err(e) => return Err(std_error(e, span)),
            };
            return self.option_string(some, span);
        }
        match result {
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
            Err(e) => Err(std_error(e, span)),
        }
    }

    // `Option.Some(text)` or `Option.None` of the prelude's `Option<String>`.
    fn option_string(&mut self, v: Option<Value>, span: Span) -> R<Value> {
        let ty = Ty::instance(Ty::enum_named("Option"), &[Ty::String]);
        let variant = if v.is_some() { "Some" } else { "None" };
        let (tag, _) = self
            .layouts()
            .variant(ty, variant)
            .ok_or_else(|| internal(span))?;
        self.new_enum(ty, tag, v.into_iter().collect(), span)
    }

    /// The `String[] args` of `main`: the arguments the host was given (`qxide run f.qx -- a b`),
    /// none on hosts without any. Their text is charged like text the program builds.
    pub fn main_args(&mut self, span: Span) -> R<Value> {
        let n = match self.host.call("process", "arg_count", Vec::new()) {
            Ok(HostValue::Int(n)) => n,
            _ => 0,
        };
        let mut vals = Vec::new();
        for i in 0..n {
            if let Ok(HostValue::Str(s)) = self.host.call("process", "arg", vec![HostValue::Int(i)])
            {
                self.charge_text(s.len(), span)?;
                vals.push(Value::Str(s.into()));
            }
        }
        self.array_lit(Ty::array_of(Ty::String), vals, span)
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

/// A build-time value as a std function receives it (`std_arg` without the text charge).
pub fn host_value(v: Value, at: Span) -> R<HostValue> {
    Ok(match v {
        Value::Int(n) => HostValue::Int(n),
        Value::Double(v) => HostValue::Double(v),
        Value::Bool(b) => HostValue::Bool(b),
        Value::Str(s) => HostValue::Str(s.to_string()),
        _ => return Err(internal(at)),
    })
}

pub fn checked_index(i: i64, len: usize, at: Span) -> R<usize> {
    usize::try_from(i).ok().filter(|&k| k < len).ok_or_else(|| {
        Diagnostic::new(Code::IndexOutOfBounds, at, msg::index_out_of_bounds(i, len))
    })
}

/// `r[i]` on a register value.
pub fn index(b: Value, i: i64, span: Span) -> R<Value> {
    match b {
        Value::QReg(v) => Ok(Value::Qubit(v[checked_index(i, v.len(), span)?])),
        Value::Bits(v) => Ok(Value::Bit(v[checked_index(i, v.len(), span)?])),
        Value::Array(a) => super::array::get(&a, i, span),
        Value::Native(n) => super::native::index(&n, i, span),
        _ => Err(internal(span)),
    }
}

/// `r.as_int()` and `r.to_binary()` on a measured register.
pub fn method(v: Value, name: &str, span: Span) -> R<Value> {
    let Value::Bits(v) = v else {
        return Err(internal(span));
    };
    match name {
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

/// `(int) x` and `(double) n`.
pub fn cast(ty: Ty, v: Value, span: Span) -> R<Value> {
    match (ty, v) {
        (Ty::Int, Value::Int(n)) => Ok(Value::Int(n)),
        // Java's `(long)` rule, which `as` matches: toward zero, NaN -> 0, saturating.
        (Ty::Int, Value::Double(v)) => Ok(Value::Int(v as i64)),
        (Ty::Double, Value::Int(n)) => Ok(Value::Double(n as f64)),
        (Ty::Double, Value::Double(v)) => Ok(Value::Double(v)),
        _ => Err(internal(span)),
    }
}

/// `-x` and `!b`.
pub fn unary(op: super::ast::UnOp, v: Value, span: Span) -> R<Value> {
    use super::ast::UnOp;
    match (op, v) {
        (UnOp::Neg, Value::Int(n)) => n
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| Diagnostic::new(Code::Overflow, span, msg::overflow())),
        (UnOp::Neg, Value::Double(v)) => Ok(Value::Double(-v)),
        (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
        _ => Err(internal(span)),
    }
}

pub fn binary(op: BinOp, l: Value, r: Value, at: Span) -> R<Value> {
    use BinOp::*;
    Ok(match (l, r) {
        (Value::Int(a), Value::Int(b)) => int_op(op, a, b, at)?,
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

/// Checked `int` arithmetic and comparisons (`Q0401`, `Q0402`).
pub fn int_op(op: BinOp, a: i64, b: i64, at: Span) -> R<Value> {
    use BinOp::*;
    let overflow = || Diagnostic::new(Code::Overflow, at, msg::overflow());
    Ok(match op {
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
        BitAnd => Value::Int(a & b),
        BitOr => Value::Int(a | b),
        BitXor => Value::Int(a ^ b),
        // Bits shifted out are dropped; a shift amount outside 0..=63 is an error (`Q0420`).
        Shl | Shr | UShr => {
            let Some(k) = u32::try_from(b).ok().filter(|k| *k < 64) else {
                return Err(Diagnostic::new(Code::ShiftRange, at, msg::shift_range()));
            };
            Value::Int(match op {
                Shl => a << k,
                Shr => a >> k,
                _ => ((a as u64) >> k) as i64,
            })
        }
        And | Or => return Err(internal(at)),
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
pub fn double_op(op: BinOp, a: f64, b: f64, at: Span) -> R<Value> {
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
        _ => return Err(internal(at)),
    })
}
