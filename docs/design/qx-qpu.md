# 양자 컴퓨터 서비스 제출 (`qxide run --backend`)

## 상태

2026-11-22 기준 `feat/cranelift-jit` 브랜치 위에 분기한 `feat/qpu-submit` 브랜치에 구현되었다. `ROADMAP.md`의 "양자 부분은 OpenQASM 3로 변환해 실제 양자컴퓨터 서비스에 제출" 항목에 해당한다.

개발 중 사용할 수 있는 계정과 토큰이 없어 실제 IBM 서비스로 요청을 전송한 적은 없다. 모든 네트워크 테스트는 `127.0.0.1`의 가짜 서버를 상대로 실행했다. 현재까지 Windows x86-64 환경에서만 빌드하고 테스트했다.

## OpenQASM 3 출력 점검

`qxide qasm`과 이번 `qxide run --backend`가 공통으로 사용하는 `Circuit::to_qasm`은 다음 형식을 출력한다.

- 헤더 및 선언: `OPENQASM 3.0;`, `include "stdgates.inc";`, `qubit[N] q;`, `bit[M] c;` (`M`이 0이면 `bit` 선언 생략)
- 게이트: `h`, `x`, `z`, `s`, `sdg`, `t`, `tdg`, `rx`, `ry`, `rz`, `cx`, `ccx` (모두 `stdgates.inc`에 정의됨)
- 측정·초기화·조건 분기: `c[k] = measure q[i];`, `reset q[i];`, `if (c[k] == true|false) { ... }`
- 회전 각도: Rust `f64`의 `{:?}` 형식으로 출력한다. 원래 값으로 정확히 왕복 변환되며 명세의 부동소수점 리터럴 형식(`5e-324`, `1e300`, `1.2345678901234569e23` 등)에 항상 부합한다. 음수는 리터럴에 단항 `-`를 붙인 형태다.

OpenQASM 3 명세와 함께 배포되는 참조 파서(`openqasm3[parser]` 1.0.2)와 `qiskit.qasm3.loads`(Qiskit 2.6.2)로 점검한 결과, 총 17개 파일(`examples/qasm_dump.rs`의 10개 예제 회로와 `examples/qx`의 7개 프로그램)이 모두 통과했다. 이에 더해 로컬에서 재사용 큐비트 위의 `if`/`else`, 부호 있는 0, 비정규화(subnormal) 각도를 포함한 경계 사례 프로그램도 추가로 검증했다.

`openqasm.com`의 명세에서 확인한 사항은 다음과 같다.

- 레지스터 크기는 컴파일 시점 상수인 음이 아닌 정수이므로 빈 회로에 대한 `qubit[0] q;`는 허용된다.
- 스칼라 `bit`와 `bool`은 식 안에서 상호 호환되며 비트 값 식을 `if` 조건으로 쓸 수 있으므로 `c[k] == true`는 명세 규칙에 부합한다(명세 예제는 `if (c)`를 직접 사용한다).

출력기 자체는 변경하지 않기로 결정했다. `if (!c[k])` 형태는 Qiskit의 실험적 Rust 임포터(`loads_experimental`)가 단항 `!`에서 패닉을 일으키므로 채택하지 않았다. 또한 해당 임포터는 `if`와 `reset`을 아직 지원하지 않는 임포터 측 제약이 있어 CI에서는 사용하지 않는다.

CI의 기존 `qasm-interop` 작업은 이제 내보낸 모든 파일을 참조 파서로도 검사하도록 확장했으며(`ci/check_qasm.py`), `examples/qasm_dump.rs`에는 `extreme_angles`와 `empty` 회로를 추가했다.

## 구조

바이너리 전용 모듈 `src/qpu/`를 새로 추가했다. wasm 플레이그라운드와 데스크톱 앱 런타임은 이 모듈을 컴파일하지 않으며, HTTP·JSON 의존성도 추가되지 않아 wasm 빌드 크기는 약 418 KB로 변동이 없다.

