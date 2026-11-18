//! # qxide — 큐비트 소유권을 Rust 타입으로 표현하는 작은 실험
//!
//! 양자역학의 두 가지 규칙을 Rust 컴파일러가 대신 검사하게 만든다.
//!
//! 1. **복제 불가 정리**: 큐비트는 복사할 수 없다.
//!    → [`Qubit`]은 `Clone`/`Copy`를 구현하지 않는다.
//! 2. **측정하면 양자 상태가 사라진다**: 측정한 큐비트는 더 이상 양자 연산에 쓸 수 없다.
//!    → [`Circuit::measure`]가 `Qubit`을 값으로 받아 소비(move)하고, 고전 비트 [`Bit`]을 돌려준다.
//!
//! 덤으로 2큐비트 게이트 `cx(&mut a, &mut b)`는 Rust의 빌림 규칙 때문에
//! 같은 큐비트를 제어와 대상에 동시에 넣을 수 없다(`cx(&mut q, &mut q)`는 컴파일 오류).
//!
//! 만든 회로는 두 곳으로 보낼 수 있다.
//! - [`Circuit::to_qasm`]: 실제 양자컴퓨터 회사들이 공통으로 받는 OpenQASM 3 문자열
//! - [`Circuit::simulate`]: 일반 컴퓨터에서 도는 작은 상태 벡터 시뮬레이터
//!
//! ```
//! use qxide::Circuit;
//!
//! let mut c = Circuit::new();
//! let mut a = c.qubit();
//! let mut b = c.qubit();
//! c.h(&mut a);
//! c.cx(&mut a, &mut b); // 벨 상태 (얽힘)
//! let ma = c.measure(a);
//! let mb = c.measure(b);
//!
//! let shot = c.simulate(42);
//! assert_eq!(shot.get(ma), shot.get(mb)); // 얽힌 두 큐비트는 항상 같은 값
//! ```
//!
//! ## 컴파일러가 막아 주는 실수들
//!
//! 큐비트 복제 (복제 불가 정리 위반):
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let q = c.qubit();
//! let copy = q.clone();
//! ```
//!
//! 측정한 큐비트를 다시 쓰기:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut q = c.qubit();
//! let _bit = c.measure(q);
//! c.h(&mut q);
//! ```
//!
//! 같은 큐비트를 제어와 대상에 동시에 쓰기:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut q = c.qubit();
//! c.cx(&mut q, &mut q);
//! ```
//!
//! 큐비트 레지스터 복제 (`QReg`도 복제 불가 정리 위반):
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let r = c.qreg(2);
//! let copy = r.clone();
//! ```
//!
//! 인덱싱으로 같은 레지스터를 두 번 가변 빌림:
//! ```compile_fail
//! let mut c = qxide::Circuit::new();
//! let mut r = c.qreg(2);
//! c.cx(&mut r[0], &mut r[1]);
//! ```
//!
//! `get_disjoint_mut`으로 서로 다른 두 큐비트를 동시에 빌리기(인덱스 중복은 실행 시 검사):
//! ```
//! let mut c = qxide::Circuit::new();
//! let mut r = c.qreg(3);
//! let [a, b] = r.get_disjoint_mut([0, 2]).unwrap();
//! c.cx(a, b);
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_CIRCUIT_ID: AtomicUsize = AtomicUsize::new(0);

/// 회로 안의 큐비트 하나. 복사할 수 없다.
#[derive(Debug)]
pub struct Qubit {
    circuit: usize,
    index: usize,
}

/// 측정 결과가 들어갈 고전 비트의 자리. 고전 정보라서 자유롭게 복사할 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bit {
    circuit: usize,
    index: usize,
}

#[derive(Debug, Clone, Copy)]
enum Gate {
    H,
    X,
    Z,
    S,
    T,
    Rx(f64),
    Ry(f64),
    Rz(f64),
}

