# Qxide Error Codes

This catalog lists every .qx compiler error code; every diagnostic prints a code such as `Q0302`, and running `qxide explain Q0302` prints that code's entry from this file. Codes are never reused and are grouped by category: `Q01xx` for lexing and parsing, `Q02xx` for names and types, `Q03xx` for qubit ownership (linearity), and `Q04xx` for errors found while running the program. Each entry includes an explanation, an example that triggers the error, and a corrected version.

## Q0101: unexpected character in source code

The lexer encountered a character that is not allowed in the `.qx` language. Because the compiler cannot form a valid token from an unrecognized character, it rejects the file during lexing rather than guessing what was intended. This prevents typos or syntax borrowed from other languages from being silently misinterpreted.

Erroneous code example:

```qx
void main() {
    int x = 1 # 2;  // error[Q0101]
    println("{}", x);
}
```

To fix it, replace the unrecognized `#` character with the `+` operator so the expression forms valid `.qx` syntax.

```qx
void main() {
    int x = 1 + 2;
    println("{}", x);
}
```

## Q0102: unterminated string literal

A string literal starts with an opening double quote `"` but has no matching closing double quote `"`. Because the lexer cannot tell where the string ends and the rest of the program resumes, it cannot tokenize the surrounding code reliably. Requiring every string literal to be explicitly closed keeps token boundaries unambiguous.

Erroneous code example:

```qx
void main() {
    println("hello);
}
```

To fix it, add the missing closing `"` after `hello` so the string literal is properly terminated.

```qx
void main() {
    println("hello");
}
```

## Q0103: unknown escape sequence in a string literal

A string literal contains a backslash escape sequence that the `.qx` language does not recognize. Only `\n`, `\t`, `\\`, and `\"` are supported escape sequences. Rejecting unknown escapes during lexing prevents mistyped escapes from being silently turned into unintended characters.

Erroneous code example:

```qx
void main() {
    println("tab\q");  // error[Q0103]
}
```

To fix it, replace the unsupported `\q` escape sequence with the supported `\t` escape sequence.

```qx
void main() {
    println("tab\t");
}
```

## Q0104: integer literal out of range for `int`

An integer literal exceeds the range representable by a 64-bit signed integer (`i64`), which backs the `int` type. Because the value cannot fit without overflowing, the compiler rejects the literal instead of truncating or wrapping it silently. This ensures that integer constants always represent the exact value written in the source code.

Erroneous code example:

```qx
void main() {
    int big = 99999999999999999999;  // error[Q0104]
    println("{}", big);
}
```

To fix it, replace the out-of-range literal with `9223372036854775807`, which fits within the `i64` range of `int`.

```qx
void main() {
    int big = 9223372036854775807;
    println("{}", big);
}
```

## Q0105: unexpected token or chained comparison

The parser encountered an unexpected token that does not fit the grammar of the `.qx` language, including chained comparison operators. In `.qx`, comparison operators cannot be chained directly in a single expression. Rejecting unexpected tokens and chained comparisons prevents ambiguous syntax and ensures compound conditions are written explicitly.

Erroneous code example:

```qx
void main() {
    int a = 1;
    boolean ok = 0 < a < 2;  // error[Q0105]
    println("{}", ok);
}
```

To fix it, split the chained comparison `0 < a < 2` into two comparisons joined with `&&`, writing `0 < a && a < 2`.

```qx
void main() {
    int a = 1;
    boolean ok = 0 < a && a < 2;
    println("{}", ok);
}
```

## Q0106: nesting depth exceeds 128 levels

Error `Q0106` occurs when syntax nesting exceeds the compiler's maximum depth of 128 levels. The parser enforces this limit to keep recursion bounded and prevent excessive stack usage on deeply nested input.

Erroneous code example:

```qx
void main() {
    int x = ((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))));  // error[Q0106]
    println("{}", x);
}
```

To fix it, remove unnecessary nesting or split deeply nested expressions into smaller parts stored in local variables so the nesting depth stays within 128 levels.

```qx
void main() {
    int x = (1);
    println("{}", x);
}
```

## Q0107: use of an unsupported reserved keyword

Error `Q0107` occurs when a program uses a reserved keyword that the `.qx` compiler does not support yet: `while`, `loop`, `break`, `continue`, `struct`, `enum`, `impl`, `use`, `mod`, `const`, `match`, `with_ancilla`, or `apply`. These words are reserved in the grammar so they cannot be used as identifiers and remain available for future language features. Because the compiler does not yet implement them, the lexer rejects a reserved keyword as soon as it reads it.

Erroneous code example:

```qx
void main() {
    int i = 0;
    while i < 3 {  // error[Q0107]
        i = i + 1;
    }
}
```

To fix it, replace the unsupported `while` loop with a supported construct such as a `for` loop over a range.

```qx
void main() {
    for i in 0..3 {
        println("{}", i);
    }
}
```

## Q0108: unknown type name

