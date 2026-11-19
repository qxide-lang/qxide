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
//!
//! ## 자동 되돌림(uncomputation)
//!
//! `with_ancilla`는 보조 큐비트에 값을 계산해 제어로만 쓰게 한 뒤 역연산을 자동으로 내보낸다. 정상 컴파일되는 예제:
//! ```
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.cx_ctrl(&a, &mut t);
//!     },
//! );
//! ```
//!
//! 계산 단계에서 `ReversibleCircuit`이 제공하지 않는 기저 변환 게이트(`h`) 쓰기:
//! ```compile_fail,E0599
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.h(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.cx_ctrl(&a, &mut t);
//!     },
//! );
//! ```
//!
//! 적용 단계에서 `&Qubit`인 보조 큐비트를 수정하기(`c.x(anc)`):
//! ```compile_fail,E0308
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.x(anc);
//!     },
//! );
//! ```
//!
//! 계산 단계에서 읽은 입력 큐비트를 적용 단계에서 수정하기(`c.h(&mut a)`, 빌림 검사기 거부):
//! ```compile_fail,E0502
//! let mut c = qxide::Circuit::new();
//! let mut a = c.qubit();
//! let mut t = c.qubit();
//! c.h(&mut a);
//! c.with_ancilla(
//!     |u, anc| {
//!         u.cx(&a, anc);
//!         u.x(anc);
//!     },
//!     |c, anc| {
//!         c.cx_ctrl(anc, &mut t);
//!         c.h(&mut a);
//!     },
//! );
//! ```

#[cfg(feature = "gpu")]
mod gpu;
mod groups;
mod rng;
mod stabilizer;
mod statevector;
mod uncompute;

pub use uncompute::ReversibleCircuit;

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
pub(crate) enum Gate {
    H,
    X,
    Z,
    S,
    T,
    Sdg,
    Tdg,
    Rx(f64),
    Ry(f64),
    Rz(f64),
}

impl Gate {
    fn name(self) -> &'static str {
        match self {
            Gate::H => "h",
            Gate::X => "x",
            Gate::Z => "z",
            Gate::S => "s",
            Gate::T => "t",
            Gate::Sdg => "sdg",
            Gate::Tdg => "tdg",
            Gate::Rx(_) => "rx",
            Gate::Ry(_) => "ry",
            Gate::Rz(_) => "rz",
        }
    }

    fn qasm(self) -> String {
        match self {
            Gate::Rx(a) | Gate::Ry(a) | Gate::Rz(a) => format!("{}({a:?})", self.name()),
            _ => self.name().into(),
        }
    }
}

/// `Circuit::simulate_with`에서 회로를 실행할 시뮬레이터를 선택한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backend {
    /// `Circuit::select_backend`로 백엔드를 고른다. Clifford 전용이면 스태빌라이저, 아니면 상태 벡터를 쓴다.
    Auto,
    /// 2^n개 복소 진폭의 밀집 상태 벡터. 모든 게이트를 지원하지만 큐비트 수에 따라 메모리가 지수적으로 늘어난다.
    StateVector,
    /// Aaronson–Gottesman 태블로: Clifford(`h`/`s`/`sdg`/`x`/`z`/`cx`/`measure`/`reset`/분기)만 쓰며 다항 시간이라 수천 큐비트도 된다.
    Stabilizer,
    /// CubeCL(wgpu) f32 진폭 GPU 상태 벡터 프로토타입(gpu 피처 전용). 첫 측정·리셋·분기 전 유니터리 구간은 GPU, 이후는 CPU가 마친다. 상세는 docs/gpu.md에 있다.
    #[cfg(feature = "gpu")]
    Gpu,
}

/// 시뮬레이션을 실행할 수 없는 이유. 조용히 다른 결과를 내는 대신 이 오류를 돌려준다.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SimError {
    /// 스태빌라이저 백엔드로 비Clifford 게이트(`t`, `tdg`, `rx`, `ry`, `rz`, `ccx`)를 실행하려 했다. 미실행 분기 안이나 Clifford 각도 회전도 거부한다.
    NotClifford {
        /// 문제가 된 첫 게이트의 OpenQASM 이름(예: `t`, `rx`)을 담는다.
        gate: &'static str,
    },
    /// 상태 벡터에 필요한 메모리(2^n × 16바이트)가 회로 한도를 넘어 아무것도 할당하지 않았다.
    OutOfMemory {
        /// 회로에 있는 큐비트 수다.
        qubits: usize,
        /// 상태 벡터에 필요한 바이트 수다(2^n × 16이며 `u128::MAX`에서 포화된다).
        required_bytes: u128,
        /// 현재 적용 중인 바이트 단위 메모리 한도다.
        limit_bytes: u128,
    },
}

