// Stabilizer backend: Aaronson–Gottesman tableau (quant-ph/0406196), rows bit-packed in u64 words.
// Rows 0..n are destabilizers, n..2n stabilizers, row 2n is scratch.

use crate::rng::SplitMix64;
use crate::{Gate, Op};

// First gate (in program order, including branch bodies) the tableau cannot represent.
pub(crate) fn first_non_clifford(ops: &[Op]) -> Option<&'static str> {
    ops.iter().find_map(|op| match op {
        Op::Gate(g @ (Gate::T | Gate::Tdg | Gate::Rx(_) | Gate::Ry(_) | Gate::Rz(_)), _) => {
            Some(g.name())
        }
        Op::Ccx(..) => Some("ccx"),
        Op::If { body, .. } => first_non_clifford(body),
        _ => None,
    })
}

pub(crate) fn run(ops: &[Op], n_qubits: usize, n_bits: usize, seed: u64) -> Vec<bool> {
    debug_assert!(first_non_clifford(ops).is_none());
    let mut rng = SplitMix64(seed);
    let mut t = Tableau::new(n_qubits);
    let mut bits = vec![false; n_bits];
    exec(ops, &mut t, &mut bits, &mut rng);
    bits
}

fn exec(ops: &[Op], t: &mut Tableau, bits: &mut [bool], rng: &mut SplitMix64) {
    for op in ops {
        match op {
            &Op::Gate(g, q) => t.gate(g, q),
            &Op::Cx(c, x) => t.cx(c, x),
            // Rejected by `first_non_clifford` before simulation.
            Op::Ccx(..) => unreachable!(),
            &Op::Measure { qubit, bit } => {
                bits[bit] = t.measure(qubit, || rng.next_u64() & 1 == 1).0;
            }
            &Op::Reset(q) => {
                if t.measure(q, || rng.next_u64() & 1 == 1).0 {
                    t.x(q);
                }
            }
            Op::If { bit, value, body } => {
                if bits[*bit] == *value {
                    exec(body, t, bits, rng);
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Tableau {
    n: usize,
    w: usize,
    x: Vec<u64>,
    z: Vec<u64>,
    r: Vec<bool>,
}

impl Tableau {
    pub(crate) fn new(n: usize) -> Self {
        let w = n.div_ceil(64).max(1);
        let rows = 2 * n + 1;
        let mut t = Tableau {
            n,
            w,
            x: vec![0; rows * w],
            z: vec![0; rows * w],
            r: vec![false; rows],
        };
        for i in 0..n {
            t.x[i * w + i / 64] |= 1 << (i % 64);
            t.z[(n + i) * w + i / 64] |= 1 << (i % 64);
        }
        t
    }

    fn gate(&mut self, g: Gate, q: usize) {
        match g {
            Gate::H => self.h(q),
            Gate::S => self.s(q),
            // S† = S³ (S has order 4).
            Gate::Sdg => {
                self.s(q);
                self.s(q);
                self.s(q);
            }
            Gate::X => self.x(q),
            Gate::Z => self.z(q),
            Gate::T | Gate::Tdg | Gate::Rx(_) | Gate::Ry(_) | Gate::Rz(_) => {
                // Non-Clifford gates are rejected by `first_non_clifford` before simulation.
                unreachable!()
            }
        }
    }

    // Applies `f(x_a, z_a, r) -> (x_a, z_a, r)` to every non-scratch row.
    fn each_row(&mut self, a: usize, mut f: impl FnMut(bool, bool, bool) -> (bool, bool, bool)) {
        let (k, m) = (a / 64, 1u64 << (a % 64));
        for row in 0..2 * self.n {
            let i = row * self.w + k;
            let (x, z) = (self.x[i] & m != 0, self.z[i] & m != 0);
            let (nx, nz, nr) = f(x, z, self.r[row]);
            self.x[i] = (self.x[i] & !m) | if nx { m } else { 0 };
            self.z[i] = (self.z[i] & !m) | if nz { m } else { 0 };
            self.r[row] = nr;
        }
    }

    pub(crate) fn h(&mut self, a: usize) {
        self.each_row(a, |x, z, r| (z, x, r ^ (x & z)));
    }

    pub(crate) fn s(&mut self, a: usize) {
        self.each_row(a, |x, z, r| (x, z ^ x, r ^ (x & z)));
    }

    pub(crate) fn x(&mut self, a: usize) {
        self.each_row(a, |x, z, r| (x, z, r ^ z));
    }

    pub(crate) fn z(&mut self, a: usize) {
        self.each_row(a, |x, z, r| (x, z, r ^ x));
    }

    pub(crate) fn cx(&mut self, a: usize, b: usize) {
        let (ka, ma) = (a / 64, 1u64 << (a % 64));
        let (kb, mb) = (b / 64, 1u64 << (b % 64));
        for row in 0..2 * self.n {
            let base = row * self.w;
            let xa = self.x[base + ka] & ma != 0;
            let za = self.z[base + ka] & ma != 0;
            let xb = self.x[base + kb] & mb != 0;
            let zb = self.z[base + kb] & mb != 0;
            self.r[row] ^= xa & zb & !(xb ^ za);
            if xa {
                self.x[base + kb] ^= mb;
            }
            if zb {
                self.z[base + ka] ^= ma;
            }
        }
    }

    fn xbit(&self, row: usize, a: usize) -> bool {
        self.x[row * self.w + a / 64] & (1 << (a % 64)) != 0
    }

    // Row h := row i · row h, tracking the sign exactly (the paper's `rowsum` with function g).
    fn rowsum(&mut self, h: usize, i: usize) {
        let (bh, bi) = (h * self.w, i * self.w);
        let mut sum: i64 = 0;
        for k in 0..self.w {
            let (x1, z1) = (self.x[bi + k], self.z[bi + k]);
            let (x2, z2) = (self.x[bh + k], self.z[bh + k]);
            let plus = (x1 & z1 & z2 & !x2) | (x1 & !z1 & z2 & x2) | (!x1 & z1 & x2 & !z2);
            let minus = (x1 & z1 & x2 & !z2) | (x1 & !z1 & z2 & !x2) | (!x1 & z1 & x2 & z2);
            sum += plus.count_ones() as i64 - minus.count_ones() as i64;
            self.x[bh + k] ^= x1;
            self.z[bh + k] ^= z1;
        }
        let total = 2 * self.r[h] as i64 + 2 * self.r[i] as i64 + sum;
        self.r[h] = total.rem_euclid(4) == 2;
    }

    // Measures qubit `a` in the Z basis. `choose` supplies the outcome when it is random.
    // Returns (outcome, was_random).
    pub(crate) fn measure(&mut self, a: usize, choose: impl FnOnce() -> bool) -> (bool, bool) {
        let (n, w) = (self.n, self.w);
        if let Some(p) = (n..2 * n).find(|&p| self.xbit(p, a)) {
            for i in 0..2 * n {
                if i != p && self.xbit(i, a) {
                    self.rowsum(i, p);
                }
            }
            let (src, dst) = (p * w, (p - n) * w);
            self.x.copy_within(src..src + w, dst);
            self.z.copy_within(src..src + w, dst);
            self.r[p - n] = self.r[p];
            self.x[src..src + w].fill(0);
            self.z[src..src + w].fill(0);
            self.z[src + a / 64] |= 1 << (a % 64);
            let outcome = choose();
            self.r[p] = outcome;
            (outcome, true)
        } else {
            let s = 2 * n;
            self.x[s * w..(s + 1) * w].fill(0);
            self.z[s * w..(s + 1) * w].fill(0);
            self.r[s] = false;
            for i in 0..n {
                if self.xbit(i, a) {
                    self.rowsum(s, i + n);
                }
            }
            (self.r[s], false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Tableau;
    use crate::statevector;
    use crate::Circuit;

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

    // Exact probability of measuring all qubits as `outcome` (bit i = qubit i).
    fn tableau_probability(t: &Tableau, n: usize, outcome: usize) -> f64 {
        let mut t = t.clone();
        let mut p = 1.0;
        for q in 0..n {
            let want = outcome >> q & 1 == 1;
            let (got, random) = t.measure(q, || want);
            if random {
                p *= 0.5;
            } else if got != want {
                return 0.0;
            }
        }
        p
    }

    #[test]
    fn exact_distributions_match_state_vector_on_random_clifford_circuits() {
        let mut rng = Lcg(42);
        for case in 0..300 {
            let n = 1 + rng.below(10) as usize;
            let mut c = Circuit::new();
            let mut qs: Vec<_> = (0..n).map(|_| c.qubit()).collect();
            let mut t = Tableau::new(n);
            for _ in 0..rng.below(60) {
                let i = rng.below(n as u64) as usize;
                match rng.below(6) {
                    0 => {
                        c.h(&mut qs[i]);
                        t.h(i);
                    }
                    1 => {
                        c.s(&mut qs[i]);
                        t.s(i);
                    }
                    2 => {
                        c.x(&mut qs[i]);
                        t.x(i);
                    }
                    3 => {
                        c.z(&mut qs[i]);
                        t.z(i);
                    }
                    _ if n > 1 => {
                        let j = (i + 1 + rng.below(n as u64 - 1) as usize) % n;
                        let [a, b] = qs.get_disjoint_mut([i, j]).unwrap();
                        c.cx(a, b);
                        t.cx(i, j);
                    }
                    _ => {}
                }
            }
            let probs = statevector::probabilities(&c.ops, n);
            for (x, &p_sv) in probs.iter().enumerate() {
                let p_st = tableau_probability(&t, n, x);
                assert!(
                    (p_sv - p_st).abs() < 1e-9,
                    "case {case} (n={n}) outcome {x:b}: state vector {p_sv}, tableau {p_st}"
                );
            }
        }
    }
}
