// GPU state-vector backend (prototype) on CubeCL's wgpu runtime, f32 amplitudes.
// The unitary prefix of the circuit runs on the GPU; from the first measurement/reset/branch on,
// the state is read back and the CPU backend finishes the run.

use crate::statevector::{self, C};
use crate::Op;
use cubecl::prelude::*;
use cubecl::Device;
use std::sync::OnceLock;

const BLOCK: u32 = 256;
const READBACK_FAILED: &str = "failed to read GPU state back to CPU";
// wgpu caps each dispatch dimension at 65535 workgroups.
const MAX_GRID_X: u32 = 65535;

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn gate1(
    re: &mut [f32],
    im: &mut [f32],
    q: u32,
    pairs: u32,
    m00r: f32,
    m00i: f32,
    m01r: f32,
    m01i: f32,
    m10r: f32,
    m10i: f32,
    m11r: f32,
    m11i: f32,
) {
    let k = ABSOLUTE_POS as u32;
    if k < pairs {
        // Insert a 0 at bit position q to get the |..0..⟩ index of the pair.
        let i = ((k >> q) << (q + 1)) | (k & ((1u32 << q) - 1));
        let j = i | (1u32 << q);
        let (iu, ju) = (i as usize, j as usize);
        let (ar, ai, br, bi) = (re[iu], im[iu], re[ju], im[ju]);
        re[iu] = m00r * ar - m00i * ai + m01r * br - m01i * bi;
        im[iu] = m00r * ai + m00i * ar + m01r * bi + m01i * br;
        re[ju] = m10r * ar - m10i * ai + m11r * br - m11i * bi;
        im[ju] = m10r * ai + m10i * ar + m11r * bi + m11i * br;
    }
}

#[cube(launch)]
fn cx(re: &mut [f32], im: &mut [f32], lo: u32, hi: u32, c: u32, quads: u32) {
    let k = ABSOLUTE_POS as u32;
    if k < quads {
        // Insert 0s at bit positions lo < hi, then set the control bit.
        let a = ((k >> lo) << (lo + 1)) | (k & ((1u32 << lo) - 1));
        let b = ((a >> hi) << (hi + 1)) | (a & ((1u32 << hi) - 1));
        let i = (b | (1u32 << c)) as usize;
        let j = (b | (1u32 << c) | (1u32 << (lo + hi - c))) as usize;
        let (tr, ti) = (re[i], im[i]);
        re[i] = re[j];
        im[i] = im[j];
        re[j] = tr;
        im[j] = ti;
    }
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn ccx(
    re: &mut [f32],
    im: &mut [f32],
    p0: u32,
    p1: u32,
    p2: u32,
    controls: u32,
    target: u32,
    octs: u32,
) {
    let k = ABSOLUTE_POS as u32;
    if k < octs {
        // Insert 0s at the sorted bit positions p0 < p1 < p2, then set both control bits.
        let a = ((k >> p0) << (p0 + 1)) | (k & ((1u32 << p0) - 1));
        let b = ((a >> p1) << (p1 + 1)) | (a & ((1u32 << p1) - 1));
        let c = ((b >> p2) << (p2 + 1)) | (b & ((1u32 << p2) - 1));
        let i = (c | controls) as usize;
        let j = (c | controls | target) as usize;
        let (tr, ti) = (re[i], im[i]);
        re[i] = re[j];
        im[i] = im[j];
        re[j] = tr;
        im[j] = ti;
    }
}

fn client() -> &'static cubecl::client::Client {
    static CLIENT: OnceLock<cubecl::client::Client> = OnceLock::new();
    CLIENT.get_or_init(|| Device::default().client())
}

fn grid(threads: u32) -> CubeCount {
    let blocks = threads.div_ceil(BLOCK).max(1);
    CubeCount::Static(blocks.min(MAX_GRID_X), blocks.div_ceil(MAX_GRID_X), 1)
}