Error `Q0108` occurs when a declaration or function signature names a type that does not exist in `.qx`, such as `long`. Every variable, parameter, and function return type must be one of the available built-in types: `int`, `double`, `boolean`, `Bit`, `BitReg`, `Qubit`, `QReg`, or `void` for functions that return nothing. Rejecting unrecognized type names ensures the compiler can statically check both classical and quantum values.

Erroneous code example:

```qx
void main() {
    long n = 1;  // error[Q0108]
    println("{}", n);
}
```

To fix it, replace the unknown type name with one of the available `.qx` types, such as `int` for integer values.

```qx
void main() {
    int n = 1;
    println("{}", n);
}
```

## Q0109: invalid format string in `println`

Error `Q0109` occurs when the format string passed to `println` uses unsupported or malformed brace syntax, such as named placeholders or unescaped braces. Format strings in `println` only allow `{}` to insert a value and `{{` or `}}` to print literal braces. Validating the format string at compile time catches formatting mistakes before the program runs.

Erroneous code example:

```qx
void main() {
    int x = 1;
    println("x = {x}", x);  // error[Q0109]
}
```

To fix it, replace any invalid placeholder in the `println` format string with `{}` for each value argument, and escape literal braces as `{{` or `}}`.

```qx
void main() {
    int x = 1;
    println("x = {}", x);
}
```

## Q0110: use of removed `let`, `fn`, or `mut` syntax

Error `Q0110` occurs when a program uses the removed Rust-style keywords `let`, `fn`, or `mut`. Since `qxide` 0.5.0, `.qx` uses Java/C-style declarations instead of these keywords, so the parser no longer accepts them. Recognizing these removed keywords allows the compiler to emit a targeted migration error rather than a generic syntax failure.

Erroneous code example:

```qx
fn main() {  // error[Q0110]
    let x = 1;
}
```

To fix it, declare functions with an explicit return type such as `void main()` and declare variables with their type such as `int x = 1;`, using `final` for constants instead of `let`, `fn`, or `mut`.

```qx
void main() {
    int x = 1;
    println("{}", x);
}
```

## Q0111: `var` used as a parameter or return type

`Q0111` occurs when `var` appears as a parameter type or return type in a function declaration. In `.qx`, `var` is allowed only in local variable declarations, not in function signatures. Parameter and return types must be written explicitly so each function signature clearly specifies the types it accepts and returns.

Erroneous code example:

```qx
var twice(int n) {  // error[Q0111]
    return n * 2;
}

void main() {
    println("{}", twice(2));
}
```

To fix it, replace `var` with the explicit return type `int` in the declaration of `twice`. In general, write an explicit type for every parameter and return type instead of `var`.

```qx
int twice(int n) {
    return n * 2;
}

void main() {
    println("{}", twice(2));
}
```

## Q0112: type name used as an identifier

`Q0112` occurs when a type name is used as the name of a variable, parameter, or function. Type names are reserved for naming types and cannot also serve as value or function identifiers in `.qx`. Keeping type names separate from ordinary identifiers prevents ambiguity when reading and parsing declarations.

Erroneous code example:

```qx
void main() {
    int Qubit = 1;  // error[Q0112]
    println("{}", Qubit);
}
```

To fix it, rename the variable from `Qubit` to an ordinary identifier such as `count` and update its references to use the new name.

```qx
void main() {
    int count = 1;
    println("{}", count);
}
```

## Q0201: use of an unknown variable

`Q0201` occurs when an expression references a variable name that has not been declared in the current scope. Every variable in `.qx` must be declared before it is used and be visible at the point where it is referenced. This check catches misspelled names, missing declarations, and out-of-scope references at compile time.

Erroneous code example:

```qx
void main() {
    println("{}", y);  // error[Q0201]
}
```

To fix it, declare `y` with `int y = 1;` before passing it to `println`. If you meant to use an existing variable, check its spelling and make sure it is in scope.

```qx
void main() {
    int y = 1;
    println("{}", y);
}
```

## Q0202: call to an unknown function

`Q0202` occurs when a call expression refers to a function name that cannot be found in the current scope. Every called function in `.qx` must be defined or available as a built-in in the scope where it is called. Checking function names at compile time catches misspelled names and calls to nonexistent functions before the circuit is built.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    hadamard(q);  // error[Q0202]
    discard(q);
}
```

To fix it, replace the unknown call `hadamard(q)` with `h(q)`. When calling your own functions, ensure the function is defined in scope and spelled consistently.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    discard(q);
}
```

## Q0203: wrong number of function arguments

