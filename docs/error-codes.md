# Qxide Error Codes

This catalog lists every .qx compiler error code; every diagnostic prints a code such as `Q0302`, and running `qxide explain Q0302` prints that code's entry from this file. Codes are never reused and are grouped by category: `Q01xx` for lexing and parsing, `Q02xx` for names and types, `Q03xx` for qubit ownership (linearity), `Q04xx` for errors found while running the program and for resource limits, and `Q05xx` for modules and imports. Each entry includes an explanation, an example that triggers the error, and a corrected version.

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

Error `Q0106` occurs when syntax nesting exceeds the compiler's maximum depth of 128 levels. Long left-associative chains also count toward this limit, because each binary operator in a chain like `a + b + c` and each index or method call in a chain like `r[0][0]` nests the syntax tree one level deeper. The parser enforces this limit to keep recursion bounded and prevent excessive stack usage and interpreter stack overflows on deeply nested input.

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

## Q0214: `{}` placeholder in a string value

A string literal used as a `String` value contains a `{}` placeholder. In `.qx`, string literals evaluate to immutable `String` values that are combined with `+`, and only the format string passed as the first argument to `println` may contain `{}` placeholders. Outside of `println`, a placeholder has no argument to fill it and is therefore rejected. To include literal brace characters in a string, write `{{` and `}}`.

Erroneous code example:

```qx
void main() {
    String name = "Ada";
    String greeting = "hello {}";  // error[Q0214]
    println("{}", greeting);
}
```

To fix it, build the text with `+` instead of using a `{}` placeholder.

```qx
void main() {
    String name = "Ada";
    String greeting = "hello " + name;
    println("{}", greeting);
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

## Q0315: qubit consumed in short-circuit operand

Q0315 occurs at compile time when a qubit is consumed (by `measure`, `discard`, or a `consume` parameter) inside the right operand of `&&` or `||`. Because `&&` and `||` use short-circuit evaluation, the right operand is skipped at run time when the left operand already decides the result, leaving the qubit consumed on some runs and not others—like consuming a qubit in only one branch of an `if` (Q0304). Consuming a qubit in the left operand is fine because it always runs, and borrowing a qubit on the right (such as applying gates via a non-`consume` parameter) is also fine. After this error is reported, the qubit counts as consumed.

Erroneous code example:

```qx
int check(consume Qubit q) {
    Bit m = measure(q);
    return 1;
}

void main() {
    Qubit q = qubit();
    boolean ready = true;
    boolean go = ready || check(q) == 1;  // error[Q0315]
    println("{}", go);
}
```

To fix this error, consume the qubit in its own statement before the condition and use the stored result:

```qx
int check(consume Qubit q) {
    Bit m = measure(q);
    return 1;
}

