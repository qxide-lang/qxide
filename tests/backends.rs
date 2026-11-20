use qxide::{Backend, Bit, Circuit, SimError};

fn stab(c: &Circuit, seed: u64) -> qxide::Shot {
    c.simulate_with(Backend::Stabilizer, seed).unwrap()
}

#[test]
fn stabilizer_bell_pair_is_correlated_and_fair() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);
    let mut ones = 0;
    for seed in 0..2000 {
        let s = stab(&c, seed);
        assert_eq!(s.get(ma), s.get(mb));
        ones += s.get(ma) as u32;
    }
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn stabilizer_same_seed_same_result() {
    let mut c = Circuit::new();
    let r = c.qreg(8);
    let mut r = r;
    for q in r.iter_mut() {
        c.h(q);
    }
    let bits = c.measure_reg(r);
    for seed in 0..20 {
        assert_eq!(stab(&c, seed).value(&bits), stab(&c, seed).value(&bits));
    }
}

#[test]
fn stabilizer_phase_gates_interfere() {
    // H S S H = X, H Z H = X: a wrong phase produces an even split
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.s(&mut a);
    c.s(&mut a);
    c.h(&mut a);
    c.h(&mut b);
    c.z(&mut b);
    c.h(&mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);
    for seed in 0..50 {
        let s = stab(&c, seed);
        assert!(s.get(ma) && s.get(mb));
    }
}

#[test]
fn stabilizer_runs_teleportation_with_branches() {
    let mut c = Circuit::new();
    let mut msg = c.qubit();
    let mut alice = c.qubit();
    let mut bob = c.qubit();
    c.h(&mut msg);
    c.s(&mut msg); // |+i⟩
    c.h(&mut alice);
    c.cx(&mut alice, &mut bob);
    c.cx(&mut msg, &mut alice);
    c.h(&mut msg);
    let m0 = c.measure(msg);
    let m1 = c.measure(alice);
    c.if_one(m1, |c| c.x(&mut bob));
    c.if_one(m0, |c| c.z(&mut bob));
    for _ in 0..3 {
        c.s(&mut bob); // S† = S³
    }
    c.h(&mut bob);
    let out = c.measure(bob);
    assert!((0..500).all(|seed| !stab(&c, seed).get(out)));
}

#[test]
fn stabilizer_reset_one_half_of_bell_pair() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.reset(&mut a);
    let ma = c.measure(a);
    let mb = c.measure(b);
    let mut ones = 0;
    for seed in 0..2000 {
        let s = stab(&c, seed);
        assert!(!s.get(ma));
        ones += s.get(mb) as u32;
    }
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn stabilizer_rejects_non_clifford_gates() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.t(&mut q);
    assert_eq!(
        c.simulate_with(Backend::Stabilizer, 0).unwrap_err(),
        SimError::NotClifford { gate: "t" }
    );

    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.rx(&mut q, std::f64::consts::PI); // reject rotation gates even at Clifford angles
    assert_eq!(
        c.simulate_with(Backend::Stabilizer, 0).unwrap_err(),
        SimError::NotClifford { gate: "rx" }
    );
}

#[test]
fn sdg_is_clifford_but_tdg_is_not() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    c.h(&mut a);
    c.s(&mut a);
    c.sdg(&mut a);
    c.sdg(&mut a);
    c.h(&mut a);
    assert!(c.is_clifford());
    let m = c.measure(a);
    // H S S† S† H = H S† H: a wrong phase produces a deterministic result
    let ones = (0..2000).filter(|&seed| stab(&c, seed).get(m)).count();
    assert!((800..1200).contains(&ones), "ones = {ones}");

    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.sdg(&mut q);
    c.sdg(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    assert!((0..50).all(|seed| stab(&c, seed).get(m)));

    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.tdg(&mut q);
    assert!(!c.is_clifford());
    assert_eq!(c.select_backend(), Backend::StateVector);
    assert_eq!(
        c.simulate_with(Backend::Stabilizer, 0).unwrap_err(),
        SimError::NotClifford { gate: "tdg" }
    );
}

#[test]
fn stabilizer_rejects_non_clifford_gate_inside_branch() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    let m = c.measure(a);
    c.if_one(m, |c| c.ry(&mut b, 0.3));
    // must not silently pass even for a seed where the branch is not taken
    for seed in 0..10 {
        assert_eq!(
            c.simulate_with(Backend::Stabilizer, seed).unwrap_err(),
            SimError::NotClifford { gate: "ry" }
        );
    }
}

