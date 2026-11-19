use qxide::{Backend, Circuit};

// Parity of a and b computed into an ancilla, used to flip `target`, then uncomputed.
fn parity_oracle(a_val: bool, b_val: bool) -> (Circuit, qxide::Bit, qxide::Bit, qxide::Bit) {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut target = c.qubit();
    if a_val {
        c.x(&mut a);
    }
    if b_val {
        c.x(&mut b);
    }
    c.with_ancilla(
        |u, anc| {
            u.cx(&a, anc);
            u.cx(&b, anc);
        },
        |c, anc| c.cx_ctrl(anc, &mut target),
    );
    let (ma, mb, mt) = (c.measure(a), c.measure(b), c.measure(target));
    (c, ma, mb, mt)
}

#[test]
fn parity_oracle_flips_target_and_keeps_inputs() {
    for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
        let (c, ma, mb, mt) = parity_oracle(a, b);
        for backend in [Backend::StateVector, Backend::Stabilizer] {
            for seed in 0..20 {
                let s = c.simulate_with(backend, seed).unwrap();
                assert_eq!((s.get(ma), s.get(mb), s.get(mt)), (a, b, a ^ b));
            }
        }
    }
}

#[test]
fn uncompute_is_emitted_in_reverse_order() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut target = c.qubit();
    c.h(&mut a);
    c.h(&mut b);
    c.with_ancilla(
        |u, anc| {
            u.cx(&a, anc);
            u.x(anc);
            u.cx(&b, anc);
        },
        |c, anc| c.cx_ctrl(anc, &mut target),
    );
    assert_eq!(
        c.to_qasm(),
        "OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[4] q;\n\
         h q[0];\nh q[1];\n\
         cx q[0], q[3];\nx q[3];\ncx q[1], q[3];\n\
         cx q[3], q[2];\n\
         cx q[1], q[3];\nx q[3];\ncx q[0], q[3];\n"
    );
}

#[test]
fn ancilla_returns_to_zero_and_its_slot_is_reused() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut target = c.qubit();
    c.h(&mut a);
    c.with_ancilla(|u, anc| u.cx(&a, anc), |c, anc| c.cx_ctrl(anc, &mut target));
    // The freed ancilla slot (q[2]) comes back as a fresh qubit; measured right away it is 0.
    let reused = c.qubit();
    assert!(c.to_qasm().contains("reset q[2];"));
    assert!(!c.may_be_entangled(&reused, &a));
    // a and target end up entangled through the ancilla (a Bell pair).
    assert!(c.may_be_entangled(&a, &target));
    let (ma, mt, mr) = (c.measure(a), c.measure(target), c.measure(reused));
    for seed in 0..200 {
        let s = c.simulate(seed);
        assert_eq!(s.get(ma), s.get(mt));
        assert!(!s.get(mr));
    }
}

#[test]
fn several_ancillas_compute_and_uncompute_together() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let b = c.qubit();
    let mut t0 = c.qubit();
    let mut t1 = c.qubit();
    c.x(&mut a);
    c.with_ancillas(
        |u, [x, y]: &mut [_; 2]| {
            u.cx(&a, x);
            u.cx(&b, y);
            u.cx(x, y);
        },
        |c, [x, y]| {
            c.cx_ctrl(x, &mut t0);
            c.cx_ctrl(y, &mut t1);
        },
    );
    let anc = c.qreg(2);
    let (mt0, mt1) = (c.measure(t0), c.measure(t1));
    let ma = c.measure_reg(anc);
    for seed in 0..20 {
        let s = c.simulate(seed);
        // x = a = 1, y = b ^ x = 1
        assert!(s.get(mt0) && s.get(mt1));
        assert_eq!(s.value(&ma), 0);
    }
}

#[test]
fn apply_returns_its_value() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut t = c.qubit();
    c.x(&mut a);
    let n = c.with_ancilla(
        |u, anc| u.cx(&a, anc),
        |c, anc| {
            c.cx_ctrl(anc, &mut t);
            42
        },
    );
    assert_eq!(n, 42);
}

#[test]
fn with_ancilla_works_inside_a_branch() {
    let mut c = Circuit::new();
    let mut flag = c.qubit();
    let mut a = c.qubit();
    let mut t = c.qubit();
    c.x(&mut flag);
    c.x(&mut a);
    let m = c.measure(flag);
    c.if_one(m, |c| {
        c.with_ancilla(|u, anc| u.cx(&a, anc), |c, anc| c.cx_ctrl(anc, &mut t));
    });
    let mt = c.measure(t);
    assert!((0..20).all(|seed| c.simulate(seed).get(mt)));
}

