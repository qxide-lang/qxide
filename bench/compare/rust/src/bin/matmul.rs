// Same algorithm as bench/compare/qx/matmul.qx: flat `Vec<f64>`, i-k-j order, indexed access.
use std::hint::black_box;
use std::time::Instant;

fn multiply(n: usize) -> f64 {
    let mut a = vec![0.0f64; n * n];
    let mut b = vec![0.0f64; n * n];
    let mut c = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            a[i * n + j] = (i as i64 - j as i64) as f64 * 0.5;
            b[i * n + j] = (i + j) as f64 * 0.25;
        }
    }
    for i in 0..n {
        for k in 0..n {
            let aik = a[i * n + k];
            for j in 0..n {
                c[i * n + j] += aik * b[k * n + j];
            }
        }
    }
    let mut sum = 0.0;
    for v in &c {
        sum += v;
    }
    sum
}

fn main() {
    let n: usize = std::env::args().nth(1).expect("N").parse().expect("int");
    let t0 = Instant::now();
    let sum = black_box(multiply(black_box(n)));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("result={sum:?} ms={ms:.3}");
}
