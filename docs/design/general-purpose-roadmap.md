# Qxide 범용 언어 갭 분석과 로드맵

## 1. 목적과 범위

2026-11-22 사용자 요청에 따라 Qxide를 Rust, C, Java처럼 다양한 영역에서 사용할 수 있는 범용 언어로 확장하기 위한 갭 분석과 단계별 로드맵을 정리한다.

다음 제약은 고정된 전제로 유지한다.

- Java/C 스타일 문법: 이름 앞에 타입을 표기한다(`int x = 1;`, `Qubit q = qubit();`, 함수 이름 앞 반환 타입).
- 컴파일러의 Rust 스타일 안전성 검사 및 Rust 수준 성능 목표.
- 영문 컴파일러 진단 메시지.
- 큐비트 규칙: 측정하거나 폐기하지 않은 큐비트는 오류이며, 소유권 이동은 `consume`으로만 일어난다.
- `var`, `(int) x` 캐스트, CLI 명령(`qxide run`, `qxide check`, `qxide qasm`).
- 정체성: 고전과 양자를 아우르는 단일 언어. 고전 기능 추가가 기존 양자 기능을 깨뜨려서는 안 된다.
- 장기 목표: 우주 시뮬레이션 같은 대규모 계산 수행.

아래 모든 항목은 커밋 `3580518` (v0.8.0) 기준 코드로 확인했으며, 각 항목의 근거는 파일 경로로 명시한다.

## 2. 현재 실행 모델

현재 실행 모델이 언어 전반의 제약과 확장 방향을 결정하므로 갭 분석에 앞서 이를 정리한다.

- **Staged 실행 모델**: `.qx` 프로그램은 단계적으로 실행된다(`docs/design/qx-language.md` §5). 고전 코드(`int`, `double`, `boolean`, `String`)는 빌드 타임에 elaborator 내부에서 `main`을 실행하며 단 한 번 수행되어 하나의 `qxide::Circuit`을 기록한다. `Bit`와 `BitReg`는 시뮬레이션 단계에서 채워지는 shot-time 값이다.
- **버퍼링된 출력**: `println`은 stdout에 즉시 쓰지 않고 출력 템플릿에 덧붙인다(`src/lang/machine.rs` `Machine::print`). 이 템플릿은 elaboration과 시뮬레이션이 모두 끝난 뒤 shot당 한 번 렌더링된다. 따라서 장시간 고전 계산 중에는 종료 전까지 출력이 표시되지 않으며 스트리밍 출력이 불가능하다.
- **리소스 제한**: `src/lang/mod.rs`의 `Limits::CLI`는 인터프리터 스텝 2^30, 회로 연산 2^24, 출력 256 MiB 등의 상한을 적용한다(`docs/design/qx-language.md` §18). 이를 상향하는 CLI 플래그는 없다. 신뢰할 수 없는 파일로부터 보호하는 보안 장치이지만, 약 10^9 스텝 규모의 정당한 대규모 계산도 함께 가로막는다.
- **세 가지 elaborator**: `src/lang/engine.rs`는 동일한 결과를 내는 세 엔진을 제공한다.
  - `ast`: 트리 순회 인터프리터(기본값).
  - `ir`: typed-IR 인터프리터(`src/lang/ir/`).
  - `jit`: IR + Cranelift 네이티브 코드(Cargo feature `jit`, 기본 비활성, `src/lang/jit/`).
  - `cargo test`에서는 `dual` 엔진이 기본값으로 동작하며 모든 엔진을 실행해 결과를 비교한다.
- **JIT 지원 범위**: `src/lang/jit/mod.rs`의 `supported` 조건에 따라, 모든 값이 `int`/`double`/`boolean`이고 동일 조건의 함수만 호출하는 함수만 JIT 컴파일된다. 문자열, `std` 호출, 양자 연산은 지원하지 않는다.

`docs/perf/QX_BENCHMARKS.md`에 기록된 실측치(최소 elaboration 시간, ms)는 다음과 같다.