- `src/qpu/mod.rs`: `dry_run`과 `run`을 정의하는 `Provider` 트레이트, `Job`(OpenQASM 3, 샷 수, `c` 레지스터 너비), `Settings`, 진행 상태 출력 및 확인을 담당하는 `Ui` 트레이트와 `TerminalUi`
- `src/qpu/ibm.rs`: IBM Quantum 제공자 구현
- `src/qpu/isa.rs`: ISA 변환기 (단위 테스트는 `src/qpu/isa_tests.rs`)
- `src/qpu/http.rs`: 시스템 `curl` 호출 계층
- `src/qpu/keychain.rs`: OS 자격 증명 저장소 조회
- `src/qpu/results.rs`: 실행 결과 복호화
- `src/qpu/msg.rs`: 사용자 안내 및 오류 메시지

새 제공자를 추가하려면 `Provider` 트레이트를 구현하고 `qpu::provider`에 접두사를 등록하면 된다(현재는 `ibm:`만 지원한다).

`src/lang`에는 `export_input`과 `Exported`를 추가했다. `qxide qasm`과 마찬가지로 프로그램을 한 번 전개하며(부수 효과 거부, `std.log` 폐기) 출력 템플릿을 보존한다. `Exported::render(records)`는 장치에서 받은 측정 레코드를 로컬 시뮬레이터의 `--shots`와 동일한 규칙과 출력 한도로 출력한다. 레코드가 하나면 프로그램 출력을 그대로 찍고, 여러 개면 횟수 내림차순 표와 각 레코드의 들여쓰기된 출력을 함께 출력한다. `qxide` 라이브러리의 공개 API는 변경하지 않았다.

실행 흐름은 다음과 같다.

```text
qxide run FILE --backend ibm:DEVICE
  -> 전개 (export_input)
  -> 논리 회로의 OpenQASM 3
  -> [--dry-run] 요청 출력
  -> [실행] 자격 증명 -> IAM 토큰 -> 장치 구성 -> ISA 변환 -> 장치 상태
           -> 안내 및 확인 -> POST /jobs -> 폴링 -> 결과 조회 -> 레코드 복호화
           -> Exported::render -> 표준 출력
```

## 명령줄

```text
qxide run [FILE] --backend ibm:<device> [--shots N] [--yes] [--dry-run] [--max-seconds S] [--device-config FILE] [--engine ...]
```

- `--shots`의 기본값은 로컬 `qxide run`과 동일하게 `1`이다(단일 레코드는 일반 출력을 표시한다).
- `--backend` 없이 `--dry-run`, `--yes`, `--max-seconds`, `--device-config`를 지정하면 사용법 오류(종료 코드 `2`)다.
- `--backend`와 함께 `--seed` 또는 `--allow`를 지정하면 사용법 오류다.
- `--device-config`는 `--dry-run`과 함께만 사용할 수 있다.
- `--max-seconds`는 1에서 10800초까지의 값을 허용하며 이를 초과하는 값은 사용법 오류(종료 코드 2)로 처리한다. 이는 해당 값을 전달하는 작업 필드 `cost`가 서비스 API 스키마에서 최댓값 10800(3시간)인 `int32`로 정의되어 있으며, 문서화된 서비스 최대 실행 시간 상한과 일치하기 때문이다.
- 알 수 없는 제공자 접두사이거나 장치 이름이 길이 1~64의 ASCII 영문자·숫자·`_`·`-` 범위를 벗어나면 종료 코드 `2`로 실패한다. 실행 단계의 오류는 종료 코드 `1`로 종료한다.
- 측정이 없는 프로그램은 장치가 측정 결과만 반환하므로 제출 전에 거부한다.

## IBM Quantum 클라이언트

공식 Python 클라이언트 `qiskit-ibm-runtime` 0.52.1의 REST 어댑터와 인증 코드를 참조해 구현했다.