impl Gate {
    fn qasm(self) -> String {
        match self {
            Gate::H => "h".into(),
            Gate::X => "x".into(),
            Gate::Z => "z".into(),
            Gate::S => "s".into(),
            Gate::T => "t".into(),
            Gate::Rx(a) => format!("rx({a:?})"),
            Gate::Ry(a) => format!("ry({a:?})"),
            Gate::Rz(a) => format!("rz({a:?})"),
        }
    }

    // [[m00, m01], [m10, m11]], following the OpenQASM 3 stdgates.inc definitions.
    fn matrix(self) -> [[C; 2]; 2] {
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let re = |x: f64| C { re: x, im: 0.0 };
        let im = |y: f64| C { re: 0.0, im: y };
        match self {
            Gate::H => [[re(r), re(r)], [re(r), re(-r)]],
            Gate::X => [[C::ZERO, C::ONE], [C::ONE, C::ZERO]],
            Gate::Z => [[C::ONE, C::ZERO], [C::ZERO, re(-1.0)]],
            Gate::S => [[C::ONE, C::ZERO], [C::ZERO, im(1.0)]],
            Gate::T => [
                [C::ONE, C::ZERO],
                [C::ZERO, C::cis(std::f64::consts::FRAC_PI_4)],
            ],
            Gate::Rx(a) => {
                let (s, c) = (a / 2.0).sin_cos();
                [[re(c), im(-s)], [im(-s), re(c)]]
            }
            Gate::Ry(a) => {
                let (s, c) = (a / 2.0).sin_cos();
                [[re(c), re(-s)], [re(s), re(c)]]
            }
            Gate::Rz(a) => [[C::cis(-a / 2.0), C::ZERO], [C::ZERO, C::cis(a / 2.0)]],
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Gate(Gate, usize),
    Cx(usize, usize),
    Measure {
        qubit: usize,
        bit: usize,
    },
    Reset(usize),
    If {
        bit: usize,
        value: bool,
        body: Vec<Op>,
    },
}

/// 양자 연산을 차례로 기록하는 회로.
#[derive(Debug)]
pub struct Circuit {
    id: usize,
    n_qubits: usize,
    n_bits: usize,
    ops: Vec<Op>,
    // Physical slots freed by measurement; reused (after a reset) by the next `qubit()`.
    free: std::collections::VecDeque<usize>,
    // Nesting level of `if_one`/`if_zero` bodies currently being recorded.
    depth: usize,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Self {
            id: NEXT_CIRCUIT_ID.fetch_add(1, Ordering::Relaxed),
            n_qubits: 0,
            n_bits: 0,
            ops: Vec::new(),
            free: std::collections::VecDeque::new(),
            depth: 0,
        }
    }

    /// |0⟩ 상태의 새 큐비트를 만든다.
    pub fn qubit(&mut self) -> Qubit {
        // Inside a branch the reset would be conditional, so only reuse slots at top level.
        let reuse = if self.depth == 0 {
            self.free.pop_front()
        } else {
            None
        };
        let index = match reuse {
            Some(i) => {
                self.ops.push(Op::Reset(i));
                i
            }
            None => {
                self.n_qubits += 1;
                self.n_qubits - 1
            }
        };
        Qubit {
            circuit: self.id,
            index,
        }
    }

    fn own(&self, q: &Qubit) -> usize {
        // 타입으로는 막을 수 없는 유일한 실수(다른 회로의 큐비트)는 실행 중에 잡는다.
        assert_eq!(q.circuit, self.id, "다른 회로의 큐비트를 사용했습니다");
        q.index
    }

    fn gate(&mut self, g: Gate, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::Gate(g, i));
    }

    /// 아다마르 게이트: |0⟩을 |0⟩과 |1⟩의 중첩으로 만든다.
    pub fn h(&mut self, q: &mut Qubit) {
        self.gate(Gate::H, q);
    }

