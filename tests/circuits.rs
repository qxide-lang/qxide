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
    // HZH = X, 간섭 때문에 확률이 아니라 항상 1이 나와야 함
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
#[should_panic(expected = "다른 회로의 큐비트")]
fn qubit_from_another_circuit_is_rejected() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let mut q = c1.qubit();
    c2.h(&mut q);
}

// ---- 회전 게이트와 s, t ----

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
    // 위상만 바꾸는 rz는 H 사이에 끼워야 간섭으로 드러난다
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
    // 위상 오류를 잡기 위한 대조군: T² = S이므로 HSH 결과는 반반이어야 한다
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
fn rotation_qasm_output() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.rx(&mut q, 0.5);
    c.ry(&mut q, -1.0);
    c.rz(&mut q, PI);
    c.s(&mut q);
    c.t(&mut q);
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\n\
         rx(0.5) q[0];\nry(-1.0) q[0];\nrz(3.141592653589793) q[0];\ns q[0];\nt q[0];\n"
    );
}

// ---- reset과 큐비트 재사용 ----

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
    let fresh = c.qubit(); // 측정으로 비워진 자리를 다시 씀
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

// ---- 측정 결과에 따른 분기 ----

#[test]
fn teleportation_moves_state_with_classical_corrections() {
    let theta = 1.0;
    let mut c = Circuit::new();
    let mut msg = c.qubit();
    let mut alice = c.qubit();
    let mut bob = c.qubit();
    c.ry(&mut msg, theta); // 보낼 상태
    c.h(&mut alice);
    c.cx(&mut alice, &mut bob); // 벨 쌍 공유
    c.cx(&mut msg, &mut alice);
    c.h(&mut msg);
    let m0 = c.measure(msg);
    let m1 = c.measure(alice);
    c.if_one(m1, |c| c.x(&mut bob));
    c.if_one(m0, |c| c.z(&mut bob));
    c.ry(&mut bob, -theta); // 되돌리면 항상 |0⟩이어야 함
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
         if (c[0] == 1) {\n  x q[2];\n  if (c[1] == 0) {\n    reset q[2];\n  }\n}\n"
    );
}

#[test]
fn qubit_allocated_inside_branch_gets_fresh_slot() {
    // 분기 안의 reset은 조건부라서, 측정으로 비운 자리를 거기서 재사용하면 안 된다
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
#[should_panic(expected = "분기 내부 측정")]
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
#[should_panic(expected = "다른 회로의 비트")]
fn branch_on_bit_from_another_circuit_is_rejected() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let q = c1.qubit();
    let m = c1.measure(q);
    c2.if_one(m, |_| {});
}

// ---- 레지스터 ----

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
    c.cx(a, b); // 0번이 1이므로 3번이 다시 0이 됨
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
#[should_panic(expected = "64비트")]
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