void main() {
    Qubit q = qubit();
    boolean ready = true;
    int result = check(q);
    boolean go = ready || result == 1;
    println("{}", go);
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

## Q0411: non-finite rotation angle

A runtime error is reported while building the circuit when `rx`, `ry`, or `rz` receives an angle that is NaN or infinite. In `.qx`, `double` arithmetic follows IEEE 754 like Java, so `0.0 / 0.0` evaluates to NaN and `1.0 / 0.0` evaluates to infinity instead of raising an error. A non-finite rotation angle would make every amplitude of the simulated state NaN and cannot be written as OpenQASM 3. The library methods `Circuit::rx`, `Circuit::ry`, and `Circuit::rz` panic for the same input.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    double steps = 0.0;
    rx(q, PI / steps);  // error[Q0411]
    Bit m = measure(q);
    println("{}", m);
}
```

To fix this error, make sure the divisor is not zero or otherwise ensure the angle passed to `rx`, `ry`, or `rz` is finite.

```qx
void main() {
    Qubit q = qubit();
    double steps = 4.0;
    rx(q, PI / steps);
    Bit m = measure(q);
    println("{}", m);
}
```

## Q0412: internal error between the checker and the interpreter

The interpreter raises Q0412 as an internal safety net when a program that passed static checks cannot be executed because the checker and the interpreter disagree—for example, when an operand that the checker considered alive turns out to already be consumed at run time. It exists so the interpreter fails explicitly on such mismatches rather than continuing in an invalid state, and replaces the misleading Q0311 error that older versions printed in this situation.

Because static checking should catch invalid programs before execution, seeing Q0412 always indicates a bug in qxide. Report the issue along with the program that triggered it.

## Q0413: execution step limit exceeded

The interpreter counts its work in steps: one for every evaluated expression, loop iteration, and function call, one for every variable it compares while looking up a name, and one per qubit or bit for whole-register work such as `qreg`, `measure` or `discard` on a register, passing a register to a function, or printing one. It reports `Q0413` when a program needs more steps than the limit, which is 1073741824 (2^30) for the `qxide` command, 67108864 (2^26) in the browser playground, and 2147483648 (2^31) for each call in a desktop app, whose page can cancel a call that runs too long. Building, comparing, measuring, or passing a `String` to or from a standard library call costs 8 steps per byte of text, so the limit also bounds the memory that strings can take. The limit stops programs that would otherwise run for hours or forever, such as a loop with a huge bound or a recursion that doubles at every level, while ordinary programs use far fewer steps.

Erroneous code example:

```qx
void main() {
    for i in 0..9223372036854775807 {  // error[Q0413]
    }
}
```

To fix it, make sure every loop bound and every recursion is as large as intended. In the corrected program, the loop runs 1000 times.

```qx
void main() {
    int total = 0;
    for i in 0..1000 {
        total = total + i;
    }
    println("{}", total);
}
```

## Q0414: circuit operation limit exceeded

Every gate, measurement, `reset`, and `if` block on a measured bit is recorded as an operation of the circuit the program builds, and the interpreter reports `Q0414` once the circuit would hold more operations than the limit: 16777216 (2^24) for the `qxide` command, 1048576 (2^20) in the browser playground, and 4194304 (2^22) for each call in a desktop app, where up to four calls run at once. The recorded circuit is kept in memory, so the limit bounds the memory a program can use before it is simulated or exported.

Erroneous code example:

```qx
void main() {
    Qubit q = qubit();
    for i in 0..100000000 {
        h(q);  // error[Q0414]
    }
    discard(q);
}
```

To fix it, check loops that apply gates for bounds that are much larger than intended. The corrected program applies `h` 100 times.

```qx
void main() {
    Qubit q = qubit();
    for i in 0..100 {
        h(q);
    }
    discard(q);
}
```

## Q0415: output size limit exceeded

`Q0415` reports output that would grow past the limit of 268435456 bytes (256 MiB) for the `qxide` command, 1048576 bytes (1 MiB) in the browser playground, or 33554432 bytes (32 MiB) for each call in a desktop app, the most a call result can carry to the page. The limit applies to the text printed with `println`, to the table that `qxide run --shots N` prints (each distinct measurement record prints its own copy of the program output), and to the OpenQASM text of `qxide qasm`, which is checked before it is built. Output is held in memory until the program ends, so the limit bounds that memory.

Erroneous code example:

```qx
void main() {
    for i in 0..100000000 {
        println("step {}", i);  // error[Q0415]
    }
}
```

To fix it, print less, or print outside of loops. The corrected program prints 10 lines.

```qx
void main() {
    for i in 0..10 {
        println("step {}", i);
    }
}
```

## Q0416: `if` blocks on measured bits nested too deeply

An `if` on a `Bit` becomes a conditional block of the circuit, and a function called inside such a block can open further blocks, so nesting is counted across function calls. The interpreter reports `Q0416` when blocks would be nested more than 64 levels deep (128 levels for a call in a desktop app). Every level adds indentation to each line of the OpenQASM output and recursion to the simulators, so deep nesting reached through recursive calls could otherwise produce very large output from a short program. A single function can still nest as deeply as the parser allows.

Erroneous code example:

```qx
void flip(Qubit q, Bit b, int n) {
    if (b) {  // error[Q0416]
        if (n > 0) {
            flip(q, b, n - 1);
        }
        x(q);
    }
}

void main() {
    Qubit a = qubit();
    h(a);
    Bit b = measure(a);
    Qubit q = qubit();
    flip(q, b, 100);
    Bit r = measure(q);
    println("{}", r);
}
```

To fix it, test the bit once and do the repeated work inside that one block. The corrected program applies the same 101 `x` gates under a single condition.

```qx
void flip(Qubit q, int n) {
    for i in 0..n {
        x(q);
    }
}