#[test]
fn stabilizer_simulates_thousand_qubit_ghz() {
    let n = 1000;
    let mut c = Circuit::new();
    let mut r = c.qreg(n);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    let bits = c.measure_reg(r);
    let mut seen = [false; 2];
    for seed in 0..8 {
        let s = stab(&c, seed);
        let first = s.get(bits[0]);
        assert!(bits.iter().all(|&b| s.get(b) == first));
        seen[first as usize] = true;
    }
    assert_eq!(seen, [true, true]);
}

// ---- Compare both simulators: random Clifford circuits (with mid-circuit measurement, reset, and branching) ----

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

fn random_clifford_circuit(rng: &mut Lcg) -> (Circuit, Vec<Bit>) {
    let n = 1 + rng.below(10) as usize;
    let mut c = Circuit::new();
    let mut qs: Vec<_> = (0..n).map(|_| c.qubit()).collect();
    let mut bits = Vec::new();
    for _ in 0..(5 + rng.below(40)) {
        let i = rng.below(n as u64) as usize;
        match rng.below(10) {
            9 => c.sdg(&mut qs[i]),
            0 | 1 => c.h(&mut qs[i]),
            2 => c.s(&mut qs[i]),
            3 => c.x(&mut qs[i]),
            4 => c.z(&mut qs[i]),
            5 | 6 if n > 1 => {
                let j = (i + 1 + rng.below(n as u64 - 1) as usize) % n;
                let [a, b] = qs.get_disjoint_mut([i, j]).unwrap();
                c.cx(a, b);
            }
            7 => c.reset(&mut qs[i]),
            8 => {
                // Measure mid-circuit, reuse the freed slot with a fresh qubit, and branch on the result
                let m = c.measure(qs.swap_remove(i));
                qs.push(c.qubit());
                bits.push(m);
                let j = rng.below(n as u64) as usize;
                if rng.below(2) == 0 {
                    c.if_one(m, |c| c.x(&mut qs[j]));
                } else {
                    c.if_zero(m, |c| {
                        c.h(&mut qs[j]);
                        c.s(&mut qs[j]);
                    });
                }
            }
            _ => {}
        }
    }
    for q in qs {
        bits.push(c.measure(q));
    }
    (c, bits)
}

#[test]
fn random_clifford_circuits_agree_between_backends() {
    let mut rng = Lcg(0x5EED);
    let shots = 3000;
    for case in 0..60 {
        let (c, bits) = random_clifford_circuit(&mut rng);
        let k = bits.len();
        // Fraction of 1s per bit and XOR fraction of adjacent bits
        let stats = |backend: Backend| {
            let mut ones = vec![0u32; k];
            let mut xors = vec![0u32; k.saturating_sub(1)];
            for seed in 0..shots {
                let s = c.simulate_with(backend, seed).unwrap();
                let v: Vec<bool> = bits.iter().map(|&b| s.get(b)).collect();
                for i in 0..k {
                    ones[i] += v[i] as u32;
                }
                for i in 0..k.saturating_sub(1) {
                    xors[i] += (v[i] ^ v[i + 1]) as u32;
                }
            }
            (ones, xors)
        };
        let (sv_ones, sv_xors) = stats(Backend::StateVector);
        let (st_ones, st_xors) = stats(Backend::Stabilizer);
        let close = |a: u32, b: u32| {
            let (pa, pb) = (a as f64 / shots as f64, b as f64 / shots as f64);
            // Clifford circuit probabilities are only 0, 1/2, or 1, so deterministic outcomes must match exactly
            if pa == 0.0 || pa == 1.0 || pb == 0.0 || pb == 1.0 {
                a == b
            } else {
                (pa - pb).abs() < 0.07
            }
        };
        for i in 0..k {
            assert!(
                close(sv_ones[i], st_ones[i]),
                "case {case} bit {i}: {} vs {}",
                sv_ones[i],
                st_ones[i]
            );
        }
        for i in 0..k.saturating_sub(1) {
            assert!(
                close(sv_xors[i], st_xors[i]),
                "case {case} xor {i}: {} vs {}",
                sv_xors[i],
                st_xors[i]
            );
        }
    }
}

// ---- Auto-selection and memory limit ----

use qxide::{state_vector_bytes, DEFAULT_MEMORY_LIMIT};

#[test]
fn auto_picks_stabilizer_only_for_clifford_circuits() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    assert!(c.is_clifford());
    assert_eq!(c.select_backend(), Backend::Stabilizer);

    c.t(&mut b);
    assert!(!c.is_clifford());
    assert_eq!(c.select_backend(), Backend::StateVector);
}

