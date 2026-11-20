// User-facing diagnostic text for the `.qx` language, kept in one place.

pub fn desc_string() -> String {
    "문자열 리터럴".into()
}

pub fn desc_eof() -> String {
    "파일 끝".into()
}

pub fn unexpected_char(ch: char) -> String {
    format!("여기에서는 `{ch}` 문자를 사용할 수 없습니다")
}

pub fn unterminated_string() -> String {
    "닫는 큰따옴표가 없어 문자열 리터럴이 닫히지 않았습니다".into()
}

pub fn invalid_escape(ch: char) -> String {
    format!("`\\{ch}` 이스케이프는 지원하지 않으며 n, t, 역슬래시, 큰따옴표만 사용할 수 있습니다")
}

pub fn int_too_large() -> String {
    "정수 리터럴이 너무 커서 `i64`에 담을 수 없습니다".into()
}

pub fn reserved_keyword(word: &str) -> String {
    format!("`{word}` 예약어는 아직 지원하지 않습니다")
}

pub fn expected_token(want: &str, found: &str) -> String {
    format!("{want}이(가) 필요하지만 {found}이(가) 발견되었습니다")
}

pub fn expected_ident(found: &str) -> String {
    format!("이름이 필요하지만 {found}이(가) 발견되었습니다")
}

pub fn expected_type(found: &str) -> String {
    format!("`i64`, `Qubit` 같은 타입이 필요하지만 {found}이(가) 발견되었습니다")
}

pub fn expected_expr(found: &str) -> String {
    format!("표현식이 필요하지만 {found}이(가) 발견되었습니다")
}

pub fn unknown_type(name: &str) -> String {
    format!("`{name}` 타입은 존재하지 않으며 `i64`, `bool`, `Bit`, `BitReg`, `Qubit`, `QReg`만 사용할 수 있습니다")
}

pub fn too_deep(limit: usize) -> String {
    format!("중첩 깊이가 제한({limit}단계)을 초과했으므로 `let` 바인딩으로 나누어 작성해 주세요")
}

pub fn chained_comparison() -> String {
    "비교 연산자는 연달아 사용할 수 없으므로 `&&`로 연결해 주세요".into()
}

pub fn bad_format() -> String {
    "형식 문자열에는 `{}` 자리 표시자만 사용할 수 있으며 중괄호 자체를 출력하려면 `{{`와 `}}`처럼 두 번 써야 합니다".into()
}

pub fn unknown_var(name: &str) -> String {
    format!("이 범위에서 `{name}` 변수를 찾을 수 없습니다")
}

pub fn unknown_fn(name: &str) -> String {
    format!("`{name}` 함수를 찾을 수 없습니다")
}

pub fn arity(name: &str, want: usize, got: usize) -> String {
    format!("`{name}` 함수는 인수를 {want}개 받지만 {got}개가 전달되었습니다")
}

pub fn type_mismatch(want: &str, found: &str) -> String {
    format!("`{want}` 타입이 필요하지만 `{found}` 타입이 발견되었습니다")
}

pub fn duplicate_def(name: &str) -> String {
    format!("`{name}`은(는) 이미 정의되어 있습니다")
}

pub fn builtin_redefined(name: &str) -> String {
    format!("`{name}`은(는) 내장 함수 이름이라 재정의할 수 없으니 다른 이름을 사용해 주세요")
}

pub fn missing_main() -> String {
    "프로그램의 진입점으로 `fn main()` 함수가 필요합니다".into()
}

pub fn main_signature() -> String {
    "`main` 함수는 매개변수와 반환값이 없어야 합니다".into()
}

pub fn immutable_assign(name: &str) -> String {
    format!("불변 변수 `{name}`에는 값을 대입할 수 없으므로 `let mut`으로 선언해 주세요")
}

pub fn not_indexable(ty: &str) -> String {
    format!("`{ty}` 타입 값은 인덱싱할 수 없으며 `QReg`와 `BitReg`만 인덱싱할 수 있습니다")
}

pub fn format_arg_count(holes: usize, args: usize) -> String {
    format!("형식 문자열에는 자리 표시자가 {holes}개 있지만 값은 {args}개가 전달되었습니다")
}

pub fn println_format() -> String {
    "`println`의 첫 번째 인수는 형식 문자열 리터럴이어야 합니다".into()
}

pub fn not_printable(ty: &str) -> String {
    format!("`{ty}` 타입 값은 출력할 수 없으며 `i64`, `bool`, `Bit`, `BitReg`만 출력할 수 있습니다")
}

pub fn missing_return(name: &str, ty: &str) -> String {
    format!("`{name}` 함수는 모든 실행 경로에서 `{ty}` 값을 반환해야 합니다")
}