`Q0203` occurs when a function call passes fewer or more arguments than the function's declaration expects. In `.qx`, every call must supply the exact number of arguments required by the target function. Enforcing argument counts at compile time ensures each parameter receives a matching value and prevents malformed operations in the resulting circuit.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    Qubit r = qubit();
    h(q, r);  // error[Q0203]
    discard(q);
    discard(r);
}
```

To fix it, call `h` once per qubit with a single argument, using `h(q)` and `h(r)` instead of passing both `q` and `r` in one call.

```qx
void main() {
    Qubit q = qubit();
    Qubit r = qubit();
    h(q);
    h(r);
    discard(q);
    discard(r);
}
```

## Q0204: type mismatch between expected and found types

This error occurs when an expression produces a type that does not match the type expected in that position, such as in a variable declaration. The `.qx` compiler checks types statically and requires the found type to match the expected type. Enforcing strict type compatibility catches type errors at compile time before the program is simulated or exported.

Erroneous code example:

```qx
void main() {
    int n = 1.5;  // error[Q0204]
    println("{}", n);
}
```

To fix it, change the declared type of `n` from `int` to `double` so that it matches the type of the value `1.5`.

```qx
void main() {
    double n = 1.5;
    println("{}", n);
}
```

## Q0205: duplicate function or parameter name

This error occurs when two functions or two parameters share the same name within the same scope. Each function and parameter name must be unique so the compiler can resolve every reference to a single definition without ambiguity. Disallowing duplicate names ensures that calls and references always refer to the intended declaration.

Erroneous code example:

```qx
int one() {
    return 1;
}

int one() {  // error[Q0205]
    return 2;
}

void main() {
    println("{}", one());
}
```

To fix it, rename the second function from `one` to `two` so each function has a unique name, and update `main` to call `one()` and `two()`.

```qx
int one() {
    return 1;
}

int two() {
    return 2;
}

void main() {
    println("{}", one() + two());
}
```

## Q0206: missing or invalid `void main()` entry point

This error occurs when a `.qx` program does not define a `main` function, or defines `main` with a signature other than `void main()`. Every `.qx` program builds a single quantum circuit starting from the `void main()` entry point. Without a valid `void main()` function, the compiler has no starting point for executing the program with `qxide run` or exporting it with `qxide qasm`.

Erroneous code example:

```qx
void start() {  // error[Q0206]
    println("hi");
}
```

To fix it, rename `start` to `main` so the program defines the required `void main()` entry point.

```qx
void main() {
    println("hi");
}
```

## Q0207: assignment to an immutable variable

This error occurs when a program attempts to assign a new value to an immutable variable, such as one declared with `final`. A `final` variable is bound to its initial value and cannot be reassigned later. Enforcing immutability at compile time prevents accidental modification of values that are intended to remain constant.

Erroneous code example:

```qx
void main() {
    final int n = 1;
    n = 2;  // error[Q0207]
    println("{}", n);
}
```

To fix it, remove the `final` modifier from the declaration of `n` so that `n` can be reassigned.

```qx
void main() {
    int n = 1;
    n = 2;
    println("{}", n);
}
```

## Q0209: indexing a value other than `QReg` or `BitReg`

This error occurs when the indexing operator `[]` is applied to a value whose type is neither `QReg` nor `BitReg`. In `.qx`, only quantum registers (`QReg`) and classical bit registers (`BitReg`) support element indexing. Rejecting indexing on non-register types such as `int` ensures that subscript expressions are only used on values that hold indexed elements.

Erroneous code example:

```qx
void main() {
    int n = 3;
    println("{}", n[0]);  // error[Q0209]
}
```

To fix it, replace the `int` with a quantum register created via `qreg(3)`, apply `h` to `r[0]`, and measure `r` into a `BitReg` `b` before indexing `b[0]`.

```qx
void main() {
    QReg r = qreg(3);
    h(r[0]);
    BitReg b = measure(r);
    println("{}", b[0]);
}
```

## Q0210: placeholder and argument count mismatch in `println`

A call to `println` passed a different number of arguments than the number of `{}` placeholders in its format string. Each `{}` placeholder requires one matching value argument after the format string, and every value argument must correspond to a placeholder. Checking these counts at compile time prevents missing values and silently ignored arguments.

Erroneous code example:

```qx
void main() {
    int a = 1;
    println("{} {}", a);  // error[Q0210]
}
```

To fix it, pass one argument for each `{}` placeholder in the format string, or adjust the number of placeholders to match the arguments. In the corrected program, passing `a` twice supplies both values required by the two `{}` placeholders.

```qx
void main() {
    int a = 1;
    println("{} {}", a, a);
}
```

## Q0211: first `println` argument is not a string literal

A call to `println` passed an expression that is not a string literal as its first argument. `println` uses its first argument as a format string containing `{}` placeholders for any values that follow, rather than printing a standalone value directly. Requiring a string literal allows the compiler to inspect the format string at compile time.

Erroneous code example:

```qx
void main() {
    int a = 1;
    println(a);  // error[Q0211]
}
```

To fix it, pass a format string literal as the first argument to `println`, followed by the value to print. In the corrected program, `"{}"` is passed as the format string and `a` is passed as the second argument.

```qx
void main() {
    int a = 1;
    println("{}", a);
}
```

## Q0212: unprintable value passed to `println`

A value whose type cannot be printed—such as `Qubit`—was passed as an argument to `println`. `println` can only format classical values, whereas a `Qubit` represents a quantum state inside the circuit rather than a directly readable classical value. Rejecting unprintable types at compile time makes it explicit that a quantum state cannot be printed directly.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    println("{}", q);  // error[Q0212]
    discard(q);
}
```