#[test]
fn auto_sees_non_clifford_gates_inside_branches() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    let m = c.measure(a);
    c.if_zero(m, |c| c.rz(&mut b, 0.1));
    assert_eq!(c.select_backend(), Backend::StateVector);
}

#[test]
fn auto_runs_large_clifford_circuit_on_stabilizer() {
    let mut c = Circuit::new();
    let mut r = c.qreg(500);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    let bits = c.measure_reg(r);
    let s = c.simulate_with(Backend::Auto, 1).unwrap();
    let first = s.get(bits[0]);
    assert!(bits.iter().all(|&b| s.get(b) == first));
}

#[test]
fn auto_and_state_vector_agree_on_small_non_clifford_circuit() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.t(&mut q);
    let m = c.measure(q);
    for seed in 0..20 {
        assert_eq!(
            c.simulate_with(Backend::Auto, seed).unwrap().get(m),
            c.simulate_with(Backend::StateVector, seed).unwrap().get(m)
        );
    }
}

#[test]
fn state_vector_memory_is_sixteen_bytes_per_amplitude() {
    assert_eq!(state_vector_bytes(0), 16);
    assert_eq!(state_vector_bytes(20), 16 << 20);
    assert_eq!(state_vector_bytes(30), 16 << 30);
    assert_eq!(state_vector_bytes(123), 16 << 123);
    assert_eq!(state_vector_bytes(124), u128::MAX); // saturates on overflow
    assert_eq!(DEFAULT_MEMORY_LIMIT, 1 << 30);
}

fn big_non_clifford(n: usize) -> Circuit {
    let mut c = Circuit::new();
    let mut r = c.qreg(n);
    c.t(&mut r[0]);
    c.measure_reg(r);
    c
}

#[test]
fn too_many_qubits_for_state_vector_reports_required_memory() {
    let c = big_non_clifford(40);
    let err = c.simulate_with(Backend::Auto, 0).unwrap_err();
    assert_eq!(
        err,
        SimError::OutOfMemory {
            qubits: 40,
            required_bytes: 16 << 40,
            limit_bytes: DEFAULT_MEMORY_LIMIT as u128,
        }
    );
    let text = err.to_string();
    assert!(text.contains("16.0 TiB"), "{text}");
    assert!(text.contains("1.0 GiB"), "{text}");
}

#[test]
fn memory_limit_can_be_changed() {
    let mut c = big_non_clifford(10); // 16 KiB
    assert!(c.simulate_with(Backend::StateVector, 0).is_ok());
    c.set_memory_limit(8 << 10);
    assert!(matches!(
        c.simulate_with(Backend::StateVector, 0),
        Err(SimError::OutOfMemory { qubits: 10, .. })
    ));
}

#[test]
fn forcing_state_vector_on_large_clifford_circuit_still_checks_memory() {
    let mut c = Circuit::new();
    let r = c.qreg(40);
    c.measure_reg(r);
    assert!(c.simulate_with(Backend::Stabilizer, 0).is_ok());
    assert!(matches!(
        c.simulate_with(Backend::StateVector, 0),
        Err(SimError::OutOfMemory { qubits: 40, .. })
    ));
}

#[test]
#[should_panic(expected = "exceeding the limit (1.0 GiB)")]
fn simulate_panics_with_memory_error_instead_of_allocating() {
    big_non_clifford(40).simulate(0);
}

#[test]
fn ccx_is_not_clifford() {
    let mut c = Circuit::new();
    let mut r = c.qreg(3);
    let [a, b, t] = r.get_disjoint_mut([0, 1, 2]).unwrap();
    c.ccx(a, b, t);
    assert!(!c.is_clifford());
    assert_eq!(c.select_backend(), Backend::StateVector);
    assert_eq!(
        c.simulate_with(Backend::Stabilizer, 0).unwrap_err(),
        SimError::NotClifford { gate: "ccx" }
    );

    let mut c = Circuit::new();
    let mut flag = c.qubit();
    let mut r = c.qreg(3);
    c.h(&mut flag);
    let m = c.measure(flag);
    c.if_one(m, |c| {
        let [a, b, t] = r.get_disjoint_mut([0, 1, 2]).unwrap();
        c.ccx(a, b, t);
    });
    assert!(!c.is_clifford());
    assert_eq!(
        c.simulate_with(Backend::Stabilizer, 0).unwrap_err(),
        SimError::NotClifford { gate: "ccx" }
    );
}