- **엔드포인트 및 인증**: 기본 주소는 `https://quantum.cloud.ibm.com/api/v1`이며 `QXIDE_IBM_REGION=eu-de`가 설정되면 `https://eu-de.quantum.cloud.ibm.com/api/v1`을 사용한다. IAM 토큰은 `POST https://iam.cloud.ibm.com/identity/token`에 `grant_type=urn:ibm:params:oauth:grant-type:apikey&apikey=...` 폼 본문을 보내 `access_token`과 `expires_in`을 받으며, 만료 5분 전에 갱신한다.
- **공통 헤더**: 모든 API 호출에 `Authorization: Bearer <token>`, `Service-CRN: <instance CRN>`, `IBM-API-Version: 2026-04-15`(공식 클라이언트가 전송하는 값), `Accept: application/json`을 포함하며, 작업 생성 `POST`에는 `Content-Type: application/json`을 추가한다.
- **요청 순서**:
  1. `GET /backends/{device}/configuration`: `n_qubits`, `basis_gates`, `coupling_map`, `supported_instructions`를 읽는다. `404`는 해당 인스턴스에서 사용할 수 없는 장치로 처리한다.
  2. `GET /backends/{device}/status`: `state`가 `false`면 제출을 거부하고, `length_queue`를 사용자 안내에 표시한다.
  3. `POST /jobs`: 본문은 `{"program_id": "sampler", "backend": device, "cost": max_seconds, "params": {"pubs": [[qasm, null, shots]], "options": {"default_shots": shots}, "version": 2, "support_qiskit": true}}` 형식이다. PUB는 공식 클라이언트가 `SamplerPub`을 직렬화하는 `(circuit, parameter values, shots)` 형태이며, `cost`는 공식 클라이언트가 `max_execution_time`을 전달하는 필드다. 응답에서 작업 `id`를 읽는다.
  4. `GET /jobs/{id}`: 5초 간격으로 `Completed`가 될 때까지 폴링한다. `Failed` 또는 `Cancelled...` 상태이면 `state.reason`을 보고한다. 폴링이 5회 연속 실패하면 대기를 중단하고 작업이 서버에서 계속 실행 중일 수 있음을 안내한다.
  5. `GET /jobs/{id}/results`: 결과 본문을 받는다.
- **결과 복호화**: `sampler` 프로그램에 대해 공식 클라이언트의 `RuntimeDecoder`가 처리하는 구조(`PrimitiveResult` → 단일 `SamplerPubResult` → `DataBin`의 `c` 필드 → `BitArray`)를 `qxide`가 직접 복호화한다. `BitArray`는 `array`(shape `(shots, ceil(bits/8))`의 `uint8` `.npy` 배열을 zlib 압축 후 base64 인코딩한 ndarray)와 `num_bits`로 구성되며, 각 행의 비트는 빅 엔디언(비트 0이 마지막 바이트의 최하위 비트)이다. base64 디코딩, 512 MiB 상한이 있는 `flate2` zlib 해제, `.npy` 헤더 검증과 `num_bits` 검사를 수행한다. 복호화에 실패하면 유료 실행 결과가 유실되지 않도록 원본 응답을 시스템 임시 디렉터리의 `qxide-ibm-<job id>-results.json`에 저장하고 오류 메시지에 작업 ID를 함께 출력한다.
- **HTTP 오류 처리**: `401`/`403`은 자격 증명 거부, 구성 조회의 `404`는 장치 없음, `409`는 사용량 한도 초과, `429`는 요청 제한 초과(사용자당 작업 생성 분당 5회 제한)로 안내한다. 그 외 상태 코드는 상태 번호와 제어 문자를 제거한 본문 앞 300자를 출력한다.

## 자격 증명과 보안

API 키는 환경 변수 `QXIDE_IBM_API_KEY`에서만 읽으며, 설정되지 않은 경우 OS 자격 증명 저장소에서 읽기 전용으로 조회한다(`qxide`는 저장소에 값을 쓰지 않는다). 키를 전달하는 명령줄 플래그나 설정 파일은 두지 않았다.