/// 상태 벡터의 기본 메모리 한도인 1 GiB로, 최대 26큐비트까지 수용한다.
pub const DEFAULT_MEMORY_LIMIT: u64 = 1 << 30;

/// `n_qubits`큐비트 상태 벡터에 필요한 메모리(16바이트 복소수 2^n개, `f64` 2개)로, 오버플로 대신 `u128::MAX`에서 포화된다.
pub fn state_vector_bytes(n_qubits: usize) -> u128 {
    if n_qubits <= 123 {
        16u128 << n_qubits
    } else {
        u128::MAX
    }
}

// Binary-prefixed size with one decimal, e.g. 16.0 TiB.
fn fmt_bytes(b: u128) -> String {
    const UNITS: [&str; 9] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimError::NotClifford { gate } => write!(
                f,
                "스태빌라이저 시뮬레이터는 Clifford 게이트만 지원하며 `{gate}` 게이트는 이에 해당하지 않습니다. 상태 벡터 백엔드 사용을 권장합니다"
            ),
            SimError::OutOfMemory {
                qubits,
                required_bytes,
                limit_bytes,
            } => {
                let (required, limit) = (fmt_bytes(*required_bytes), fmt_bytes(*limit_bytes));
                write!(
                    f,
                    "{qubits}큐비트 상태 벡터 시뮬레이션에 {required}가 필요해 제한({limit})을 초과합니다. `set_memory_limit`이나 스태빌라이저 백엔드용 Clifford 전용 회로를 권장합니다"
                )
            }
        }
    }
}

impl std::error::Error for SimError {}

#[derive(Debug, Clone)]
pub(crate) enum Op {
    Gate(Gate, usize),
    Cx(usize, usize),
    Ccx(usize, usize, usize),
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
    memory_limit: u64,
    groups: groups::Groups,
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
            memory_limit: DEFAULT_MEMORY_LIMIT,
            groups: groups::Groups::default(),
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
                self.groups.add_slot();
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

    /// S†(S-dagger) 게이트: S의 역게이트이자 OpenQASM 3 `sdg`로, |1⟩에 위상 -i를 더한다(P(-π/2)와 같다).
    pub fn sdg(&mut self, q: &mut Qubit) {
        self.gate(Gate::Sdg, q);
    }

    /// T†(T-dagger) 게이트: T의 역게이트이자 OpenQASM 3 `tdg`로, |1⟩에 위상 e^{-iπ/4}를 더한다.
    pub fn tdg(&mut self, q: &mut Qubit) {
        self.gate(Gate::Tdg, q);
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
        self.push_cx(c, t);
    }

    fn push_cx(&mut self, c: usize, t: usize) {
        self.groups.union(c, t);
        self.ops.push(Op::Cx(c, t));
    }

    /// Toffoli(CCX, OpenQASM 3 `ccx`): 두 제어 큐비트가 모두 |1⟩일 때만 `target`을 뒤집는다. 세 인자 모두 `&mut`이므로 같은 큐비트를 중복 전달할 수 없으며(컴파일 오류), 비Clifford 게이트라 스태빌라이저 백엔드에서는 거부된다.
    pub fn ccx(&mut self, control1: &mut Qubit, control2: &mut Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        self.push_ccx(a, b, t);
    }

    /// `ccx`와 같은 게이트지만 제어 큐비트를 공유 참조(`&Qubit`)로 받아 `with_ancilla`의 적용 단계 등에서 `&Qubit`인 보조·입력 큐비트에도 사용할 수 있다.
    /// 공유 참조는 별칭(aliasing)이 가능하므로 두 제어 큐비트에 같은 큐비트를 전달하면 패닉한다.
    pub fn ccx_ctrl(&mut self, control1: &Qubit, control2: &Qubit, target: &mut Qubit) {
        let (a, b, t) = (self.own(control1), self.own(control2), self.own(target));
        check_distinct_controls(a, b);
        self.push_ccx(a, b, t);
    }

    fn push_ccx(&mut self, a: usize, b: usize, t: usize) {
        self.groups.union(a, t);
        self.groups.union(b, t);
        self.ops.push(Op::Ccx(a, b, t));
    }

