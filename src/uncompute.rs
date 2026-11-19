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

use crate::{Circuit, Op, Qubit};

/// `Circuit::with_ancilla`와 `Circuit::with_ancillas`의 계산 단계에 전달되는 제한된 회로 빌더다.
/// 계산 기저 상태만 치환하는 `x`, `cx`, `ccx`만 제공하므로 계산 단계는 |x⟩|0⟩을 |x⟩|f(x)⟩로 옮기고, 이를 역순으로 다시 실행하면 모든 보조 큐비트가 정확히 |0⟩으로 돌아온다.
/// `measure`, `reset`, 분기, 기저를 바꾸는 게이트(`h`, `rx`, `ry` 등)는 보조 큐비트가 `t`와 얽힌 채 남을 수 있어(예: `h(anc)` 후 `cx_ctrl(anc, t)` 뒤 `h(anc)`) 제공하지 않고 대각 위상 게이트도 정확히 상쇄되므로 빠져 있어, 이런 실수는 컴파일 오류로 차단된다.
#[derive(Debug)]
pub struct ReversibleCircuit {
    circuit: usize,
    ops: Vec<Op>,
}

impl ReversibleCircuit {
    fn own(&self, q: &Qubit) -> usize {
        assert_eq!(
            q.circuit, self.circuit,
            "보조 큐비트 계산 단계에서 다른 회로의 큐비트를 사용했습니다"
        );
        q.index
    }

    /// 계산 단계에 기록하는 파울리 X(양자 NOT) 게이트로, 자기 자신이 역게이트다.
    pub fn x(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::Gate(crate::Gate::X, i));
    }

    /// 계산 단계에 기록하는 제어 NOT 게이트로 자기 자신이 역게이트이며, 제어를 공유 참조(`&Qubit`)로만 빌리므로 여기서 읽기만 한 데이터 큐비트는 적용 단계에서도 `&Qubit`으로 읽을 수는 있지만 수정할 수는 없다.
    pub fn cx(&mut self, control: &Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.ops.push(Op::Cx(c, t));
    }

    /// 계산 단계에 기록되는 자기 역원(self-inverse) Toffoli 게이트로, 계산 단계가 AND형(비선형) 불 함수를 보조 큐비트에 계산할 수 있게 한다.
    /// 두 제어 큐비트가 같으면 패닉한다.
    pub fn ccx(&mut self, control1: &Qubit, control2: &Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        crate::check_distinct_controls(a, b);
        self.ops.push(Op::Ccx(a, b, t));
    }
}

impl Circuit {
    /// 제어를 공유 참조(`&Qubit`)로 받는다는 점만 빼면 `cx`와 같은 게이트다.
    /// CX는 제어 큐비트의 계산 기저 값을 바꾸지 않으므로, `with_ancilla`의 적용 단계가 `&Qubit`으로만 볼 수 있는 보조 큐비트나 입력 큐비트를 제어로 쓸 수 있다.
    pub fn cx_ctrl(&mut self, control: &Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.push_cx(c, t);
    }

    /// `|0⟩` 상태의 새 보조 큐비트 하나를 할당해 범위 기반 자동 되돌림(uncomputation)을 수행한다.
    /// `compute(&mut ReversibleCircuit, &mut Qubit)`으로 가역 게이트를 기록해 내보낸 뒤 `apply(&mut Circuit, &Qubit) -> R`을 실행하며, 적용 단계에서는 보조 큐비트를 `cx_ctrl` 같은 제어로만 쓸 수 있다.
    /// 이어서 계산 단계 게이트를 역순으로 다시 내보내(되돌림) 보조 큐비트를 `|0⟩`으로 복원하고, 슬롯을 재사용하도록 해제하며 얽힘 그룹에서 제거한 뒤 `apply`의 값을 반환한다.
    /// 계산 단계에서 읽은 큐비트는 범위가 끝날 때까지 빌려져 있으므로 적용 단계에서 읽기만 가능하고 바꾸거나 측정할 수 없으며, 이는 빌림 검사기가 강제한다.
    /// 다만 이 보장은 일반 빌림에만 적용되며, `RefCell`, `Mutex` 등의 내부 가변성 뒤에 있는 큐비트는 적용 단계에서도 여전히 바꿀 수 있어 보조 큐비트가 `|0⟩`으로 복원되지 않고 범위가 끝난 뒤 `may_be_entangled`가 얽히지 않았다고 잘못 보고할 수 있으므로 이러한 큐비트는 두 단계에 걸쳐 사용해서는 안 된다.
    /// 여러 보조 큐비트가 필요하면 `with_ancillas`를 사용한다.
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

    /// `with_ancilla`와 같지만 `N`개의 보조 큐비트를 `compute`에는 `&mut [Qubit; N]`으로, `apply`에는 `&[Qubit; N]`으로 넘겨 함께 되돌리고 해제한다.
    /// `N`을 추론할 수 있도록 `compute` 클로저에 `[_; N]` 형식 표기(예: `|u, [x, y]: &mut [_; 2]|`)가 필요할 수 있다.
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
        let result = apply(self, &anc);
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
            Op::Gate(g, q) => self.ops.push(Op::Gate(g, q)),
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
