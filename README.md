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

## 라이선스

[MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중 원하는 쪽을 선택해 사용할 수 있습니다.