- **Windows**: Credential Manager 일반 자격 증명 `qxide:ibm-quantum` (`CredReadW` 사용, 저장 예: `cmdkey /generic:qxide:ibm-quantum /user:apikey /pass:<key>`)
- **macOS**: 로그인 키체인 서비스 `qxide`, 계정 `ibm-quantum` (`security find-generic-password` 사용, 저장 예: `security add-generic-password -s qxide -a ibm-quantum -w`)
- **기타(Linux 등)**: Secret Service `service=qxide account=ibm-quantum` (`secret-tool lookup` 사용, 저장 예: `secret-tool store --label=qxide service qxide account ibm-quantum`)

인스턴스 CRN은 `QXIDE_IBM_INSTANCE` 환경 변수에서 읽는다. API 키와 CRN은 헤더에 넣기 전에 공백과 제어 문자가 없는 출력 가능한 ASCII 문자열인지 검증한다.

HTTP 통신은 패키지 레지스트리와 마찬가지로 새 크레이트를 추가하지 않고 시스템 `curl`을 사용한다. URL, 토큰을 포함한 헤더, IAM 요청의 API 키를 포함한 본문 전체를 `curl --config -`의 표준 입력으로 전달하므로 비밀 값이 프로세스 인자에 노출되지 않는다. 리다이렉트는 따라가지 않으며, 루프백 주소를 제외하고는 `https`만 허용한다. 요청 타임아웃은 120초, 응답 크기 상한은 256 MiB로 제한한다.

테스트 전용 환경 변수인 `QXIDE_IBM_API_URL`과 `QXIDE_IBM_IAM_URL`은 반드시 둘 다 함께 설정되어야 하며 둘 다 `localhost` 또는 루프백 주소를 가리켜야 한다. 이를 위반하면 실행을 거부해 변경된 환경 변수가 API 키를 외부 호스트로 유출하지 못하도록 막는다. `QXIDE_IBM_POLL_MS`는 테스트에서 폴링 간격을 단축하는 용도로 사용한다.

`--dry-run`은 자격 증명을 읽지 않으며 네트워크 요청도 보내지 않는다.

## ISA 변환

IBM 장치는 결합된 물리 큐비트 위의 네이티브 게이트로 구성된 ISA 회로만 허용하며, 공식 클라이언트도 전송 전 `validate_isa_circuits`로 이를 검사한다. `qxide`는 Python Qiskit이나 원격 트랜스파일러 서비스에 의존하지 않고 Rust에서 직접 ISA 변환을 수행한다.

