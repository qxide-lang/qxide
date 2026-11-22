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
    "string literal contains `{}` placeholders, which are only allowed in the format string of `println`; write literal braces as `{{` and `}}`".into()
}

pub fn bad_condition(ty: &str) -> String {
    format!("an `if` condition must be `boolean` or `Bit`, found `{ty}`")
}

pub fn while_condition(ty: &str) -> String {
    format!("a `while` condition must be `boolean`, found `{ty}`")
}

pub fn loop_control_outside(word: &str) -> String {
    format!("`{word}` outside of a loop")
}

pub fn note_loop_exits_here(word: &str) -> String {
    format!("`{word}` exits the iteration here")
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

pub fn app_qubit_limit(qubits: usize, limit: usize) -> String {
    format!("state-vector simulation of {qubits} qubits exceeds the desktop app limit of {limit} qubits per call")
}

pub fn help_app_qubit_limit() -> String {
    "use fewer qubits, or run the program with `qxide run`, which has no qubit cap".into()
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

pub fn step_limit(limit: u64) -> String {
    format!("program exceeded the limit of {limit} execution steps")
}

pub fn op_limit(limit: u64) -> String {
    format!("circuit exceeded the limit of {limit} operations")
}

pub fn output_limit(limit: u64) -> String {
    format!("program output exceeded the limit of {limit} bytes")
}

pub fn qasm_limit(estimate: u64, limit: u64) -> String {
    format!("OpenQASM output of about {estimate} bytes exceeds the limit of {limit} bytes")
}

pub fn shots_output_limit(shots: u64, limit: u64) -> String {
    format!("output of {shots} shots exceeded the limit of {limit} bytes")
}

pub fn cond_depth_limit(limit: usize) -> String {
    format!("`if` blocks on measured bits are nested more than {limit} levels deep")
}

pub fn sim_work_limit(shots: u64, qubits: usize, ops: u64, limit: u64) -> String {
    let shots = if shots == 1 {
        "1 shot".to_string()
    } else {
        format!("{shots} shots")
    };
    format!("simulating {shots} of a circuit with {qubits} qubits and {ops} operations exceeds the simulation work limit of {limit}")
}

pub fn too_many_diagnostics(omitted: usize, limit: usize) -> String {
    let more = if omitted == 1 {
        "1 more error was".to_string()
    } else {
        format!("{omitted} more errors were")
    };
    format!("{more} not shown; at most {limit} errors are printed")
}

pub fn diagnostics_too_long(omitted: usize, limit: u64) -> String {
    match omitted {
        0 => format!("error report was cut at the limit of {limit} bytes"),
        1 => format!(
            "error report was cut at the limit of {limit} bytes; 1 more error was not shown"
        ),
        n => format!(
            "error report was cut at the limit of {limit} bytes; {n} more errors were not shown"
        ),
    }
}

pub fn fmt_changed_tokens(name: &str) -> String {
    format!("qxide: internal error: formatting `{name}` would change tokens or comments; nothing was written; please report this bug in qxide\n")
}

/// Stderr line naming the random seed `run`/`repl` chose because `--seed` was not given.
pub fn seed_note(seed: u64) -> String {
    format!("seed: {seed} (pass --seed {seed} to reproduce)\n")
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
        Code::UnknownType => "use one of the available types: `int`, `double`, `boolean`, `String`, `Bit`, `BitReg`, `Qubit`, `QReg`, or `void` for functions that return nothing".into(),
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
        Code::StepLimit => "check for a loop bound or recursion that is much larger than intended; every evaluated expression, loop iteration and call counts as a step".into(),
        Code::OpLimit => "check for a loop that applies gates far more often than intended; gates, measurements, resets and `if` blocks on measured bits each count as one operation".into(),
        Code::OutputLimit => "print less, or print outside loops; with several shots, each distinct measurement record prints its own copy of the output".into(),
        Code::CondDepth => "nesting is counted across function calls, so call the function before or after the outer `if`, or combine the conditions".into(),
        Code::SimWorkLimit => "use fewer shots, qubits or operations, or run `qxide qasm` to export the circuit without simulating it".into(),
        Code::TooManyDiagnostics => "fix the errors shown first, then run the command again to see the rest".into(),
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
        Code::StdUnknown => "available standard modules are `std.log`, `std.time`, `std.fs`, `std.app`, and `std.qxide`; check the spelling".into(),
        Code::StdUnavailable => "apps built with `qxide app build` offer more standard modules than this command, and `std.app` is only available in apps".into(),
        Code::StdDenied => "add the module name to `permissions` in the `[app]` section of `qxide.toml`, or pass `--allow fs` to `qxide run` for `std.fs`".into(),
        Code::StdIo => "check that the file exists in the data folder (use `fs.exists` first) and is UTF-8 text no larger than 16 MiB".into(),
        Code::StdPath => "use a relative path such as `results/run.txt`, without `..`, drive letters, or a leading slash".into(),
        Code::StdArgument => "pass a value within the documented range for this standard function".into(),
        Code::UnknownCommand => "add the function to `commands` in the `[app]` section of `qxide.toml`, or fix the name passed to `qxide.invoke`".into(),
        Code::BadArguments => "pass an object whose keys match the exact parameter names with values of the expected types; `qxide.commands()` lists them".into(),
        Code::Cancelled => "the run was stopped by the page or because the window closed, so you can simply start it again".into(),
        Code::BadCommand => "mark the function `public` and use only `int`, `double`, `boolean`, or `String` parameters".into(),
        Code::BadAppConfig => "see the `[app]` section reference in `docs/design/qx-app.md`".into(),
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

// ---- standard library and app commands (docs/design/qx-app.md) ----

pub fn std_unknown_module(name: &str) -> String {
    format!("unknown standard library module `std.{name}`")
}

pub fn std_unknown_fn(name: &str, module: &str) -> String {
    format!("standard module `{module}` has no public function `{name}`")
}

pub fn std_unavailable(what: &str, host: &str) -> String {
    format!("standard library item `{what}` is not available in {host}")
}

pub fn std_denied(module: &str, how: &str) -> String {
    format!("standard module `std.{module}` is not permitted here; grant access with {how}")
}

pub fn std_dependency_denied(module: &str, package: &str) -> String {
    format!("dependency package `{package}` cannot import `std.{module}`; dependencies may only import `std.log`")
}

pub fn std_sleep_range(ms: i64, max: i64) -> String {
    format!("duration {ms} ms for `sleep_millis` is outside the allowed range 0 to {max}")
}

pub fn std_io(path: &str, err: &str) -> String {
    format!("file operation on `{path}` failed: {err}")
}

pub fn std_file_too_large(path: &str, mib: u64) -> String {
    format!("file `{path}` exceeds the {mib} MiB read limit")
}

pub fn std_not_utf8(path: &str) -> String {
    format!("file `{path}` is not valid UTF-8 text")
}

pub fn http_bad_url(url: &str) -> String {
    format!("invalid URL `{url}`; only `https://` URLs on port 443 without user info are accepted")
}

pub fn http_host_denied(host: &str) -> String {
    format!("host `{host}` is not listed in `[app]` `http_hosts` in `qxide.toml`; add it to `http_hosts` to allow requests")
}

pub fn http_failed(url: &str, reason: &str) -> String {
    format!("request to `{url}` failed: {reason}")
}

pub fn http_status(url: &str, status: u16) -> String {
    format!("request to `{url}` returned HTTP status {status}; only 200-299 is accepted (redirects are not followed)")
}

pub fn http_too_large(url: &str, mib: usize) -> String {
    format!("response from `{url}` exceeds the {mib} MiB size limit")
}

pub fn http_not_utf8(url: &str) -> String {
    format!("response from `{url}` is not valid UTF-8 text")
}

pub fn std_bad_path(path: &str) -> String {
    format!("path `{path}` is not allowed: `std.fs` paths must be relative paths inside the data folder")
}

pub fn cancelled() -> String {
    "call was cancelled before it finished".into()
}

pub fn unknown_command(name: &str) -> String {
    format!("unknown app command `{name}`: not listed in `[app]` `commands`")
}

pub fn command_not_found(name: &str, module: &str) -> String {
    format!("command `{name}` listed in `[app]` `commands` is not a function in module `{module}`")
}

pub fn command_not_public(name: &str) -> String {
    format!("command `{name}` must be declared `public` to be called from the page")
}

pub fn command_bad_param(name: &str, param: &str, ty: &str) -> String {
    format!("command `{name}` has parameter `{param}` of unsupported type `{ty}`; command parameters must be `int`, `double`, `boolean`, or `String`")
}

pub fn arg_missing(param: &str, name: &str) -> String {
    format!("missing argument `{param}` in call to command `{name}`")
}

pub fn arg_unknown(arg: &str, name: &str) -> String {
    format!("command `{name}` has no parameter named `{arg}`")
}

pub fn arg_type(param: &str, name: &str, want: &str, got: &str) -> String {
    format!("invalid argument `{param}` for command `{name}`: expected `{want}`, got {got}")
}

pub fn too_many_shots(shots: u64, max: u64) -> String {
    format!("requested {shots} shots, exceeding the app limit of {max} shots per call")
}

pub fn bad_app_config(reason: &str) -> String {
    format!("invalid `[app]` section in `qxide.toml`: {reason}")
}

pub fn std_size_range(width: i64, height: i64) -> String {
    format!("invalid window size {width}x{height} in `std.app.set_size`: each side must be between 200 and 8192 pixels")
}
