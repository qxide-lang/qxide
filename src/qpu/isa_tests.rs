use super::*;

// A small dense simulator (bit `q` of a basis index is qubit `q`) that runs both the logical and
// the ISA circuit, so every rewrite is checked as a unitary up to one global phase.
#[derive(Clone, Copy, Debug, PartialEq)]
struct C(f64, f64);

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
}

impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
}

impl C {
    fn conj(self) -> C {
        C(self.0, -self.1)
    }
    fn abs(self) -> f64 {
        self.0.hypot(self.1)
    }
    fn expi(a: f64) -> C {
        C(a.cos(), a.sin())
    }
}

const Z0: C = C(0.0, 0.0);
const ONE: C = C(1.0, 0.0);
const I: C = C(0.0, 1.0);

type M2 = [[C; 2]; 2];

fn apply1(s: &mut [C], q: usize, m: M2) {
    let bit = 1 << q;
    for i in 0..s.len() {
        if i & bit == 0 {
            let (a, b) = (s[i], s[i | bit]);
            s[i] = m[0][0] * a + m[0][1] * b;
            s[i | bit] = m[1][0] * a + m[1][1] * b;
        }
    }
}

// Applies `m` to `t` where every qubit of `controls` is 1.
fn controlled(s: &mut [C], controls: &[usize], t: usize, m: M2) {
    let mask: usize = controls.iter().map(|c| 1 << c).sum();
    let bit = 1 << t;
    for i in 0..s.len() {
        if i & bit == 0 && i & mask == mask {
            let (a, b) = (s[i], s[i | bit]);
            s[i] = m[0][0] * a + m[0][1] * b;
            s[i | bit] = m[1][0] * a + m[1][1] * b;
        }
    }
}

fn r(a: f64) -> C {
    C(a, 0.0)
}

fn h() -> M2 {
    let k = std::f64::consts::FRAC_1_SQRT_2;
    [[r(k), r(k)], [r(k), r(-k)]]
}
fn x() -> M2 {
    [[Z0, ONE], [ONE, Z0]]
}
fn diag(a: C, b: C) -> M2 {
    [[a, Z0], [Z0, b]]
}
fn rx(t: f64) -> M2 {
    let (c, s) = ((t / 2.0).cos(), (t / 2.0).sin());
    [[r(c), C(0.0, -s)], [C(0.0, -s), r(c)]]
}
fn ry(t: f64) -> M2 {
    let (c, s) = ((t / 2.0).cos(), (t / 2.0).sin());
    [[r(c), r(-s)], [r(s), r(c)]]
}
fn rz(t: f64) -> M2 {
    diag(C::expi(-t / 2.0), C::expi(t / 2.0))
}
fn sx() -> M2 {
    [[C(0.5, 0.5), C(0.5, -0.5)], [C(0.5, -0.5), C(0.5, 0.5)]]
}
fn s_gate() -> M2 {
    diag(ONE, I)
}

fn g1(g: G1) -> M2 {
    use std::f64::consts::FRAC_PI_4;
    match g {
        G1::H => h(),
        G1::X => x(),
        G1::Z => diag(ONE, r(-1.0)),
        G1::S => s_gate(),
        G1::Sdg => diag(ONE, I.conj()),
        G1::T => diag(ONE, C::expi(FRAC_PI_4)),
        G1::Tdg => diag(ONE, C::expi(-FRAC_PI_4)),
        G1::Rx(a) => rx(a),
        G1::Ry(a) => ry(a),
        G1::Rz(a) => rz(a),
    }
}

fn run_logical(s: &mut [C], ops: &[Op], bits: &[bool]) {
    for op in ops {
        match *op {
            Op::G1(g, q) => apply1(s, q, g1(g)),
            Op::Cx(c, t) => controlled(s, &[c], t, x()),
            Op::Ccx(a, b, t) => controlled(s, &[a, b], t, x()),
            Op::If {
                bit,
                value,
                ref body,
            } => {
                if bits[bit] == value {
                    run_logical(s, body, bits);
                }
            }
            Op::Measure { .. } | Op::Reset(_) => panic!("unitary circuits only"),
        }
    }
}

