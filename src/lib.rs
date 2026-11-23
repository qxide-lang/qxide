//! # qxide — A small experiment expressing qubit ownership with Rust types
//!
//! Uses the Rust compiler to enforce two rules of quantum mechanics.
//!
//! 1. **No-cloning theorem**: Qubits cannot be copied.
//!    → [`Qubit`] does not implement `Clone`/`Copy`.
//! 2. **Measurement destroys the quantum state**: A measured qubit can no longer be used in quantum operations.
//!    → [`Circuit::measure`] consumes `Qubit` by value (move) and returns a classical bit [`Bit`].
//!
//! As a bonus, Rust's borrow rules prevent the two-qubit gate `cx(&mut a, &mut b)` from
//! using the same qubit as both control and target (`cx(&mut q, &mut q)` is a compile error).
//!
//! The built circuit can be sent to two targets.
//! - [`Circuit::to_qasm`]: an OpenQASM 3 string commonly accepted by real quantum hardware providers
//! - [`Circuit::simulate`]: a small state-vector simulator that runs on a regular computer
//!
//! ```
//! use qxide::Circuit;
//!
//! let mut c = Circuit::new();
//! let mut a = c.qubit();
//! let mut b = c.qubit();
//! c.h(&mut a);
//! c.cx(&mut a, &mut b); // Bell state (entanglement)
//! let ma = c.measure(a);
//! let mb = c.measure(b);
//!
//! let shot = c.simulate(42);
//! assert_eq!(shot.get(ma), shot.get(mb)); // Entangled qubits always yield the same value
//! ```
//!
//! ## Mistakes prevented by the compiler
//!
//! Cloning a qubit (violating the no-cloning theorem):
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let q = c.qubit();
//! let copy = q.clone();
//! ```
//!
//! Reusing a measured qubit:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut q = c.qubit();
//! let _bit = c.measure(q);
//! c.h(&mut q);
//! ```
//!
//! Using the same qubit as both control and target:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut q = c.qubit();
//! c.cx(&mut q, &mut q);
//! ```
//!
//! Cloning a qubit register (`QReg` also violates the no-cloning theorem):
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let r = c.qreg(2);
//! let copy = r.clone();
//! ```
//!
//! Mutably borrowing the same register twice via indexing:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut r = c.qreg(2);
//! c.cx(&mut r[0], &mut r[1]);
//! ```
//!
//! Borrowing two distinct qubits simultaneously with `get_disjoint_mut` (duplicate indices are checked at runtime):
//! ```
//! let mut c = qxide::Circuit::new();
//! let mut r = c.qreg(3);
//! let [a, b] = r.get_disjoint_mut([0, 2]).unwrap();
//! c.cx(a, b);
//! ```
//!
//! ## Automatic uncomputation
//!
//! `with_ancilla` computes a value into an ancilla qubit, allows using it only as a control, and automatically emits the inverse operations. Example that compiles:
//! ```
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.cx_ctrl(&a, &mut t);
//!     },
//! );
//! ```
//!
//! Using a basis-changing gate (`h`) not provided by `ReversibleCircuit` in the compute phase:
//! ```compile_fail,E0599
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.h(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.cx_ctrl(&a, &mut t);
//!     },
//! );
//! ```
//!
//! Mutating the `&Qubit` ancilla qubit in the apply phase (`c.x(anc)`):
//! ```compile_fail,E0308
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.x(anc);
//!     },
//! );
//! ```
//!
//! Mutating an input qubit read in the compute phase during the apply phase (`c.h(&mut a)`, rejected by the borrow checker):
//! ```compile_fail,E0502
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.h(&mut a);
//!     },
//! );
//! ```
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/png/qxide-icon-512.png",
    html_favicon_url = "https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/png/qxide-icon-32.png"
)]

#[cfg(feature = "gpu")]
mod gpu;
mod groups;
// The `.qx` front end and interpreters (also built into the `qxide` binary, the playground and
// the app runtime). Only the items `lang` re-exports are public.
#[cfg(feature = "unstable-lang")]
#[allow(dead_code, unused_imports)]
#[path = "lang/mod.rs"]
mod lang_impl;
// `src/lang` names this crate `qxide`, as the binary does.
#[cfg(feature = "unstable-lang")]
extern crate self as qxide;

