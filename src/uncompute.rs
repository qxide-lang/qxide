// Scoped uncomputation (docs/design/uncomputation.md).
//
// Soundness argument: the compute step may only permute computational basis states
// (`x`, `cx`, `ccx`), so it maps |x>|0> to |x>|f(x)>. The apply step sees the ancillas and every
// qubit the compute step read only through `&Qubit`, and every `Circuit` method that takes
// a `&Qubit` acts block-diagonally in that qubit's computational basis. Hence applying the
// recorded compute ops in reverse restores every ancilla to exactly |0>.
// Basis-changing gates (`h`, `rx`, `ry`) are deliberately absent: compute `h(anc)`,
// apply `cx_ctrl(anc, t)`, uncompute `h(anc)` leaves the ancilla entangled with `t`.
// Diagonal gates are absent too: they commute with the apply step and cancel exactly.
// Interior mutability (RefCell, Mutex) gets past the borrow checker, so `Circuit::locked`
// enforces the same apply-step rule at run time.

use crate::{Circuit, Op, Qubit};

/// A restricted circuit builder passed to the compute phase of `Circuit::with_ancilla` and `Circuit::with_ancillas`.
/// Because it exposes only `x`, `cx`, and `ccx`—which only permute computational basis states—the compute phase maps |x⟩|0⟩ to |x⟩|f(x)⟩, and replaying it in reverse returns all ancilla qubits to |0⟩ exactly.
/// `measure`, `reset`, branching, and basis-changing gates (`h`, `rx`, `ry`, etc.) are not provided because they could leave an ancilla entangled with `t` (e.g., `h(anc)`, then `cx_ctrl(anc, t)`, then `h(anc)`), and diagonal phase gates are also omitted because they cancel out exactly, so such mistakes are caught as compile errors.
#[derive(Debug)]
pub struct ReversibleCircuit {
    circuit: usize,
    ops: Vec<Op>,
}

impl ReversibleCircuit {
    fn own(&self, q: &Qubit) -> usize {
        assert_eq!(
            q.circuit, self.circuit,
            "qubit from another circuit used in the ancilla compute phase"
        );
        q.index
    }

    /// Pauli-X (quantum NOT) gate recorded in the compute phase; it is self-inverse.
    pub fn x(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::Gate(crate::Gate::X, i));
    }

    /// Controlled-NOT gate recorded in the compute phase; it is self-inverse and borrows the control only by shared reference (`&Qubit`), so data qubits read here can still be read as `&Qubit` in the apply phase, but cannot be mutated.
    pub fn cx(&mut self, control: &Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.ops.push(Op::Cx(c, t));
    }

    /// Self-inverse Toffoli gate recorded in the compute phase, allowing the compute phase to compute AND-like (nonlinear) Boolean functions into an ancilla qubit.
    /// Panics if the two control qubits are the same.
    pub fn ccx(&mut self, control1: &Qubit, control2: &Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        crate::check_distinct_controls(a, b);
        self.ops.push(Op::Ccx(a, b, t));
    }
}

impl Circuit {
    /// Same as `cx`, except that it takes the control by shared reference (`&Qubit`).
    /// Because CX does not change the computational-basis value of the control qubit, the apply phase of `with_ancilla` can use an ancilla or input qubit visible only as `&Qubit` as a control.
    pub fn cx_ctrl(&mut self, control: &Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.push_cx(c, t);
    }

    /// Allocates a new ancilla qubit in the `|0⟩` state and performs scoped automatic uncomputation.
    /// Records and emits reversible gates via `compute(&mut ReversibleCircuit, &mut Qubit)`, then runs `apply(&mut Circuit, &Qubit) -> R`, where the ancilla qubit can only be used as a control such as in `cx_ctrl`.
    /// Then re-emits the compute-phase gates in reverse order (uncomputation) to restore the ancilla qubit to `|0⟩`, releases its slot for reuse, removes it from its entanglement group, and returns the value from `apply`.
    /// Qubits read in the compute phase remain borrowed until the end of the scope, so they can only be read—and cannot be mutated or measured—in the apply phase, enforced by the borrow checker.
    /// However, because qubits behind interior mutability such as `RefCell` or `Mutex` can bypass the borrow checker, the same rule is also checked at runtime: using any qubit touched by the compute phase (input or ancilla qubits) in the apply phase for anything other than a control or a diagonal phase gate (`z`, `s`, `sdg`, `t`, `tdg`, `rz`)—such as `h`/`x`/`rx`/`ry`, the target of `cx`/`ccx`, `measure`, `reset`, or writes from a nested scope's compute phase—panics.
    /// Use `with_ancillas` when multiple ancilla qubits are needed.
    pub fn with_ancilla<R>(
        &mut self,
        compute: impl FnOnce(&mut ReversibleCircuit, &mut Qubit),
        apply: impl FnOnce(&mut Circuit, &Qubit) -> R,
    ) -> R {
        self.with_ancillas(
            |u, [anc]: &mut [Qubit; 1]| compute(u, anc),
            |c, [anc]| apply(c, anc),
        )
    }

