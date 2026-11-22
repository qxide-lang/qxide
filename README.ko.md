<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/qxide-mark-light.svg">
    <img src="https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/qxide-mark.svg" alt="Qxide 로고" width="96" height="96">
  </picture>
</p>

# Qxide

[English](README.md) · [qxide.dev](https://qxide.dev)

양자컴퓨터와 일반 컴퓨터에서 함께 쓰는 프로그래밍 언어를 목표로 하는 프로젝트입니다. 소스 파일 확장자는 `.qx`이며, Java와 C 개발자에게 익숙한 문법(타입을 이름 앞에 적는 `int x = 1;`, `Qubit q = qubit();`, `void main()` 형태)에 Rust 수준의 컴파일 타임 큐비트 안전성을 결합하고, 장기적으로 네이티브 성능을 목표로 합니다.

현재는 `qxide` 명령줄 도구(`run`, `check`, `qasm`)와 Rust 라이브러리 크레이트(`qxide`)를 함께 제공하며, CPU 상태벡터·안정화자 시뮬레이터와 OpenQASM 3 출력을 지원합니다. 0.8.0은 타입이 지정된 IR 엔진과 선택적 Cranelift JIT(`--engine ir|jit`), IBM Quantum 기기 제출(`qxide run --backend ibm:<기기>`), `String`과 `std` 라이브러리, 데스크톱 앱, 언어 서버와 VS Code 확장을 추가했으며 아직 초기 실험 버전입니다. 문법과 API는 앞으로도 바뀔 수 있습니다.

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

## 설치

`qxide` 명령은 세 가지 방법으로 설치할 수 있으며, 어느 방법을 쓰든 같은 바이너리가 설치됩니다.

qxide.dev 단축 URL을 통해 GitHub Releases의 미리 빌드된 바이너리를 설치 스크립트로 받습니다:

```sh
# Linux / macOS
curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"
```

crates.io에서 설치합니다(Rust 툴체인 필요):

```sh
cargo install qxide
```

PyPI에서 설치합니다(wheel에는 Python API가 아니라 동일한 명령줄 바이너리가 들어 있습니다):

```sh
pip install qxide
```

설치가 끝나면 `qxide --version`으로 확인할 수 있습니다. Rust 라이브러리로 쓰려면 `cargo add qxide`로 의존성을 추가합니다. 이 저장소에서 직접 실행하고 테스트하려면 `cargo run -- run examples/qx/bell.qx`와 `cargo test`를 사용합니다. 릴리스 절차는 [docs/release.md](docs/release.md)에 정리되어 있습니다.

## `.qx` 언어 빠른 시작

두 큐비트를 얽힘 상태(Bell 쌍)로 만드는 `.qx` 프로그램입니다. Java처럼 타입을 이름 앞에 적는 문법(`Qubit a = qubit();`)을 쓰며, `examples/qx/` 디렉터리에 `bell.qx`, `ghz.qx`, `teleport.qx` 예제가 있습니다.

```java
// examples/qx/bell.qx
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}
```

```text
$ qxide run examples/qx/bell.qx
seed: 12059043656824605579 (pass --seed 12059043656824605579 to reproduce)
a=0 b=0
$ qxide run examples/qx/bell.qx --shots 1000
seed: 15940593722917789681 (pass --seed 15940593722917789681 to reproduce)
[0, 0]  0b00 = 0  ×521
  a=0 b=0
[1, 1]  0b11 = 3  ×479
  a=1 b=1
$ qxide check examples/qx/ghz.qx
examples/qx/ghz.qx: ok
$ qxide qasm examples/qx/bell.qx
OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];
```

- `qxide run <FILE> [--shots N] [--seed S]`: 1회 실행 시 프로그램 출력을 그대로 보여 주며, `N > 1`이면 측정 기록별로 한 줄씩 배열(`c[0]`이 먼저), 이진수(상위 비트가 먼저), 정수(`c[0]`이 최하위 비트)와 횟수를 출력하고 그 아래에 해당 결과의 프로그램 출력을 이어서 표시합니다. `--seed`를 주지 않으면 실행할 때마다 무작위 시드를 쓰고 실제로 쓴 시드를 `stderr`에 `seed: S (pass --seed S to reproduce)` 한 줄로 알리며(측정이 없는 프로그램 등 측정 결과에 따라 출력이 달라지지 않으면 시드를 알리지 않음), `--seed S`를 주면 시드 줄은 출력하지 않고 같은 결과를 재현합니다.
- `qxide check <FILE>`: 실행하지 않고 이미 측정한 큐비트를 다시 쓰거나 측정하지 않은 큐비트 같은 오류를 줄·열 번호와 함께 보고합니다.
- `qxide qasm <FILE>`: OpenQASM 3 회로를 출력합니다.
- `qxide fmt <FILE> [--check]`: 소스 파일을 서식에 맞춰 제자리에서 다시 씁니다. `--check`를 지정하면 파일을 수정하지 않고 변경 필요 여부만 확인합니다.
- `qxide repl [--seed S]`: 변수, 함수, 큐비트 상태를 유지하는 대화형 세션을 시작합니다. 측정에는 시드 `S`를 사용하며, `--seed`가 없으면 무작위 시드를 쓰고 측정한 비트를 처음 출력할 때 그 시드를 `stderr`에 한 번 알립니다.
- `qxide explain <CODE>`: 영문 카탈로그 [docs/error-codes.md](docs/error-codes.md)에서 해당 오류 코드의 발생 원인, 잘못된 예시, 수정 방법을 찾아 출력합니다.
- `qxide new <NAME>`: `NAME` 패키지 프로젝트를 만들고 `qxide.toml`(폴더 이름에서 생성한 패키지 이름, 버전 `0.1.0`, 필요한 `qxide` 버전), 벨 쌍 예제 `src/main.qx`, 짧은 `README.md`, `target/`을 무시하는 `.gitignore`를 생성합니다. 프로젝트 폴더 안에서는 파일 없이 `qxide run`을 실행하면 `src/main.qx`를 실행합니다.

언어 사양과 문법의 자세한 내용은 [docs/design/qx-language.md](docs/design/qx-language.md)에서 확인할 수 있습니다.

## 지금 지원하는 것

- `.qx` 언어: Java식 선언(`int`, `double`, `boolean`, `Qubit`, `QReg`, `Bit`, `BitReg`, `final`, `var`), `(int)` 캐스트, `PI` 상수, 큐비트 함수(기본 대여·`consume` 매개변수, `Qubit`/`QReg` 반환, 측정 결과 `Bit` 조건 `if` 안의 게이트 전용 함수 호출), 선형 큐비트 검사, `qxide run`/`check`/`qasm`
- 명령줄 도구: `qxide run`/`check`/`qasm` 및 `fmt`(서식 정리), `repl`(대화형 세션), `explain`(오류 코드 설명), `new`(새 프로젝트 생성)
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
| **qxide** | Rust 라이브러리(크레이트) (독립 `.qx`: Java식 문법, 자체 선형 큐비트 검사, `qxide run`/`check`/`qasm`) | 복사·측정 후 재사용·`cx(&mut a, &mut b)` 중복 컴파일 타임 거부; 묵시적 drop 허용(아핀, 선형 아님) | 지원 (`with_ancilla` 범위 기반, 계산 단계 `x`/`cx`/`ccx`, 제어 큐비트 전용 컴파일·런타임(`RefCell` 등) 검증) | 중간 측정 및 `if_one`/`if_zero` 고전 분기(분기 내부 측정 미지원), `reset` | OpenQASM 3; CPU 상태벡터·안정화 시뮬레이터, GPU 프로토타입 |

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