void main() {
    Qubit a = qubit();
    h(a);
    Bit b = measure(a);
    Qubit q = qubit();
    if (b) {
        flip(q, 101);
    }
    Bit r = measure(q);
    println("{}", r);
}
```

## Q0417: simulation work limit exceeded

Before simulating, `qxide run` estimates the work of all shots from the number of qubits, the recorded operations, and the backend: a state-vector shot updates every one of the 2^n amplitudes for each operation, and a stabilizer shot updates every row of the tableau for each gate and may combine rows for each measurement, which grows with the cube of the number of qubits. Every shot simulates the whole circuit again, so the estimate is multiplied by the number of shots. `Q0417` reports an estimate above the limit, which corresponds to at most about three minutes of simulation for the `qxide` command and a few seconds in the browser playground. Desktop apps use the same limit as the `qxide` command, because a single shot cannot be cancelled. Branch bodies are counted as if they always run.

Erroneous code example:

```qx
void main() {  // error[Q0417]
    QReg r = qreg(32768);
    h(r[0]);
    for i in 1..32768 {
        cx(r[0], r[i]);
    }
    BitReg m = measure(r);
    println("{}", m[0]);
}
```

To fix it, use fewer qubits, operations, or shots, or export the circuit with `qxide qasm` without simulating it. The corrected program prepares the same state on 1024 qubits.

```qx
void main() {
    QReg r = qreg(1024);
    h(r[0]);
    for i in 1..1024 {
        cx(r[0], r[i]);
    }
    BitReg m = measure(r);
    println("{}", m[0]);
}
```

## Q0418: too many errors to show

`qxide check`, `qxide run`, and `qxide qasm` print at most 50 errors and stop the error report before it passes the output limit of `Q0415`; each echoed source line is also shortened to a window of 120 characters around the error column. Desktop apps, including Qxide Studio, show at most 100 errors with windows of 240 characters. When errors are left out, the report ends with `Q0418`, which says how many were not shown. Without these limits, a short file with thousands of errors on one long line produced gigabytes of output, because every error repeated the whole line.

Erroneous code example:

```qx
void main() {
    int total = a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15 + a16 + a17 + a18 + a19 + a20 + a21 + a22 + a23 + a24 + a25 + a26 + a27 + a28 + a29 + a30 + a31 + a32 + a33 + a34 + a35 + a36 + a37 + a38 + a39 + a40 + a41 + a42 + a43 + a44 + a45 + a46 + a47 + a48 + a49 + a50 + a51 + a52 + a53 + a54;  // error[Q0418]
    println("{}", total);
}
```

To fix it, correct the errors that are shown and run the command again to see the rest. Here every name was undeclared; the corrected program declares the value it prints.

```qx
void main() {
    int total = 0;
    println("{}", total);
}
```

## Q0501: unknown package in an import

The first segment of an `import` path names a package that is neither the current package nor a dependency declared in `[dependencies]` in `qxide.toml`. Package names with hyphens, such as `bell-utils`, are imported using underscores (`bell_utils`). Requiring every external package to be declared in `qxide.toml` keeps builds reproducible and ensures dependencies are explicitly resolved before compilation.

Erroneous code example:

```java
// qxide.toml of package `app` has no [dependencies] yet
// src/main.qx
import bell_utils.bell_pair;  // error[Q0501]
```

To fix it, run `qxide add bell-utils`, which adds `bell-utils` to `[dependencies]` in `qxide.toml` and installs it so `bell_utils` can be imported.

```java
// after `qxide add bell-utils`
// src/main.qx
import bell_utils.bell_pair;
```

## Q0502: module file not found

An `import` statement refers to a module whose corresponding `.qx` file under `src/` does not exist or cannot be read. Each module path maps to a file relative to the package's `src/` directory (for example, `app.gates` maps to `src/gates.qx` and the package root `app` maps to `src/lib.qx`), and the diagnostic note shows the path the compiler tried to open. Checking that every imported module resolves to an existing source file catches typos and missing files before type checking.

Erroneous code example:

```java
// src/main.qx of package `app`; there is no src/gates.qx
import app.gates.entangle;  // error[Q0502]
```

To fix it, create the missing `src/gates.qx` module file (or correct the module path in the `import` statement) and define the `public` function you want to import.

```java
// src/gates.qx
public void entangle(Qubit control, Qubit target) {
    cx(control, target);
}
```

## Q0503: function not found in module

An `import` statement names a function that does not exist in the target module, usually because of a typo or because the function has not been defined there. The module file itself was found, but none of its function declarations match the requested name. Reporting missing imports upfront prevents unresolved calls from failing later during compilation.

Erroneous code example:

```java
// src/main.qx
import bell_utils.bellpair;  // error[Q0503]: the function is `bell_pair`
```

To fix it, check the spelling of the function in the target module and update the `import` statement to use the exact name `bell_pair`.

```java
// src/main.qx
import bell_utils.bell_pair;
```

## Q0504: imported function is not `public`

An `import` statement names a function that exists in the target module, but the function is not marked `public`. In `.qx`, functions are private to their defining module by default so that helper functions remain an implementation detail that can change without breaking callers. Only functions explicitly declared `public` are part of a module's interface and may be imported by other modules.

Erroneous code example:

```java
// src/gates.qx
void entangle(Qubit control, Qubit target) {
    cx(control, target);
}