| 벤치마크 | `ast` (ms) | `ir` (ms) | `jit` (ms) |
| --- | ---: | ---: | ---: |
| `arith` | 813.2 | 542.9 | 83.7 |
| `recursion` | 179.1 | 56.9 | 5.4 |

## 3. 갭 표

### 3.1 언어 핵심

| 항목 | 현재 Qxide (근거) | Java / C / Rust | 갭 | 우선순위 |
| --- | --- | --- | --- | --- |
| 원시 타입 | `int` (64-bit, overflow는 오류), `double`, `boolean`, `String` (immutable), 양자 `Qubit`/`QReg`, shot-time `Bit`/`BitReg` (`src/lang/ast.rs` `Ty`) | 고정 폭 정수(`i8`..`u64` 등), `char`, 바이트 타입 지원 | 고정 폭 정수(`i8`..`u64`), `char`, 바이트 부재 | P2 |
| 제어 흐름 | `if`/`else`, `for i in a..b`, `return` (`src/lang/ast.rs` `Stmt`). `while`, `loop`, `break`, `continue`는 예약어이나 거부됨 (`src/lang/lexer.rs` `RESERVED`). 복합 대입(`+=`), `++` 없음 | 조건 루프, `break`/`continue`, 복합 대입 등 지원 | `while`, `loop`, `break`, `continue`, 복합 대입(`+=`), `++` 부재 | P0 |
| 배열 / 리스트 | 고전 값용 없음. `QReg`/`BitReg` 레지스터만 존재 | 고전 타입 배열 및 가변 리스트 지원 | 고전 값용 배열/리스트 부재 | P0 |
| 구조체 / 레코드 | 없음 (`struct` 예약어) | 사용자 정의 구조체/레코드 지원 | 구조체/레코드 부재 | P0 |
| 열거형 + 패턴 매칭 | 없음 (`enum`, `match` 예약어) | 열거형 및 패턴 매칭/분기 지원 | `enum`, `match` 부재 | P1 |
| 제네릭 | 없음 | 제네릭 타입 및 함수 지원 (Java, Rust) | 제네릭 부재 (내장 제네릭 우선, 사용자 제네릭은 후순위) | P2 |
| 인터페이스 / 트레이트 | 없음 (`impl` 예약어) | 인터페이스(Java) / trait(Rust) 지원 | 인터페이스/트레이트 부재 | P2 |
| 클로저 / 함수 값 | 없음 | 함수 포인터(C), 람다(Java), 클로저(Rust) 지원 | 클로저 및 함수 값 부재 | P2 |
| 모듈 | 지원 — `import pkg.module.fn;`, `public`, `qxide.toml` 기반 패키지 (`docs/design/qx-packages.md`, `src/lang/link.rs`) | 함수와 사용자 정의 타입을 모듈 단위로 export | export할 사용자 정의 타입이 아직 없음 | - |
| 오류 처리 | 런타임 오류는 치명적 진단(`Q04xx`), 실패하는 `std` 호출은 프로그램 중단(`Q0704` 등). 복구 가능한 오류 값이나 예외 없음 | 예외(Java), 오류 반환(C), `Result`(Rust)로 복구 가능한 오류 처리 | 복구 가능한 오류 값 및 처리 체계 부재 | P1 |
| 소유권 / 빌림 | 선형 큐비트는 정적 검사(`Q03xx`, `src/lang/check.rs`). 고전 값은 복사됨. 고전 힙 데이터가 없어 관련 소유권 모델 미정의 | GC 참조(Java), 수동 메모리 관리(C), 소유권/빌림(Rust) | 배열/구조체 도입 시 고전 힙 데이터의 소유권 모델 결정 필요 | 결정 필요 |
| 상수 / 전역 변수 | `final` 지역 변수, 내장 `PI`만 존재. 전역 변수 및 사용자 상수 없음 (`const` 예약어) | 전역 상수 및 정적 변수 지원 | 전역 변수 및 사용자 상수 부재 | P2 |

### 3.2 표준 라이브러리

`std` 모듈은 `src/lang/stdlib.rs`의 `MODULES`에 `.qx` 스텁으로 정의되며 `import std.fs;` 후 `fs.read_text(p)` 형태로 호출한다.

