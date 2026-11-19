#![cfg(feature = "gpu")]

use qxide::{Backend, Circuit, SimError};

#[test]
fn gpu_interference_is_exact_enough_to_be_deterministic() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    for _ in 0..4 {
        c.t(&mut q);
    }
    c.h(&mut q);
    let m = c.measure(q);
    for seed in 0..50 {
        assert!(c.simulate_with(Backend::Gpu, seed).unwrap().get(m));
    }
}

#[test]
fn gpu_inverse_phase_gates_cancel() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.s(&mut q);
    c.t(&mut q);
    c.tdg(&mut q);
    c.sdg(&mut q);
    c.h(&mut q);
    let m = c.measure(q);
    for seed in 0..50 {
        assert!(!c.simulate_with(Backend::Gpu, seed).unwrap().get(m));
    }
}

#[test]
fn gpu_teleportation_with_branches_matches_cpu() {
    let theta = 1.0;
    let mut c = Circuit::new();
    let mut msg = c.qubit();
    let mut alice = c.qubit();
    let mut bob = c.qubit();
    c.ry(&mut msg, theta);
    c.h(&mut alice);
    c.cx(&mut alice, &mut bob);
    c.cx(&mut msg, &mut alice);
    c.h(&mut msg);
    let m0 = c.measure(msg);
    let m1 = c.measure(alice);
    c.if_one(m1, |c| c.x(&mut bob));
    c.if_one(m0, |c| c.z(&mut bob));
    c.ry(&mut bob, -theta);
    let out = c.measure(bob);
    let mut differ = 0;
    for seed in 0..300 {
        let g = c.simulate_with(Backend::Gpu, seed).unwrap();
        let s = c.simulate_with(Backend::StateVector, seed).unwrap();
        assert!(!g.get(out));
        differ += (g.get(m0) != s.get(m0)) as u32 + (g.get(m1) != s.get(m1)) as u32;
    }
    // Same seed, same random stream: only f32 rounding right at a sampling threshold can differ.
    assert!(differ <= 2, "differ = {differ}");
}

#[test]
fn gpu_respects_memory_limit() {
    let mut c = Circuit::new();
    let r = c.qreg(12);
    c.measure_reg(r);
    c.set_memory_limit(1 << 10);
    assert!(matches!(
        c.simulate_with(Backend::Gpu, 0),
        Err(SimError::OutOfMemory { qubits: 12, .. })
    ));
}

#[test]
fn gpu_ccx_truth_table_matches_cpu() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        for input in 0..8u8 {
            let mut c = Circuit::new();
            let mut r = c.qreg(3);
            for (k, q) in r.iter_mut().enumerate() {
                if input >> k & 1 == 1 {
                    c.x(q);
                }
            }
            let [c1, c2, t] = r.get_disjoint_mut(order).unwrap();
            c.ccx(c1, c2, t);
            let m = c.measure_reg(r);
            let gpu = c.simulate_with(Backend::Gpu, 0).unwrap().value(&m);
            let cpu = c.simulate(0).value(&m);
            assert_eq!(gpu, cpu, "order {order:?} input {input}");
        }
    }
}