// src/main.qx
import app.gates.entangle;  // error[Q0504]
```

To fix it, mark `entangle` as `public` in `src/gates.qx` if it is intended to be imported by other modules.

```java
// src/gates.qx
public void entangle(Qubit control, Qubit target) {
    cx(control, target);
}
```

## Q0505: import cycle between modules

Two or more modules import each other, directly or transitively, forming an import cycle; the error message displays the chain of modules involved. The compiler rejects circular imports so that modules always load in a well-defined, acyclic order. Breaking cycles also keeps module boundaries clearer and easier to maintain.

Erroneous code example:

```java
// src/a.qx
import app.b.g;  // error[Q0505]: app.a -> app.b -> app.a
public void f(Qubit q) {
    g(q);
}

// src/b.qx
import app.a.f;
public void g(Qubit q) {
    h(q);
}
```

To fix it, remove the unneeded import from `src/b.qx`, or move any shared function into a separate module that both `app.a` and `app.b` can import.

```java
// src/a.qx
import app.b.g;
public void f(Qubit q) {
    g(q);
}

// src/b.qx (no longer imports app.a)
public void g(Qubit q) {
    h(q);
}
```

## Q0506: imported name conflicts with an existing function

An `import` statement introduces a function name that clashes either with a function defined in the same file or with a different function imported under the same name. Allowing two distinct definitions to claim the same name in a module's top-level scope would make it ambiguous which function a call refers to. Rejecting the conflict keeps every top-level function name in the file unambiguous.

Erroneous code example:

```java
// src/main.qx
import bell_utils.bell_pair;  // error[Q0506]

void bell_pair(Qubit a, Qubit b) {
    h(a);
}
```

To fix it, rename the local function so it does not clash with the imported name, or drop the `import` if you only need the local function.

```java
// src/main.qx
import bell_utils.bell_pair;

void my_pair(Qubit a, Qubit b) {
    h(a);
}
```

## Q0507: ambiguous call from wildcard imports

A function call matches `public` functions with the same name brought in by two or more wildcard imports (`import pkg.module.*;`). Overlapping wildcard imports do not trigger an error on their own; the diagnostic is reported at the call site only when the colliding name is actually used. Because the compiler cannot safely choose between the competing definitions, you must resolve the ambiguity explicitly.

Erroneous code example:

```java
// src/main.qx
import bell_utils.*;
import other.*;

void main() {
    Qubit q = qubit();
    flip(q);  // error[Q0507]: both modules have a public `flip`
    discard(q);
}
```

To fix it, import the intended function explicitly with a single-function import such as `import other.flip;`. Because single-function imports take priority over wildcard imports, the call to `flip` resolves unambiguously.

```java
// src/main.qx
import bell_utils.*;
import other.*;
import other.flip;

void main() {
    Qubit q = qubit();
    flip(q);
    discard(q);
}
```

## Q0508: `import` used outside a project

A lone `.qx` file contains an `import` statement, but there is no `qxide.toml` in its directory or any parent directory. Without a project manifest, the compiler has no package root or `[dependencies]` table to resolve package and module paths from. Restricting `import` to files inside a project ensures module resolution is always anchored to a known `qxide.toml`.

Erroneous code example:

```qx
import bell_utils.bell_pair;  // error[Q0508]

void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    bell_pair(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("{} {}", ma, mb);
}
```

To fix it, either move the file into a project with a `qxide.toml` that depends on `bell-utils`, or, for a lone file, write the gates directly as below.

```qx
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("{} {}", ma, mb);
}
```

## Q0509: import path is too short

An `import` statement contains only a bare package name instead of naming a function or wildcard. Every `import` in `.qx` must name a module and a function (`import pkg.function;` or `import pkg.module.function;`), or end with `.*` to import all `public` functions from a module. Requiring a function name or wildcard makes it explicit which symbols enter the file's scope.

Erroneous code example:

```java
// src/main.qx
import bell_utils;  // error[Q0509]
```

To fix it, name the specific function to import from the module (such as `bell_utils.bell_pair`), or end the path with `.*` to import all `public` functions from that module.

```java
// src/main.qx
import bell_utils.bell_pair;
```

## Q0510: `import` after a function declaration

An `import` statement appears after one or more function declarations in a `.qx` file. All `import` statements must come first in the file before any functions are declared. Keeping imports grouped at the top ensures a module's dependencies are immediately visible and parsed before any function bodies.

Erroneous code example:

```java
// src/main.qx
void main() {
}