To fix it, obtain a printable classical value before calling `println`. In the corrected program, `measure(q)` consumes `q` and produces a `Bit` value `b`, which `println` can print.

```qx
void main() {
    Qubit q = qubit();
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0213: function does not return on all paths

A function declared with a non-`void` return type has at least one execution path that reaches the end of the function body without executing a `return` statement. Callers rely on receiving a value of the declared return type regardless of which branches execute. Requiring every control-flow path to return guarantees that the function always produces a value.

Erroneous code example:

```qx
int sign(int n) {
    if n < 0 {
        return -1;
    }
}  // error[Q0213]

void main() {
    println("{}", sign(-5));
}
```

To fix it, ensure that every control-flow path in the function ends with a `return` statement of the declared type. In the corrected program, adding `return 1;` after the `if` block handles the case where the condition is false so `sign` always returns an `int`.

```qx
int sign(int n) {
    if n < 0 {
        return -1;
    }
    return 1;
}

void main() {
    println("{}", sign(-5));
}
```

## Q0214: string literal used outside `println`

A string literal was used outside the first argument of a `println` call. In `.qx`, string literals are only allowed as the format argument to `println` and cannot be assigned to variables or used as general expression values. Restricting string literals to `println` ensures they are only used where the language supports them.

Erroneous code example:

```qx
void main() {
    int n = "three";  // error[Q0214]
    println("{}", n);
}
```

To fix it, replace the string literal with a value of the expected type and use string literals only as the format argument of `println`. In the corrected program, `n` is initialized with the `int` value `3` instead of a string literal.

```qx
void main() {
    int n = 3;
    println("{}", n);
}
```

## Q0215: `if` condition is not `boolean` or `Bit`

This error occurs when the condition expression of an `if` statement has a type other than `boolean` or `Bit`. The `.qx` compiler does not implicitly convert other types, such as `int`, into a truth value. Requiring an explicit `boolean` or `Bit` keeps branch conditions unambiguous and prevents accidental type mismatches.

Erroneous code example:

```qx
void main() {
    int n = 1;
    if n {  // error[Q0215]
        println("one");
    }
}
```

To fix it, replace the `int` condition with a comparison expression that evaluates to `boolean`, such as `n == 1`.

```qx
void main() {
    int n = 1;
    if n == 1 {
        println("one");
    }
}
```

## Q0217: redefinition of a built-in function

This error occurs when a user-defined function is declared with the same name as a built-in function. Built-in function names are reserved by `.qx` and cannot be shadowed or redefined by user code. Disallowing redefinitions prevents name conflicts and keeps calls to built-in operations unambiguous.

Erroneous code example:

```qx
void h() {  // error[Q0217]
    println("hi");
}

void main() {
    println("start");
}
```

To fix it, rename the user-defined function to a name that does not conflict with a built-in, such as `hello`, and call it by the new name.

```qx
void hello() {
    println("hi");
}

void main() {
    hello();
}
```

## Q0218: `Bit` or `BitReg` used for a build-time value

This error occurs when a `Bit` or `BitReg` measurement result is used in a position that requires a value known at circuit build time. A `.qx` program constructs one quantum circuit before execution, whereas measurement results are only known per shot when the circuit runs. Because the measured value does not exist while the circuit is being built, it cannot be used where a build-time value is required.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    for i in 0..b {  // error[Q0218]
        println("{}", i);
    }
}
```

To fix it, stop using the `Bit` value `b` as a loop bound and instead print `b` directly with `println`. A `Bit` can only be used in per-shot contexts such as an `if` condition or `println`.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0219: `Qubit` or `QReg` in function signature (retired)

Error code `Q0219` is retired and is no longer emitted by the compiler. Through version 0.5.0, the compiler reported this error for functions with `Qubit` or `QReg` parameters or return types because quantum values in function signatures were not yet supported. Since version 0.6.0, qubit parameters are borrowed by default or moved with `consume`, and functions can return `Qubit` and `QReg` values; the error number is reserved and not reused.

No action is needed on current compiler versions because `Q0219` is no longer produced. If you encounter `Q0219` with an older compiler, upgrade to version 0.6.0 or later to use `Qubit` and `QReg` in function signatures.

## Q0220: unknown method on a type

This error occurs when a method call names a method that does not exist on the receiver's type. In `.qx`, only `to_binary()` and `as_int()` are supported as methods on `BitReg` values. Checking method names at compile time ensures that every call refers to an operation actually defined for that type.

Erroneous code example:

```qx
void main() {
    QReg r = qreg(2);
    BitReg b = measure(r);
    println("{}", b.as_string());  // error[Q0220]
}
```