    /// 파울리 X (양자 NOT).
    pub fn x(&mut self, q: &mut Qubit) {
        self.gate(Gate::X, q);
    }

    /// 파울리 Z (위상 뒤집기).
    pub fn z(&mut self, q: &mut Qubit) {
        self.gate(Gate::Z, q);
    }

    /// S 게이트: Z의 제곱근으로, |1⟩에 위상 i를 더한다(P(π/2)와 같다).
    pub fn s(&mut self, q: &mut Qubit) {
        self.gate(Gate::S, q);
    }

    /// T 게이트: S의 제곱근으로, |1⟩에 위상 e^{iπ/4}를 더한다.
    pub fn t(&mut self, q: &mut Qubit) {
        self.gate(Gate::T, q);
    }

    /// X축 주위로 `theta` 라디안 회전한다(exp(-iθX/2), OpenQASM 3 `rx`와 같다).
    pub fn rx(&mut self, q: &mut Qubit, theta: f64) {
        self.gate(Gate::Rx(theta), q);
    }

    /// Y축 주위로 `theta` 라디안 회전한다(exp(-iθY/2)).
    pub fn ry(&mut self, q: &mut Qubit, theta: f64) {
        self.gate(Gate::Ry(theta), q);
    }

    /// Z축 주위로 `theta` 라디안 회전하며(exp(-iθZ/2)), 위상만 바꾼다.
    pub fn rz(&mut self, q: &mut Qubit, theta: f64) {
        self.gate(Gate::Rz(theta), q);
    }

    /// 제어 NOT: control이 |1⟩이면 target을 뒤집는다.
    /// 두 인자가 모두 `&mut`이라 같은 큐비트를 동시에 넣을 수 없다.
    pub fn cx(&mut self, control: &mut Qubit, target: &mut Qubit) {
        let (c, t) = (self.own(control), self.own(target));
        self.ops.push(Op::Cx(c, t));
    }

    /// 큐비트를 측정한다. 큐비트는 여기서 소비되어 사라지고, 결과 자리를 가리키는 비트가 남는다.
    pub fn measure(&mut self, q: Qubit) -> Bit {
        let i = self.own(&q);
        // A conditional measurement would leave its bit undefined on the untaken path.
        assert!(
            self.depth == 0,
            "조건부 분기 내부 측정은 지원되지 않으므로 밖에서 측정해야 합니다"
        );
        let b = Bit {
            circuit: self.id,
            index: self.n_bits,
        };
        self.n_bits += 1;
        self.ops.push(Op::Measure {
            qubit: i,
            bit: b.index,
        });
        self.free.push_back(i);
        b
    }

    /// |0⟩ 상태의 새 큐비트 `n`개로 이루어진 레지스터(`QReg`)를 한 번에 만든다.
    pub fn qreg(&mut self, n: usize) -> QReg {
        QReg {
            qubits: (0..n).map(|_| self.qubit()).collect(),
        }
    }

    /// 레지스터의 모든 큐비트를 측정해 소비하고, 같은 순서의 결과 비트를 담은 `BitReg`를 돌려준다.
    pub fn measure_reg(&mut self, reg: QReg) -> BitReg {
        BitReg {
            bits: reg.into_iter().map(|q| self.measure(q)).collect(),
        }
    }