#[test]
#[should_panic]
fn ancilla_compute_rejects_qubit_from_another_circuit() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let a = c2.qubit();
    c1.with_ancilla(|u, anc| u.cx(&a, anc), |_, _| ());
}

// ---- runtime guard: qubits the compute step touched are control-only during apply ----
// Interior mutability bypasses the borrow checker, so these use RefCell on purpose.

use std::cell::RefCell;

// Builds a circuit where `a` (behind a RefCell) is read by compute, then runs `misuse` in apply.
fn misuse_in_apply(misuse: impl FnOnce(&mut Circuit, &RefCell<Option<qxide::Qubit>>)) -> Circuit {
    let mut c = Circuit::new();
    let a = RefCell::new(Some(c.qubit()));
    c.h(a.borrow_mut().as_mut().unwrap());
    c.with_ancilla(
        |u, anc| u.cx(a.borrow().as_ref().unwrap(), anc),
        |c, _| misuse(c, &a),
    );
    c
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_h_on_compute_input() {
    misuse_in_apply(|c, a| c.h(a.borrow_mut().as_mut().unwrap()));
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_x_on_compute_input() {
    misuse_in_apply(|c, a| c.x(a.borrow_mut().as_mut().unwrap()));
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_rotation_on_compute_input() {
    misuse_in_apply(|c, a| c.ry(a.borrow_mut().as_mut().unwrap(), 0.3));
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_compute_input_as_cx_target() {
    misuse_in_apply(|c, a| {
        let mut other = c.qubit();
        c.cx(&mut other, a.borrow_mut().as_mut().unwrap());
    });
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_compute_input_as_ccx_target() {
    misuse_in_apply(|c, a| {
        let mut o1 = c.qubit();
        let mut o2 = c.qubit();
        c.ccx(&mut o1, &mut o2, a.borrow_mut().as_mut().unwrap());
    });
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_measuring_compute_input() {
    misuse_in_apply(|c, a| {
        let q = a.borrow_mut().take().unwrap();
        c.measure(q);
    });
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_reset_of_compute_input() {
    misuse_in_apply(|c, a| c.reset(a.borrow_mut().as_mut().unwrap()));
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_misuse_inside_a_branch_of_apply() {
    misuse_in_apply(|c, a| {
        let mut f = c.qubit();
        c.h(&mut f);
        let m = c.measure(f);
        c.if_one(m, |c| c.h(a.borrow_mut().as_mut().unwrap()));
    });
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_rejects_inner_compute_writing_outer_input() {
    misuse_in_apply(|c, a| {
        c.with_ancilla(|u, _| u.x(a.borrow_mut().as_mut().unwrap()), |_, _| ());
    });
}

#[test]
#[should_panic(expected = "with_ancilla")]
fn guard_keeps_outer_lock_after_inner_scope_sharing_the_input() {
    misuse_in_apply(|c, a| {
        let mut t = c.qubit();
        c.with_ancilla(
            |u, anc| u.cx(a.borrow().as_ref().unwrap(), anc),
            |c, anc| c.cx_ctrl(anc, &mut t),
        );
        // The inner scope released its lock, but the outer scope still holds one.
        c.h(a.borrow_mut().as_mut().unwrap());
    });
}

#[test]
fn guard_allows_control_and_diagonal_use_of_compute_input() {
    misuse_in_apply(|c, a| {
        let mut t = c.qubit();
        let mut u = c.qubit();
        let mut q = a.borrow_mut();
        let q = q.as_mut().unwrap();
        c.z(q);
        c.s(q);
        c.sdg(q);
        c.t(q);
        c.tdg(q);
        c.rz(q, 0.7);
        c.cx(q, &mut t);
        c.cx_ctrl(q, &mut t);
        c.ccx(q, &mut u, &mut t);
        c.ccx_ctrl(q, &u, &mut t);
    });
}

#[test]
fn guard_allows_nested_scope_reading_outer_ancilla() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut t = c.qubit();
    c.x(&mut a);
    c.with_ancilla(
        |u, anc| u.cx(&a, anc),
        |c, outer| {
            c.with_ancilla(
                |u, inner| u.cx(outer, inner),
                |c, inner| c.cx_ctrl(inner, &mut t),
            );
        },
    );
    let mt = c.measure(t);
    assert!((0..20).all(|seed| c.simulate(seed).get(mt)));
}

#[test]
fn guard_releases_the_lock_when_the_scope_ends() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut t = c.qubit();
    c.with_ancilla(|u, anc| u.cx(&a, anc), |c, anc| c.cx_ctrl(anc, &mut t));
    c.h(&mut a);
    c.measure(a);
}
