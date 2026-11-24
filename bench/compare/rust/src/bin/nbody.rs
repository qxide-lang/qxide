// Same algorithm as bench/compare/qx/nbody.qx: five planets, `STEPS` steps of 0.01, energy.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
struct Body {
    x: f64,
    y: f64,
    z: f64,
    vx: f64,
    vy: f64,
    vz: f64,
    mass: f64,
}

fn bodies() -> Vec<Body> {
    let pi = 3.141592653589793;
    let mass = 4.0 * pi * pi;
    let days = 365.24;
    let b = |x, y, z, vx: f64, vy: f64, vz: f64, m: f64| Body {
        x,
        y,
        z,
        vx: vx * days,
        vy: vy * days,
        vz: vz * days,
        mass: m * mass,
    };
    let mut bs = vec![
        Body {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            mass,
        },
        b(
            4.84143144246472090e+00,
            -1.16032004402742839e+00,
            -1.03622044471123109e-01,
            1.66007664274403694e-03,
            7.69901118419740425e-03,
            -6.90460016972063023e-05,
            9.54791938424326609e-04,
        ),
        b(
            8.34336671824457987e+00,
            4.12479856412430479e+00,
            -4.03523417114321381e-01,
            -2.76742510726862411e-03,
            4.99852801234917238e-03,
            2.30417297573763929e-05,
            2.85885980666130812e-04,
        ),
        b(
            1.28943695621391310e+01,
            -1.51111514016986312e+01,
            -2.23307578892655734e-01,
            2.96460137564761618e-03,
            2.37847173959480950e-03,
            -2.96589568540237556e-05,
            4.36624404335156298e-05,
        ),
        b(
            1.53796971148509165e+01,
            -2.59193146099879641e+01,
            1.79258772950371181e-01,
            2.68067772490389322e-03,
            1.62824170038242295e-03,
            -9.51592254519715870e-05,
            5.15138902046611451e-05,
        ),
    ];
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for b in &bs {
        px += b.vx * b.mass;
        py += b.vy * b.mass;
        pz += b.vz * b.mass;
    }
    bs[0].vx = -px / mass;
    bs[0].vy = -py / mass;
    bs[0].vz = -pz / mass;
    bs
}

fn advance(bs: &mut [Body], dt: f64) {
    let n = bs.len();
    for i in 0..n {
        for j in i + 1..n {
            let dx = bs[i].x - bs[j].x;
            let dy = bs[i].y - bs[j].y;
            let dz = bs[i].z - bs[j].z;
            let d2 = dx * dx + dy * dy + dz * dz;
            let mag = dt / (d2 * d2.sqrt());
            let (mi, mj) = (bs[i].mass, bs[j].mass);
            bs[i].vx -= dx * mj * mag;
            bs[i].vy -= dy * mj * mag;
            bs[i].vz -= dz * mj * mag;
            bs[j].vx += dx * mi * mag;
            bs[j].vy += dy * mi * mag;
            bs[j].vz += dz * mi * mag;
        }
    }
    for b in bs.iter_mut() {
        b.x += dt * b.vx;
        b.y += dt * b.vy;
        b.z += dt * b.vz;
    }
}

fn energy(bs: &[Body]) -> f64 {
    let mut e = 0.0;
    for i in 0..bs.len() {
        let a = bs[i];
        e += 0.5 * a.mass * (a.vx * a.vx + a.vy * a.vy + a.vz * a.vz);
        for b in &bs[i + 1..] {
            let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
            e -= a.mass * b.mass / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}

fn simulate(steps: i64) -> f64 {
    let mut bs = bodies();
    for _ in 0..steps {
        advance(&mut bs, 0.01);
    }
    energy(&bs)
}

fn main() {
    let steps: i64 = std::env::args()
        .nth(1)
        .expect("STEPS")
        .parse()
        .expect("int");
    let t0 = Instant::now();
    let e = black_box(simulate(black_box(steps)));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("result={e:?} ms={ms:.3}");
}