| 항목 | 현재 Qxide (근거) | Java / C / Rust | 갭 | 우선순위 |
| --- | --- | --- | --- | --- |
| 수학 | `+`, `-`, `*`, `/`, `%`, 캐스트, `PI`만 존재. `sqrt`, `sin`, `exp`, `abs`, `min`, `pow` 없음 (`src/lang/check.rs` `BUILTINS`) | 표준 수학 함수 제공 | 기본 수학 함수 부재 (과학 계산 활용 제약) | P0 |
| 문자열 | `+` 연결, `==`/`!=`, `len` (Unicode 문자 수) | 부분 문자열, 탐색, 분할, 치환, 숫자 파싱, 문자 접근 제공 | substring, find, split, replace, parse-int/double, 문자 접근 부재 | P0 |
| 컬렉션 | 없음 (list, map, set 없음) | list, map, set 등 표준 컬렉션 제공 | list, map, set 부재 | P1 |
| 파일 시스템 | `std.fs`의 `read_text`/`write_text`/`append_text`/`exists`/`remove`. `data/` 루트로 제한되며 `qxide run --allow fs` 필요 (`src/lang/stdlib.rs` `confine`, `Permissions::cli`) | 디렉터리 목록, 바이너리 파일, 임의 경로 접근 지원 | 디렉터리 목록 조회, 바이너리 파일, 임의 경로 접근 부재 | P1 |
| 경로 | 없음 | 경로 조작 API 제공 | 경로 모듈 부재 | P1 |
| 프로세스 / 환경 | 커맨드라인 인자, 환경 변수, stdin, 종료 코드, 서브프로세스 없음 | 인자, 환경 변수, stdin, 종료 코드, 서브프로세스 지원 | CLI 도구 작성에 필요한 프로세스/환경 API 전반 부재 | P1 |
| 시간 | `std.time`의 `now_millis`/`now_iso`/`sleep_millis` | wall clock과 단조 시계(monotonic clock) 제공 | 벤치마크용 단조 시계 부재 | P2 |
| 난수 | 고전 PRNG 없음 (무작위성은 양자 측정으로만 가능) | 고전 PRNG 제공 | 시뮬레이션용 고전 PRNG 부재 | P1 |
| JSON | 언어 차원 지원 없음 (앱 IPC는 내부적으로 JSON 사용) | JSON 파싱/직렬화 지원 | 언어 내 JSON 모듈 부재 | P1 |
| 정규표현식 | 없음 | 정규표현식 매칭 지원 | 정규표현식 부재 | P3 |
| 네트워크 / HTTP | `std.http.get`만 데스크톱 앱에서 `[app] http_hosts` allow-list와 함께 HTTPS 전용 지원 (`docs/design/qx-app.md` §6). 소켓 및 서버 없음 | 소켓 및 HTTP 클라이언트/서버 지원 | 소켓 및 서버 기능 부재 | P2 |
| 스레드 / 동시성 / 비동기 | 언어 차원 지원 없음 (데스크톱 런타임은 명령 처리를 위해 내부적으로 4개 워커 스레드 사용, 미노출) | 스레드, 병렬 처리, 비동기 실행 지원 | 언어 수준 병렬성/스레드 부재 (시뮬레이션용 데이터 병렬 루프 우선 P2, async P3) | P2 (async: P3) |

### 3.3 실행 방식