    /// Same as `with_ancilla`, except that `N` ancilla qubits are passed as `&mut [Qubit; N]` to `compute` and as `&[Qubit; N]` to `apply`, and are uncomputed and released together.
    /// The `compute` closure may need a `[_; N]` type annotation (e.g., `|u, [x, y]: &mut [_; 2]|`) so that `N` can be inferred.
    pub fn with_ancillas<const N: usize, R>(
        &mut self,
        compute: impl FnOnce(&mut ReversibleCircuit, &mut [Qubit; N]),
        apply: impl FnOnce(&mut Circuit, &[Qubit; N]) -> R,
    ) -> R {
        let mut anc: [Qubit; N] = std::array::from_fn(|_| self.qubit());
        let mut rev = ReversibleCircuit {
            circuit: self.id,
            ops: Vec::new(),
        };
        compute(&mut rev, &mut anc);
        for op in &rev.ops {
            self.push_reversible(op);
        }
        // Everything compute touched stays control/diagonal-only until uncompute.
        let mut touched: Vec<usize> = anc.iter().map(|q| q.index).collect();
        for op in &rev.ops {
            match *op {
                Op::Gate(_, q) => touched.push(q),
                Op::Cx(c, t) => touched.extend([c, t]),
                Op::Ccx(a, b, t) => touched.extend([a, b, t]),
                Op::Measure { .. } | Op::Reset(_) | Op::If { .. } => unreachable!(),
            }
        }
        touched.sort_unstable();
        touched.dedup();
        for &q in &touched {
            self.locked[q] += 1;
        }
        let result = apply(self, &anc);
        for &q in &touched {
            self.locked[q] -= 1;
        }
        // `x`, `cx` and `ccx` are self-inverse, so the inverse is the same ops in reverse order.
        for op in rev.ops.iter().rev() {
            self.push_reversible(op);
        }
        for q in anc {
            self.release_clean(q.index);
        }
        result
    }

    fn push_reversible(&mut self, op: &Op) {
        match *op {
            Op::Cx(c, t) => self.push_cx(c, t),
            Op::Ccx(a, b, t) => self.push_ccx(a, b, t),
            Op::Gate(g, q) => self.push_gate(g, q),
            // ReversibleCircuit only records `x`, `cx` and `ccx`.
            Op::Measure { .. } | Op::Reset(_) | Op::If { .. } => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::statevector;
    use crate::{Circuit, Gate, Op};

    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }
    }

    // Probability that slot `q` reads 1.
    fn weight_on_one(ops: &[Op], n_qubits: usize, q: usize) -> f64 {
        statevector::amplitudes(ops, n_qubits)
            .iter()
            .enumerate()
            .filter(|(i, _)| i >> q & 1 == 1)
            .map(|(_, a)| a.norm_sqr())
            .sum()
    }

    // Why `h` is not offered by ReversibleCircuit: the same scope shape leaves the ancilla dirty.
    #[test]
    fn basis_changing_compute_would_leave_ancilla_dirty() {
        let (anc, t) = (0, 1);
        let ops = [
            Op::Gate(Gate::H, anc),
            Op::Cx(anc, t),
            Op::Gate(Gate::H, anc),
        ];
        assert!((weight_on_one(&ops, 2, anc) - 0.5).abs() < 1e-12);
    }

