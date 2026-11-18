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