| 항목 | 현재 Qxide (근거) | Java / C / Rust | 갭 | 우선순위 |
| --- | --- | --- | --- | --- |
| 인터프리터 / JIT | `ast`(기본), `ir`, `jit`(Cargo feature `jit`) — 2절 참조 | JIT(Java) 또는 네이티브 컴파일(C, Rust) | JIT 기본 비활성 및 지원 범위 제한 | - |
| AOT 네이티브 실행 파일 | 없음. `qxide app build`는 데스크톱 앱용 사전 빌드 런타임 실행 파일에 번들을 덧붙이나(`docs/design/qx-app.md`), 웹뷰 앱이지 CLI 실행 파일이 아님 | 독립 네이티브 실행 파일 생성 (C, Rust) | 독립 CLI 네이티브 실행 파일 빌드 부재 | P1 |
| WASM | 컴파일러와 인터프리터가 브라우저 플레이그라운드에서 동작(`crates/qxide-wasm`가 `src/lang` 포함). 프로그램을 WASM으로 컴파일 불가 | WASM 타깃 컴파일 지원 (C, Rust) | `.qx` 프로그램의 WASM 컴파일 부재 | P2 |
| 크로스 컴파일 | 아직 해당 없음 (파일 대상 코드 생성 없음) | 타깃 지정 크로스 컴파일 지원 (C, Rust) | 크로스 컴파일 미지원 | P3 |

### 3.4 상호 운용

| 항목 | 현재 Qxide (근거) | Java / C / Rust | 갭 | 우선순위 |
| --- | --- | --- | --- | --- |
| C FFI / C ABI | 없음 (`extern` 함수는 `ROADMAP.md` stage 3에서 셀프 호스팅 선행 조건으로 명시됨) | C ABI 호출 및 네이티브 라이브러리 연동 지원 | C FFI / C ABI 부재 | P1 |
| Rust | `qxide` 크레이트는 회로/시뮬레이터 라이브러리(`src/lib.rs`)를 공개하나 `.qx` 프런트엔드(`src/lang`)는 바이너리 전용 비공개 모듈(`src/main.rs` `mod lang;`)이라 Rust에서 `.qx` 임베드 불가 | 크레이트 단위 라이브러리 임베드 가능 (Rust) | `.qx` 프런트엔드의 Rust 임베딩 API 부재 | P2 |
| Python | PyPI에 CLI 바이너리만 배포(`pyproject.toml` `bindings = "bin"`). Python API 없음 | 네이티브 바인딩을 통한 Python API 제공 가능 (C, Rust) | Python API 바인딩 부재 | P2 |
| 양자 백엔드 | OpenQASM 3 내보내기 및 IBM Quantum 제출 지원 (`qxide qasm`, `qxide run --backend ibm:<device>`, `docs/design/qx-qpu.md`) | 언어 내장 양자 백엔드 없음 | 없음 (기존 강점 유지) | - |

### 3.5 도구

| 항목 | 현재 Qxide (근거) | Java / C / Rust | 갭 | 우선순위 |
| --- | --- | --- | --- | --- |
| 패키지 매니저 | 지원 (`qxide add/remove/install/publish`, 레지스트리 `qxide-lang/registry`) | 패키지 매니저 제공 | 기본 기능 구비 | - |
| LSP 및 에디터 | 지원 (`qxide lsp`, `docs/design/lsp.md`) 및 VS Code 확장(`editors/vscode`) | LSP 및 IDE 지원 | 신규 문법/타입 반영 지속 필요 | - |
| 포매터 / REPL / 오류 설명 | 지원 (`qxide fmt`, `qxide repl`, `qxide explain`) | 포매터, REPL, 진단 도구 제공 | 기본 기능 구비 | - |
| 테스트 러너 | 없음 (`qxide test`, 테스트 함수 없음) | 테스트 프레임워크/러너 지원 | `qxide test` 및 테스트 함수 부재 | P1 |
| 문서 생성기 | 없음 | API 문서 생성기 지원 | 문서 생성기 부재 | P2 |
| 디버거 | 없음 (중단점, DAP 없음) | 디버거 및 DAP 지원 | 디버거 및 DAP 부재 | P3 |
| 벤치마크 | 내부 스위트만 존재(`bench/qx`, `src/lang/bench.rs`, ignored test). 사용자용 벤치마크 커맨드 없음 | 사용자용 벤치마크 실행 지원 | 사용자용 벤치마크 커맨드 부재 | P2 |

### 3.6 쓰임새별 평가