#[cfg(feature = "unstable-lang")]
pub mod lang {
    //! Check and run `.qx` programs from Rust using the same front end and interpreters as the
    //! `qxide` CLI.
    //!
    //! Use `check`, `run`, and `run_with` with a file name and source text. For configured runs,
    //! `check_input` and `run_input` take an `Input` (entry file, optional package `Project`, a
    //! file reader such as `read_file`, and `Permissions`) and a `Host` that handles
    //! standard-library calls (`native_host` provides the CLI's host). Use `test_input` to run
    //! `void testName()` functions like `qxide test`. Errors are returned as `Failure` holding
    //! rendered diagnostics.
    //!
    //! Available with the `unstable-lang` Cargo feature. Unstable until 1.0: any item in this
    //! module may change in a minor version.
    pub use crate::lang_impl::stdlib::{
        Access, Host, HostValue, Permissions, StdError, StdErrorKind, StdResult,
    };
    pub use crate::lang_impl::{
        check, check_input, qasm, read_file, run, run_input, run_with, test_input, Failure, Input,
        Limits, PackageRoot, Project, ReadFn, TestOutcome,
    };

    /// Returns the host used by `qxide run`: `std.log` to stderr, `std.time`, `std.io`, `std.env`,
    /// and `std.fs` for paths inside `data_root`.
    pub fn native_host(data_root: std::path::PathBuf) -> crate::lang_impl::stdlib::NativeStd {
        crate::lang_impl::cli_host(data_root)
    }

    pub use crate::lang_impl::stdlib::NativeStd;
}
mod rng;
mod stabilizer;
mod statevector;
mod uncompute;

pub use uncompute::ReversibleCircuit;

use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CIRCUIT_ID: AtomicUsize = AtomicUsize::new(0);

/// A single qubit in a circuit. Cannot be copied.
#[derive(Debug)]
pub struct Qubit {
    circuit: usize,
    index: usize,
}

/// A classical bit slot that stores a measurement outcome. Because it holds classical information, it can be freely copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bit {
    circuit: usize,
    index: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Gate {
    H,
    X,
    Z,
    S,
    T,
    Sdg,
    Tdg,
    Rx(f64),
    Ry(f64),
    Rz(f64),
}

impl Gate {
    fn name(self) -> &'static str {
        match self {
            Gate::H => "h",
            Gate::X => "x",
            Gate::Z => "z",
            Gate::S => "s",
            Gate::T => "t",
            Gate::Sdg => "sdg",
            Gate::Tdg => "tdg",
            Gate::Rx(_) => "rx",
            Gate::Ry(_) => "ry",
            Gate::Rz(_) => "rz",
        }
    }

    fn qasm(self) -> String {
        match self {
            Gate::Rx(a) | Gate::Ry(a) | Gate::Rz(a) => format!("{}({a:?})", self.name()),
            _ => self.name().into(),
        }
    }
}

/// Selects the simulator backend used to execute a circuit in `Circuit::simulate_with`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backend {
    /// Selects the backend via `Circuit::select_backend`. Uses the stabilizer simulator for Clifford-only circuits, and the state-vector simulator otherwise.
    Auto,
    /// Dense state vector of 2^n complex amplitudes. Supports all gates, but memory usage grows exponentially with the number of qubits.
    StateVector,
    /// Aaronson–Gottesman tableau: supports only Clifford operations (`h`/`s`/`sdg`/`x`/`z`/`cx`/`measure`/`reset`/branching), running in polynomial time to scale to thousands of qubits.
    Stabilizer,
    /// CubeCL (wgpu) f32-amplitude GPU state-vector prototype (gpu feature only). The unitary prefix before the first measurement, reset, or branch runs on the GPU, and the CPU finishes the rest. See docs/gpu.md for details.
    #[cfg(feature = "gpu")]
    Gpu,
}