To fix it, replace the unknown `as_string()` call on `b` with `to_binary()`, one of the two methods available on `BitReg` values alongside `as_int()`.

```qx
void main() {
    QReg r = qreg(2);
    BitReg b = measure(r);
    println("{}", b.to_binary());
}
```

## Q0221: `consume` used on a classical parameter

A function parameter with a classical type, such as `int`, was declared with the `consume` modifier. In `.qx`, `consume` applies only to quantum parameter types (`Qubit` and `QReg`), because qubits have linear ownership that can be transferred into a function. Classical values are always copied when passed as arguments, so ownership transfer does not apply to them.

Erroneous code example:

```qx
int twice(consume int n) {  // error[Q0221]
    return n * 2;
}

void main() {
    println("{}", twice(2));
}
```

To fix it, remove `consume` from the `int` parameter `n` in `twice` so the classical argument is passed normally as a copied value.

```qx
int twice(int n) {
    return n * 2;
}

void main() {
    println("{}", twice(2));
}
```

## Q0222: `var` used with an unsupported type

The `var` keyword was used to declare a variable whose initializer does not have type `int`, `double`, or `boolean`. In `.qx`, `var` is allowed only for `int`, `double`, and `boolean` values. Restricting `var` keeps other types, such as `Qubit` and `Bit`, explicit in declarations rather than inferred.

Erroneous code example:

```qx
void main() {
    var q = qubit();  // error[Q0222]
    discard(q);
}
```

To fix it, replace `var` with the explicit type `Qubit` in the declaration of `q`.

```qx
void main() {
    Qubit q = qubit();
    discard(q);
}
```

## Q0223: invalid cast between unsupported types

A cast expression attempted to convert between types other than `int` and `double`. In `.qx`, explicit casts are allowed only between `int` and `double`, converting in either direction. Disallowing other casts prevents conversions from unrelated types such as `boolean`.

Erroneous code example:

```qx
void main() {
    boolean ok = true;
    int n = (int) ok;  // error[Q0223]
    println("{}", n);
}
```

To fix it, cast a `double` value (`d`) to `int` instead of casting a `boolean` value.

```qx
void main() {
    double d = 2.7;
    int n = (int) d;
    println("{}", n);
}
```

## Q0301: use of a consumed qubit

A `Qubit` was used after it had already been consumed by being measured, discarded, or moved. In `.qx`, qubits have linear types checked at compile time, so once a qubit is consumed it is no longer available for further operations. This prevents a program from applying gates or other operations to a qubit whose quantum state has already been measured, discarded, or transferred.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    Bit b = measure(q);
    h(q);  // error[Q0301]
    println("{}", b);
}
```

To fix it, apply `h(q)` before calling `measure(q)` so that `q` is used only before it is consumed.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0302: unconsumed qubit at scope exit or `return`

A `Qubit` or `QReg` value, including a `consume` parameter that is not consumed on some path, reached the end of its scope or a `return` statement without being consumed. Because qubits in `.qx` have linear types checked at compile time, each value must be explicitly accounted for. While the Rust library would silently drop such a qubit, `.qx` requires an explicit `measure` or `discard` so that a forgotten measurement or an unintended leftover qubit is caught at compile time.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();  // error[Q0302]
    h(q);
}
```

To fix it, consume `q` before the end of `main` by calling `measure(q)` (or `discard(q)` if the measurement result is not needed). In the corrected program, `q` is measured into `Bit b` and printed before `main` ends.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0303: same qubit passed twice in one call

This error occurs when the same `Qubit` appears more than once in one call to `cx`, `ccx`, or a user function. A whole register counts as overlapping each of its elements, so passing `r` and `r[0]` together is also rejected. The check applies at compile time when possible, and duplicates that depend on runtime indices, such as `cx(r[i], r[j])` with `i == j`, are reported with the same code when the call runs.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    cx(q, q);  // error[Q0303]
    discard(q);
}
```

To fix it, allocate a second `Qubit` (`t`) with `qubit()` and pass distinct qubits to `cx(q, t)`. Make sure both `q` and `t` are consumed with `discard` before leaving scope.

```qx
void main() {
    Qubit q = qubit();
    Qubit t = qubit();
    cx(q, t);
    discard(q);
    discard(t);
}
```

## Q0304: qubit consumed in only one `if` branch

This error occurs when an `if` statement consumes a `Qubit` in one branch while leaving it unconsumed in the other, including when there is no `else` branch. Because `.qx` checks linear qubit types at compile time, the compiler must know statically whether a `Qubit` is live or consumed after the `if` statement finishes. When the two branches disagree, the qubit's state after the `if` would depend on the runtime condition.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    boolean done = true;
    if done {
        discard(q);  // error[Q0304]
    }
}
```

To fix it, add an `else` branch that also calls `discard(q)` so `q` is consumed in both branches of the `if` statement. Both paths then leave `q` in the same consumed state once control flow rejoins.