| 쓰임새 | 지금 가능한가 | 막는 갭 |
| --- | --- | --- |
| CLI 도구 | 불가 | 커맨드라인 인자, stdin, 종료 코드, 스트리밍 출력 부재 |
| 서버 | 불가 | 소켓 부재, I/O를 동반한 장기 실행 루프 불가, 스텝 제한 |
| 데스크톱 앱 | 가능 (앱 프레임워크 경유) | HTML/JS UI + `.qx` 명령 구조로 동작 (`docs/design/qx-app.md`) |
| 웹 (WASM) | 플레이그라운드만 가능 | 브라우저 플레이그라운드 실행만 지원하며 `.qx`의 WASM 컴파일 불가 |
| 과학 계산 / 시뮬레이션 | 부분 가능 | 순수 숫자 함수는 JIT 가능하나 배열, 수학 함수, `while`, 병렬성 부재 및 스텝 제한 |
| 임베디드 | 장기 과제 | AOT 컴파일 및 런타임 할당 비의존 구조 필요 |

## 4. 설계 결정

아래 항목은 권장안이며 사용자가 변경할 수 있다. 추후 되돌리기 어려운 결정은 `[되돌리기 어려움]`으로 표시한다. 각 결정의 구현 현황은 6절을 참조한다.

- **D1 루프와 복합 대입**: `while cond { ... }`, `break;`, `continue;`를 기존 `if`/`for`와 동일한 괄호 없는 헤더 스타일로 도입한다(`(cond)`는 식이므로 괄호 사용도 허용된다). 변수에 대해 복합 대입 `+=`, `-=`, `*=`, `/=`, `%=`를 추가한다. `++`/`--`는 문(statement)과 식(expression) 간 모호성 때문에 당장 넣지 않으며 추후 추가할 수 있다. `loop`와 `do-while`은 추가하지 않는다.
- **D2 오류 처리 `[되돌리기 어려움]`**: 예외(exception)가 아니라 `Result` 스타일 값과 명시적 전파를 채택한다. 예외는 살아 있는 큐비트를 보유한 프레임을 되감기(unwind)하므로 선형 큐비트 검사기가 모든 throw 경로를 추적해야 한다. 반면 명시적 조기 반환은 이미 검사되고 있으며(살아 있는 큐비트가 있을 때 `return` 시 `Q0302`), Rust 스타일의 명시적 오류 모델이면서도 Java/C 사용자에게 읽기 쉽다. 계획된 표면은 내장 제네릭 `Result<T, E>`, `Ok(v)`/`Err(e)`, `?` 스타일 전파 연산자이며 정확한 문법은 `enum`/`match`와 함께 확정한다.
- **D3 복합 데이터 범위**: 1단계에서는 빌드 타임 고전 값만 허용한다. 배열과 구조체 안에 `Qubit`/`Bit`를 넣지 않으며 `QReg`/`BitReg`가 양자 컬렉션 역할을 유지한다. 이로써 기존 큐비트 검사기를 변경하지 않는다.
- **D4 순수 수학 및 문자열 함수 배치**: 새 전역 내장 함수가 아니라 `std` 모듈(`import std.math;` 후 `math.sqrt(x)`)에 둔다. 사용자 함수는 내장 함수 이름을 재사용할 수 없으므로(`src/lang/check.rs` `signatures`, `Q0217`) 새 전역 내장 함수를 추가하면 기존 프로그램이 깨진다. 이 함수들은 순수 함수로 권한이 필요 없으며 모든 호스트에서 허용된다.
- **D5 배열 `[되돌리기 어려움]`**: Java 문법을 따른다(`int[] a = new int[n];`, `new int[]{1, 2, 3}`, 선언 초기화 `int[] a = {1, 2, 3};`, 원소 읽기/쓰기 `a[i]`, `a[i] = v;`, 길이 `len(a)`). 권장안은 Java와 같은 참조 의미론(reference semantics)이며, 대안은 C/Rust와 같은 값 의미론(value semantics)이다.
- **D6 구조체 `[되돌리기 어려움]`**: 이미 예약어인 C 키워드 `struct`로 선언하고(`struct Point { double x; double y; }`), Java 스타일의 `new Point(1.0, 2.0)`으로 생성하며, `p.x`, `p.x = 2.0;`으로 필드에 접근한다. `null`은 도입하지 않는다.
- **D7 신뢰할 수 있는 로컬 코드의 리소스 제한 `[보안 관련 결정]`**: `Limits::CLI` 기본값은 유지한다. 이를 상향하는 opt-in 플래그 추가는 보안에 영향을 미치는 사용자 결정 사항으로 둔다.

