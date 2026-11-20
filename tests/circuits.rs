use qxide::Circuit;

#[test]
fn bell_pair_is_always_correlated_and_roughly_fair() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);

    let mut ones = 0;
    for seed in 0..2000 {
        let s = c.simulate(seed);
        assert_eq!(s.get(ma), s.get(mb));
        ones += s.get(ma) as u32;
    }
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn x_then_measure_is_always_one() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.x(&mut q);
    let m = c.measure(q);
    for seed in 0..50 {
        assert!(c.simulate(seed).get(m));
    }
}

#[test]
fn h_z_h_acts_like_x() {
    // HZH = X; due to interference, the outcome must always be 1 rather than probabilistic
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.z(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    for seed in 0..50 {
        assert!(c.simulate(seed).get(m));
    }
}

#[test]
fn same_seed_same_result() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    let m = c.measure(q);
    for seed in 0..20 {
        assert_eq!(c.simulate(seed).get(m), c.simulate(seed).get(m));
    }
}

#[test]
fn qasm_output() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.measure(a);
    c.measure(b);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[2] q;\nbit[2] c;\n\
         h q[0];\ncx q[0], q[1];\nc[0] = measure q[0];\nc[1] = measure q[1];\n"
    );
}

#[test]
#[should_panic(expected = "qubit from another circuit")]
fn qubit_from_another_circuit_is_rejected() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let mut q = c1.qubit();
    c2.h(&mut q);
}

// ---- Rotation gates and s, t ----

use std::f64::consts::{FRAC_PI_2, PI};

fn always_one(c: &Circuit, m: qxide::Bit) -> bool {
    (0..50).all(|seed| c.simulate(seed).get(m))
}

fn count_ones(c: &Circuit, m: qxide::Bit, shots: u64) -> u64 {
    (0..shots).map(|seed| c.simulate(seed).get(m) as u64).sum()
}

#[test]
fn rx_pi_acts_like_x() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.rx(&mut q, PI);
    let m = c.measure(q);
    assert!(always_one(&c, m));
}

#[test]
fn ry_half_pi_is_fair() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.ry(&mut q, FRAC_PI_2);
    let m = c.measure(q);
    let ones = count_ones(&c, m, 2000);
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn h_rz_pi_h_acts_like_x() {
    // rz only changes phase, so it must be sandwiched between H gates to be revealed by interference
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.rz(&mut q, PI);
    c.h(&mut q);
    let m = c.measure(q);
    assert!(always_one(&c, m));
}

#[test]
fn s_twice_is_z() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.s(&mut q);
    c.s(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    assert!(always_one(&c, m));
}

#[test]
fn t_four_times_is_z() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    for _ in 0..4 {
        c.t(&mut q);
    }
    c.h(&mut q);
    let m = c.measure(q);
    assert!(always_one(&c, m));
}

#[test]
fn t_twice_is_not_z() {
    // Control test to catch phase bugs: since T² = S, HSH must yield a 50/50 split
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.t(&mut q);
    c.t(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    let ones = count_ones(&c, m, 2000);
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn sdg_undoes_s_and_tdg_undoes_t() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.s(&mut a);
    c.sdg(&mut a);
    c.h(&mut a);
    c.h(&mut b);
    c.t(&mut b);
    c.tdg(&mut b);
    c.h(&mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);
    assert!((0..50).all(|seed| {
        let s = c.simulate(seed);
        !s.get(ma) && !s.get(mb)
    }));
}

#[test]
fn sdg_twice_is_z_and_tdg_four_times_is_z() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.sdg(&mut a);
    c.sdg(&mut a);
    c.h(&mut a);
    c.h(&mut b);
    for _ in 0..4 {
        c.tdg(&mut b);
    }
    c.h(&mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);
    assert!(always_one(&c, ma));
    assert!(always_one(&c, mb));
}

#[test]
fn sdg_has_opposite_phase_to_s() {
    // S·S† = I whereas S·S = Z: if sdg used the same matrix as s, this circuit would always yield 1
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.s(&mut q);
    c.sdg(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    assert_eq!(count_ones(&c, m, 50), 0);
}

#[test]
fn rotation_qasm_output() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.rx(&mut q, 0.5);
    c.ry(&mut q, -1.0);
    c.rz(&mut q, PI);
    c.s(&mut q);
    c.t(&mut q);
    c.sdg(&mut q);
    c.tdg(&mut q);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\n\
         rx(0.5) q[0];\nry(-1.0) q[0];\nrz(3.141592653589793) q[0];\ns q[0];\nt q[0];\n\
         sdg q[0];\ntdg q[0];\n"
    );
}

// ---- reset and qubit reuse ----