```qx
void main() {
    Qubit q = qubit();
    boolean done = true;
    if done {
        discard(q);
    } else {
        discard(q);
    }
}
```

## Q0305: outer qubit consumed inside a `for` loop

This error occurs when a `Qubit` declared outside a `for` loop is consumed inside the loop body, such as by calling `measure` or `discard`. A linear `Qubit` can only be consumed once, so consuming an outer qubit inside a loop would attempt to consume the same qubit again on later iterations. The compiler also reports `Q0302` for the outer qubit because a loop may run zero times, leaving the qubit unconsumed when it goes out of scope.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    for i in 0..2 {
        Bit b = measure(q);  // error[Q0305]
    }
}
```

To fix it, move the `qubit()` allocation inside the `for` loop body so each iteration allocates and consumes its own `Qubit` with `measure`. Because `q` is created and measured within the same iteration, it is neither consumed more than once nor left unconsumed after the loop.

```qx
void main() {
    for i in 0..2 {
        Qubit q = qubit();
        Bit b = measure(q);
        println("{}", b);
    }
}
```

## Q0306: move of a single qubit from a register

This error occurs when an individual qubit indexed from a `QReg` is moved out of the register, such as by passing it to `measure`. A `QReg` is tracked as a single linear value at compile time, so consuming one element would leave the register partially consumed while still in scope. Gates can still use individual indexed qubits without consuming them, whereas linear ownership applies to the register as a whole.

Erroneous code example:

```qx
void main() {
    QReg r = qreg(2);
    Bit b = measure(r[0]);  // error[Q0306]
    discard(r);
}
```

To fix it, keep gate calls such as `h(r[0])` on indexed elements, and call `measure(r)` on the entire `QReg` instead of on a single element. Measuring the whole register consumes `r` cleanly and returns a `BitReg`.

```qx
void main() {
    QReg r = qreg(2);
    h(r[0]);
    BitReg b = measure(r);
    println("{}", b);
}
```

## Q0307: reassignment of a quantum variable

This error occurs when a new value is assigned to an existing quantum variable after its declaration. Because `.qx` enforces linear qubit types at compile time, overwriting a quantum variable would lose track of the lifetime of the qubit previously bound to it. Disallowing reassignment ensures that each quantum variable tracks a single linear resource from declaration until consumption.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    q = qubit();  // error[Q0307]
    discard(q);
}
```

To fix it, consume the first `Qubit` with `discard(q)` and declare a new variable `p` for the second `qubit()` allocation instead of reassigning `q`. Then consume `p` with `discard(p)` before the end of the scope.

```qx
void main() {
    Qubit q = qubit();
    discard(q);
    Qubit p = qubit();
    discard(p);
}
```

## Q0308: disallowed operation inside an `if` on a `Bit`

An `if` statement whose condition is a `Bit` measurement result contains a disallowed operation such as `println`. Because a `.qx` program builds a single quantum circuit, the outcome of `measure` is not known until the circuit is simulated or executed. For that reason, only gates, `reset`, `for`, nested `if`, classical variable declarations, and calls to gate-only functions are allowed inside a measurement-conditioned `if`.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    if b {
        println("one");  // error[Q0308]
    }
}
```

To fix it, move the `println` call out of the `if` block so it runs after the measurement. In the corrected program, the measurement result `b` is printed directly instead of inside the `if`.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0309: outer variable assignment inside an `if` on a `Bit`

This error occurs when code inside an `if` whose condition is a `Bit` measurement result assigns to a variable declared outside the `if`. Because the condition comes from a measurement, its value depends on the shot when the quantum circuit runs, whereas an assignment to an outer classical variable would not depend on the shot. The compiler rejects the assignment so that classical variables outside the branch do not appear to change based on per-shot measurement outcomes.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    int n = 0;
    if b {
        n = 1;  // error[Q0309]
    }
    println("{}", n);
}
```

To fix it, avoid assigning to an outer variable inside the `if` and use the measurement result directly. The corrected program removes `n` and the `if` block, passing `b` straight to `println`.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
```

## Q0310: shadowing a qubit that is still alive

This error occurs when a declaration introduces a name that shadows an existing `Qubit` variable while that qubit is still alive. Because `Qubit` values follow linear ownership rules checked at compile time, every allocated qubit must eventually be consumed. Shadowing a live qubit would hide the original binding and make it impossible to consume.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    Qubit q = qubit();  // error[Q0310]
    discard(q);
}
```

To fix it, consume the first qubit before declaring another variable with the same name, or choose a different name. The corrected program calls `discard` on the first `q` before declaring the second `q` and discarding it as well.

```qx
void main() {
    Qubit q = qubit();
    discard(q);
    Qubit q = qubit();
    discard(q);
}
```

## Q0311: invalid expression used as a gate operand

