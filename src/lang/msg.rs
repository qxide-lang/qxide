// User-facing diagnostic text for the `.qx` language, kept in one place.

use super::diag::Code;

pub fn desc_string() -> String {
    "string literal".into()
}

pub fn desc_eof() -> String {
    "end of file".into()
}

pub fn unexpected_char(ch: char) -> String {
    format!("unexpected character `{ch}`")
}

pub fn unterminated_string() -> String {
    "unterminated string literal".into()
}

pub fn invalid_escape(ch: char) -> String {
    format!("unknown escape sequence `\\{ch}`")
}

pub fn int_too_large() -> String {
    "integer literal is too large for `int`".into()
}

pub fn reserved_keyword(word: &str) -> String {
    format!("reserved keyword `{word}` is not supported yet")
}

pub fn expected_token(want: &str, found: &str) -> String {
    format!("expected {want}, found {found}")
}

pub fn expected_ident(found: &str) -> String {
    format!("expected identifier, found {found}")
}

pub fn expected_type(found: &str) -> String {
    format!("expected type, found {found}")
}

pub fn expected_expr(found: &str) -> String {
    format!("expected expression, found {found}")
}

pub fn unknown_type(name: &str) -> String {
    format!("unknown type `{name}`")
}

pub fn too_deep(limit: usize) -> String {
    format!("nesting depth exceeds the limit of {limit} levels")
}

pub fn chained_comparison() -> String {
    "comparison operators cannot be chained".into()
}

pub fn help_chained_comparison() -> String {
    "split the comparison with `&&`, for example `a < b && b < c`".into()
}

pub fn bad_format() -> String {
    "invalid format string in `println`".into()
}

pub fn unknown_var(name: &str) -> String {
    format!("cannot find value `{name}` in this scope")
}

pub fn unknown_fn(name: &str) -> String {
    format!("cannot find function `{name}` in this scope")
}

pub fn arity(name: &str, want: usize, got: usize) -> String {
    format!("function `{name}` takes {want} argument(s) but {got} were supplied")
}

pub fn type_mismatch(want: &str, found: &str) -> String {
    format!("mismatched types: expected `{want}`, found `{found}`")
}

pub fn duplicate_def(name: &str) -> String {
    format!("the name `{name}` is defined multiple times")
}

pub fn builtin_redefined(name: &str) -> String {
    format!("cannot redefine built-in function `{name}`")
}

pub fn missing_main() -> String {
    "missing `void main()` entry point".into()
}

pub fn main_signature() -> String {
    "function `main` must take no parameters and return nothing".into()
}

pub fn immutable_assign(name: &str) -> String {
    format!("cannot assign twice to immutable variable `{name}`")
}

pub fn not_indexable(ty: &str) -> String {
    format!("cannot index into a value of type `{ty}`")
}

pub fn format_arg_count(holes: usize, args: usize) -> String {
    format!("format string has {holes} placeholder(s), but {args} argument(s) were given")
}

pub fn println_format() -> String {
    "first argument to `println` must be a string literal".into()
}

pub fn not_printable(ty: &str) -> String {
    format!("cannot print a value of type `{ty}`")
}

pub fn missing_return(name: &str, ty: &str) -> String {
    format!("function `{name}` does not return a value of type `{ty}` on all paths")
}

pub fn string_not_allowed() -> String {
    "string literals are only allowed as the format argument of `println`".into()
}

pub fn bad_condition(ty: &str) -> String {
    format!("an `if` condition must be `boolean` or `Bit`, found `{ty}`")
}

pub fn bit_at_build_time(ty: &str) -> String {
    format!("measurement result `{ty}` is only known per shot and cannot be used here")
}

pub fn quantum_signature() -> String {
    "functions taking or returning `Qubit` or `QReg` are not yet supported".into()
}

pub fn use_after_consume(name: &str) -> String {
    format!("use of consumed qubit `{name}`")
}

pub fn note_consumed_here() -> String {
    "qubit consumed here".into()
}

pub fn never_consumed(name: &str) -> String {
    format!("qubit `{name}` is never measured or discarded")
}

pub fn note_scope_ends() -> String {
    "goes out of scope here".into()
}

pub fn note_returns_here() -> String {
    "function returns here".into()
}

pub fn note_declared_here() -> String {
    "declared here".into()
}

pub fn duplicate_qubit(gate: &str) -> String {
    format!("same qubit passed to `{gate}` more than once")
}

pub fn note_first_use() -> String {
    "first passed here".into()
}

pub fn branch_mismatch(name: &str) -> String {
    format!("qubit `{name}` is consumed in only one branch of this `if`")
}

pub fn consumed_in_loop(name: &str) -> String {
    format!("qubit `{name}` is consumed inside a loop")
}

pub fn partial_move() -> String {
    "cannot move a single qubit out of a register".into()
}

pub fn quantum_assign(name: &str) -> String {
    format!("cannot reassign quantum variable `{name}`")
}