#[test]
fn reset_returns_qubit_to_zero() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.x(&mut q);
    c.reset(&mut q);
    let m = c.measure(q);
    assert!((0..50).all(|seed| !c.simulate(seed).get(m)));
}

#[test]
fn reset_one_half_of_bell_pair_leaves_partner_random() {
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
        let s = c.simulate(seed);
        assert!(!s.get(ma));
        ones += s.get(mb) as u32;
    }
    assert!((800..1200).contains(&ones), "ones = {ones}");
}

#[test]
fn measured_qubit_slot_is_reused_after_reset() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.x(&mut q);
    let first = c.measure(q);
    let fresh = c.qubit(); // reuses the slot freed by measurement
    let second = c.measure(fresh);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\nbit[2] c;\n\
         x q[0];\nc[0] = measure q[0];\nreset q[0];\nc[1] = measure q[0];\n"
    );
    for seed in 0..50 {
        let s = c.simulate(seed);
        assert!(s.get(first));
        assert!(!s.get(second));
    }
}

#[test]
fn reset_qasm_output() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.reset(&mut q);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\nreset q[0];\n"
    );
}

// ---- Branching on measurement results ----

#[test]
fn teleportation_moves_state_with_classical_corrections() {
    let theta = 1.0;
    let mut c = Circuit::new();
    let mut msg = c.qubit();
    let mut alice = c.qubit();
    let mut bob = c.qubit();
    c.ry(&mut msg, theta); // state to send
    c.h(&mut alice);
    c.cx(&mut alice, &mut bob); // share a Bell pair
    c.cx(&mut msg, &mut alice);
    c.h(&mut msg);
    let m0 = c.measure(msg);
    let m1 = c.measure(alice);
    c.if_one(m1, |c| c.x(&mut bob));
    c.if_one(m0, |c| c.z(&mut bob));
    c.ry(&mut bob, -theta); // Should always be |0⟩ after reversing
    let out = c.measure(bob);
    assert!((0..500).all(|seed| !c.simulate(seed).get(out)));
}

#[test]
fn branch_runs_only_on_matching_outcome() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut z = c.qubit();
    c.h(&mut a);
    let ma = c.measure(a);
    c.if_one(ma, |c| c.x(&mut b));
    c.if_zero(ma, |c| c.x(&mut z));
    let mb = c.measure(b);
    let mz = c.measure(z);
    let mut seen = [false; 2];
    for seed in 0..200 {
        let s = c.simulate(seed);
        assert_eq!(s.get(mb), s.get(ma));
        assert_eq!(s.get(mz), !s.get(ma));
        seen[s.get(ma) as usize] = true;
    }
    assert_eq!(seen, [true, true]);
}

#[test]
fn branch_qasm_output_nests() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut t = c.qubit();
    c.h(&mut a);
    c.h(&mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);
    c.if_one(ma, |c| {
        c.x(&mut t);
        c.if_zero(mb, |c| c.reset(&mut t));
    });
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[3] q;\nbit[2] c;\n\
         h q[0];\nh q[1];\nc[0] = measure q[0];\nc[1] = measure q[1];\n\
         if (c[0] == true) {\n  x q[2];\n  if (c[1] == false) {\n    reset q[2];\n  }\n}\n"
    );
}

#[test]
fn qubit_allocated_inside_branch_gets_fresh_slot() {
    // A reset inside a branch is conditional, so a slot freed by measurement must not be reused there
    let mut c = Circuit::new();
    let q = c.qubit();
    let m = c.measure(q);
    let mut slot = None;
    c.if_one(m, |c| slot = Some(c.qubit()));
    assert!(slot.is_some());
    let qasm = c.to_qasm();
    assert!(qasm.contains("qubit[2] q;"), "{qasm}");
    assert!(!qasm.contains("reset"), "{qasm}");
}

#[test]
#[should_panic(expected = "measurement inside a conditional branch")]
fn measuring_inside_branch_is_rejected() {
    let mut c = Circuit::new();
    let a = c.qubit();
    let b = c.qubit();
    let ma = c.measure(a);
    c.if_one(ma, |c| {
        c.measure(b);
    });
}

#[test]
#[should_panic(expected = "bit belongs to a different circuit")]
fn branch_on_bit_from_another_circuit_is_rejected() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let q = c1.qubit();
    let m = c1.measure(q);
    c2.if_one(m, |_| {});
}

// ---- Registers ----

use qxide::{BitReg, QReg};

