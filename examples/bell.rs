//! Bell state (entanglement) example: `cargo run --example bell`

use qxide::{Backend, Circuit};

fn main() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();

    c.h(&mut a); // put a into a superposition of 0 and 1
    c.cx(&mut a, &mut b); // entangle a and b

    let ma = c.measure(a); // a and b are consumed here, leaving only classical bits
    let mb = c.measure(b);

    println!("--- OpenQASM 3 (format that can be sent to a real quantum computer) ---");
    print!("{}", c.to_qasm());

    println!("\n--- Running 1000 times on the simulator ---");
    let mut counts = [0u32; 4];
    for shot in c.simulate_shots(Backend::StateVector, 0, 1000) {
        let shot = shot.unwrap();
        let k = (shot.get(ma) as usize) << 1 | shot.get(mb) as usize;
        counts[k] += 1;
    }
    for (k, n) in counts.iter().enumerate() {
        println!("{}{}: {n}", k >> 1, k & 1);
    }
    println!("(Because the qubits are entangled, 01 and 10 never appear)");
}
