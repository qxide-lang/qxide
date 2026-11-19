# Qxide

양자컴퓨터와 일반 컴퓨터에서 함께 쓰는 Rust 닮은 프로그래밍 언어를 목표로 하는 프로젝트입니다. 소스 파일 확장자는 `.qx`입니다.

지금은 첫 단계로, Rust 라이브러리(`qxide` 크레이트) 형태로 핵심 아이디어를 시험하고 있습니다.

## 핵심 아이디어

양자역학의 규칙이 Rust의 소유권 규칙과 그대로 맞아떨어집니다. 그래서 물리적으로 불가능한 코드는 컴파일되지 않습니다.

| 양자역학 규칙 | Qxide에서 표현한 방법 |
| --- | --- |
| 큐비트는 복사할 수 없다 (복제 불가 정리) | `Qubit`에 `Clone`/`Copy` 없음 |
| 측정하면 양자 상태가 사라진다 | `measure(q: Qubit) -> Bit`가 큐비트를 소비 |
| 2큐비트 게이트의 두 큐비트는 서로 달라야 한다 | `cx(&mut a, &mut b)` — 같은 큐비트를 두 번 빌릴 수 없음 |

```rust
use qxide::Circuit;

let mut c = Circuit::new();
let mut a = c.qubit();
let mut b = c.qubit();
c.h(&mut a);           // 중첩
c.cx(&mut a, &mut b);  // 얽힘
let ma = c.measure(a); // a는 사라지고 고전 비트만 남음
let mb = c.measure(b);

println!("{}", c.to_qasm());   // 실제 양자컴퓨터로 보낼 OpenQASM 3
let shot = c.simulate(42);     // 일반 컴퓨터에서 시뮬레이션
assert_eq!(shot.get(ma), shot.get(mb));
```

## 사용하기

```
cargo add qxide
```

이 저장소에서 직접 실행하려면:

```
cargo run --example bell   # 벨 상태: QASM 출력 + 1000번 시뮬레이션
cargo test                 # 시뮬레이터 테스트 + 컴파일 오류가 나야 하는 예제
```

## 지금 지원하는 것

- 게이트: `h`, `x`, `z`, `s`, `t`, `rx`, `ry`, `rz`, `cx`, `measure`, `reset`
- 분기: `c.if_one`·`c.if_zero`로 측정 비트별 실행, OpenQASM 3 `if` 출력, 분기 내 측정 불가
- 재사용: `c.reset`으로 활성 큐비트 |0⟩ 복귀, `measure` 후 `c.qubit()`가 물리 슬롯 초기화·재사용
- 레지스터: `c.qreg`·`measure_reg`, `Shot::value` 정수, 복사 불가·인덱싱·순회·`get_disjoint_mut`
- 출력: OpenQASM 3
- 시뮬레이터: 상태 벡터(모든 게이트·메모리 제한)와 스태빌라이저(Clifford·수천 큐비트), `Backend::Auto` 자동 선택, 같은 seed면 같은 결과
- GPU: CubeCL wgpu·f32 프로토타입(`gpu` 피처·`Backend::Gpu`), 24~26큐비트 게이트 전용 회로에서 단일 스레드 CPU 대비 실측 약 11배 빠름([docs/gpu.md](docs/gpu.md))

앞으로의 계획은 [ROADMAP.md](ROADMAP.md)에 있습니다.

## 큐비트별 상태 벡터 메모리

상태 벡터 백엔드는 16바이트 복소수 2^n개를 저장하므로 큐비트가 하나 늘 때마다 메모리가 두 배가 됩니다. 기본 한도는 1 GiB(26큐비트)이며 `Circuit::set_memory_limit`으로 바꿀 수 있고, `Backend::Auto`를 쓰면 Clifford 전용 회로는 이런 메모리가 필요 없는 스태빌라이저 백엔드에서 자동으로 실행됩니다.

| 큐비트 수 | 필요 메모리 |
| ---: | ---: |
| 10 | 16.0 KiB |
| 20 | 16.0 MiB |
| 24 | 256.0 MiB |
| 26 | 1.0 GiB |
| 28 | 4.0 GiB |
| 29 | 8.0 GiB |
| 30 | 16.0 GiB |
| 32 | 64.0 GiB |
| 34 | 256.0 GiB |
| 40 | 16.0 TiB |
| 50 | 16.0 PiB |

## 다른 양자 언어와의 비교 및 차이점