import bell_utils.bell_pair;  // error[Q0510]
```

To fix it, move the `import` statement to the top of the file before all function declarations.

```java
// src/main.qx
import bell_utils.bell_pair;

void main() {
}
```

## Q0601: `qxide.toml` not found

A package command such as `qxide install`, `qxide add`, `qxide remove`, or `qxide publish` requires a project, but no `qxide.toml` exists in the current directory or any parent directory.

Run the command from inside a project directory, or create a new project with `qxide new <name>`, which writes `qxide.toml` and `src/main.qx`.

## Q0602: invalid `qxide.toml`

The project manifest is not valid TOML or violates the manifest rules. `[package]` must define `name` and `version`, where `version` is a semver version such as `1.2.3` and `name` is at most 64 characters of lowercase ASCII letters, digits, and single hyphens, starting with a letter and not ending with a hyphen. `qxide` and dependency requirements must be valid semver requirements, each dependency must be a requirement string or an inline table with `version`, `path`, or both, and all values must have the expected types.

Correct the key reported in the error message so `qxide.toml` is valid TOML and follows the manifest schema.

```toml
[package]
name = "bell-utils"
version = "0.1.0"
qxide = ">=0.7"

[dependencies]
other-pkg = "0.3"
local-pkg = { version = "0.1", path = "../local-pkg" }
```

## Q0603: invalid `qxide.lock`

The lock file cannot be used because it is not valid TOML, has an unsupported format version, contains an invalid package or dependency name, has an invalid `source`, has a checksum that is not a lowercase 64-digit SHA-256 hexadecimal string, or lists the same package more than once. `qxide.lock` is generated by `qxide install` and `qxide add` and should not be edited by hand.

Delete `qxide.lock` and run `qxide install` to regenerate it. Regenerating re-resolves dependencies to the newest matching versions, so review the new lock file before committing it.

## Q0604: package not found

A dependency could not be found: the package name does not exist in the registry index, a path dependency's folder has no readable `qxide.toml`, the package name in that `qxide.toml` differs from the dependency name, or two path dependencies specify different folders for the same package name.

Check the package spelling (for example with `qxide search <word>`), verify the configured registry (`--registry` or `QXIDE_REGISTRY`), and check the `path` entries in `qxide.toml`.

## Q0605: no version matches the requirement

The package exists in the registry, but no published version that is not yanked satisfies the requirement. A `note:` line lists the available versions.

Update the requirement in `qxide.toml` to match one of the available versions, or add the dependency again with an explicit requirement using `qxide add <package>@<requirement>`.

## Q0606: version conflict

Dependency resolution failed because only one version of each package can be included in a build and resolution does not backtrack. This happens when two packages require incompatible versions of the same package (with `note:` lines showing which package requires what), when a package depends on itself, or when resolution does not converge.

Align the requirements (for example by updating a dependency that pins an older version), and remove any self-dependency from `[dependencies]`.

## Q0607: checksum mismatch

The SHA-256 checksum of a downloaded or cached archive differs from the checksum recorded in `qxide.lock`, or the registry index now lists a different checksum for a locked version. Registry versions are immutable, so this indicates that the archive or index was changed or tampered with; `note:` lines show the expected and actual checksums, and a corrupted cached archive is downloaded again automatically.

Do not bypass this error; investigate the registry or local cache. Only if the change is known to be legitimate, delete `qxide.lock` and run `qxide install` to lock the new archive.

## Q0608: registry unavailable or download failed

The registry index or a package archive could not be downloaded due to a network or TLS error, an HTTP status other than 200 or 404, a `curl` transfer that did not finish successfully (such as a connection cut off midway, in which case nothing is cached), `curl` not being installed, or an unsupported URL. HTTPS downloads use the system `curl` executable, and only `https://` URLs, `file://` URLs, and local paths are allowed; plain `http://` URLs are refused.

Check your network connection and verify that `curl` is installed, check `--registry` or `QXIDE_REGISTRY`, or pass `--offline` to work from the local cache when everything needed is already cached.

## Q0609: invalid registry index