    /// 런타임 유니온 파인드로 추적한 얽힘 그룹을 기준으로 두 큐비트의 얽힘 가능성을 반환한다.
    /// `if_one`이나 `if_zero` 본문을 포함한 모든 `cx`는 두 큐비트의 그룹을 전이적으로 병합하며, 측정과 최상위 `reset`은 해당 큐비트만 그룹에서 제거하고 나머지는 유지하지만 분기 본문 안의 `reset`은 실행되지 않을 수 있어 제거하지 않는다.
    /// 결과는 보수적인 과근사이므로 `false`는 얽히지 않은 곱 상태임을 보장하고 `true`는 제어 큐비트가 `|0⟩`인 `cx` 등을 포함해 얽혔을 수 있음만 뜻하며, 같은 큐비트를 두 번 전달하면 `true`를 반환한다.
    /// 큐비트가 다른 회로에 속해 있으면 패닉한다.
    pub fn may_be_entangled(&self, a: &Qubit, b: &Qubit) -> bool {
        self.groups.same(self.own(a), self.own(b))
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
        self.release_clean(i);
        b
    }

    // Returns a slot whose qubit is gone (measured or uncomputed) to the reuse pool.
    fn release_clean(&mut self, slot: usize) {
        self.free.push_back(slot);
        self.groups.detach(slot);
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
        // A reset inside a branch body may not run, so it cannot break any correlation.
        if self.depth == 0 {
            self.groups.detach(i);
        }
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

    fn check_memory(&self) -> Result<(), SimError> {
        let required = state_vector_bytes(self.n_qubits);
        if required > u128::from(self.memory_limit) {
            return Err(SimError::OutOfMemory {
                qubits: self.n_qubits,
                required_bytes: required,
                limit_bytes: self.memory_limit.into(),
            });
        }
        Ok(())
    }

    /// 분기 본문을 포함해 회로가 Clifford 연산(`h`, `s`, `sdg`, `x`, `z`, `cx`, `measure`, `reset`)만 쓰면 `true`를 반환한다.
    pub fn is_clifford(&self) -> bool {
        stabilizer::first_non_clifford(&self.ops).is_none()
    }

    /// `Backend::Auto`가 선택할 백엔드로, Clifford 전용이면 `Stabilizer`, 아니면 `StateVector`를 돌려준다.
    pub fn select_backend(&self) -> Backend {
        if self.is_clifford() {
            Backend::Stabilizer
        } else {
            Backend::StateVector
        }
    }

    /// 이 회로의 상태 벡터 백엔드 최대 할당 메모리를 바꾼다. 한도를 넘으면 `SimError::OutOfMemory`를 돌려준다.
    pub fn set_memory_limit(&mut self, bytes: u64) {
        self.memory_limit = bytes;
    }

    /// 지정한 `backend`로 회로를 한 번 실행한다. 같은 백엔드에서는 `seed`가 같으면 결과가 같지만, 난수 소비 방식이 달라 백엔드 사이에서는 다를 수 있다.
    pub fn simulate_with(&self, backend: Backend, seed: u64) -> Result<Shot, SimError> {
        let backend = match backend {
            Backend::Auto => self.select_backend(),
            b => b,
        };
        let bits = match backend {
            // Auto was resolved to a concrete backend just above.
            Backend::Auto => unreachable!(),
            Backend::StateVector => {
                self.check_memory()?;
                statevector::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
            // The GPU result is read back into a CPU state vector, so the same limit applies.
            #[cfg(feature = "gpu")]
            Backend::Gpu => {
                self.check_memory()?;
                gpu::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
            Backend::Stabilizer => {
                if let Some(gate) = stabilizer::first_non_clifford(&self.ops) {
                    return Err(SimError::NotClifford { gate });
                }
                stabilizer::run(&self.ops, self.n_qubits, self.n_bits, seed)
            }
        };
        Ok(Shot {
            circuit: self.id,
            bits,
        })
    }

    /// 상태 벡터 시뮬레이터로 회로를 한 번 실행한다(한 번의 "샷").
    /// 같은 seed면 같은 결과가 나온다. 큐비트가 n개면 2^n 크기의 벡터를 쓰므로 작은 회로용이다.
    pub fn simulate(&self, seed: u64) -> Shot {
        self.simulate_with(Backend::StateVector, seed)
            .unwrap_or_else(|e| panic!("{e}"))
    }
}

// Shared-reference controls can alias; OpenQASM rejects a gate with a repeated qubit.
fn check_distinct_controls(a: usize, b: usize) {
    assert_ne!(a, b, "ccx의 두 제어 큐비트는 서로 달라야 합니다");
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
            Op::Ccx(a, b, t) => *s += &format!("ccx q[{a}], q[{b}], q[{t}];\n"),
            Op::Measure { qubit, bit } => *s += &format!("c[{bit}] = measure q[{qubit}];\n"),
            Op::Reset(i) => *s += &format!("reset q[{i}];\n"),
            Op::If { bit, value, body } => {
                // Qiskit's importer only accepts `bit == const bool` for single-bit conditions.
                *s += &format!("if (c[{bit}] == {value}) {{\n");
                write_qasm(body, depth + 1, s);
                *s += &pad;
                *s += "}\n";
            }
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