pub fn forbidden_in_bit_if(op: &str) -> String {
    format!("cannot use `{op}` inside an `if` on a measurement result")
}

pub fn note_bit_if_here() -> String {
    "condition is a measurement result".into()
}

pub fn outer_assign_in_bit_if(name: &str) -> String {
    format!("cannot assign to `{name}` inside an `if` on a measurement result because it is declared outside the `if`")
}

pub fn shadow_live(name: &str) -> String {
    format!("declaration shadows qubit `{name}`, which is still alive")
}

pub fn place_required() -> String {
    "gate operand must be a qubit variable or register element such as `r[i]`".into()
}

pub fn temp_dropped() -> String {
    "newly allocated qubit is never measured or discarded".into()
}

pub fn overflow() -> String {
    "attempt to compute a value that overflows `int`".into()
}

pub fn div_by_zero() -> String {
    "attempt to divide by zero".into()
}

pub fn index_out_of_bounds(index: i64, len: usize) -> String {
    format!("index out of bounds: the length is {len} but the index is {index}")
}

pub fn recursion_limit(limit: usize) -> String {
    format!("call depth exceeded the limit of {limit}")
}

pub fn simulation_failed(err: &str) -> String {
    format!("simulation failed: {err}")
}

pub fn stabilizer_too_large(qubits: usize, required: &str, limit: &str) -> String {
    format!("stabilizer simulation of {qubits} qubits requires {required}, exceeding the {limit} memory limit")
}

pub fn state_vector_too_large(qubits: usize, required: &str, limit: &str) -> String {
    format!("state-vector simulation of {qubits} qubits requires {required}, exceeding the {limit} memory limit")
}

pub fn help_too_large() -> String {
    "use fewer qubits, or run `qxide qasm` to export the circuit without simulating it".into()
}

pub fn no_return(name: &str) -> String {
    format!("function `{name}` ended without returning a value")
}

pub fn negative_size(n: i64) -> String {
    format!("register size for `qreg` must not be negative, got {n}")
}

pub fn too_many_qubits(limit: usize) -> String {
    format!("program allocates more than {limit} qubits in total")
}

/// Generic advice for an error code, shown as a `help:` line.
pub fn help(code: Code) -> Option<String> {
    Some(match code {
        Code::InvalidEscape => "supported escapes are `\\n`, `\\t`, `\\\\`, and `\\\"`".into(),
        Code::TooDeep => "split the expression into smaller parts and store them in local variables".into(),
        Code::UnknownType => "use one of the available types: `int`, `double`, `boolean`, `Bit`, `BitReg`, `Qubit`, `QReg`, or `void` for functions that return nothing".into(),
        Code::BadFormat => "use `{}` to insert a value, and `{{` or `}}` to print literal braces".into(),
        Code::ImmutableAssign => "remove `final` from its declaration".into(),
        Code::BuiltinRedefined => "rename the function to avoid conflicting with a built-in".into(),
        Code::BitAtBuildTime => "a `Bit` can be used as an `if` condition or printed with `println`".into(),
        Code::NeverConsumed => "call `measure` or `discard` on it before the end of its scope".into(),
        Code::BranchMismatch => "consume the qubit in both branches or in neither".into(),
        Code::ConsumedInLoop => "allocate the qubit inside the loop body instead".into(),
        Code::PartialMove => "measure or discard the whole register; gates may still use `r[i]`".into(),
        Code::ForbiddenInBitIf => "move this operation before or after the `if`; only gates, `reset`, `for`, nested `if`, and classical variable declarations are allowed inside".into(),
        Code::OuterAssignInBitIf => "this assignment would not depend on the shot; use a variable declared inside the `if` body instead".into(),
        Code::ShadowLive => "consume the qubit first or choose another name".into(),
        Code::TempDropped => "store it in a variable such as `Qubit q = qubit();`, then call `measure` or `discard`".into(),
        Code::RecursionLimit => "check for recursion without a base case".into(),
        Code::RemovedSyntax => "declare variables like `int x = 1;` or `Qubit q = qubit();`, functions like `void main()` or `int add(int a, int b)`, and use `final` for constants".into(),
        Code::UnknownMethod => "use `to_binary()` or `as_int()`, the available methods on `BitReg` values".into(),
        Code::AsIntTooWide => "print it with `to_binary()` or as an array using a plain placeholder instead".into(),
        _ => return None,
    })
}

pub fn double_out_of_range() -> String {
    "floating-point literal is out of range for `double`".into()
}

pub fn removed_syntax(word: &str) -> String {
    format!("keyword `{word}` is no longer supported since qxide 0.5.0")
}

pub fn void_variable() -> String {
    "type `void` can only be used as a function return type".into()
}

pub fn unknown_method(name: &str, ty: &str) -> String {
    format!("no method named `{name}` found for type `{ty}`")
}

pub fn as_int_too_wide(len: usize) -> String {
    format!("method `as_int()` supports at most 64 bits, but the register has {len}")
}