    /// 회로 도중 큐비트를 `measure`와 달리 소비 없이 |0⟩으로 되돌려 재사용한다. `qubit()`도 측정된 자리를 초기화해 자동 재사용한다.
    pub fn reset(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::Reset(i));
    }

    /// 측정 비트가 1일 때만 `body` 연산을 실행하는 고전 피드포워드다(양자·고전 결합). 안에서는 `measure`를 쓸 수 없다.
    pub fn if_one(&mut self, b: Bit, body: impl FnOnce(&mut Circuit)) {
        self.branch(b, true, body);
    }

    /// 측정 비트가 0일 때만 `body`의 연산을 실행한다.
    pub fn if_zero(&mut self, b: Bit, body: impl FnOnce(&mut Circuit)) {
        self.branch(b, false, body);
    }

    fn branch(&mut self, b: Bit, value: bool, body: impl FnOnce(&mut Circuit)) {
        check_bit(self.id, b);
        // Record the body into a fresh op list, then splice it in as one conditional op.
        let outer = std::mem::take(&mut self.ops);
        self.depth += 1;
        body(self);
        self.depth -= 1;
        let body = std::mem::replace(&mut self.ops, outer);
        self.ops.push(Op::If {
            bit: b.index,
            value,
            body,
        });
    }

    /// OpenQASM 3 문자열로 내보낸다.
    pub fn to_qasm(&self) -> String {
        let mut s = String::from("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n");
        s += &format!("qubit[{}] q;\n", self.n_qubits);
        if self.n_bits > 0 {
            s += &format!("bit[{}] c;\n", self.n_bits);
        }
        write_qasm(&self.ops, 0, &mut s);
        s
    }

    /// 상태 벡터 시뮬레이터로 회로를 한 번 실행한다(한 번의 "샷").
    /// 같은 seed면 같은 결과가 나온다. 큐비트가 n개면 2^n 크기의 벡터를 쓰므로 작은 회로용이다.
    pub fn simulate(&self, seed: u64) -> Shot {
        assert!(
            self.n_qubits <= 20,
            "작은 회로 전용 시뮬레이터입니다 (최대 20큐비트)"
        );
        let mut rng = SplitMix64(seed);
        let mut state = vec![C::ZERO; 1usize << self.n_qubits];
        state[0] = C::ONE;
        let mut bits = vec![false; self.n_bits];
        run(&self.ops, &mut state, &mut bits, &mut rng);
        Shot {
            circuit: self.id,
            bits,
        }
    }
}

fn check_bit(circuit: usize, b: Bit) {
    assert_eq!(b.circuit, circuit, "다른 회로의 비트입니다");
}

fn write_qasm(ops: &[Op], depth: usize, s: &mut String) {
    let pad = "  ".repeat(depth);
    for op in ops {
        s.push_str(&pad);
        match op {
            Op::Gate(g, i) => *s += &format!("{} q[{i}];\n", g.qasm()),
            Op::Cx(a, b) => *s += &format!("cx q[{a}], q[{b}];\n"),
            Op::Measure { qubit, bit } => *s += &format!("c[{bit}] = measure q[{qubit}];\n"),
            Op::Reset(i) => *s += &format!("reset q[{i}];\n"),
            Op::If { bit, value, body } => {
                *s += &format!("if (c[{bit}] == {}) {{\n", u8::from(*value));
                write_qasm(body, depth + 1, s);
                *s += &pad;
                *s += "}\n";
            }
        }
    }
}

fn run(ops: &[Op], state: &mut [C], bits: &mut [bool], rng: &mut SplitMix64) {
    let dim = state.len();
    for op in ops {
        match op {
            &Op::Gate(g, q) => {
                let m = 1 << q;
                let [[m00, m01], [m10, m11]] = g.matrix();
                for i in 0..dim {
                    if i & m == 0 {
                        let (a, b) = (state[i], state[i | m]);
                        state[i] = m00 * a + m01 * b;
                        state[i | m] = m10 * a + m11 * b;
                    }
                }
            }
            &Op::Cx(c, t) => {
                let (mc, mt) = (1 << c, 1 << t);
                for i in 0..dim {
                    if i & mc != 0 && i & mt == 0 {
                        state.swap(i, i | mt);
                    }
                }
            }
            &Op::Measure { qubit, bit } => {
                bits[bit] = collapse(state, qubit, rng);
            }
            &Op::Reset(q) => {
                // Reset = measure, then flip back to |0⟩ if the outcome was 1.
                if collapse(state, q, rng) {
                    let m = 1 << q;
                    for i in 0..dim {
                        if i & m == 0 {
                            state.swap(i, i | m);
                        }
                    }
                }
            }
            Op::If { bit, value, body } => {
                if bits[*bit] == *value {
                    run(body, state, bits, rng);
                }
            }
        }
    }
}

