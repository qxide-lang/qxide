# Changelog

Notable changes to qxide (the Rust crate, the `qxide` CLI, and the `.qx` language) are listed here by version; older releases (0.5.0 and earlier) are documented on [GitHub Releases](https://github.com/qxide-lang/qxide/releases) and in `docs/release-notes/`.

## [Unreleased]

### Fixed

- **Biased and under-dispersed sampling across shots**: Shot `k` of `--seed S` previously used seed `S + k`, causing runs with nearby seeds to reuse nearly all of the same shots (over 200 seeds of 1,000-shot Bell runs, `00` counts had a standard deviation of 2.94 instead of ~15.8 for a binomial distribution). Each shot now receives an independent seed mixed from `(S, k)` via `qxide::shot_seed` and `Circuit::simulate_shots`, bringing the 200-seed Bell standard deviation to 16.7 (mean 497.6 vs. 506.1 before). Outputs remain reproducible for any given `--seed`, though counts and single-shot results differ from 0.6.0 across `qxide run`, `qxide bell`, `qxide repl`, and the browser playground.

### Added

- Added `qxide::shot_seed(seed, shot)` to derive an independent seed for each shot and `Circuit::simulate_shots(backend, seed, shots)` to iterate over `Result<Shot, SimError>` using `shot_seed`. `Circuit::simulate` and `Circuit::simulate_with` are unchanged and continue to execute a single shot with the exact seed provided.

## [0.6.0] - 2026-11-20

`qxide v0.6.0` builds on `v0.5.0` by extending the `.qx` language with qubit-passing and qubit-returning functions, gate-only function calls inside `if` blocks conditioned on a measured `Bit`, `var` local type inference for classical scalar types, Java-style numeric casts, and the built-in `PI` constant. For Rust library users, the public API of the `qxide` crate is unchanged since `v0.5.0`, and rustdoc, panic messages, and `SimError` `Display` text are now in English.

### Breaking changes

All breaking changes in `v0.6.0` apply to the `.qx` language only:

- `var` and `consume` are now keywords and can no longer be used as identifiers.
- `PI` is now a built-in `double` constant. Declaring a variable, parameter, `for` loop variable, or function named `PI` fails with `Q0217`, and assigning to `PI` fails with `Q0207`.
- Type names (`int`, `double`, `boolean`, `void`, `Bit`, `BitReg`, `Qubit`, `QReg`) can no longer be used as variable, parameter, or function names (`Q0112`).

### New features

- **Qubit function parameters (`consume` and default borrowing)**: `Qubit` and `QReg` parameters are borrowed by default—the callee may apply gates to them, while measuring, discarding, moving, or passing them to a `consume` parameter is rejected with `Q0313`. Marking a qubit parameter with `consume` transfers ownership to the callee, which must measure or discard it on every path (`Q0302`), while the caller can no longer reuse it (`Q0301`). Using `consume` on a classical parameter is rejected with `Q0221`.
- **Call-argument aliasing checks**: Passing aliased qubits to a function call—including passing a whole `QReg` together with one of its elements—is rejected with `Q0303`. A run-time aliasing check is also performed for run-time register indices.
- **Functions returning `Qubit` and `QReg`**: Functions can now return `Qubit` or `QReg` values (`Q0219` is retired). The caller receives ownership and must consume the returned value.
- **Gate-only function calls inside measured-`Bit` `if` blocks**: Gate-only user functions (supporting borrowed qubits, loops, nested `if`, recursion, and classical helpers) can now be called inside an `if` branch on a measured `Bit` and are lowered to OpenQASM 3 conditional blocks identical to the corresponding inline code. Functions that consume, return qubits, allocate, measure, discard, or print—directly or transitively—are rejected in that context with `Q0314`.
- **`var` local type inference**: Local variables can be declared with `var` for `int`, `double`, and `boolean` only. Using `var` for other types fails with `Q0222`, and using `var` in parameter or return positions fails with `Q0111`.
- **Java-style numeric casts**: `(int) x` and `(double) n` convert between `double` and `int`, with `(int)` truncating toward zero, mapping `NaN` to `0`, and saturating on overflow. Other casts are rejected with `Q0223`.
- **Built-in `PI` constant**: `PI` is available as a built-in `double` constant.
- **New `.qx` examples**: Added `examples/qx/functions.qx`, `examples/qx/rotation.qx`, `examples/qx/teleport_fn.qx`, and `examples/qx/conditional_fn.qx`. All examples are exported to OpenQASM 3 and parsed by Qiskit in CI.

### Documentation

- Crate rustdoc on `docs.rs` is now written in English.
- Added sections 13 and 14 to the language design document (`docs/design/qx-language.md`).
- Added a typed IR design draft in `docs/design/qx-ir.md` (not implemented).
- Added the Qxide logo to `README.md` and the website (`qxide.dev`).
- Updated `README.md` to describe the new `.qx` features.

### Rust library

- The public API of the `qxide` crate is unchanged since `v0.5.0`.
- Rustdoc comments, panic messages, and `SimError` `Display` text are now in English instead of Korean. This only affects code that matched the previous Korean message text.