The registry's JSON files cannot be used because `config.json` or `search.json` failed to parse, or a package index file has no usable entry. Unusable index lines (malformed JSON, a different package name, a version with build metadata, or a repeated version, where the first line wins) are skipped, and this error is reported only when no usable line remains. This is a problem in the registry, not in your project.

Report the issue to the registry maintainers, or use another registry with `--registry` or `QXIDE_REGISTRY`.

## Q0610: unsafe or invalid package archive

A package archive was rejected before anything was unpacked because it contains an entry outside the package's root folder, an absolute or drive path, a `..` component, a symbolic or hard link, a non-portable file name (such as a trailing dot or space, a reserved Windows device name like `CON` or `NUL`, or a character disallowed on Windows), two paths differing only in letter case, too many files, an archive larger than 10 MiB, or a corrupt gzip or tar stream. Nothing is written to disk when an archive is rejected.

This indicates a broken or malicious package; report it to the registry maintainers and do not use that version.

## Q0611: dependency not installed

The project has dependencies, but either there is no `qxide.lock` file or a locked dependency is not unpacked in the local cache (`$QXIDE_HOME`, defaulting to `~/.qxide`). `qxide run`, `qxide check`, and `qxide qasm` never download packages.

Run `qxide install` in the project directory to install the dependencies.

## Q0612: `qxide.lock` is out of date

The lock file no longer matches `qxide.toml` or the dependency graph: a direct or transitive requirement changed, a path dependency's `qxide.toml` changed, a package comes from a different registry or source, or `qxide.lock` lists packages that nothing depends on. `qxide run`, `qxide check`, `qxide qasm`, and `qxide install --locked` report this error instead of changing the lock file.

Run `qxide install` without `--locked` to update `qxide.lock`, then commit the updated lock file.

## Q0613: `qxide` version requirement not met

The `qxide` requirement in the project's or a dependency's `qxide.toml` (for example `qxide = ">=0.7"`) is not satisfied by the running `qxide` version.

Upgrade `qxide`, or use a dependency version whose requirement your installed `qxide` satisfies. When writing a requirement, prefer the `>=0.7` style, because a caret requirement such as `^0.7` rejects `0.8`.

## Q0614: publish rejected

`qxide publish` refused the package, for example because `description`, `license`, or `qxide` is missing from `qxide.toml`, a dependency is path-only (with no `version`), `src/lib.qx` is missing, the package does not pass `qxide check`, the version already exists in the registry, the version includes build metadata (such as `1.0.0+build`), or a file name is not portable.

Correct the reported problem before publishing again. Published versions are immutable, so release changes under a new version number.

## Q0615: not available offline

Offline mode (`--offline` or `QXIDE_OFFLINE=1`) was requested, but an index file or archive needed by the command is not in the local cache under `$QXIDE_HOME`.

Run the command once without offline mode while connected so the required data is cached.

## Q0616: filesystem error

Reading or writing a file or folder failed (for example due to missing permissions or a full disk, with the path and operating system error shown in the message), or the cache folder could not be determined because neither `HOME` nor `USERPROFILE` is set.

Check the reported path and its permissions, or set `QXIDE_HOME` to a writable folder.

## Q0617: not a declared dependency

`qxide remove <package>` was given a package name that is not listed in `[dependencies]` of `qxide.toml`.

Check the spelling against `qxide.toml`. Package names use hyphens, whereas `.qx` imports use underscores (for example, `bell-utils` is imported as `bell_utils`).

## Q0701: unknown standard library module or function

The program imports a `std` module that does not exist or calls a function that the module does not define, such as `fs.read(...)` instead of `fs.read_text(...)`. Qxide validates standard library imports and member calls at compile time so typos and nonexistent APIs fail before execution. The available standard library modules are `std.log`, `std.time`, `std.fs`, `std.app`, and `std.qxide`.

Erroneous code example:

```qx
import std.net;  // error[Q0701]

void main() {
    println("hello");
}
```

To fix it, import an existing standard library module and call a function it provides.

```qx
import std.log;

void main() {
    log.info("hello");
}
```

## Q0702: standard module not available here

A standard library module was imported in an execution environment that does not offer it. Specifically, `std.app` and `std.qxide` exist only in desktop apps (`qxide app`), the playground and REPL offer only `std.log`, and `qxide qasm` refuses `std.fs` and `std.app` calls because exporting a circuit must not have side effects. Limiting modules per host keeps each execution target safe and predictable for its environment.

Erroneous code example:

```qx
import std.app;  // error[Q0702]

void main() {
    app.set_title("Bell lab");
}
```

To fix it, use only the standard modules offered by the current host, or move the code into a desktop app.

```qx
import std.log;

void main() {
    log.info("Bell lab");
}
```

## Q0703: standard module not permitted

The program imports a standard library module that exists in the current host but has not been granted permission. Under `qxide run`, `std.fs` requires `--allow fs`; in an app, the module name must be listed in `permissions` under the `[app]` section of `qxide.toml`; and packages used as dependencies may only import `std.log`. This rule enforces least privilege so code cannot touch files or host capabilities unless explicitly granted.

Erroneous code example:

```java
// qxide run note.qx   (no --allow fs)
import std.fs;  // error[Q0703]

void main() {
    fs.write_text("note.txt", "hello");
}
```

To fix it, grant the required permission via the CLI flag or `qxide.toml` as shown in the comments below.

```java
// qxide run --allow fs note.qx
// in an app: [app] permissions = ["fs"] in qxide.toml
import std.fs;

void main() {
    fs.write_text("note.txt", "hello");
}
```

## Q0704: file operation failed

A `std.fs` call failed at run time because the file does not exist, is larger than 16 MiB, is not valid UTF-8 text, or triggered an operating system I/O error. The diagnostic includes the underlying OS error message to identify the cause. Because `.qx` does not have runtime error handling yet, the program stops immediately rather than continuing with missing or invalid file data.

Erroneous code example:

```java
// qxide run --allow fs read.qx   (data/missing.txt does not exist)
String text = fs.read_text("missing.txt");  // error[Q0704]
```

To fix it, check whether the file exists with `fs.exists` before reading it.

```java
String text = "";
if (fs.exists("missing.txt")) {
    text = fs.read_text("missing.txt");
}
```

## Q0705: path not allowed

A path passed to `std.fs` is not a valid relative path inside the sandboxed data folder (`data/` for `qxide run --allow fs`, or the per-user app data folder in an app). Absolute paths, drive letters, UNC paths, `..`, empty segments, Windows device names such as `CON` or `NUL`, and paths that leave the folder through a symlink are all rejected. Restricting paths to the data folder prevents programs from reading or modifying arbitrary files on the host system.

Erroneous code example:

```java
fs.write_text("../secrets.txt", "x");  // error[Q0705]
```

To fix it, use a relative path that stays inside the data folder.

```java
fs.write_text("notes/secrets.txt", "x");
```

## Q0706: standard function argument out of range

A standard library function received an argument outside its allowed range, such as passing a negative duration or a value greater than 60000 milliseconds to `time.sleep_millis`. Enforcing argument bounds prevents invalid runtime operations and protects execution from unbounded waits or resource exhaustion.

Erroneous code example:

```qx
import std.time;

void main() {
    time.sleep_millis(120000);  // error[Q0706]
}
```

To fix it, pass a value that falls within the function's allowed range.

```qx
import std.time;

void main() {
    time.sleep_millis(10);
}
```

## Q0707: failed `http.get` request

A call to `http.get` in a desktop app fails when the request cannot be completed due to a network or TLS error or a 30-second timeout, when the server returns a status outside 200-299 (redirects are not followed), when the response body exceeds 16 MiB, or when the body is not valid UTF-8 text. HTTP requests are build-time effects that run once per command call before any shot executes, so a failed request fails the entire command call with this error. The timeout, response size cap, and redirect restriction prevent a single command call from hanging indefinitely or consuming unbounded memory.

Erroneous code example:

```java
import std.http;

public String latest() {
    return http.get("https://api.example.com/moved");  // error[Q0707]: status 301
}
```

To fix it, verify the URL and server availability, request a UTF-8 text resource of 16 MiB or less, or call the final destination URL directly instead of a URL that redirects.

```java
import std.http;

public String latest() {
    return http.get("https://api.example.com/v2/latest");
}
```

## Q0708: disallowed `http.get` URL

`http.get` only accepts `https://` URLs on port 443 without user info whose host matches an entry in `http_hosts` under `[app]` in `qxide.toml` using exact lowercase matching with no wildcards. The runtime validates the URL before sending any network traffic. Because the allow-list is embedded in the built app, neither page scripts nor dependencies can widen it.

Erroneous code example:

```java
// qxide.toml: [app] permissions = ["http"], http_hosts = ["api.example.com"]
import std.http;

public String page() {
    return http.get("http://example.org/");  // error[Q0708]
}
```