#[test]
fn ghz_with_register_is_all_zero_or_all_one() {
    let mut c = Circuit::new();
    let mut r: QReg = c.qreg(3);
    assert_eq!(r.len(), 3);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    let bits: BitReg = c.measure_reg(r);
    let mut seen = [false; 2];
    for seed in 0..200 {
        let v = c.simulate(seed).value(&bits);
        assert!(v == 0 || v == 0b111, "v = {v}");
        seen[(v != 0) as usize] = true;
    }
    assert_eq!(seen, [true, true]);
}

#[test]
fn register_value_is_little_endian() {
    let mut c = Circuit::new();
    let mut r = c.qreg(3);
    c.x(&mut r[1]);
    let bits = c.measure_reg(r);
    let s = c.simulate(0);
    assert_eq!(s.value(&bits), 0b010);
    assert!(s.get(bits[1]));
    assert!(!s.get(bits[0]));
}

#[test]
fn gates_can_be_broadcast_over_a_register() {
    let mut c = Circuit::new();
    let mut r = c.qreg(4);
    for q in r.iter_mut() {
        c.x(q);
    }
    let [a, b] = r.get_disjoint_mut([0, 3]).unwrap();
    c.cx(a, b); // Qubit 0 is 1, so qubit 3 flips back to 0
    let bits = c.measure_reg(r);
    assert_eq!(c.simulate(0).value(&bits), 0b0111);
}

#[test]
fn register_qubits_can_be_moved_out_and_measured_one_by_one() {
    let mut c = Circuit::new();
    let r = c.qreg(2);
    let bits: Vec<_> = r.into_iter().map(|q| c.measure(q)).collect();
    assert_eq!(bits.len(), 2);
}

#[test]
fn measured_register_slots_are_reused() {
    let mut c = Circuit::new();
    let r = c.qreg(2);
    c.measure_reg(r);
    let r = c.qreg(2);
    c.measure_reg(r);
    assert!(c.to_qasm().contains("qubit[2] q;\nbit[4] c;"));
}

#[test]
#[should_panic(expected = "64 bits")]
fn register_value_wider_than_64_bits_is_rejected() {
    let mut c = Circuit::new();
    let mut bits = Vec::new();
    for _ in 0..65 {
        let q = c.qubit();
        bits.push(c.measure(q));
    }
    let reg = BitReg::from(bits);
    c.simulate(0).value(&reg);
}

// ---- ccx (Toffoli) ----

const ROLE_ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

// Runs ccx with qubit roles (c1, c2, target) = slots `order`, on basis input `bits`.
fn ccx_truth(order: [usize; 3], bits: [bool; 3], shared_controls: bool) -> [bool; 3] {
    let mut c = Circuit::new();
    let mut r = c.qreg(3);
    for (q, &b) in r.iter_mut().zip(&bits) {
        if b {
            c.x(q);
        }
    }
    let [c1, c2, t] = r.get_disjoint_mut(order).unwrap();
    if shared_controls {
        c.ccx_ctrl(c1, c2, t);
    } else {
        c.ccx(c1, c2, t);
    }
    let m = c.measure_reg(r);
    let s = c.simulate(0);
    [s.get(m[0]), s.get(m[1]), s.get(m[2])]
}

#[test]
fn ccx_flips_target_only_when_both_controls_are_one() {
    for order in ROLE_ORDERS {
        for input in 0..8u8 {
            let bits = [input & 1 != 0, input & 2 != 0, input & 4 != 0];
            let mut expected = bits;
            expected[order[2]] ^= bits[order[0]] && bits[order[1]];
            for shared in [false, true] {
                assert_eq!(
                    ccx_truth(order, bits, shared),
                    expected,
                    "order {order:?} input {bits:?} shared {shared}"
                );
            }
        }
    }
}

#[test]
fn ccx_keeps_superposition_coherent() {
    // H on target, Toffoli with both controls 1 acts as X, and X|+> = |+>: H brings it back to 0.
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut t = c.qubit();
    c.x(&mut a);
    c.x(&mut b);
    c.h(&mut t);
    c.ccx(&mut a, &mut b, &mut t);
    c.h(&mut t);
    let m = c.measure(t);
    assert_eq!(count_ones(&c, m, 50), 0);
}

#[test]
fn ccx_qasm_output() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut t = c.qubit();
    c.ccx(&mut t, &mut a, &mut b);
    c.ccx_ctrl(&a, &b, &mut t);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[3] q;\n\
         ccx q[2], q[0], q[1];\nccx q[0], q[1], q[2];\n"
    );
}

#[test]
#[should_panic(expected = "ccx")]
fn ccx_ctrl_rejects_the_same_control_twice() {
    let mut c = Circuit::new();
    let a = c.qubit();
    let mut t = c.qubit();
    c.ccx_ctrl(&a, &a, &mut t);
}