## 5. 단계별 로드맵

### 공통 테스트 기준

모든 단계는 다음 기준을 공통으로 충족해야 한다.

- `cargo test --workspace` 통과.
- `ast`/`ir`/`jit` dual-engine 동등성 테스트가 신규 기능을 커버.
- 기존 예제와 WASM 빌드(`crates/qxide-wasm`) 정상 동작 유지.
- 성능 관련 주장은 실측 수치로 뒷받침될 때만 기재.

### Stage 1 — 언어 핵심과 순수 라이브러리

이 브랜치에서 순차적으로 구현을 시작한다.

- **1a `while`/`break`/`continue` + 복합 대입**
  - 내용: `while`, `break`, `continue`와 복합 대입 연산자(`+=`, `-=`, `*=`, `/=`, `%=`) 추가.
  - 완료 기준: 세 엔진(`ast`, `ir`, `jit`) 모두 지원. 검사기가 루프 탈출 시 큐비트 규칙을 강제(`break`/`continue`를 통한 큐비트 누수 금지, `for`와 동일하게 루프 본문에서 외부 큐비트 소비 금지 `Q0305`, 측정된 `Bit` 조건의 `if`를 `break`/`continue`로 벗어나는 것 금지). 포매터, LSP, VS Code 문법이 새 키워드를 인식.
  - 테스트 기준: 파서/검사기 단위 테스트, dual-engine 실행 테스트, 스텝 예산 동등성 검증, 예제 프로그램.
- **1b `std.math` 및 문자열 함수 (순수 함수, 모든 호스트)**
  - 내용: 순수 수학 함수와 문자열 조작 함수를 `std` 모듈로 구현.
  - 완료 기준: 모든 엔진에서 동일한 결과와 스텝 과금으로 동작. 권한 없이 모든 호스트에서 사용 가능.
  - 테스트 기준: NaN, 음수 `sqrt`, 범위 밖 substring의 진단 오류 등 경계 사례를 포함한 dual-engine 테스트.
- **1c 고전 타입 배열**
  - 내용: D3 및 D5에 따른 고전 타입 배열 구현.
  - 완료 기준: 배열 할당 시 스텝 예산을 차감(`new double[huge]` 등에서 OOM 중단 방지), 범위 밖 인덱스는 진단 오류로 처리, 모든 엔진 결과 일치.
  - 테스트 기준: dual-engine 테스트, 리소스 제한 테스트.
- **1d 구조체 (`struct`)**
  - 내용: D3 및 D6에 따른 고전 필드 구조체 선언, 생성, 필드 접근 구현.
  - 완료 기준: 모든 엔진에서 동일 동작, 타입 검사 및 진단 지원.
  - 테스트 기준: 파서/검사기 단위 테스트, dual-engine 테스트.
- **1e 열거형(`enum`) + `match`**
  - 내용: `enum` 선언 및 `match` 패턴 매칭 구현.
  - 완료 기준: 모든 엔진에서 동일 동작, 정적 검사 지원.
  - 테스트 기준: 파서/검사기 단위 테스트, dual-engine 테스트.
- **1f `Result<T, E>` 오류 처리 (D2)**
  - 내용: D2에 따른 `Result<T, E>`, `Ok`/`Err`, 전파 연산자 구현.
  - 완료 기준: 모든 엔진에서 동일 동작, 오류 전파 경로에서 선형 큐비트 규칙 준수.
  - 테스트 기준: 파서/검사기(큐비트 안전성 포함) 단위 테스트, dual-engine 테스트.

### Stage 2 — 표준 라이브러리 기초