pub fn string_not_allowed() -> String {
    "문자열 리터럴은 `println`의 형식 인수로만 사용할 수 있습니다".into()
}

pub fn bad_condition(ty: &str) -> String {
    format!("`if` 조건은 `bool` 또는 `Bit` 타입이어야 하지만 `{ty}` 타입이 발견되었습니다")
}

pub fn bit_at_build_time(ty: &str) -> String {
    format!("`{ty}` 측정 결과는 샷마다 결정되므로 빌드 시점 연산에 쓸 수 없고 `if`나 `println`에서만 사용할 수 있습니다")
}

pub fn quantum_signature() -> String {
    "`Qubit`이나 `QReg`를 매개변수나 반환값으로 사용하는 함수는 아직 지원하지 않습니다".into()
}

pub fn use_after_consume(name: &str) -> String {
    format!("`{name}` 큐비트는 이미 소비(측정, 폐기 또는 이동)되어 다시 사용할 수 없습니다")
}

pub fn note_consumed_here() -> String {
    "여기에서 소비됨 (측정·폐기·이동)".into()
}

pub fn never_consumed(name: &str) -> String {
    format!("`{name}` 큐비트가 소비되지 않은 채 범위를 벗어나므로 `measure`나 `discard`를 호출해 주세요")
}

pub fn note_scope_ends() -> String {
    "여기에서 범위가 끝남".into()
}

pub fn note_returns_here() -> String {
    "여기에서 함수가 반환됨".into()
}

pub fn note_declared_here() -> String {
    "여기에서 선언됨".into()
}

pub fn duplicate_qubit(gate: &str) -> String {
    format!("`{gate}` 게이트에 같은 큐비트를 중복해서 전달할 수 없습니다")
}

pub fn note_first_use() -> String {
    "여기에서 처음 전달됨".into()
}

pub fn branch_mismatch(name: &str) -> String {
    format!(
        "`{name}` 큐비트가 `if`의 한 분기에서만 소비되므로 양쪽 분기의 소비 여부가 일치해야 합니다"
    )
}

pub fn consumed_in_loop(name: &str) -> String {
    format!("반복문 밖에서 선언된 `{name}` 큐비트는 반복문 안에서 소비할 수 없습니다")
}

pub fn partial_move() -> String {
    "레지스터에서 개별 큐비트만 꺼내 이동할 수 없으므로 레지스터 전체를 측정하거나 폐기해 주세요"
        .into()
}

pub fn quantum_assign(name: &str) -> String {
    format!("양자 변수 `{name}`에는 값을 다시 대입할 수 없습니다")
}

pub fn forbidden_in_bit_if(op: &str) -> String {
    format!("측정 결과(`Bit`)가 조건인 `if` 블록 안에서는 `{op}`을(를) 사용할 수 없습니다")
}

pub fn note_bit_if_here() -> String {
    "이 조건은 측정 결과(`Bit`)임".into()
}

pub fn outer_assign_in_bit_if(name: &str) -> String {
    format!("대입은 샷 결과에 따라 분기할 수 없으므로 측정 결과 조건 `if` 안에서 바깥 변수 `{name}`에 대입할 수 없습니다")
}

pub fn shadow_live(name: &str) -> String {
    format!("아직 살아 있는 `{name}` 큐비트를 새 `{name}` 변수로 가릴 수 없으므로 먼저 소비하거나 다른 이름을 사용해 주세요")
}

pub fn place_required() -> String {
    "게이트 인수는 큐비트 변수나 `r[i]` 같은 레지스터 원소여야 합니다".into()
}

pub fn overflow() -> String {
    "정수 연산 결과가 `i64` 범위를 초과했습니다".into()
}

pub fn div_by_zero() -> String {
    "0으로 나눌 수 없습니다".into()
}

pub fn index_out_of_bounds(index: i64, len: usize) -> String {
    format!("인덱스 {index}이(가) 길이 {len}의 범위를 벗어났습니다")
}

pub fn recursion_limit(limit: usize) -> String {
    format!("함수 호출 깊이가 제한({limit}단계)을 초과했으므로 무한 재귀가 없는지 확인해 주세요")
}

pub fn simulation_failed(err: &str) -> String {
    format!("시뮬레이션에 실패했습니다: {err}")
}

pub fn no_return(name: &str) -> String {
    format!("`{name}` 함수가 값을 반환하지 않고 종료되었습니다")
}

pub fn negative_size(n: i64) -> String {
    format!("레지스터 크기는 0 이상이어야 하지만 {n}이(가) 지정되었습니다")
}

pub fn too_many_qubits(limit: usize) -> String {
    format!("프로그램이 총 {limit}개를 초과하는 큐비트를 할당하려고 합니다")
}
