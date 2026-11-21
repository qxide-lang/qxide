<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/qxide-mark-light.svg">
    <img src="https://raw.githubusercontent.com/qxide-lang/qxide/main/assets/brand/qxide-mark.svg" alt="Qxide logo" width="96" height="96">
  </picture>
</p>

# Qxide

[Korean](README.ko.md) · [qxide.dev](https://qxide.dev)

Qxide is a programming language for classical and quantum computers with Java/C-style syntax and compile-time qubit safety that compiles quantum code to OpenQASM 3.

The `qxide` command-line tool simulates `.qx` programs with built-in state-vector and stabilizer simulators (`qxide run`), checks them (`qxide check`), and exports them to OpenQASM 3 (`qxide qasm`). The same `qxide` crate can also be used directly as a Rust library. Version 0.7.0 is still an early experiment adding modules and packages, new commands (`new`, `fmt`, `repl`, `explain`), and independent per-shot sampling; both the language and APIs may still change.

## Install

There are three ways to install the `qxide` command; all of them install the same binary.

Install a prebuilt binary from GitHub Releases using the install script at qxide.dev:

```sh
# Linux / macOS
curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"
```

Install from crates.io (requires a Rust toolchain):

```sh
cargo install qxide
```

Install from PyPI (the wheel bundles the same command-line binary, not a Python API):

```sh
pip install qxide
```

Verify the installation with:

```sh
qxide --version
```

## Quick start with `.qx`

The following `.qx` program entangles two qubits into a Bell pair using Java/C-style declarations. Additional examples (`bell.qx`, `ghz.qx`, `teleport.qx`) are in `examples/qx/`.

```java
// examples/qx/bell.qx
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}
```

Run the program with `qxide run`: a single shot prints the program's output, while `--shots N` executes N shots.

```text
$ qxide run examples/qx/bell.qx
a=1 b=1
$ qxide run examples/qx/bell.qx --shots 1000
[1, 1]  0b11 = 3  ×521
  a=1 b=1
[0, 0]  0b00 = 0  ×479
  a=0 b=0
```

In the multi-shot table, each row represents one measurement record followed by its count: shown as an array with `c[0]` first (`[0, 1, 1]`), in binary with the highest bit first (`0b110`), and as an integer with `c[0]` as the least significant bit (`= 6`). The program's own output for that outcome is indented below the row. Use `qxide check` to validate a program without running it, or `qxide qasm` to emit OpenQASM 3.

```text
$ qxide check examples/qx/bell.qx
examples/qx/bell.qx: ok
$ qxide qasm examples/qx/bell.qx
OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];
```

Qubits are linear values: they cannot be copied, reused after measurement, or passed twice to the same gate, and every qubit must be measured or explicitly discarded (with `discard`) before it goes out of scope. In the example below, using a qubit after measuring it produces a compile-time error with line and column numbers, caught by `qxide check` before anything runs.

```java
void main() {
    Qubit q = qubit();
    Bit m = measure(q);
    h(q);
}
```

```text
$ qxide check use_after_measure.qx
error[Q0301]: use of consumed qubit `q`
 --> use_after_measure.qx:4:7
  |
4 |     h(q);
  |       ^
note: qubit consumed here
 --> use_after_measure.qx:3:13
  |
3 |     Bit m = measure(q);
  |             ^^^^^^^^^^
```

- Types: `int` (64-bit), `double`, `boolean`, `Qubit`, `QReg`, `Bit`, `BitReg`, and `final` declarations
- Gates and operations: `h`, `x`, `z`, `s`, `sdg`, `t`, `tdg`, `rx`, `ry`, `rz`, `cx`, `ccx`, `reset`, `measure`, `discard`
- Control flow: `if` on a measured `Bit` lowers to a classical feed-forward branch in OpenQASM 3
- Register output: `r.to_binary()` and `r.as_int()`
- Qubit functions: parameters borrowed by default or moved with `consume`, `Qubit`/`QReg` return values, and gate-only calls inside `if` on a measured `Bit` (see `examples/qx/functions.qx`, `examples/qx/teleport_fn.qx`)
- Type inference, casts, and constants: `var` for `int`/`double`/`boolean` locals, Java-style `(int) x` and `(double) n` casts, and `PI` (see `examples/qx/rotation.qx`)

See [docs/design/qx-language.md](docs/design/qx-language.md) for the full language design document.

## More `qxide` commands

Besides `run`, `check`, and `qasm`, the `qxide` command can start a project with `qxide new`, format code with `qxide fmt`, run an interactive session with `qxide repl`, and explain error codes with `qxide explain`.

`qxide new <NAME>` creates a new package project containing `qxide.toml` (with a package name derived from the folder name, version `0.1.0`, and the required `qxide` version), a Bell-pair `src/main.qx`, a short `README.md`, and a `.gitignore` that ignores `target/`; inside the folder, `qxide run` with no file runs `src/main.qx`. It fails with exit code 1 without creating or overwriting anything if the path already exists, and removes the folder again if writing a file fails. `qxide fmt <FILE>` rewrites a file in place to use standard 4-space indentation, brace placement, spacing, and blank-line rules while preserving comments and exact literal spelling; if the file does not parse, it prints the diagnostic and leaves the file untouched (type and ownership errors do not block formatting). Passing `--check` never writes to the file, exiting with 1 if the file would change and 0 if it is already formatted.

```text
$ qxide new hello
Created project `hello` in `hello` with `qxide.toml`, `src/main.qx`, `README.md`, and `.gitignore`
Next:
  qxide run hello/src/main.qx
  qxide fmt hello/src/main.qx
$ cat messy.qx
void main(){
Qubit q=qubit();h( q );
Bit m=measure(q);println("{}",m);}
$ qxide fmt --check messy.qx
qxide: `messy.qx` is not formatted; run `qxide fmt messy.qx` to format it
$ qxide fmt messy.qx
$ cat messy.qx
void main() {
    Qubit q = qubit();
    h(q);
    Bit m = measure(q);
    println("{}", m);
}
```

`qxide repl` starts an interactive session where you can enter statements, function definitions, or bare expressions whose values are printed, with variables, functions, and qubits persisting across inputs. Each entry is checked under the same rules as `qxide run`, so errors such as reusing a measured qubit (`Q0301`) are reported and the rejected input is not kept; qubits may stay live while you work, and any left unmeasured or undiscarded when the session ends are reported as `Q0302` with exit code 1. Built-in commands include `:help` for a summary, `:qasm` to print the OpenQASM 3 circuit built so far, `:reset` to clear the session, and `:quit` to exit.

```text
$ qxide repl
qxide 0.7.0 interactive .qx session; type :help for commands and :quit to exit.
qx> Qubit q = qubit();
qx> h(q);
qx> Bit m = measure(q);
qx> m
1
qx> h(q);
error[Q0301]: use of consumed qubit `q`
 --> <repl>:5:3
  |
5 | h(q);
  |   ^
note: qubit consumed here
 --> <repl>:3:9
  |
3 | Bit m = measure(q);
  |         ^^^^^^^^^^
qx> :quit
```

`qxide explain <CODE>` prints what triggers an error code and why the rule exists, along with an erroneous example, how to fix it, and the corrected program (codes are case-insensitive). The text comes from [docs/error-codes.md](docs/error-codes.md), which lists every error code and can also be read directly on GitHub.

````text
$ qxide explain Q0301
Q0301: use of a consumed qubit

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
...
````

## Rust library

The same linear-qubit rules are available in the `qxide` Rust library, where the Rust compiler enforces ownership directly. `Qubit` does not implement `Clone`, `measure` consumes the qubit by value, and `cx(&mut a, &mut b)` prevents passing the same qubit twice.

```sh
cargo add qxide
```

```rust
use qxide::Circuit;

let mut c = Circuit::new();
let mut a = c.qubit();
let mut b = c.qubit();
c.h(&mut a);           // superposition
c.cx(&mut a, &mut b);  // entanglement
let ma = c.measure(a); // `a` is consumed; only a classical bit remains
let mb = c.measure(b);

println!("{}", c.to_qasm());   // OpenQASM 3 for real quantum hardware
let shot = c.simulate(42);     // simulate on an ordinary computer
assert_eq!(shot.get(ma), shot.get(mb));
```

The library also provides registers, mid-circuit measurement with `if_one`/`if_zero`, `reset`, scoped uncomputation with `with_ancilla`, automatic backend selection, and an optional GPU state-vector prototype behind the `gpu` feature; API documentation is available on [docs.rs](https://docs.rs/qxide).

## Status

- `.qx` programs currently run in a tree-walking interpreter.
- A typed IR and native code generation (Cranelift first, LLVM optional) are planned; no performance claims are made before benchmarks exist.
- Submitting jobs to physical quantum hardware is not implemented yet.
- See [ROADMAP.md](ROADMAP.md) (Korean) and [docs/design/qx-language.md](docs/design/qx-language.md) for current plans and design details.

## Links

- Website: https://qxide.dev
- Korean README: [README.ko.md](README.ko.md)
- Language design: [docs/design/qx-language.md](docs/design/qx-language.md)
- Releases: https://github.com/qxide-lang/qxide/releases
- crates.io: https://crates.io/crates/qxide
- PyPI: https://pypi.org/project/qxide/

## License

Licensed under either of MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE)) at your option.
