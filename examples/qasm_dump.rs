// Writes the OpenQASM 3 of several sample circuits to `<dir>/<name>.qasm` (default dir: `qasm-out`).
// CI parses these with `qiskit.qasm3.loads` (see `ci/check_qasm.py`).
//
// `cargo run --example qasm_dump -- qasm-out`

use qxide::Circuit;
use std::f64::consts::PI;

fn bell() -> Circuit {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    c.measure(a);
    c.measure(b);
    c
}

fn gates() -> Circuit {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c.x(&mut q);
    c.z(&mut q);
    c.s(&mut q);
    c.t(&mut q);
    c.sdg(&mut q);
    c.tdg(&mut q);
    c.rx(&mut q, 0.5);
    c.ry(&mut q, -1.0);
    c.rz(&mut q, PI);
    c.rx(&mut q, 1e-20);
    c.measure(q);
    c
}

fn teleport() -> Circuit {
    let mut c = Circuit::new();
    let mut msg = c.qubit();
    let mut alice = c.qubit();
    let mut bob = c.qubit();
    c.ry(&mut msg, 1.0);
    c.h(&mut alice);
    c.cx(&mut alice, &mut bob);
    c.cx(&mut msg, &mut alice);
    c.h(&mut msg);
    let m0 = c.measure(msg);
    let m1 = c.measure(alice);
    c.if_one(m1, |c| c.x(&mut bob));
    c.if_one(m0, |c| c.z(&mut bob));
    c.measure(bob);
    c
}

fn nested_branches() -> Circuit {
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
    c.measure(t);
    c
}

fn reset_and_reuse() -> Circuit {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.x(&mut q);
    c.reset(&mut q);
    c.measure(q);
    let mut again = c.qubit(); // reuses the measured slot after an implicit reset
    c.h(&mut again);
    c.measure(again);
    c
}

fn ghz_register() -> Circuit {
    let mut c = Circuit::new();
    let mut r = c.qreg(5);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    c.measure_reg(r);
    c
}

fn no_measurement() -> Circuit {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    c
}

fn ancilla_parity() -> Circuit {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut target = c.qubit();
    c.h(&mut a);
    c.h(&mut b);
    c.with_ancillas(
        |u, [x, y]: &mut [_; 2]| {
            u.cx(&a, x);
            u.cx(&b, x);
            u.x(y);
        },
        |c, [x, y]| {
            c.cx_ctrl(x, &mut target);
            c.cx_ctrl(y, &mut target);
        },
    );
    c.measure(a);
    c.measure(b);
    c.measure(target);
    c
}

fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "qasm-out".into());
    std::fs::create_dir_all(&dir)?;
    type Sample = (&'static str, fn() -> Circuit);
    let circuits: [Sample; 8] = [
        ("bell", bell),
        ("gates", gates),
        ("teleport", teleport),
        ("nested_branches", nested_branches),
        ("reset_and_reuse", reset_and_reuse),
        ("ghz_register", ghz_register),
        ("no_measurement", no_measurement),
        ("ancilla_parity", ancilla_parity),
    ];
    for (name, make) in circuits {
        let path = std::path::Path::new(&dir).join(format!("{name}.qasm"));
        std::fs::write(&path, make().to_qasm())?;
        println!("{}", path.display());
    }
    Ok(())
}
