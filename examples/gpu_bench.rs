// `cargo run --release --features gpu --example gpu_bench [max_qubits]`
// Prints CSV: mode,qubits,gates,cpu_ms,gpu_ms,speedup (end-to-end `simulate_with`, incl. GPU upload/readback).
// mode `measured` ends by measuring every qubit (done on the CPU in both backends); `gates` has no measurement.

use qxide::{Backend, Circuit};
use std::time::{Duration, Instant};

// Two layers of (h on all, rz on all, cx chain), optionally followed by measuring everything.
fn circuit(n: usize, measure: bool) -> (Circuit, usize) {
    let mut c = Circuit::new();
    let mut r = c.qreg(n);
    let mut gates = 0;
    for _ in 0..2 {
        for (i, q) in r.iter_mut().enumerate() {
            c.h(q);
            c.rz(q, 0.1 * (i + 1) as f64);
            gates += 2;
        }
        for i in 0..n - 1 {
            let [a, b] = r.get_disjoint_mut([i, i + 1]).unwrap();
            c.cx(a, b);
            gates += 1;
        }
    }
    if measure {
        c.measure_reg(r);
    }
    c.set_memory_limit(u64::MAX);
    (c, gates)
}

fn median_time(runs: usize, mut f: impl FnMut()) -> Duration {
    let mut t: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    t.sort();
    t[runs / 2]
}

fn main() {
    let max: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(26);
    // Warm-up: device init and kernel compilation are excluded from the timings.
    let (warm, _) = circuit(4, true);
    warm.simulate_with(Backend::Gpu, 0).unwrap();

    println!("mode,qubits,gates,cpu_ms,gpu_ms,speedup");
    for (mode, n) in ["gates", "measured"]
        .into_iter()
        .flat_map(|m| (10..=max).step_by(2).map(move |n| (m, n)))
    {
        let (c, gates) = circuit(n, mode == "measured");
        let runs = if n <= 20 { 5 } else { 3 };
        let cpu = median_time(runs, || {
            c.simulate_with(Backend::StateVector, 1).unwrap();
        });
        let gpu = median_time(runs, || {
            c.simulate_with(Backend::Gpu, 1).unwrap();
        });
        println!(
            "{mode},{n},{gates},{:.2},{:.2},{:.2}",
            cpu.as_secs_f64() * 1e3,
            gpu.as_secs_f64() * 1e3,
            cpu.as_secs_f64() / gpu.as_secs_f64()
        );
    }
}