    // Allowed apply-step uses (nested scope reading the outer ancilla, diagonal gates and
    // controls on a compute input) must still leave both ancillas exactly |0>.
    #[test]
    fn nested_scope_and_diagonal_use_keep_ancillas_clean() {
        let mut c = Circuit::new();
        let mut a = c.qubit();
        let mut b = c.qubit();
        let mut t = c.qubit();
        c.h(&mut a);
        c.ry(&mut b, 1.1);
        c.h(&mut t);
        let mut slots = [0usize; 2];
        let a_cell = std::cell::RefCell::new(a);
        c.with_ancilla(
            |u, anc| u.ccx(&a_cell.borrow(), &b, anc),
            |c, outer| {
                slots[0] = outer.index;
                c.t(&mut a_cell.borrow_mut());
                c.rz(&mut a_cell.borrow_mut(), 0.4);
                c.with_ancilla(
                    |u, inner| u.cx(outer, inner),
                    |c, inner| {
                        slots[1] = inner.index;
                        c.ccx_ctrl(inner, &a_cell.borrow(), &mut t);
                    },
                );
            },
        );
        for q in slots {
            let w = weight_on_one(&c.ops, c.n_qubits, q);
            assert!(w < 1e-12, "ancilla slot {q} has weight {w} on |1>");
        }
    }

    // Random reversible compute on inputs in superposition, random control-only apply step:
    // straight after the scope every ancilla must be |0> with probability 1 (no reset masks it).
    #[test]
    fn ancillas_are_exactly_zero_after_scope() {
        let mut rng = Lcg(0xA1C1);
        for case in 0..150 {
            let n_in = 1 + rng.below(3) as usize;
            let n_t = 1 + rng.below(2) as usize;
            let mut c = Circuit::new();
            let mut inputs: Vec<_> = (0..n_in).map(|_| c.qubit()).collect();
            let mut targets: Vec<_> = (0..n_t).map(|_| c.qubit()).collect();
            for q in inputs.iter_mut().chain(targets.iter_mut()) {
                match rng.below(3) {
                    0 => c.h(q),
                    1 => c.ry(q, 0.3 + rng.below(20) as f64 / 7.0),
                    _ => {
                        c.h(q);
                        c.t(q);
                    }
                }
            }
            let compute_plan: Vec<(u64, u64, u64)> = (0..1 + rng.below(8))
                .map(|_| (rng.below(4), rng.below(8), rng.below(8)))
                .collect();
            let apply_plan: Vec<(u64, u64, u64)> = (0..1 + rng.below(6))
                .map(|_| (rng.below(4), rng.below(8), rng.below(8)))
                .collect();
            let mut anc_slots = [0usize; 3];
            let inputs_ref = &inputs;
            c.with_ancillas(
                |u, anc: &mut [_; 3]| {
                    for &(kind, i, j) in &compute_plan {
                        let a = j as usize % 3;
                        match kind {
                            0 => u.x(&mut anc[a]),
                            1 => u.cx(&inputs_ref[i as usize % n_in], &mut anc[a]),
                            3 => {
                                // Two distinct controls: another ancilla and an input.
                                let b = (a + 1 + i as usize % 2) % 3;
                                let [x, y] = anc.get_disjoint_mut([b, a]).unwrap();
                                u.ccx(x, &inputs_ref[j as usize % n_in], y);
                            }
                            _ => {
                                let b = (a + 1 + i as usize % 2) % 3;
                                let [x, y] = anc.get_disjoint_mut([b, a]).unwrap();
                                u.cx(x, y);
                            }
                        }
                    }
                },
                |c, anc| {
                    anc_slots = std::array::from_fn(|k| anc[k].index);
                    for &(kind, i, j) in &apply_plan {
                        let t = &mut targets[j as usize % n_t];
                        match kind {
                            0 => c.cx_ctrl(&anc[i as usize % 3], t),
                            1 => c.cx_ctrl(&inputs_ref[i as usize % n_in], t),
                            3 => {
                                c.ccx_ctrl(&anc[i as usize % 3], &inputs_ref[j as usize % n_in], t)
                            }
                            _ => {
                                c.h(t);
                                c.t(t);
                            }
                        }
                    }
                },
            );
            for &q in &anc_slots {
                let w = weight_on_one(&c.ops, c.n_qubits, q);
                assert!(
                    w < 1e-12,
                    "case {case}: ancilla slot {q} has weight {w} on |1>"
                );
            }
        }
    }
}