// ECR from its standard definition: s q0; sx q1; cx q0, q1; x q0.
fn ecr(s: &mut [C], a: usize, b: usize) {
    apply1(s, a, s_gate());
    apply1(s, b, sx());
    controlled(s, &[a], b, x());
    apply1(s, a, x());
}

fn run_native(s: &mut [C], ops: &[Native], two: TwoQ, bits: &[bool]) {
    for op in ops {
        match *op {
            Native::Rz(a, p) => apply1(s, p, rz(a)),
            Native::Sx(p) => apply1(s, p, sx()),
            Native::X(p) => apply1(s, p, x()),
            Native::Two(a, b) => match two {
                TwoQ::Cz => controlled(s, &[a], b, diag(ONE, r(-1.0))),
                TwoQ::Cx => controlled(s, &[a], b, x()),
                TwoQ::Ecr => ecr(s, a, b),
            },
            Native::If {
                bit,
                value,
                ref body,
            } => {
                if bits[bit] == value {
                    run_native(s, body, two, bits);
                }
            }
            Native::Measure { .. } | Native::Reset(_) => panic!("unitary circuits only"),
        }
    }
}

fn scatter(x: usize, layout: &[usize]) -> usize {
    layout
        .iter()
        .enumerate()
        .map(|(l, &p)| ((x >> l) & 1) << p)
        .sum()
}

// Every logical basis state, run through both circuits, must give the same state up to one
// global phase shared by all inputs (so the whole unitary matches).
fn assert_equivalent(c: &Logical, isa: &Isa, dev: &Device, bits: &[bool]) {
    let n = c.qubits;
    let mut phase: Option<C> = None;
    for input in 0..1usize << n {
        let mut log = vec![Z0; 1 << n];
        log[input] = ONE;
        run_logical(&mut log, &c.ops, bits);
        let mut phys = vec![Z0; 1 << dev.qubits];
        phys[scatter(input, &isa.layout)] = ONE;
        run_native(&mut phys, &isa.ops, isa.two, bits);
        let mut overlap = Z0;
        for (y, &a) in log.iter().enumerate() {
            overlap = overlap + a.conj() * phys[scatter(y, &isa.final_layout)];
        }
        assert!(
            (overlap.abs() - 1.0).abs() < 1e-9,
            "input {input}: |overlap| = {} for {c:?} on {}",
            overlap.abs(),
            dev.name
        );
        match phase {
            None => phase = Some(overlap),
            Some(p) => assert!(
                (p.0 - overlap.0).abs() < 1e-9 && (p.1 - overlap.1).abs() < 1e-9,
                "input {input}: phase {overlap:?} differs from {p:?}"
            ),
        }
    }
}

