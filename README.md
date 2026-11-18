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
- 시뮬레이터: 최대 20큐비트 상태 벡터, 측정 후 붕괴, 같은 seed면 같은 결과

앞으로의 계획은 [ROADMAP.md](ROADMAP.md)에 있습니다.

## 라이선스

[MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중 원하는 쪽을 선택해 사용할 수 있습니다.