| 언어 / 도구 | 형태 | 큐비트 복사·폐기 규칙 (검사 시점) | 자동 역계산 | 중간 측정 및 고전 제어 | 주요 출력 / 실행 대상 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Guppy** | Python 내장 언어 | 복제·삭제 불가, 선형성 제약 (컴파일 타임) | 문서에서 확인되지 않음 (`dagger`, `control` 수정자 제공) | 중간 측정 및 고전 제어 흐름 지원 | HUGR IR; Quantinuum Helios/H2 하드웨어, Selene 에뮬레이터 |
| **Silq** | 독립 언어 (타입 체커·컴파일러·실험적 인터프리터) | 고전 `!τ` 값만 복제·폐기 가능, 비물리적 프로그램 거부 (컴파일 타임) | 지원 (`qfree`로 안전한 자동 역계산 가능) | `measure`가 고전 값 반환; 양자 `if` 분기는 `mfree` 필요 | 내장 실험적 시뮬레이터 (하드웨어·IR 출력은 문서화되지 않음) |
| **Q#** | 독립 고수준 오픈소스 언어 | 불투명 식별자(비선형); 해제 시 \|0⟩ 복귀 규칙은 컴파일러 미강제 (시뮬레이터에서 런타임 강제 가능) | 명시적 범위 기반 자동 역연산 (`within { U } apply { V }`) | 타깃 프로파일에 따라 다름 | QIR; Azure Quantum 타깃, QDK 시뮬레이터 |
| **Qiskit** | Python SDK (라이브러리 + 호스트) | 선형/아핀 큐비트 개념 문서화되지 않음 | 없음 | 동적 회로 지원 (`if_test`, `switch`, `for_loop`, `while_loop`) | OpenQASM 3 (`qiskit.qasm3`); runtime primitives를 통한 IBM 하드웨어 |
| **Cirq** | Python 라이브러리 (라이브러리 + 호스트) | 선형 큐비트 개념 없음; 단일 연산 내 큐비트 중복만 런타임 검사 | 문서에서 확인되지 않음 | 측정 키 조건부 고전 제어 (`with_classical_controls`) | OpenQASM 2.0/3.0; 내장 시뮬레이터, `qsim` |
| **qxide** | Rust 라이브러리(크레이트) (독립 `.qx` 문법은 ROADMAP 1단계 계획) | 복사·측정 후 재사용·`cx(&mut a, &mut b)` 중복 컴파일 타임 거부; 묵시적 drop 허용(아핀, 선형 아님) | 지원 (`with_ancilla` 범위 기반, 계산 단계 `x`/`cx`/`ccx`, 제어 큐비트 전용 컴파일·런타임(`RefCell` 등) 검증) | 중간 측정 및 `if_one`/`if_zero` 고전 분기(분기 내부 측정 미지원), `reset` | OpenQASM 3; CPU 상태벡터·안정화 시뮬레이터, GPU 프로토타입 |

큐비트를 복사할 수 없는 값으로 다루는 접근(선형/아핀 타입)은 qxide가 새로 만든 개념이 아니라 Guppy와 Silq에 이미 존재하는 아이디어입니다. 정확히 구분하면 Rust의 소유권 모델은 **아핀(affine) 타입**을 제공합니다. 따라서 qxide의 `Qubit`은 복사할 수 없고 `measure`로 소비된 뒤 재사용할 수 없지만, 측정 없이 스코프를 벗어나 조용히 drop되는 것은 막지 않습니다. 반면 Guppy는 컴파일 타임에 큐비트의 복제와 삭제를 모두 금지하는 선형성 제약을 강제하므로, 이 점에서는 Guppy가 더 엄격합니다.

qxide의 차이점은 다음 세 가지입니다.

- **별도 타입 체커 부재:** 큐비트 소유권 및 중복 참조 검사를 별도의 타입 체커 없이 수정하지 않은 Rust 컴파일러(`rustc`의 소유권·빌림 검사기)로 수행합니다.
- **네이티브 고전 연산:** 고전 제어와 연산이 일반 Rust 코드이므로 네이티브 Rust 속도로 실행됩니다.
- **CPU·GPU·양자 하드웨어를 잇는 단일 언어 지향 (현재 상태):** CPU 상태벡터 및 안정화(stabilizer) 시뮬레이터, GPU 상태벡터 프로토타입(`gpu` feature), 그리고 CI에서 Qiskit 임포터로 파싱을 검증하는 OpenQASM 3 출력까지 구현되어 있습니다. 실제 양자 하드웨어로의 작업 제출은 아직 구현되지 않았습니다(ROADMAP 2단계 계획).

### 참고 출처 (확인일: 2026-11-19)

- **Guppy (Quantinuum):** [GitHub](https://github.com/CQCL/guppylang), [문서 홈](https://docs.quantinuum.com/guppy/), [Ownership 가이드](https://docs.quantinuum.com/guppy/language_guide/ownership.html), [Static 컴파일 가이드](https://docs.quantinuum.com/guppy/language_guide/static.html)
- **Silq (ETH Zurich):** [GitHub](https://github.com/eth-sri/silq), [공식 사이트](https://silq.ethz.ch/), [공식 문서](https://silq.ethz.ch/documentation)
- **Q# (Microsoft):** [개요](https://learn.microsoft.com/en-us/azure/quantum/qsharp-overview), [Quantum Memory Management](https://learn.microsoft.com/en-us/azure/quantum/user-guide/language/statements/quantummemorymanagement), [Conjugations (`within`/`apply`)](https://learn.microsoft.com/en-us/azure/quantum/user-guide/language/expressions/conjugations), [Target Profiles](https://learn.microsoft.com/en-us/azure/quantum/quantum-computing-target-profiles), [QIR 개념](https://learn.microsoft.com/en-us/azure/quantum/concepts-qir)
- **Qiskit (IBM):** [GitHub](https://github.com/Qiskit/qiskit), [Classical Feedforward & Control Flow](https://quantum.cloud.ibm.com/docs/en/guides/classical-feedforward-and-control-flow), [`qiskit.qasm3` API](https://quantum.cloud.ibm.com/docs/en/api/qiskit/qasm3)
- **Cirq (Google):** [GitHub](https://github.com/quantumlib/Cirq), [Classical Control](https://quantumai.google/cirq/build/classical_control), [Interop (OpenQASM)](https://quantumai.google/cirq/build/interop)

## 라이선스

[MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중 원하는 쪽을 선택해 사용할 수 있습니다.