fn device(name: &str, qubits: usize, basis: &[&str], coupling: &[(usize, usize)]) -> Device {
    Device {
        name: name.into(),
        qubits,
        basis: basis.iter().map(|s| s.to_string()).collect(),
        coupling: coupling.to_vec(),
        instructions: Some(
            ["measure", "reset", "if_else", "delay"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        ),
    }
}

fn both_ways(edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    edges.iter().flat_map(|&(a, b)| [(a, b), (b, a)]).collect()
}

fn devices() -> Vec<Device> {
    let line = [(0, 1), (1, 2), (2, 3), (3, 4)];
    vec![
        device(
            "line_cz",
            5,
            &["cz", "id", "rz", "sx", "x"],
            &both_ways(&line),
        ),
        // ECR is directional: alternate the calibrated direction along the line.
        device(
            "line_ecr",
            5,
            &["ecr", "id", "rz", "sx", "x"],
            &[(0, 1), (2, 1), (2, 3), (4, 3)],
        ),
        // A T shape with directed CX and no `x` (so X is SX·SX).
        device("tee_cx", 4, &["cx", "rz", "sx"], &[(1, 0), (1, 2), (3, 1)]),
    ]
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn angle(&mut self) -> f64 {
        (self.next() % 10_000) as f64 / 1000.0 - 5.0
    }
}

fn random_ops(rng: &mut Rng, n: usize, len: usize, bits: usize, depth: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    for _ in 0..len {
        let q = rng.below(n);
        let op = match rng.below(if depth < 2 && bits > 0 { 14 } else { 13 }) {
            0 => Op::G1(G1::H, q),
            1 => Op::G1(G1::X, q),
            2 => Op::G1(G1::Z, q),
            3 => Op::G1(G1::S, q),
            4 => Op::G1(G1::Sdg, q),
            5 => Op::G1(G1::T, q),
            6 => Op::G1(G1::Tdg, q),
            7 => Op::G1(G1::Rx(rng.angle()), q),
            8 => Op::G1(G1::Ry(rng.angle()), q),
            9 => Op::G1(G1::Rz(rng.angle()), q),
            10 | 11 if n >= 2 => {
                let t = (q + 1 + rng.below(n - 1)) % n;
                Op::Cx(q, t)
            }
            12 if n >= 3 => {
                let b = (q + 1 + rng.below(n - 1)) % n;
                let mut t = rng.below(n);
                while t == q || t == b {
                    t = rng.below(n);
                }
                Op::Ccx(q, b, t)
            }
            13 => {
                let (bit, value, len) = (rng.below(bits), rng.below(2) == 1, 1 + rng.below(6));
                Op::If {
                    bit,
                    value,
                    body: random_ops(rng, n, len, bits, depth + 1),
                }
            }
            _ => Op::G1(G1::H, q),
        };
        ops.push(op);
    }
    ops
}

#[test]
fn random_circuits_are_equivalent_on_every_device() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for dev in devices() {
        for n in 1..=dev.qubits {
            for _ in 0..12 {
                let c = Logical {
                    qubits: n,
                    bits: 0,
                    ops: random_ops(&mut rng, n, 25, 0, 0),
                };
                let isa = lower(&c, &dev).unwrap();
                assert_equivalent(&c, &isa, &dev, &[]);
            }
        }
    }
}

#[test]
fn branches_restore_the_layout_whichever_way_the_bit_goes() {
    let mut rng = Rng(42);
    for dev in devices() {
        let n = dev.qubits;
        for _ in 0..12 {
            let c = Logical {
                qubits: n,
                bits: 2,
                ops: random_ops(&mut rng, n, 20, 2, 0),
            };
            let isa = lower(&c, &dev).unwrap();
            for bits in [[false, false], [false, true], [true, false], [true, true]] {
                assert_equivalent(&c, &isa, &dev, &bits);
            }
        }
    }
}

#[test]
fn native_gates_respect_basis_and_coupling() {
    let mut rng = Rng(7);
    for dev in devices() {
        let c = Logical {
            qubits: dev.qubits,
            bits: 1,
            ops: random_ops(&mut rng, dev.qubits, 60, 1, 0),
        };
        let isa = lower(&c, &dev).unwrap();
        let directed = isa.two != TwoQ::Cz;
        let qasm = to_qasm(&isa);
        check_native(&isa.ops, &dev, directed);
        assert!(
            !qasm.contains("qubit["),
            "ISA circuits use physical qubits only"
        );
    }
}

fn check_native(ops: &[Native], dev: &Device, directed: bool) {
    for op in ops {
        match *op {
            Native::Two(a, b) => assert!(
                dev.coupling.contains(&(a, b)) || (!directed && dev.coupling.contains(&(b, a))),
                "{a},{b} not coupled on {}",
                dev.name
            ),
            Native::X(_) => assert!(dev.basis.iter().any(|g| g == "x")),
            Native::Rz(a, _) => assert!(a > -PI && a <= PI && a != 0.0),
            Native::If { ref body, .. } => check_native(body, dev, directed),
            _ => {}
        }
    }
}