- 내용: 프로세스 인자/환경 변수/종료 코드/stdin, 고전 전용 프로그램의 스트리밍 stdout, 경로(path), `std.fs` 확장, 단조 시계(time monotonic), 시드 지정 PRNG(random), 컬렉션(`List`/`Map`), JSON.
- 완료 기준: 인자로 받은 파일의 단어 수를 세는 등의 CLI 도구를 `.qx`로 작성할 수 있다.
- 테스트 기준: 모듈별 CLI 통합 테스트, 호스트별 권한 테스트.

### Stage 3 — 실행 방식

- 내용: 독립 실행 파일을 생성하는 `qxide build`(1차는 인터프리터 + 프로그램 번들링 방식임을 명시하고, 이후 고전 코드용 `cranelift-object` 기반 Cranelift AOT로 확장), 배열/수학에 대한 JIT 커버리지 확대, lowering 캐싱.
- 완료 기준: `qxide build app.qx -o app.exe`로 생성한 프로그램이 `qxide` CLI 없이 실행된다.
- 테스트 기준: Windows/Linux/macOS CI에서 빌드 및 실행 테스트.

### Stage 4 — 상호 운용

- 내용: 최소 C FFI(C ABI의 `extern` 함수, `int`/`double`만 지원, 명시적 권한 뒤에서 허용), `lang`의 공개 Rust 임베딩 API, Python 바인딩.
- 완료 기준: `.qx` 프로그램이 공유 라이브러리의 C 함수를 호출할 수 있다.
- 테스트 기준: CI에서 소규모 C 라이브러리를 빌드해 호출하는 FFI 테스트.

### Stage 5 — 도구

- 내용: `qxide test`(테스트 함수), `qxide doc`, 사용자용 벤치마크 커맨드, 추후 디버거(DAP).
- 완료 기준: 각 CLI 도구 명령이 `.qx` 프로젝트에서 동작한다(DAP는 후속).
- 테스트 기준: CLI 통합 테스트 및 공통 테스트 기준 충족.

### Stage 6 — 대규모 계산

- 내용: 공유 데이터 소유권 모델을 갖춘 데이터 병렬 루프/스레드, SIMD, 고전 GPU 커널, 신뢰할 수 있는 코드의 리소스 제한 상향(D7).
- 완료 기준: N-body 시뮬레이션 예제가 멀티 코어에서 실측 속도 향상을 보이며 확장된다.
- 테스트 기준: 병렬 실행 정확성 테스트 및 멀티 코어 실측 벤치마크.

## 6. 진행 현황

각 단계가 구현되어 반영될 때마다 아래 표를 갱신한다.

| 단계 | 상태 |
| --- | --- |
| 1a~1f | 진행 예정 (이 문서와 같은 브랜치 `feat/general-purpose`에서 순서대로 구현) |
| Stage 2 | 미착수 |
| Stage 3 | 미착수 |
| Stage 4 | 미착수 |
| Stage 5 | 미착수 |
| Stage 6 | 미착수 |

## 7. 사용자 결정이 필요한 항목

- **D1 (루프와 복합 대입)**: 괄호 없는 `while` 헤더, `break`/`continue`, 복합 대입 채택 및 `loop`/`do-while` 미도입 여부.
- **`++`/`--` 연산자**: 문/식 모호성으로 인해 당장 제외할지, 지금 추가할지 여부.
- **D2 (오류 처리)**: 예외 대신 `Result<T, E>`와 명시적 전파 방식을 채택할지 여부.
- **D5 (배열 의미론)**: 참조 의미론(Java 스타일)과 값 의미론(C/Rust 스타일) 중 선택.
- **배열 길이 접근**: `len(a)` 외에 Java 스타일의 `a.length`도 함께 허용할지 여부.
- **D6 (`struct`와 `new` 키워드)**: `new`를 새 키워드로 도입할지 여부(`new`를 식별자로 사용하는 기존 프로그램은 깨진다).
- **D7 (리소스 제한 상향 플래그)**: `Limits::CLI` 기본값을 유지하면서 신뢰할 수 있는 로컬 코드용 opt-in 상향 플래그를 추가할지 여부.
