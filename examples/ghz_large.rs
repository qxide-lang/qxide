//! `cargo run --release --example ghz_large [qubits]`

use qxide::{Backend, Circuit};
use std::time::Instant;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5000);

    let mut c = Circuit::new();
    let mut r = c.qreg(n);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    let bits = c.measure_reg(r);

    for seed in 0..3 {
        let start = Instant::now();
        let shot = c.simulate_with(Backend::Stabilizer, seed).unwrap();
        let elapsed = start.elapsed();
        let ones = bits.iter().filter(|&&b| shot.get(b)).count();
        println!("시드 {seed} | {n}큐비트, 측정값 1: {ones}개 ({elapsed:?})");
    }
}
