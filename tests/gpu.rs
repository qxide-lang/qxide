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