#[test]
fn parses_what_the_library_prints() {
    let mut c = qxide::Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    let mut t = c.qubit();
    c.h(&mut a);
    c.rx(&mut b, -0.0);
    c.ry(&mut t, 1e300);
    c.sdg(&mut a);
    c.ccx(&mut a, &mut b, &mut t);
    let m = c.measure(a);
    c.if_zero(m, |c| {
        c.cx(&mut b, &mut t);
        c.tdg(&mut t);
    });
    c.reset(&mut b);
    c.measure(b);
    c.measure(t);
    let parsed = parse(&c.to_qasm()).unwrap();
    assert_eq!(parsed.qubits, 3);
    assert_eq!(parsed.bits, 3);
    assert_eq!(
        parsed.ops,
        vec![
            Op::G1(G1::H, 0),
            Op::G1(G1::Rx(-0.0), 1),
            Op::G1(G1::Ry(1e300), 2),
            Op::G1(G1::Sdg, 0),
            Op::Ccx(0, 1, 2),
            Op::Measure { qubit: 0, bit: 0 },
            Op::If {
                bit: 0,
                value: false,
                body: vec![Op::Cx(1, 2), Op::G1(G1::Tdg, 2)],
            },
            Op::Reset(1),
            Op::Measure { qubit: 1, bit: 1 },
            Op::Measure { qubit: 2, bit: 2 },
        ]
    );
    assert!(
        parse("OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\nu3(1,2,3) q[0];\n").is_err()
    );
    assert!(parse("OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[1] q;\nh q[1];\n").is_err());
}

#[test]
fn measurements_and_resets_follow_the_layout() {
    let dev = &devices()[0];
    // q0 and q4 start at the two ends of the line, so the CNOT moves q0 next to q4.
    let c = Logical {
        qubits: 5,
        bits: 2,
        ops: vec![
            Op::Cx(0, 4),
            Op::Measure { qubit: 0, bit: 0 },
            Op::Reset(0),
            Op::Measure { qubit: 4, bit: 1 },
        ],
    };
    let isa = lower(&c, dev).unwrap();
    let tail: Vec<&Native> = isa.ops.iter().rev().take(3).collect();
    assert_eq!(
        tail,
        vec![
            &Native::Measure {
                qubit: isa.final_layout[4],
                bit: 1
            },
            &Native::Reset(isa.final_layout[0]),
            &Native::Measure {
                qubit: isa.final_layout[0],
                bit: 0
            },
        ]
    );
    assert_ne!(isa.layout, isa.final_layout);
}

#[test]
fn devices_that_cannot_run_the_circuit_are_refused() {
    let line = devices()[0].clone();
    let five = Logical {
        qubits: 5,
        bits: 0,
        ops: vec![Op::Cx(0, 1)],
    };
    let small = Device {
        qubits: 4,
        coupling: both_ways(&[(0, 1), (1, 2), (2, 3)]),
        ..line.clone()
    };
    assert!(lower(&five, &small).is_err());
    let split = Device {
        coupling: both_ways(&[(0, 1), (2, 3)]),
        ..line.clone()
    };
    let three = Logical {
        qubits: 3,
        bits: 0,
        ops: vec![],
    };
    assert!(lower(&three, &split).is_err());
    let no_two = Device {
        basis: vec!["rz".into(), "sx".into(), "x".into()],
        ..line.clone()
    };
    assert!(lower(&five, &no_two).is_err());
    let static_only = Device {
        instructions: Some(vec!["measure".into()]),
        ..line.clone()
    };
    let reset = Logical {
        qubits: 1,
        bits: 0,
        ops: vec![Op::Reset(0)],
    };
    assert!(lower(&reset, &static_only).is_err());
    let branch = Logical {
        qubits: 1,
        bits: 1,
        ops: vec![
            Op::Measure { qubit: 0, bit: 0 },
            Op::If {
                bit: 0,
                value: true,
                body: vec![Op::G1(G1::X, 0)],
            },
        ],
    };
    assert!(lower(&branch, &static_only).is_err());
    assert!(lower(&branch, &line).is_ok());
}

#[test]
fn ecr_definition_is_emitted_only_when_used() {
    let dev = &devices()[1];
    let cx = Logical {
        qubits: 2,
        bits: 0,
        ops: vec![Op::Cx(0, 1)],
    };
    let one = Logical {
        qubits: 1,
        bits: 0,
        ops: vec![Op::G1(G1::H, 0)],
    };
    assert!(to_qasm(&lower(&cx, dev).unwrap()).contains("gate ecr"));
    assert!(!to_qasm(&lower(&one, dev).unwrap()).contains("gate ecr"));
}