This error occurs when an expression other than a `Qubit` variable or a register element such as `r[i]` is passed as a gate operand. Because `.qx` checks linear qubit ownership at compile time, a gate operates on a qubit that remains alive afterward and must later be consumed. Requiring a qubit variable or register element lets the compiler track the qubit's lifetime and prevents an unnamed temporary value from being lost.

Erroneous code example:

```qx
void main() {
    h(qubit());  // error[Q0311]
}
```

To fix it, bind the allocated qubit to a variable before passing it to the gate. The corrected program stores the result of `qubit()` in `q`, applies `h` to `q`, and then consumes `q` with `discard`.

```qx
void main() {
    Qubit q = qubit();
    h(q);
    discard(q);
}
```

## Q0312: newly allocated qubit or register never consumed

This error occurs when a newly allocated qubit or register is neither bound to a variable nor consumed. In `.qx`, qubits have linear types checked at compile time, so every allocated qubit must be explicitly consumed by `measure` or `discard`. Ignoring an allocation leaves a live qubit with no name and no way to consume it.

Erroneous code example:

```qx
void main() {
    qubit();  // error[Q0312]
}
```

To fix it, store the allocated qubit in a variable and consume it with `measure` or `discard`. The corrected program binds the result of `qubit()` to `q` and passes `q` to `discard`.

```qx
void main() {
    Qubit q = qubit();
    discard(q);
}
```

## Q0313: consuming a borrowed qubit parameter

This error occurs when a function attempts to measure, discard, move, or pass to a `consume` parameter a qubit parameter that was only borrowed. Without the `consume` modifier on the parameter type, the caller retains ownership of the qubit and remains responsible for measuring or discarding it after the call returns. Allowing the callee to consume a borrowed qubit would break linear ownership and could cause the same qubit to be consumed twice.

Erroneous code example:

```qx
Bit peek(Qubit q) {
    return measure(q);  // error[Q0313]
}

void main() {
    Qubit q = qubit();
    Bit b = peek(q);
    discard(q);
    println("{}", b);
}
```

To fix it, add `consume` before the parameter type (such as `consume Qubit q`) so the function takes ownership of the qubit and becomes responsible for measuring or discarding it. Then remove the caller's `discard` call, since the function now consumes the qubit.

```qx
Bit peek(consume Qubit q) {
    return measure(q);
}

void main() {
    Qubit q = qubit();
    Bit b = peek(q);
    println("{}", b);
}
```

## Q0314: disallowed function call inside a measurement `if`

This error occurs when an `if` statement whose condition is a measurement result calls a function that performs an operation disallowed inside a conditional block. A function called inside a measurement-conditioned `if` may only apply gates and `reset` to borrowed qubits. It cannot allocate, measure, discard, print, take a `consume` parameter, or return a qubit, because those operations cannot run conditionally on a measurement result.

Erroneous code example:

```qx
void reveal(Qubit q) {
    println("flipping");
    x(q);
}

void main() {
    Qubit coin = qubit();
    h(coin);
    Bit m = measure(coin);
    Qubit t = qubit();
    if m {
        reveal(t);  // error[Q0314]
    }
    discard(t);
}
```

To fix it, remove disallowed operations such as `println` from the function called inside the `if` block so it only applies gates or `reset` to borrowed qubits. If you still need those operations, call them before or after the `if` statement instead.

```qx
void flip(Qubit q) {
    x(q);
}

void main() {
    Qubit coin = qubit();
    h(coin);
    Bit m = measure(coin);
    Qubit t = qubit();
    if m {
        flip(t);
    }
    discard(t);
}
```

## Q0401: integer arithmetic overflow on `int`

This error occurs when an arithmetic operation produces a result outside the representable range of `int`, which is a 64-bit signed integer (`i64`). Instead of silently wrapping around to an unintended value, `.qx` reports an overflow error when a calculation exceeds the type's bounds.

Erroneous code example:

```qx
void main() {
    int n = 9223372036854775807;
    int more = n + 1;  // error[Q0401]
    println("{}", more);
}
```

To fix it, switch the variables and literals to `double` so the computation does not overflow `int`, or keep the arithmetic within the `int` range.

```qx
void main() {
    double n = 9223372036854775807.0;
    double more = n + 1.0;
    println("{}", more);
}
```

## Q0402: division or remainder by zero

This error occurs when a division or remainder operation has a divisor of `0`. Because division and remainder by zero are mathematically undefined, the operation cannot produce a valid result and fails with an error.

Erroneous code example:

```qx
void main() {
    int zero = 0;
    println("{}", 10 / zero);  // error[Q0402]
}
```

To fix it, replace the `0` divisor with a non-zero value, or ensure the divisor is non-zero before performing division or remainder.

```qx
void main() {
    int two = 2;
    println("{}", 10 / two);
}
```

## Q0403: index out of bounds

This error occurs when an indexing operation uses an index outside the valid bounds of the value being indexed, such as a `QReg`. Indexing is zero-based, so valid indices run from `0` through one less than the length. Rejecting out-of-bounds indices prevents accessing a nonexistent element.