- **입력**: `Circuit::to_qasm`이 출력하는 OpenQASM 3 부분집합을 `isa::parse`로 다시 파싱한다(각도는 정확히 왕복 변환된다). 임의의 OpenQASM 3 문서는 받지 않는다.
- **1큐비트 게이트 분해**: 전역 위상 차이를 제외하고 `rz`/`sx`/`x`로 정확히 변환한다. `H = rz(π/2)·sx·rz(π/2)`이고, `Z`, `S`, `S†`, `T`, `T†`, `RZ`는 `rz`로 통합한다. `RX(θ) = H·RZ(θ)·H`, `RY(θ) = S·RX(θ)·S†`로 전개하며, `X`는 `x`(장치 기저에 `x`가 없으면 `sx·sx`)로 변환한다.
- **2큐비트 및 3큐비트 게이트 분해**: 장치 기저에 `cz`가 있으면 `cz`, 없으면 `ecr`, 그다음 `cx`를 선택한다. `cz` 기반 `CX`는 타깃 큐비트에 `H·CZ·H`를 적용한다. 방향성이 있는 `ECR` 기반 `CX`는 `X₀·ECR·(S₀†⊗SX₁†)`를 사용하며, 반대 방향만 보정되어 있는 경우 양쪽 큐비트에 `H`를 둘러 제어와 타깃을 바꾼다. `CCX`는 `H`, `T`, `T†`와 6개의 `CNOT`으로 분해한다.
- **배치와 라우팅**: 논리 큐비트 수 이상을 수용할 수 있는 연결 요소 중 가장 번호가 작은 큐비트에서 너비 우선 탐색 순서로 물리 큐비트를 배정한다. 각 `CNOT` 전에 제어 큐비트를 최단 경로를 따라 `SWAP`(3개의 `CNOT`)으로 인접할 때까지 이동한다. `if` 블록 안에서 수행한 `SWAP`은 블록이 끝나기 전에 되돌려 배치가 측정 비트 값에 의존하지 않도록 유지한다. 측정과 `reset`은 현재 배치를 따른다.
- **정리**: 같은 큐비트의 연속된 `rz`를 병합하고 각도를 `(-π, π]` 범위로 정규화한 뒤 0 회전을 제거한다.
- **사전 검사**: 큐비트 수, 충분한 크기의 연결 요소 존재 여부, 기저 게이트에 `rz`·`sx` 및 2큐비트 게이트 포함 여부를 확인한다. 회로가 `measure`, `reset`(측정 후 큐비트 재사용 시 생성됨), `if`를 사용하면 각각 `supported_instructions`에 `measure`, `reset`, `if_else`가 포함되어 있는지 검사한다.
- **출력**: `bit[M] c;` 선언과 물리 큐비트 `$n`(`qubit` 선언 없음)을 출력하며, `ecr`을 사용하는 경우 `qiskit.qasm3.dumps`와 동일한 인라인 `ecr` 게이트 정의를 포함한다. `if` 구문 형식은 기존과 같다.

`--dry-run`은 기본적으로 변환 전 논리 회로를 그렇게 표시해 출력한다. 저장된 구성 JSON을 `--device-config FILE`로 전달하면 해당 장치에 맞게 변환된 실제 ISA 회로를 출력한다.

## 비용·대기열 안내

실제 제출 전 표준 오류(`stderr`)에 다음 내용을 출력한다.

- 시뮬레이터가 아닌 실제 장치에서 `N`샷을 실행한다는 점
- 해당 인스턴스의 QPU 시간을 소비한다는 점(유료 플랜은 과금되며 무료 Open 플랜은 월간 소량 할당량이 있으므로 IBM Quantum Platform에서 플랜 확인 필요)
- `--max-seconds`(기본값 `120`)초의 QPU 시간이 지나면 서비스에 작업 중단을 요청한다는 점
- 장치 대기열의 작업 수와 결과가 수 분에서 수 시간 걸릴 수 있다는 점

이어 터미널에서 `[y/N]` 확인을 받는다. 터미널이 없고 `--yes`도 지정되지 않았으면 네트워크 요청을 보내기 전에 실행을 거부한다.

제출이 완료되면 작업 ID를 출력하고, `Ctrl-C`는 대기만 중단할 뿐 작업은 대기열이나 실행 상태로 남아 QPU 시간을 계속 소비하며 IBM Quantum Platform에서 상태를 보거나 취소할 수 있음을 안내한다. `qxide`는 자체적으로 요금을 표시하지 않는다.

## 테스트