/// Reason a simulation cannot be executed. Returned instead of silently producing a different result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SimError {
    /// Attempted to run a non-Clifford gate (`t`, `tdg`, `rx`, `ry`, `rz`, `ccx`) on the stabilizer backend. Gates in unexecuted branches and rotations at Clifford angles are also rejected.
    NotClifford {
        /// The OpenQASM name of the first offending gate (e.g., `t`, `rx`).
        gate: &'static str,
    },
    /// The memory required for the state vector (2^n × 16 bytes) exceeded the circuit limit, so nothing was allocated.
    OutOfMemory {
        /// Number of qubits in the circuit.
        qubits: usize,
        /// Number of bytes required for the state vector (2^n × 16, saturating at `u128::MAX`).
        required_bytes: u128,
        /// Currently active memory limit in bytes.
        limit_bytes: u128,
    },
}

/// Default state-vector memory limit of 1 GiB, accommodating up to 26 qubits.
pub const DEFAULT_MEMORY_LIMIT: u64 = 1 << 30;

/// Memory required for an `n_qubits`-qubit state vector (2^n 16-byte complex numbers, 2 `f64` values each), saturating at `u128::MAX` instead of overflowing.
pub fn state_vector_bytes(n_qubits: usize) -> u128 {
    if n_qubits <= 123 {
        16u128 << n_qubits
    } else {
        u128::MAX
    }
}

// Binary-prefixed size with one decimal, e.g. 16.0 TiB.
fn fmt_bytes(b: u128) -> String {
    const UNITS: [&str; 9] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimError::NotClifford { gate } => write!(
                f,
                "The stabilizer simulator only supports Clifford gates, and `{gate}` is not one. Consider using the state-vector backend"
            ),
            SimError::OutOfMemory {
                qubits,
                required_bytes,
                limit_bytes,
            } => {
                let (required, limit) = (fmt_bytes(*required_bytes), fmt_bytes(*limit_bytes));
                write!(
                    f,
                    "{qubits}-qubit state-vector simulation requires {required}, exceeding the limit ({limit}). Consider calling `set_memory_limit` or using a Clifford-only circuit with the stabilizer backend"
                )
            }
        }
    }
}

impl std::error::Error for SimError {}

#[derive(Debug, Clone)]
pub(crate) enum Op {
    Gate(Gate, usize),
    Cx(usize, usize),
    Ccx(usize, usize, usize),
    Measure {
        qubit: usize,
        bit: usize,
    },
    Reset(usize),
    If {
        bit: usize,
        value: bool,
        body: Vec<Op>,
    },
}

