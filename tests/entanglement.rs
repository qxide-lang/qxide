use qxide::Circuit;

#[test]
fn fresh_qubits_and_single_qubit_gates_do_not_entangle() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.rx(&mut b, 0.3);
    c.t(&mut a);
    assert!(!c.may_be_entangled(&a, &b));
}

#[test]
fn a_qubit_is_trivially_grouped_with_itself() {
    let mut c = Circuit::new();
    let a = c.qubit();
    assert!(c.may_be_entangled(&a, &a));
}

#[test]
fn cx_groups_are_transitive_and_symmetric() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut d = c.qubit();
    let e = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.cx(&mut b, &mut d);
    assert!(c.may_be_entangled(&a, &b));
    assert!(c.may_be_entangled(&b, &a));
    assert!(c.may_be_entangled(&a, &d));
    assert!(!c.may_be_entangled(&a, &e));
    assert!(!c.may_be_entangled(&d, &e));
}

#[test]
fn measuring_one_member_keeps_the_rest_grouped() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut d = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.cx(&mut a, &mut d);
    c.measure(a);
    assert!(c.may_be_entangled(&b, &d));
}

#[test]
fn measured_slot_reused_by_new_qubit_starts_unentangled() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.measure(a);
    let fresh = c.qubit(); // reuses a's physical slot
    assert!(c.to_qasm().contains("reset q[0];"));
    assert!(!c.may_be_entangled(&fresh, &b));
}

#[test]
fn top_level_reset_detaches_the_qubit() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut d = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.cx(&mut a, &mut d);
    c.reset(&mut a);
    assert!(!c.may_be_entangled(&a, &b));
    assert!(c.may_be_entangled(&b, &d));
}

#[test]
fn reset_inside_branch_is_conditional_and_does_not_detach() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut flag = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.h(&mut flag);
    let m = c.measure(flag);
    c.if_one(m, |c| c.reset(&mut a));
    assert!(c.may_be_entangled(&a, &b));
}

#[test]
fn cx_inside_branch_is_counted_conservatively() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut flag = c.qubit();
    c.h(&mut flag);
    let m = c.measure(flag);
    c.if_zero(m, |c| c.cx(&mut a, &mut b));
    assert!(c.may_be_entangled(&a, &b));
}

#[test]
fn register_qubits_are_tracked_individually() {
    let mut c = Circuit::new();
    let mut r = c.qreg(3);
    let [x, y] = r.get_disjoint_mut([0, 1]).unwrap();
    c.cx(x, y);
    assert!(c.may_be_entangled(&r[0], &r[1]));
    assert!(!c.may_be_entangled(&r[0], &r[2]));
}

#[test]
#[should_panic]
fn qubit_from_another_circuit_is_rejected() {
    let mut c1 = Circuit::new();
    let mut c2 = Circuit::new();
    let a = c1.qubit();
    let b = c2.qubit();
    c1.may_be_entangled(&a, &b);
}

#[test]
fn ccx_groups_all_three_qubits() {
    let mut c = Circuit::new();
    let mut r = c.qreg(4);
    let [a, b, t] = r.get_disjoint_mut([0, 1, 2]).unwrap();
    c.ccx(a, b, t);
    assert!(c.may_be_entangled(&r[0], &r[1]));
    assert!(c.may_be_entangled(&r[1], &r[2]));
    assert!(!c.may_be_entangled(&r[0], &r[3]));
}