// Projective measurement of qubit `q`: samples the outcome, collapses and renormalises the state.
fn collapse(state: &mut [C], q: usize, rng: &mut SplitMix64) -> bool {
    let m = 1 << q;
    let p1: f64 = state
        .iter()
        .enumerate()
        .filter(|(i, _)| i & m != 0)
        .map(|(_, a)| a.norm_sqr())
        .sum();
    let one = rng.next_f64() < p1;
    let keep = if one { p1 } else { 1.0 - p1 };
    let renorm = 1.0 / keep.sqrt();
    for (i, amp) in state.iter_mut().enumerate() {
        *amp = if ((i & m) != 0) == one {
            amp.scale(renorm)
        } else {
            C::ZERO
        };
    }
    one
}

/// 한 번 실행한 측정 결과.
#[derive(Debug, Clone)]
pub struct Shot {
    circuit: usize,
    bits: Vec<bool>,
}

impl Shot {
    pub fn get(&self, b: Bit) -> bool {
        check_bit(self.circuit, b);
        self.bits[b.index]
    }

    /// 레지스터의 첫 비트를 최하위 비트(LSB)로 삼아 결과를 부호 없는 정수로 읽는다.
    pub fn value(&self, reg: &BitReg) -> u64 {
        assert!(
            reg.len() <= 64,
            "최대 64비트까지만 정수로 변환할 수 있습니다"
        );
        reg.iter()
            .enumerate()
            .fold(0, |v, (i, &b)| v | (u64::from(self.get(b)) << i))
    }
}

/// 여러 큐비트를 담는 레지스터. `Qubit`처럼 복사할 수 없으며, 슬라이스처럼 인덱싱과 순회가 가능하다.
#[derive(Debug)]
pub struct QReg {
    qubits: Vec<Qubit>,
}

impl std::ops::Deref for QReg {
    type Target = [Qubit];
    fn deref(&self) -> &[Qubit] {
        &self.qubits
    }
}

impl std::ops::DerefMut for QReg {
    fn deref_mut(&mut self) -> &mut [Qubit] {
        &mut self.qubits
    }
}

impl IntoIterator for QReg {
    type Item = Qubit;
    type IntoIter = std::vec::IntoIter<Qubit>;
    fn into_iter(self) -> Self::IntoIter {
        self.qubits.into_iter()
    }
}

impl<'a> IntoIterator for &'a mut QReg {
    type Item = &'a mut Qubit;
    type IntoIter = std::slice::IterMut<'a, Qubit>;
    fn into_iter(self) -> Self::IntoIter {
        self.qubits.iter_mut()
    }
}

/// 측정 결과를 받는 고전 비트 레지스터. 고전 정보라서 자유롭게 복사할 수 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitReg {
    bits: Vec<Bit>,
}

impl std::ops::Deref for BitReg {
    type Target = [Bit];
    fn deref(&self) -> &[Bit] {
        &self.bits
    }
}

impl From<Vec<Bit>> for BitReg {
    fn from(bits: Vec<Bit>) -> Self {
        Self { bits }
    }
}

// ---- 내부: 복소수와 난수 (외부 의존성 없이) ----

#[derive(Debug, Clone, Copy)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    const ZERO: C = C { re: 0.0, im: 0.0 };
    const ONE: C = C { re: 1.0, im: 0.0 };
    fn scale(self, k: f64) -> C {
        C {
            re: self.re * k,
            im: self.im * k,
        }
    }
    fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    // e^{iφ}
    fn cis(phi: f64) -> C {
        let (im, re) = phi.sin_cos();
        C { re, im }
    }
}

impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
}

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
