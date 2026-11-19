// Dense state-vector backend: 2^n complex amplitudes.

use crate::rng::SplitMix64;
use crate::{Gate, Op};

pub(crate) fn run(ops: &[Op], n_qubits: usize, n_bits: usize, seed: u64) -> Vec<bool> {
    let mut state = vec![C::ZERO; 1usize << n_qubits];
    state[0] = C::ONE;
    run_from(ops, state, n_bits, seed)
}

// Continues a run from an already-prepared state (used by the GPU backend).
#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
pub(crate) fn run_from(ops: &[Op], mut state: Vec<C>, n_bits: usize, seed: u64) -> Vec<bool> {
    let mut rng = SplitMix64(seed);
    let mut bits = vec![false; n_bits];
    exec(ops, &mut state, &mut bits, &mut rng);
    bits
}

// Exact |amplitude|² of every basis state after running a measurement-free circuit.
#[cfg(test)]
pub(crate) fn probabilities(ops: &[Op], n_qubits: usize) -> Vec<f64> {
    amplitudes(ops, n_qubits)
        .iter()
        .map(|a| a.norm_sqr())
        .collect()
}

#[cfg(test)]
pub(crate) fn amplitudes(ops: &[Op], n_qubits: usize) -> Vec<C> {
    let mut rng = SplitMix64(0);
    let mut state = vec![C::ZERO; 1usize << n_qubits];
    state[0] = C::ONE;
    exec(ops, &mut state, &mut [], &mut rng);
    state
}

// [[m00, m01], [m10, m11]], following the OpenQASM 3 stdgates.inc definitions.
pub(crate) fn matrix(g: Gate) -> [[C; 2]; 2] {
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let re = |x: f64| C { re: x, im: 0.0 };
    let im = |y: f64| C { re: 0.0, im: y };
    match g {
        Gate::H => [[re(r), re(r)], [re(r), re(-r)]],
        Gate::X => [[C::ZERO, C::ONE], [C::ONE, C::ZERO]],
        Gate::Z => [[C::ONE, C::ZERO], [C::ZERO, re(-1.0)]],
        Gate::S => [[C::ONE, C::ZERO], [C::ZERO, im(1.0)]],
        Gate::T => [
            [C::ONE, C::ZERO],
            [C::ZERO, C::cis(std::f64::consts::FRAC_PI_4)],
        ],
        Gate::Sdg => [[C::ONE, C::ZERO], [C::ZERO, im(-1.0)]],
        Gate::Tdg => [
            [C::ONE, C::ZERO],
            [C::ZERO, C::cis(-std::f64::consts::FRAC_PI_4)],
        ],
        Gate::Rx(a) => {
            let (s, c) = (a / 2.0).sin_cos();
            [[re(c), im(-s)], [im(-s), re(c)]]
        }
        Gate::Ry(a) => {
            let (s, c) = (a / 2.0).sin_cos();
            [[re(c), re(-s)], [re(s), re(c)]]
        }
        Gate::Rz(a) => [[C::cis(-a / 2.0), C::ZERO], [C::ZERO, C::cis(a / 2.0)]],
    }
}

fn exec(ops: &[Op], state: &mut [C], bits: &mut [bool], rng: &mut SplitMix64) {
    let dim = state.len();
    for op in ops {
        match op {
            &Op::Gate(g, q) => {
                let m = 1 << q;
                let [[m00, m01], [m10, m11]] = matrix(g);
                for i in 0..dim {
                    if i & m == 0 {
                        let (a, b) = (state[i], state[i | m]);
                        state[i] = m00 * a + m01 * b;
                        state[i | m] = m10 * a + m11 * b;
                    }
                }
            }
            &Op::Cx(c, t) => {
                let (mc, mt) = (1 << c, 1 << t);
                for i in 0..dim {
                    if i & mc != 0 && i & mt == 0 {
                        state.swap(i, i | mt);
                    }
                }
            }
            &Op::Measure { qubit, bit } => {
                bits[bit] = collapse(state, qubit, rng);
            }
            &Op::Reset(q) => {
                // Reset = measure, then flip back to |0⟩ if the outcome was 1.
                if collapse(state, q, rng) {
                    let m = 1 << q;
                    for i in 0..dim {
                        if i & m == 0 {
                            state.swap(i, i | m);
                        }
                    }
                }
            }
            Op::If { bit, value, body } => {
                if bits[*bit] == *value {
                    exec(body, state, bits, rng);
                }
            }
        }
    }
}

// Projective measurement of qubit `q`: samples the outcome, collapses and renormalises the state.
fn collapse(state: &mut [C], q: usize, rng: &mut SplitMix64) -> bool {
    let m = 1 << q;
    let p1: f64 = state
        .iter()
        .enumerate()
        .filter(|(i, _)| i & m != 0)
        .map(|(_, a)| a.norm_sqr())
        .sum();
    let one = rng.next_f64() < p1;
    let keep = if one { p1 } else { 1.0 - p1 };
    let renorm = 1.0 / keep.sqrt();
    for (i, amp) in state.iter_mut().enumerate() {
        *amp = if ((i & m) != 0) == one {
            amp.scale(renorm)
        } else {
            C::ZERO
        };
    }
    one
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct C {
    pub(crate) re: f64,
    pub(crate) im: f64,
}

impl C {
    const ZERO: C = C { re: 0.0, im: 0.0 };
    const ONE: C = C { re: 1.0, im: 0.0 };
    fn scale(self, k: f64) -> C {
        C {
            re: self.re * k,
            im: self.im * k,
        }
    }
    pub(crate) fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    // e^{iφ}
    fn cis(phi: f64) -> C {
        let (im, re) = phi.sin_cos();
        C { re, im }
    }
}

impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
}

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
}