To fix it, use an `https://` URL on port 443 without user info and add its host to `http_hosts` under `[app]` in `qxide.toml`.

```java
// qxide.toml: [app] permissions = ["http"], http_hosts = ["api.example.com", "example.org"]
import std.http;

public String page() {
    return http.get("https://example.org/");
}
```

## Q0711: unknown app command

The page called `qxide.invoke` with a command name that is not listed in `commands` under the `[app]` section of `qxide.toml`, so the runtime refused the call before running any code. Restricting invocation to explicitly listed functions prevents page code from calling arbitrary internal functions in your Qxide program.

Erroneous code example:

```js
// [app] commands = ["measure_bell"]
await qxide.invoke("measure", {});  // rejects with code Q0711
```

To fix it, call a command that is already listed in `[app]` or add the function to `commands`.

```js
await qxide.invoke("measure_bell", {});
```

## Q0712: bad command arguments

The arguments object passed to `qxide.invoke` does not match the command's parameters or shot limits: a parameter is missing, an unexpected key is present, a value has the wrong type (for example, a number for a `String`), or the requested shot count is 0 or above 1,000,000. The runtime validates arguments before executing anything so malformed requests fail cleanly without running invalid or excessive work.

Erroneous code example:

```js
// public String save_result(String text)
await qxide.invoke("save_result", { text: 42 });  // rejects with code Q0712
```

To fix it, pass matching argument keys, value types, and a valid shot count, using `qxide.commands()` to check the expected parameter names and types.

```js
await qxide.invoke("save_result", { text: "00=493 11=507" });
```

## Q0713: call cancelled

An in-flight call was cancelled through its `AbortSignal` or because the window closed. The runtime checks for cancellation between shots, inside `time.sleep_millis`, and during elaboration so aborted calls stop promptly rather than continuing to consume resources. Nothing is wrong with the program itself; the runtime is reporting that execution was stopped externally.

Erroneous code example:

```js
const ctrl = new AbortController();
const run = qxide.invoke("measure_bell", {}, { shots: 1000000, signal: ctrl.signal });
ctrl.abort();  // run rejects with code Q0713
```

To fix it, start the call again without cancelling its `AbortSignal` or allow the call to finish before closing the window.

```js
const r = await qxide.invoke("measure_bell", {}, { shots: 1000000 });
```

## Q0714: invalid command signature

A function listed in `[app]` `commands` cannot be called from the page because it is missing, is not declared `public`, or accepts a parameter whose type is not `int`, `double`, `boolean`, or `String`. This error is reported by `qxide app dev` and `qxide app build` before the app runs, and by the runtime on each call, because page code can only pass plain values across the boundary and cannot own qubits or measurement results.

Erroneous code example:

```java
// [app] commands = ["flip"]
public Bit flip(Qubit q) {  // error[Q0714]
    h(q);
    return measure(q);
}
```

To fix it, ensure the command exists, make it `public`, accept only `int`, `double`, `boolean`, or `String` parameters, and allocate any qubits inside the command.

```java
public Bit flip() {
    Qubit q = qubit();
    h(q);
    return measure(q);
}
```

## Q0715: invalid `[app]` section

The `[app]` section of `qxide.toml` is malformed, such as when `commands` is missing or empty, a key has the wrong type, `permissions` names an unknown module, or `module` is not a valid module path. Reported by `qxide app dev` and `qxide app build` (the built app carries an already checked manifest), this validation catches configuration mistakes upfront so the runtime never launches with an inconsistent app manifest.

Erroneous code example:

```toml
[app]
commands = []  # error[Q0715]: at least one command is required
```

To fix it, correct the `[app]` section in `qxide.toml` so that all keys have valid types, `module` is a valid module path, `commands` is non-empty, and every entry in `permissions` names a known module.

```toml
[app]
commands = ["measure_bell"]
```

## Q0716: call result too large

The desktop app runtime hands each call result to the page as one script, and it refuses results larger than 32 MiB. The call's `Promise` rejects with `Q0716` instead, so a command cannot stall or exhaust the webview with a huge reply. Large results usually come from many shots that each produce a different measurement record or `println` output, or from a long `String` return value.

Erroneous code example:

```js
// `noise` measures 24 qubits; a million shots give hundreds of thousands of distinct records.
await qxide.invoke("noise", {}, { shots: 1000000 });  // rejects with code Q0716
```

To fix it, request fewer shots, return less data (for example a summary computed in `.qx`), or split the work into several calls.

```js
await qxide.invoke("noise", {}, { shots: 1000 });
```