Erroneous code example:

```qx
void main() {
    QReg r = qreg(2);
    h(r[2]);  // error[Q0403]
    discard(r);
}
```

To fix it, allocate the `QReg` with enough elements to cover the target index—such as `qreg(3)` when indexing element `2`—or adjust the index so it stays within bounds.

```qx
void main() {
    QReg r = qreg(3);
    h(r[2]);
    discard(r);
}
```

## Q0405: function call depth limit exceeded

The runtime enforces a maximum function call depth of 200 and reports `Q0405` when nested calls go past that limit. This usually happens when a recursive function has no base case or never reaches one. Bounding the call depth stops runaway recursion from exhausting the stack and crashing execution.

Erroneous code example:

```qx
int count(int n) {
    return count(n + 1);  // error[Q0405]
}

void main() {
    println("{}", count(0));
}
```

To fix it, ensure every recursive function has a reachable base case that stops further calls before the depth reaches 200. In the corrected program, `count` returns `n` once `n >= 10`, terminating the recursion.

```qx
int count(int n) {
    if n >= 10 {
        return n;
    }
    return count(n + 1);
}

void main() {
    println("{}", count(0));
}
```

## Q0406: quantum circuit simulation failed

When `qxide run` executes a circuit, the simulator reports `Q0406` if simulation fails with a `SimError`, such as exceeding the memory limit. State-vector simulation requires memory that grows rapidly with the number of qubits, so simulating too many qubits exceeds the simulator's memory limit. Stopping the run before allocation prevents the process from exhausting system memory.

Erroneous code example:

```qx
void main() {  // error[Q0406]
    QReg r = qreg(30);
    t(r[0]);
    BitReg b = measure(r);
    println("{}", b);
}
```

To fix it, allocate fewer qubits so the simulation fits within the memory limit, as the corrected program does by reducing the `qreg` size from `30` to `10`. If you do not need to simulate the circuit, you can also run `qxide qasm` to export it as OpenQASM 3 without running the simulator.

```qx
void main() {
    QReg r = qreg(10);
    t(r[0]);
    BitReg b = measure(r);
    println("{}", b);
}
```

## Q0407: function exited without returning a value

The interpreter raises `Q0407` as an internal safety net if a function with a return type finishes executing without returning a value. The checker already rejects missing return paths with `Q0213` before the program runs, so normal execution should never reach this error. It exists so the interpreter fails explicitly rather than continuing with a missing value if an unchecked path slips through.

Because `Q0213` should always catch missing returns before execution, seeing `Q0407` indicates a bug in `qxide`. Report the issue along with the program that triggered it.

## Q0408: negative register size for `qreg`

Passing a negative integer to `qreg` triggers `Q0408` at runtime. A quantum register (`QReg`) represents a count of qubits to allocate in the circuit, so a negative size is meaningless. Because the argument can come from a runtime `int` value, the interpreter verifies that it is non-negative before creating the register.

Erroneous code example:

```qx
void main() {
    int n = -1;
    QReg r = qreg(n);  // error[Q0408]
    discard(r);
}
```

To fix it, pass a non-negative `int` to `qreg`. In the corrected program, `n` is changed from `-1` to `1` so the register allocation succeeds.

```qx
void main() {
    int n = 1;
    QReg r = qreg(n);
    discard(r);
}
```

## Q0409: total qubit allocations exceeded the limit

The runtime reports `Q0409` when the cumulative number of qubits allocated across a program exceeds 1048576 (`2^20`). This limit counts all allocations over the entire run, even when earlier qubits or registers are discarded. Capping total qubit allocations keeps circuit construction bounded and prevents runaway allocation.

Erroneous code example:

```qx
void main() {
    QReg r = qreg(2000000);  // error[Q0409]
    discard(r);
}
```

To fix it, keep the total number of qubits allocated across the program at or below 1048576. In the corrected program, the `qreg` size is reduced from `2000000` to `1000`, staying within the limit.

```qx
void main() {
    QReg r = qreg(1000);
    discard(r);
}
```

## Q0410: calling `as_int()` on a register exceeding 64 bits

This error occurs when `as_int()` is called on a `BitReg` with more than 64 bits. The `as_int()` method supports at most 64 bits because a wider register cannot fit into a 64-bit integer. Any call to `as_int()` on a register exceeding 64 bits is rejected with `Q0410`.

Erroneous code example:

```qx
void main() {
    QReg r = qreg(65);
    BitReg b = measure(r);
    println("{}", b.as_int());  // error[Q0410]
}
```

To fix it, keep the register at 64 bits or fewer, as in `qreg(64)`, so `as_int()` can convert the resulting `BitReg`. If you need a register wider than 64 bits, print it with `to_binary()` or as an array using a plain placeholder instead.

```qx
void main() {
    QReg r = qreg(64);
    BitReg b = measure(r);
    println("{}", b.as_int());
}
```