/// A circuit that records quantum operations in sequence.
#[derive(Debug)]
pub struct Circuit {
    id: usize,
    n_qubits: usize,
    n_bits: usize,
    ops: Vec<Op>,
    // Physical slots freed by measurement; reused (after a reset) by the next `qubit()`.
    free: std::collections::VecDeque<usize>,
    // Nesting level of `if_one`/`if_zero` bodies currently being recorded.
    depth: usize,
    memory_limit: u64,
    groups: groups::Groups,
    // Per slot: how many open `with_ancilla` scopes restrict it to control/diagonal use.
    locked: Vec<u32>,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self {
            id: NEXT_CIRCUIT_ID.fetch_add(1, Ordering::Relaxed),
            n_qubits: 0,
            n_bits: 0,
            ops: Vec::new(),
            free: std::collections::VecDeque::new(),
            depth: 0,
            memory_limit: DEFAULT_MEMORY_LIMIT,
            groups: groups::Groups::default(),
            locked: Vec::new(),
        }
    }

    /// Creates a new qubit in the |0⟩ state.
    pub fn qubit(&mut self) -> Qubit {
        // Inside a branch the reset would be conditional, so only reuse slots at top level.
        let reuse = if self.depth == 0 {
            self.free.pop_front()
        } else {
            None
        };
        let index = match reuse {
            Some(i) => {
                self.ops.push(Op::Reset(i));
                i
            }
            None => {
                self.n_qubits += 1;
                self.groups.add_slot();
                self.locked.push(0);
                self.n_qubits - 1
            }
        };
        Qubit {
            circuit: self.id,
            index,
        }
    }

    fn own(&self, q: &Qubit) -> usize {
        // Catch the one mistake the type system cannot prevent (a qubit from another circuit) at runtime.
        assert_eq!(q.circuit, self.id, "used a qubit from another circuit");
        q.index
    }

    fn gate(&mut self, g: Gate, q: &mut Qubit) {
        let i = self.own(q);
        self.push_gate(g, i);
    }

    fn push_gate(&mut self, g: Gate, i: usize) {
        // Diagonal gates keep the computational-basis value, so they are fine on locked slots.
        if !matches!(
            g,
            Gate::Z | Gate::S | Gate::Sdg | Gate::T | Gate::Tdg | Gate::Rz(_)
        ) {
            self.check_unlocked(i);
        }
        self.ops.push(Op::Gate(g, i));
    }

    // Runtime half of the `with_ancilla` contract; catches what interior mutability lets past the borrow checker.
    fn check_unlocked(&self, slot: usize) {
        assert!(self.locked[slot] == 0, "in the apply phase of with_ancilla, qubits used by compute cannot be used in operations other than control or diagonal phase gates");
    }

    /// Hadamard gate: puts |0⟩ into a superposition of |0⟩ and |1⟩.
    pub fn h(&mut self, q: &mut Qubit) {
        self.gate(Gate::H, q);
    }

    /// Pauli X (quantum NOT).
    pub fn x(&mut self, q: &mut Qubit) {
        self.gate(Gate::X, q);
    }

    /// Pauli Z (phase flip).
    pub fn z(&mut self, q: &mut Qubit) {
        self.gate(Gate::Z, q);
    }

    /// S gate: square root of Z, applying a phase of i to |1⟩ (equivalent to P(π/2)).
    pub fn s(&mut self, q: &mut Qubit) {
        self.gate(Gate::S, q);
    }

    /// T gate: square root of S, applying a phase of e^{iπ/4} to |1⟩.
    pub fn t(&mut self, q: &mut Qubit) {
        self.gate(Gate::T, q);
    }

    /// S† (S-dagger) gate: inverse of S and OpenQASM 3 `sdg`, applying a phase of -i to |1⟩ (equivalent to P(-π/2)).
    pub fn sdg(&mut self, q: &mut Qubit) {
        self.gate(Gate::Sdg, q);
    }

    /// T† (T-dagger) gate: inverse of T and OpenQASM 3 `tdg`, applying a phase of e^{-iπ/4} to |1⟩.
    pub fn tdg(&mut self, q: &mut Qubit) {
        self.gate(Gate::Tdg, q);
    }

    /// Rotates around the X axis by `theta` radians (exp(-iθX/2), equivalent to OpenQASM 3 `rx`).
    ///
    /// # Panics
    ///
    /// Panics if `theta` is NaN or infinite, since such an angle would make the simulated state NaN and cannot be written as OpenQASM.
    pub fn rx(&mut self, q: &mut Qubit, theta: f64) {
        check_angle("rx", theta);
        self.gate(Gate::Rx(theta), q);
    }

    /// Rotates around the Y axis by `theta` radians (exp(-iθY/2)).
    ///
    /// # Panics
    ///
    /// Panics if `theta` is NaN or infinite, since such an angle would make the simulated state NaN and cannot be written as OpenQASM.
    pub fn ry(&mut self, q: &mut Qubit, theta: f64) {
        check_angle("ry", theta);
        self.gate(Gate::Ry(theta), q);
    }

    /// Rotates around the Z axis by `theta` radians (exp(-iθZ/2)), changing only the phase.
    ///
    /// # Panics
    ///
    /// Panics if `theta` is NaN or infinite, since such an angle would make the simulated state NaN and cannot be written as OpenQASM.
    pub fn rz(&mut self, q: &mut Qubit, theta: f64) {
        check_angle("rz", theta);
        self.gate(Gate::Rz(theta), q);
    }

    /// Controlled NOT: flips target if control is |1⟩.
    /// Because both arguments are `&mut`, the same qubit cannot be passed for both.
    pub fn cx(&mut self, control: &mut Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.push_cx(c, t);
    }

    fn push_cx(&mut self, c: usize, t: usize) {
        self.check_unlocked(t);
        self.groups.union(c, t);
        self.ops.push(Op::Cx(c, t));
    }

    /// Toffoli (CCX, OpenQASM 3 `ccx`): flips `target` only when both control qubits are |1⟩. All three arguments are `&mut`, so passing the same qubit more than once is a compile error; as a non-Clifford gate, it is rejected by the stabilizer backend.
    pub fn ccx(&mut self, control1: &mut Qubit, control2: &mut Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        self.push_ccx(a, b, t);
    }

    /// Same gate as `ccx`, but takes the control qubits by shared reference (`&Qubit`), so it can also be used with ancilla or input qubits borrowed as `&Qubit`, such as during the apply phase of `with_ancilla`.
    /// Because shared references can alias, this panics if the same qubit is passed for both control qubits.
    pub fn ccx_ctrl(&mut self, control1: &Qubit, control2: &Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        check_distinct_controls(a, b);
        self.push_ccx(a, b, t);
    }

    fn push_ccx(&mut self, a: usize, b: usize, t: usize) {
        self.check_unlocked(t);
        self.groups.union(a, t);
        self.groups.union(b, t);
        self.ops.push(Op::Ccx(a, b, t));
    }

    /// Returns whether two qubits may be entangled, based on entanglement groups tracked at runtime with union-find.
    /// Every `cx`, including those inside `if_one` or `if_zero` bodies, transitively merges the two qubits' groups; measurement and top-level `reset` detach only that qubit while keeping the rest intact, whereas a `reset` inside a branch body may not execute and does not detach it.
    /// The result is a conservative over-approximation: `false` guarantees an unentangled product state, while `true` only means they may be entangled (including cases like a `cx` whose control qubit is `|0⟩`); passing the same qubit twice returns `true`.
    /// Panics if either qubit belongs to a different circuit.
    pub fn may_be_entangled(&self, a: &Qubit, b: &Qubit) -> bool {
        self.groups.same(self.own(a), self.own(b))
    }

    /// Measures the qubit. The qubit is consumed here, leaving a bit referencing the measurement result slot.
    pub fn measure(&mut self, q: Qubit) -> Bit {
        let i = self.own(&q);
        self.check_unlocked(i);
        // A conditional measurement would leave its bit undefined on the untaken path.
        assert!(
            self.depth == 0,
            "measurement inside a conditional branch is not supported; measure outside the branch instead"
        );
        let b = Bit {
            circuit: self.id,
            index: self.n_bits,
        };
        self.n_bits += 1;
        self.ops.push(Op::Measure {
            qubit: i,
            bit: b.index,
        });
        self.release_clean(i);
        b
    }

    // Returns a slot whose qubit is gone (measured or uncomputed) to the reuse pool.
    fn release_clean(&mut self, slot: usize) {
        self.free.push_back(slot);
        self.groups.detach(slot);
    }

    /// Allocates a register (`QReg`) of `n` new qubits initialized to |0⟩ at once.
    pub fn qreg(&mut self, n: usize) -> QReg {
        QReg {
            qubits: (0..n).map(|_| self.qubit()).collect(),
        }
    }

    /// Measures and consumes all qubits in the register, returning a `BitReg` of result bits in the same order.
    pub fn measure_reg(&mut self, reg: QReg) -> BitReg {
        BitReg {
            bits: reg.into_iter().map(|q| self.measure(q)).collect(),
        }
    }

    /// Resets a qubit to |0⟩ mid-circuit for reuse without consuming it, unlike `measure`. `qubit()` also automatically resets and reuses previously measured slots.
    pub fn reset(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.check_unlocked(i);
        self.ops.push(Op::Reset(i));
        // A reset inside a branch body may not run, so it cannot break any correlation.
        if self.depth == 0 {
            self.groups.detach(i);
        }
    }

    /// Classical feed-forward (quantum-classical coupling) that executes `body` only when the measured bit is 1. `measure` cannot be used inside it.
    pub fn if_one(&mut self, b: Bit, body: impl FnOnce(&mut Circuit)) {
        self.branch(b, true, body);
    }

    /// Executes the operations in `body` only when the measured bit is 0.
    pub fn if_zero(&mut self, b: Bit, body: impl FnOnce(&mut Circuit)) {
        self.branch(b, false, body);
    }

    fn branch(&mut self, b: Bit, value: bool, body: impl FnOnce(&mut Circuit)) {
        check_bit(self.id, b);
        // Record the body into a fresh op list, then splice it in as one conditional op.
        let outer = std::mem::take(&mut self.ops);
        self.depth += 1;
        body(self);
        self.depth -= 1;
        let body = std::mem::replace(&mut self.ops, outer);
        self.ops.push(Op::If {
            bit: b.index,
            value,
            body,
        });
    }

    /// Exports the circuit as an OpenQASM 3 string.
    pub fn to_qasm(&self) -> String {
        let mut s = String::from("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n");
        s += &format!("qubit[{}] q;\n", self.n_qubits);
        if self.n_bits > 0 {
            s += &format!("bit[{}] c;\n", self.n_bits);
        }
        write_qasm(&self.ops, 0, &mut s);
        s
    }

    fn check_memory(&self) -> Result<(), SimError> {
        let required = state_vector_bytes(self.n_qubits);
        if required > u128::from(self.memory_limit) {
            return Err(SimError::OutOfMemory {
                qubits: self.n_qubits,
                required_bytes: required,
                limit_bytes: self.memory_limit.into(),
            });
        }
        Ok(())
    }

    /// Returns `true` if the circuit, including branch bodies, uses only Clifford operations (`h`, `s`, `sdg`, `x`, `z`, `cx`, `measure`, `reset`).
    pub fn is_clifford(&self) -> bool {
        stabilizer::first_non_clifford(&self.ops).is_none()
    }

    /// Returns the backend that `Backend::Auto` would choose: `Stabilizer` for Clifford-only circuits, or `StateVector` otherwise.
    pub fn select_backend(&self) -> Backend {
        if self.is_clifford() {
            Backend::Stabilizer
        } else {
            Backend::StateVector
        }
    }

    /// Sets the maximum memory allocation for this circuit's state-vector backend. Returns `SimError::OutOfMemory` if the limit is exceeded.
    pub fn set_memory_limit(&mut self, bytes: u64) {
        self.memory_limit = bytes;
    }

    /// Executes the circuit once using the specified `backend`. The same `seed` yields identical results on the same backend, though results may differ across backends due to different RNG consumption.
    pub fn simulate_with(&self, backend: Backend, seed: u64) -> Result<Shot, SimError> {
        let backend = match backend {
            Backend::Auto => self.select_backend(),
            b => b,
        };
        let bits = match backend {
            // Auto was resolved to a concrete backend just above.
            Backend::Auto => unreachable!(),
            Backend::StateVector => {
                self.check_memory()?;
                statevector::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
            // The GPU result is read back into a CPU state vector, so the same limit applies.
            #[cfg(feature = "gpu")]
            Backend::Gpu => {
                self.check_memory()?;
                gpu::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
            Backend::Stabilizer => {
                if let Some(gate) = stabilizer::first_non_clifford(&self.ops) {
                    return Err(SimError::NotClifford { gate });
                }
                stabilizer::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
        };
        Ok(Shot {
            circuit: self.id,
            bits,
        })
    }

    /// Executes the circuit once (a single "shot") on the state-vector simulator.
    /// The same seed produces the same result. Uses a vector of size 2^n for n qubits, so this is intended for small circuits.
    pub fn simulate(&self, seed: u64) -> Shot {
        self.simulate_with(Backend::StateVector, seed)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Runs `shots` independent shots and yields them in order.
    /// Shot `k` calls [`Circuit::simulate_with`] with `backend` and the [`shot_seed`] for `(seed, k)`, so shots are independent and the same `seed` reproduces the same shots.
    /// Use this instead of calling [`Circuit::simulate`] with consecutive seeds when sampling many shots.
    /// Yielded errors are those returned by [`Circuit::simulate_with`].
    pub fn simulate_shots(
        &self,
        backend: Backend,
        seed: u64,
        shots: u64,
    ) -> impl Iterator<Item = Result<Shot, SimError>> + '_ {
        (0..shots).map(move |k| self.simulate_with(backend, shot_seed(seed, k)))
    }
}

/// Returns the seed passed to the simulator for shot number `shot` of a run with seed `seed`.
/// Mixes both values so runs with nearby seeds (for example `--seed 0` and `--seed 1`) share no shots.
/// Used by [`Circuit::simulate_shots`] and `qxide run --seed`.
pub fn shot_seed(seed: u64, shot: u64) -> u64 {
    // Two finalizer rounds (run, then shot) return a mixed value, never `base + k * gamma`: the
    // simulators step their own state by gamma, so gamma-spaced seeds would share draws across shots.
    let run = rng::SplitMix64(seed).next_u64();
    rng::SplitMix64(run ^ shot.wrapping_mul(0xD1B5_4A32_D192_ED03)).next_u64()
}

// Shared-reference controls can alias; OpenQASM rejects a gate with a repeated qubit.
// NaN/inf would make every amplitude NaN and print `rx(NaN)`, which OpenQASM cannot parse.
fn check_angle(gate: &str, theta: f64) {
    assert!(
        theta.is_finite(),
        "the rotation angle of {gate} must be finite, got {theta}"
    );
}

fn check_distinct_controls(a: usize, b: usize) {
    assert_ne!(a, b, "the two control qubits of ccx must be distinct");
}

fn check_bit(circuit: usize, b: Bit) {
    assert_eq!(b.circuit, circuit, "bit belongs to a different circuit");
}

fn write_qasm(ops: &[Op], depth: usize, s: &mut String) {
    let pad = "  ".repeat(depth);
    for op in ops {
        s.push_str(&pad);
        match op {
            Op::Gate(g, i) => *s += &format!("{} q[{i}];\n", g.qasm()),
            Op::Cx(a, b) => *s += &format!("cx q[{a}], q[{b}];\n"),
            Op::Ccx(a, b, t) => *s += &format!("ccx q[{a}], q[{b}], q[{t}];\n"),
            Op::Measure { qubit, bit } => *s += &format!("c[{bit}] = measure q[{qubit}];\n"),
            Op::Reset(i) => *s += &format!("reset q[{i}];\n"),
            Op::If { bit, value, body } => {
                // Qiskit's importer only accepts `bit == const bool` for single-bit conditions.
                *s += &format!("if (c[{bit}] == {value}) {{\n");
                write_qasm(body, depth + 1, s);
                *s += &pad;
                *s += "}\n";
            }
        }
    }
}

/// Measurement results from a single execution.
#[derive(Debug, Clone)]
pub struct Shot {
    circuit: usize,
    bits: Vec<bool>,
}

impl Shot {
    pub fn get(&self, b: Bit) -> bool {
        check_bit(self.circuit, b);
        self.bits[b.index]
    }

    /// Reads the result as an unsigned integer, treating the first bit of the register as the least significant bit (LSB).
    pub fn value(&self, reg: &BitReg) -> u64 {
        assert!(
            reg.len() <= 64,
            "cannot convert more than 64 bits to an integer"
        );
        reg.iter()
            .enumerate()
            .fold(0, |v, (i, &b)| v | (u64::from(self.get(b)) << i))
    }
}

/// A register holding multiple qubits. Like `Qubit`, it cannot be copied, and it supports indexing and iteration like a slice.
#[derive(Debug)]
pub struct QReg {
    qubits: Vec<Qubit>,
}

impl std::ops::Deref for QReg {
    type Target = [Qubit];
    fn deref(&self) -> &[Qubit] {
        &self.qubits
    }
}

impl std::ops::DerefMut for QReg {
    fn deref_mut(&mut self) -> &mut [Qubit] {
        &mut self.qubits
    }
}

impl IntoIterator for QReg {
    type Item = Qubit;
    type IntoIter = std::vec::IntoIter<Qubit>;
    fn into_iter(self) -> Self::IntoIter {
        self.qubits.into_iter()
    }
}

impl<'a> IntoIterator for &'a mut QReg {
    type Item = &'a mut Qubit;
    type IntoIter = std::slice::IterMut<'a, Qubit>;
    fn into_iter(self) -> Self::IntoIter {
        self.qubits.iter_mut()
    }
}

/// A classical bit register that holds measurement results. Because it represents classical information, it can be freely copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitReg {
    bits: Vec<Bit>,
}

impl std::ops::Deref for BitReg {
    type Target = [Bit];
    fn deref(&self) -> &[Bit] {
        &self.bits
    }
}

impl From<Vec<Bit>> for BitReg {
    fn from(bits: Vec<Bit>) -> Self {
        Self { bits }
    }
}
