// Conservative entanglement groups: union-find over nodes, with each physical slot
// pointing at its current node. Union-find cannot delete, so detaching a slot
// (measure/reset) just moves it to a fresh singleton node; the old node keeps
// linking the remaining members together.

#[derive(Debug, Default)]
pub(crate) struct Groups {
    parent: Vec<usize>,
    node: Vec<usize>,
}

impl Groups {
    fn new_node(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }

    fn root(&self, mut x: usize) -> usize {
        while self.parent[x] != x {
            x = self.parent[x];
        }
        x
    }

    // Registers a newly allocated slot (slots are allocated densely from 0).
    pub(crate) fn add_slot(&mut self) {
        let n = self.new_node();
        self.node.push(n);
    }

    pub(crate) fn detach(&mut self, slot: usize) {
        self.node[slot] = self.new_node();
    }

    pub(crate) fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.root(self.node[a]), self.root(self.node[b]));
        // Link the younger root under the older one, then compress both paths.
        let keep = ra.min(rb);
        self.parent[ra.max(rb)] = keep;
        for mut x in [self.node[a], self.node[b]] {
            while self.parent[x] != x {
                let next = self.parent[x];
                self.parent[x] = keep;
                x = next;
            }
        }
    }

    pub(crate) fn same(&self, a: usize, b: usize) -> bool {
        self.root(self.node[a]) == self.root(self.node[b])
    }
}

#[cfg(test)]
mod tests {
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

    // Reduced density matrix of `qs` (bit k of the index is qubit qs[k]); entries as (re, im).
    fn reduced(psi: &[statevector::C], qs: &[usize]) -> Vec<Vec<(f64, f64)>> {
        let d = 1 << qs.len();
        let mask: usize = qs.iter().map(|&q| 1 << q).sum();
        let place = |base: usize, v: usize| {
            qs.iter()
                .enumerate()
                .fold(base, |acc, (k, &q)| acc | (((v >> k) & 1) << q))
        };
        let mut rho = vec![vec![(0.0, 0.0); d]; d];
        for base in (0..psi.len()).filter(|i| i & mask == 0) {
            for (a, row) in rho.iter_mut().enumerate() {
                for (b, cell) in row.iter_mut().enumerate() {
                    let (x, y) = (psi[place(base, a)], psi[place(base, b)]);
                    cell.0 += x.re * y.re + x.im * y.im;
                    cell.1 += x.im * y.re - x.re * y.im;
                }
            }
        }
        rho
    }

    // Soundness of the over-approximation: qubits reported as not entangled must be in a
    // product state, i.e. rho_ij == rho_i (x) rho_j.
    #[test]
    fn unentangled_pairs_are_product_states() {
        let mut rng = Lcg(0xE17A);
        for case in 0..200 {
            let n = 2 + rng.below(5) as usize;
            let mut c = Circuit::new();
            let mut qs: Vec<_> = (0..n).map(|_| c.qubit()).collect();
            for _ in 0..rng.below(25) {
                let i = rng.below(n as u64) as usize;
                match rng.below(6) {
                    0 => c.h(&mut qs[i]),
                    1 => c.t(&mut qs[i]),
                    2 => c.rx(&mut qs[i], 0.1 + rng.below(30) as f64 / 10.0),
                    3 => c.ry(&mut qs[i], 0.1 + rng.below(30) as f64 / 10.0),
                    _ => {
                        let j = (i + 1 + rng.below(n as u64 - 1) as usize) % n;
                        let [a, b] = qs.get_disjoint_mut([i, j]).unwrap();
                        c.cx(a, b);
                    }
                }
            }
            let psi = statevector::amplitudes(&c.ops, c.n_qubits);
            for i in 0..n {
                for j in i + 1..n {
                    if c.may_be_entangled(&qs[i], &qs[j]) {
                        continue;
                    }
                    let (si, sj) = (qs[i].index, qs[j].index);
                    let (pij, pi, pj) = (
                        reduced(&psi, &[si, sj]),
                        reduced(&psi, &[si]),
                        reduced(&psi, &[sj]),
                    );
                    for a in 0..4 {
                        for b in 0..4 {
                            let (x, y) = (pi[a & 1][b & 1], pj[a >> 1][b >> 1]);
                            let prod = (x.0 * y.0 - x.1 * y.1, x.0 * y.1 + x.1 * y.0);
                            let got = pij[a][b];
                            assert!(
                                (got.0 - prod.0).abs() < 1e-9 && (got.1 - prod.1).abs() < 1e-9,
                                "case {case}: qubits {i},{j} reported unentangled but correlated"
                            );
                        }
                    }
                }
            }
        }
    }
}