// Runs the leading unitary ops on the GPU. Returns the resulting state (as f64) and
// how many ops were consumed.
pub(crate) fn run_prefix(ops: &[Op], n_qubits: usize) -> (Vec<C>, usize) {
    let dim = 1usize << n_qubits;
    let client = client();
    let mut re0 = vec![0f32; dim];
    re0[0] = 1.0;
    let re = client.create_from_slice(f32::as_bytes(&re0));
    let im = client.create_from_slice(f32::as_bytes(&vec![0f32; dim]));
    let arg = |h: &cubecl::server::Handle| unsafe { BufferArg::from_raw_parts(h.clone(), dim) };

    let mut done = 0;
    for op in ops {
        match *op {
            Op::Gate(g, q) => {
                let m = statevector::matrix(g).map(|row| row.map(|c| (c.re as f32, c.im as f32)));
                let pairs = (dim / 2) as u32;
                gate1::launch(
                    client,
                    grid(pairs),
                    CubeDim::new_1d(BLOCK),
                    arg(&re),
                    arg(&im),
                    q as u32,
                    pairs,
                    m[0][0].0,
                    m[0][0].1,
                    m[0][1].0,
                    m[0][1].1,
                    m[1][0].0,
                    m[1][0].1,
                    m[1][1].0,
                    m[1][1].1,
                );
            }
            Op::Cx(c, t) => {
                let quads = (dim / 4) as u32;
                cx::launch(
                    client,
                    grid(quads),
                    CubeDim::new_1d(BLOCK),
                    arg(&re),
                    arg(&im),
                    c.min(t) as u32,
                    c.max(t) as u32,
                    c as u32,
                    quads,
                );
            }
            Op::Ccx(a, b, t) => {
                let mut p = [a as u32, b as u32, t as u32];
                p.sort_unstable();
                let octs = (dim / 8) as u32;
                ccx::launch(
                    client,
                    grid(octs),
                    CubeDim::new_1d(BLOCK),
                    arg(&re),
                    arg(&im),
                    p[0],
                    p[1],
                    p[2],
                    (1u32 << a) | (1u32 << b),
                    1u32 << t,
                    octs,
                );
            }
            Op::Measure { .. } | Op::Reset(_) | Op::If { .. } => break,
        }
        done += 1;
    }
    let re = f32::from_bytes(&client.read_one(re).expect(READBACK_FAILED)).to_vec();
    let im = f32::from_bytes(&client.read_one(im).expect(READBACK_FAILED)).to_vec();
    let state = re
        .iter()
        .zip(&im)
        .map(|(&r, &i)| C {
            re: r.into(),
            im: i.into(),
        })
        .collect();
    (state, done)
}

pub(crate) fn run(ops: &[Op], n_qubits: usize, n_bits: usize, seed: u64) -> Vec<bool> {
    let (state, done) = run_prefix(ops, n_qubits);
    statevector::run_from(&ops[done..], state, n_bits, seed)
}

#[cfg(test)]
mod tests {
    use crate::{statevector, Circuit};

    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }
    }

    // f32 amplitudes: each gate adds ~1e-7 relative error; 1e-4 leaves a wide margin for ~200 gates.
    const TOL: f64 = 1e-4;

    #[test]
    fn gpu_amplitudes_match_cpu_on_random_circuits() {
        let mut rng = Lcg(7);
        for case in 0..40 {
            let n = 1 + rng.below(12) as usize;
            let mut c = Circuit::new();
            let mut qs: Vec<_> = (0..n).map(|_| c.qubit()).collect();
            for _ in 0..rng.below(200) {
                let i = rng.below(n as u64) as usize;
                let theta = rng.below(10_000) as f64 / 1000.0 - 5.0;
                match rng.below(10) {
                    9 if n > 2 => {
                        let j = (i + 1 + rng.below(n as u64 - 1) as usize) % n;
                        let mut k = rng.below(n as u64) as usize;
                        while k == i || k == j {
                            k = (k + 1) % n;
                        }
                        let [a, b, t] = qs.get_disjoint_mut([i, j, k]).unwrap();
                        c.ccx(a, b, t);
                    }
                    0 => c.h(&mut qs[i]),
                    1 => c.s(&mut qs[i]),
                    2 => c.t(&mut qs[i]),
                    3 => c.x(&mut qs[i]),
                    4 => c.rx(&mut qs[i], theta),
                    5 => c.ry(&mut qs[i], theta),
                    6 => c.rz(&mut qs[i], theta),
                    _ if n > 1 => {
                        let j = (i + 1 + rng.below(n as u64 - 1) as usize) % n;
                        let [a, b] = qs.get_disjoint_mut([i, j]).unwrap();
                        c.cx(a, b);
                    }
                    _ => {}
                }
            }
            let (gpu, done) = super::run_prefix(&c.ops, n);
            assert_eq!(done, c.ops.len());
            let cpu = statevector::amplitudes(&c.ops, n);
            for (k, (g, e)) in gpu.iter().zip(&cpu).enumerate() {
                let err = ((g.re - e.re).powi(2) + (g.im - e.im).powi(2)).sqrt();
                assert!(err < TOL, "case {case} (n={n}) amplitude {k}: error {err}");
            }
        }
    }

    #[test]
    fn gpu_handles_more_than_65535_workgroups() {
        // 2^25 pairs / 256 threads = 131072 workgroups > the 65535 per-dimension cap.
        let n = 26;
        let mut c = Circuit::new();
        let mut qs: Vec<_> = (0..n).map(|_| c.qubit()).collect();
        // Every amplitude (hence every thread index) is touched: uniform superposition,
        // a sign on bit 0, then a cx controlled by the top qubit moves that sign in the upper half.
        for q in qs.iter_mut() {
            c.h(q);
        }
        c.z(&mut qs[0]);
        let [t, ctrl] = qs.get_disjoint_mut([0, n - 1]).unwrap();
        c.cx(ctrl, t);
        let (state, _) = super::run_prefix(&c.ops, n);
        let mag = (0.5f64).powf(n as f64 / 2.0);
        for (k, a) in state.iter().enumerate() {
            let negative = (k & 1 != 0) ^ (k >> (n - 1) != 0);
            let expected = if negative { -mag } else { mag };
            assert!(
                (a.re - expected).abs() < 1e-7 && a.im.abs() < 1e-7,
                "index {k}: {} {}",
                a.re,
                a.im
            );
        }
    }
}