- **단위 테스트 (`src/qpu/isa_tests.rs`)**: 소형 밀집 상태 벡터 시뮬레이터로 모든 기저 입력에 대해 논리 회로와 ISA 회로의 전체 유니터리(하나의 공통 전역 위상까지 일치)를 비교한다. 3가지 장치 토폴로지(`cz` 기반 5큐비트 직선, 방향이 교차하는 `ecr` 기반 5큐비트 직선, `x`가 없는 방향성 `cx` 기반 4큐비트 T자형)에서 모든 게이트를 포함한 무작위 회로와 조건 비트의 모든 조합에 대한 `if` 블록을 검증한다. 의도적으로 망가뜨린 분해(잘못된 ECR 사전 회전, 잘못된 `RX`, `if` 내부 `SWAP` 미복원)가 이 테스트에서 실패함을 확인했다. 라이브러리 출력 파싱, 거부 조건, 기저 게이트 및 결합 제약 준수도 검증한다.
- **가짜 서버 통합 테스트 (`tests/qpu.rs`, 12개)**: `127.0.0.1`에서 구동하는 소형 HTTP 서버가 모든 요청을 기록한다. 정확한 요청 순서, IAM 폼 본문, 헤더, 작업 요청 본문과 ISA 회로, 7샷 출력 표, `c[0]`만 설정된 10비트 레코드의 종단 간 비트 순서, `--dry-run`의 무요청 동작(`--device-config` 유무 및 오버라이드 미설정 시 실제 주소 출력 포함), `--yes` 미지정 시 무요청 거부, 실패 작업의 `state.reason` 보고, 복호화 불가능한 결과 파일 저장, `401` 처리, 키 누락, 비루프백 및 한쪽만 설정된 URL 오버라이드 거부, 플래그 오류, 측정 없는 프로그램 거부를 검증한다.
- **결과 픽스처 (`tests/data/qpu/`)**: 공식 클라이언트의 `RuntimeEncoder`로 생성하며(`ci/qpu_fixtures.py`), 생성 시 `RuntimeDecoder`로 다시 복호화되는지 함께 확인한다.
- **오프라인 장치 ISA 검증 (`ci/check_isa.py`)**: CI의 `qasm-interop` 작업에서 네트워크 없이 실행된다. `qiskit_ibm_runtime.fake_provider`의 오프라인 스냅샷 `FakeTorino`, `FakeFez`(`cz`, Heron)와 `FakeBrisbane`(방향성 `ecr`, Eagle)의 구성을 추출해 `examples/qx`의 각 프로그램을 `--dry-run --device-config`로 변환한 뒤, 참조 파서·`qiskit.qasm3.loads`·공식 `validate_isa_circuits`(해당 장치 타깃 기준)를 모두 통과하는지 확인한다. 로컬에서는 `ccx`, `if`/`else`, 큐비트 재사용을 포함한 추가 프로그램 2개를 더해 총 27개 프로그램·장치 조합이 모두 통과했다.
- 전체 테스트 스위트, `clippy -- -D warnings`, wasm 크레이트 clippy 및 빌드, 데스크톱 앱 런타임 검사가 모두 통과했다.
- Windows 자격 증명 관리자 읽기(`CredReadW`)는 개발 머신에서 `cmdkey`로 생성한 일회용 일반 자격 증명 `qxide:qxide-test`를 대상으로 무시된 테스트(`QXIDE_TEST_KEYCHAIN=1 cargo test keychain -- --ignored`)를 1회 실행해 확인한 뒤 해당 항목을 삭제했으며, macOS 및 Secret Service 경로는 실행하지 않았다.
- `ci/check_isa.py`는 거부된 프로그램을 실패로 처리하므로 모든 변환이 실패하는 회귀가 조용히 통과할 수 없으며, 전체 CI 명령(`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`)과 `cargo test --features jit` 모두 통과한다.

## 결정 사항

아래 항목은 모두 추천안으로 결정했으며 사용자 검토 대상이다.

