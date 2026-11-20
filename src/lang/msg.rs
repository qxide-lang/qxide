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

pub fn short_circuit_consume(name: &str, op: &str) -> String {
    format!("qubit `{name}` is consumed on the right of `{op}`, which is skipped when the left operand decides the result")
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

pub fn internal_error() -> String {
    "internal error in qxide: checked program failed to run; please report this bug".into()
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

pub fn playground_qubit_limit(qubits: usize, limit: usize) -> String {
    format!("state-vector simulation of {qubits} qubits exceeds the browser playground limit of {limit} qubits")
}

pub fn help_playground_qubit_limit() -> String {
    "use fewer qubits, or install `qxide` to run locally with `qxide run` (no qubit cap); the QASM button still exports the circuit".into()
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

pub fn fmt_changed_tokens(name: &str) -> String {
    format!("qxide: internal error: formatting `{name}` would change tokens or comments; nothing was written; please report this bug in qxide\n")
}

pub fn repl_banner(version: &str) -> String {
    format!("qxide {version} interactive .qx session; type :help for commands and :quit to exit.\n")
}

pub fn repl_help() -> String {
    "Type statements in main, function definitions, or expressions to print; missing\nsemicolons are added. Variables, functions, and qubits stay until :reset, with\nqxide run rules (measured qubits cannot be reused) and state-vector simulation.\n\n  :help   Show this help\n  :qasm   Print the OpenQASM 3 circuit built so far\n  :reset  Forget all variables, functions, and qubits\n  :quit   Exit; any qubit never measured or discarded is reported (exit code 1)\n".into()
}

pub fn repl_reset() -> String {
    "Session reset; all variables, functions, and qubits cleared\n".into()
}

pub fn repl_unknown_command(cmd: &str) -> String {
    format!("error: unknown command `:{cmd}`; type `:help` for a list of commands\n")
}

pub fn repl_unbalanced() -> String {
    "error: input closes a bracket it did not open, which is not allowed in the REPL\n".into()
}

pub fn repl_return() -> String {
    "error: top-level `return` is not allowed in the REPL because it would end the session's `main` function and skip later input\n".into()
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
        Code::ShortCircuitConsume => "consume the qubit in a separate statement before the condition and store the result in a variable".into(),
        Code::BranchMismatch => "consume the qubit in both branches or in neither".into(),
        Code::ConsumedInLoop => "allocate the qubit inside the loop body instead".into(),
        Code::PartialMove => "measure or discard the whole register; gates may still use `r[i]`".into(),
        Code::ForbiddenInBitIf => "move this operation before or after the `if`; only gates, `reset`, `for`, nested `if`, classical variable declarations, and calls to gate-only functions are allowed inside".into(),
        Code::OuterAssignInBitIf => "this assignment would not depend on the shot; use a variable declared inside the `if` body instead".into(),
        Code::ShadowLive => "consume the qubit first or choose another name".into(),
        Code::TempDropped => "store it in a variable such as `Qubit q = qubit();`, then call `measure` or `discard`".into(),
        Code::RecursionLimit => "check for recursion without a base case".into(),
        Code::RemovedSyntax => "declare variables like `int x = 1;` or `Qubit q = qubit();`, functions like `void main()` or `int add(int a, int b)`, and use `final` for constants".into(),
        Code::UnknownMethod => "use `to_binary()` or `as_int()`, the available methods on `BitReg` values".into(),
        Code::VarNotAllowed => "write the type explicitly, such as `int n` or `double theta`".into(),
        Code::TypeNameAsIdent => "choose a different name".into(),
        Code::ConsumeNonQuantum => "remove `consume`; classical values are always copied".into(),
        Code::VarInfer => "use `var` only for `int`, `double`, and `boolean` values; otherwise write the type explicitly, such as `Qubit q = qubit();` or `Bit m = measure(q);`".into(),
        Code::InvalidCast => "only `int` and `double` can be cast to each other, such as `(int) x` or `(double) n`".into(),
        Code::BorrowedConsume => "add `consume` before the parameter type, such as `void f(consume Qubit q)`, to take ownership and become responsible for measuring or discarding it".into(),
        Code::ConditionalCall => "a function called inside an `if` on a measurement result may only apply gates and `reset` to borrowed qubits, so it cannot allocate, measure, discard, print, take a `consume` parameter, or return a qubit; call it before or after the `if`".into(),
        Code::NonFiniteAngle => "check the arithmetic computing the angle, such as division by zero".into(),
        Code::AsIntTooWide => "print it with `to_binary()` or as an array using a plain placeholder instead".into(),
        Code::UnknownPackage => "declare the dependency by running `qxide add <package>`, which edits `qxide.toml`".into(),
        Code::PrivateFunction => "add `public` before the function declaration, e.g. `public void prepare(Qubit q)`".into(),
        Code::ImportCycle => "move the shared functions into a separate module that both modules import".into(),
        Code::ImportOutsideProject => "put the file in a project with a `qxide.toml` manifest and sources under `src/`".into(),
        Code::BadImportPath => "use `import package.function;`, `import package.module.function;`, or `import package.module.*;`".into(),
        Code::MisplacedImport => "move every `import` to the top of the file, before the first function".into(),
        Code::AmbiguousName => "import the function by full path, e.g. `import pkg.module.function;`, to override wildcard imports".into(),
        Code::ImportConflict => "rename the local function or remove one of the conflicting imports".into(),
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

pub fn non_finite_angle(gate: &str, value: f64) -> String {
    format!("rotation angle for `{gate}` must be a finite number, got {value}")
}

pub fn as_int_too_wide(len: usize) -> String {
    format!("method `as_int()` supports at most 64 bits, but the register has {len}")
}

pub fn var_not_allowed() -> String {
    "cannot use `var` as a parameter or return type; it is only allowed in local variable declarations".into()
}

pub fn type_name_as_ident(name: &str) -> String {
    format!("cannot use type name `{name}` as an identifier")
}

pub fn consume_non_quantum(ty: &str) -> String {
    format!("cannot use `consume` on classical parameter type `{ty}`; it applies only to `Qubit` or `QReg`")
}

pub fn var_infer(ty: &str) -> String {
    format!("cannot use `var` with type `{ty}`")
}

pub fn invalid_cast(from: &str, to: &str) -> String {
    format!("cannot cast `{from}` to `{to}`")
}

pub fn borrowed_consume(name: &str) -> String {
    format!("cannot consume borrowed parameter `{name}`")
}

pub fn note_borrowed_here() -> String {
    "borrowed here without `consume`".into()
}

pub fn borrow_place_required() -> String {
    "borrowed quantum argument must be a `Qubit` or `QReg` variable or register element such as `r[i]`".into()
}

pub fn constant_redefined(name: &str) -> String {
    format!("cannot declare `{name}`; it is a built-in constant")
}

pub fn constant_assign(name: &str) -> String {
    format!("cannot assign to built-in constant `{name}`")
}

pub fn help_constant_assign() -> String {
    "declare a local variable instead, such as `double angle = PI;`, and modify that variable"
        .into()
}

pub fn help_constant_redefined() -> String {
    "choose another name because `PI` always refers to the built-in constant".into()
}

pub fn help_borrow_temp() -> String {
    "store the qubit in a variable first (e.g., `Qubit q = qubit();`), pass `q`, and measure or discard it after the call".into()
}

pub fn conditional_call(name: &str) -> String {
    format!("cannot call function `{name}` inside an `if` on a measurement result")
}

pub fn note_cond_consume(param: &str) -> String {
    format!("parameter `{param}` takes ownership via `consume`, which a conditional call cannot do")
}

pub fn note_cond_returns(ty: &str) -> String {
    format!("returns a quantum value (`{ty}`), which cannot be created inside a conditional block")
}

pub fn note_cond_op(op: &str) -> String {
    format!("uses `{op}`, which cannot run inside a conditional block")
}

pub fn note_cond_calls(callee: &str) -> String {
    format!("calls `{callee}`, which cannot run inside a conditional block")
}

// Modules and imports (Q05xx).

pub fn misplaced_import() -> String {
    "`import` declarations must appear before function declarations".into()
}

pub fn bad_import_path() -> String {
    "import path must name a package or module and a function, or end with `.*`".into()
}

pub fn note_imported_by(module: &str) -> String {
    format!("`{module}` imports the next module here")
}

pub fn import_cycle(chain: &str) -> String {
    format!("import cycle detected: {chain}")
}

pub fn import_outside_project() -> String {
    "cannot resolve `import` outside a project with a `qxide.toml` manifest".into()
}

pub fn unknown_package(name: &str) -> String {
    format!("unknown package `{name}` in import")
}

pub fn too_many_modules(limit: usize) -> String {
    format!("exceeded maximum of {limit} imported modules")
}

pub fn module_not_found(module: &str) -> String {
    format!("failed to load module `{module}`")
}

pub fn note_module_file(path: &str, err: &str) -> String {
    format!("tried reading `{path}`: {err}")
}

pub fn unknown_import(name: &str, module: &str) -> String {
    format!("cannot find function `{name}` in module `{module}`")
}

pub fn private_function(name: &str, module: &str) -> String {
    format!("function `{name}` in module `{module}` is private")
}

pub fn import_conflicts_local(name: &str) -> String {
    format!("imported function `{name}` conflicts with a local function")
}

pub fn import_conflicts_import(name: &str) -> String {
    format!("conflicting imports for function `{name}`")
}

pub fn note_first_import() -> String {
    "first imported here".into()
}

pub fn ambiguous_name(name: &str, modules: &str) -> String {
    format!("ambiguous call to `{name}`: imported via wildcard from {modules}")
}
