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
enum Op {
    H(usize),
    X(usize),
    Z(usize),
    Cx(usize, usize),
    Measure { qubit: usize, bit: usize },
}

/// 양자 연산을 차례로 기록하는 회로.
#[derive(Debug)]
pub struct Circuit {
    id: usize,
    n_qubits: usize,
    n_bits: usize,
    ops: Vec<Op>,
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
        }
    }

    /// |0⟩ 상태의 새 큐비트를 만든다.
    pub fn qubit(&mut self) -> Qubit {
        let q = Qubit {
            circuit: self.id,
            index: self.n_qubits,
        };
        self.n_qubits += 1;
        q
    }

    fn own(&self, q: &Qubit) -> usize {
        // 타입으로는 막을 수 없는 유일한 실수(다른 회로의 큐비트)는 실행 중에 잡는다.
        assert_eq!(q.circuit, self.id, "다른 회로의 큐비트를 사용했습니다");
        q.index
    }

    /// 아다마르 게이트: |0⟩을 |0⟩과 |1⟩의 중첩으로 만든다.
    pub fn h(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::H(i));
    }

    /// 파울리 X (양자 NOT).
    pub fn x(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::X(i));
    }

    /// 파울리 Z (위상 뒤집기).
    pub fn z(&mut self, q: &mut Qubit) {
        let i = self.own(q);
        self.ops.push(Op::Z(i));
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
        let b = Bit {
            circuit: self.id,
            index: self.n_bits,
        };
        self.n_bits += 1;
        self.ops.push(Op::Measure {
            qubit: i,
            bit: b.index,
        });
        b
    }

    /// OpenQASM 3 문자열로 내보낸다.
    pub fn to_qasm(&self) -> String {
        let mut s = String::from("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n");
        s += &format!("qubit[{}] q;\n", self.n_qubits);
        if self.n_bits > 0 {
            s += &format!("bit[{}] c;\n", self.n_bits);
        }
        for op in &self.ops {
            s += &match *op {
                Op::H(i) => format!("h q[{i}];\n"),
                Op::X(i) => format!("x q[{i}];\n"),
                Op::Z(i) => format!("z q[{i}];\n"),
                Op::Cx(a, b) => format!("cx q[{a}], q[{b}];\n"),
                Op::Measure { qubit, bit } => format!("c[{bit}] = measure q[{qubit}];\n"),
            };
        }
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
        let dim = 1usize << self.n_qubits;
        let mut state = vec![C::ZERO; dim];
        state[0] = C::ONE;
        let mut bits = vec![false; self.n_bits];

        for op in &self.ops {
            match *op {
                Op::H(q) => {
                    let m = 1 << q;
                    let r = std::f64::consts::FRAC_1_SQRT_2;
                    for i in 0..dim {
                        if i & m == 0 {
                            let (a, b) = (state[i], state[i | m]);
                            state[i] = (a + b).scale(r);
                            state[i | m] = (a - b).scale(r);
                        }
                    }
                }
                Op::X(q) => {
                    let m = 1 << q;
                    for i in 0..dim {
                        if i & m == 0 {
                            state.swap(i, i | m);
                        }
                    }
                }
                Op::Z(q) => {
                    let m = 1 << q;
                    for (i, amp) in state.iter_mut().enumerate() {
                        if i & m != 0 {
                            *amp = amp.scale(-1.0);
                        }
                    }
                }
                Op::Cx(c, t) => {
                    let (mc, mt) = (1 << c, 1 << t);
                    for i in 0..dim {
                        if i & mc != 0 && i & mt == 0 {
                            state.swap(i, i | mt);
                        }
                    }
                }
                Op::Measure { qubit, bit } => {
                    let m = 1 << qubit;
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
                    bits[bit] = one;
                }
            }
        }
        Shot {
            circuit: self.id,
            bits,
        }
    }
}

/// 한 번 실행한 측정 결과.
#[derive(Debug, Clone)]
pub struct Shot {
    circuit: usize,
    bits: Vec<bool>,
}

impl Shot {
    pub fn get(&self, b: Bit) -> bool {
        assert_eq!(b.circuit, self.circuit, "다른 회로의 비트입니다");
        self.bits[b.index]
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

impl std::ops::Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        C {
            re: self.re - o.re,
            im: self.im - o.im,
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