1. **HTTP 클라이언트**: 새 크레이트를 추가하지 않고 시스템 `curl`을 사용하며 요청 전체를 표준 입력으로 전달한다.
2. **자격 증명 경로**: API 키는 환경 변수 또는 OS 자격 증명 저장소(읽기 전용)에서만 읽고, 인스턴스 CRN은 환경 변수에서만 읽는다.
3. **`--dry-run` 범위**: 네트워크와 자격 증명에 전혀 접근하지 않으며, 실제 장치 ISA 회로는 `--device-config`를 제공한 경우에만 출력한다.
4. **제출 확인과 시간 상한**: 실제 제출 시 터미널 확인 또는 `--yes`를 요구하며, 기본 QPU 시간 상한은 `cost` 필드를 통해 120초로 설정한다.
5. **Rust 내장 ISA 변환**: Python Qiskit이나 원격 트랜스파일러에 의존하지 않고 Rust에서 단순 배치·라우팅(노이즈 인식이나 최적화가 아닌 기본 구현)으로 ISA 회로를 생성한다.
6. **요청·결과 형식**: 공식 클라이언트를 따라 `support_qiskit: true`와 `RuntimeEncoder` 결과를 사용하며, 샷 수는 PUB와 `default_shots` 양쪽에 모두 기록한다.
7. **부수 효과 및 충돌 옵션**: 부수 효과는 `qxide qasm`과 동일하게 거부하고, `--backend`와 함께 `--seed` 및 `--allow`를 지정하면 거부한다.
8. **기본 샷 수**: 로컬 `qxide run`과 일관되게 기본값 `1`을 유지한다.
9. **라이브러리 공개 API 유지**: ISA 변환기는 `qxide`가 출력한 OpenQASM을 파싱하고, 결과 렌더링은 비트 조회 클로저를 사용해 라이브러리 공개 API를 변경하지 않는다.
10. **로드맵 상태 유지**: 실제 서비스 제출을 검증하기 전까지 `ROADMAP.md` 항목은 미완료 상태로 둔다.

## 실제 서비스로 확인하지 않은 점

- 실제 서비스가 PUB `[qasm, null, shots]`와 `cost` 필드를 전송한 형태 그대로 수락하는지 여부(공식 클라이언트 코드에서 확인했을 뿐 실제 서버에 실행하지 못했다).
- 결과 형식: `sampler`에 대해 공식 클라이언트가 복호화하는 `RuntimeEncoder` JSON을 가정했으나, 공개 OpenAPI 문서에는 executor 결과용으로 다른 텐서 봉투 형식(`schema_version` v2.0)이 기술되어 있다.
- IBM 컴파일러가 `if (c[k] == true)` 구문과 인라인 `ecr` 게이트 정의를 수용하는지 여부(Qiskit 임포터는 둘 다 허용하지만, Qiskit 출력기는 `if (c[k])`를 사용한다).
- 결함 큐비트와 결함 간선(장치 properties)은 읽지 않으며, 배치 시 오류율은 고려하지 않는다.
- 가짜 서버가 루프백에서 평문 HTTP로만 통신하므로, `curl`의 HTTPS 경로(`proto =https`, TLS)는 테스트에서 한 번도 실행되지 않았다.
- 상태 응답의 `state`·`length_queue` 필드명과 장시간 대기 중 IAM 토큰 갱신은 실제 서비스 대상으로 검증되지 않았다. `curl` 설정 전달 경로는 현재까지 Windows에서만 실행되었으며, CI의 `test` 잡이 Linux와 Windows에서 `cargo test`를 실행하므로 이 브랜치에서 CI가 실행되면 fake-server 테스트는 Linux에서도 실행되지만, macOS(`curl` 및 `security` 키체인 경로)는 CI 범위에 포함되지 않는다.

## 후속 과제

- 사용자 계정으로 실제 장치에 첫 제출을 수행해 위의 미확인 항목을 검증한다.
- `Ctrl-C` 이후 작업 ID로 상태를 다시 추적하거나 작업을 취소하는 명령을 추가한다.
- 세션(session) 및 배치(batch) 실행 모드를 검토한다.
- 오류율과 결함 정보를 반영한 노이즈 인식 배치와 `SWAP` 수 절감을 도입한다.
- `Provider` 트레이트 뒤에 다른 양자 컴퓨터 서비스(예: Amazon Braket, Azure Quantum)를 추가한다.
- CI에 macOS 러너를 추가해 `curl` 및 키체인 경로도 함께 테스트한다.
