# Changelog

Notable changes to qxide (the Rust crate, the `qxide` CLI, and the `.qx` language) are listed here by version; older releases (0.5.0 and earlier) are documented on [GitHub Releases](https://github.com/qxide-lang/qxide/releases) and in `docs/release-notes/`.

## [Unreleased]

### Breaking changes

- **New `.qx` type name (`String`)**: `String` is now a `.qx` type name and can no longer be used as a variable or function name.
- **Reserved package name `std`**: `std` is reserved for the standard library. `qxide new`, `qxide.toml` validation, and the registry validator reject a package named `std`, and `qxide new std` names the package `std-app`.
- **String literals as values (`Q0204` / `Q0214`)**: String literals are now values, so `int s = "hi";` reports a type mismatch (`Q0204`) instead of `Q0214`. `Q0214` now reports a `{}` placeholder in a string value, as placeholders are only allowed in `println`'s format string.

### Added

- **Typed IR interpreter (`--engine ir`)**: Added a `--engine ast|ir` option to `qxide run`, `qxide qasm`, and `qxide repl`, with the `QXIDE_ENGINE` environment variable providing its default; `ast` remains the default engine. Selecting `ir` lowers the checked program to a typed IR (design record in `docs/design/qx-ir.md`) and executes it through an IR interpreter that reproduces the tree-walking interpreter's output, OpenQASM, seeded measurement results, diagnostics, and resource-limit behaviour. Also added a benchmark suite of programs in `bench/qx/` run by an ignored test (`cargo test --release --bin qxide bench_suite -- --ignored --nocapture`), with the method and results documented in `docs/perf/QX_BENCHMARKS.md`.
- **Cranelift JIT (`--engine jit`)**: Added an optional `jit` Cargo feature (off by default; pulls in Cranelift 0.138) that exposes the `jit` engine for `qxide run`, `qxide qasm`, and `qxide repl` (as well as `QXIDE_ENGINE=jit`), while builds without the feature reject `jit` with exit code 2. Build-time classical functions using only `int`, `double`, and `boolean` values and calling only such functions are compiled to native code on their first call, and everything else runs in the typed IR interpreter with the same output, diagnostics, and resource-limit behaviour. The browser playground and desktop app runtime do not include the JIT; see `docs/design/qx-jit.md` for the design record and `docs/perf/QX_BENCHMARKS.md` for measurements.
- **Quantum computer submission (`qxide run --backend ibm:<device>`)**: Added QPU submission support for IBM Quantum via the Qiskit Runtime REST API, converting the elaborated `.qx` circuit to the target device's native basis gates and coupled physical qubits and rendering the returned measurement records in the same format as local `--shots`. API keys are read only from `QXIDE_IBM_API_KEY` or the OS credential store (read-only) and the instance CRN from `QXIDE_IBM_INSTANCE`, with all HTTP traffic sent through the system `curl` via stdin config so secrets never appear in process arguments; real submissions print cost and queue notices, require terminal confirmation (or `--yes`), and pass a QPU time cap via `--max-seconds` (default 120), while `--dry-run` prints the request without touching credentials or the network (`--device-config` outputs the exact ISA circuit). This flow is tested against a local fake server and has not yet been verified against the live IBM service (design record in `docs/design/qx-qpu.md`). CI now also validates exported OpenQASM 3 files with the specification's reference parser and checks generated device circuits with the official client's ISA validator against offline IBM device snapshots.
- **`String` type**: Added immutable build-time text supporting `+` concatenation with numbers and booleans, `==` and `!=`, `len(s)`, and printing. `String` is usable in parameters, returns, `final`, and `var`.
- **Standard library (`std`)**: Added `std.log`, `std.time`, `std.fs`, `std.app`, and `std.qxide`, imported with `import std.fs;` (called as `fs.read_text(...)`), `import std.fs.read_text;`, or `import std.fs.*;`, with new error codes `Q0701`–`Q0706`. Standard library calls run once while the program is elaborated, before any shot. Each host offers modules under permissions: `qxide run` allows `std.log` and `std.time`, and enables `std.fs` only with the new `--allow fs` flag (for files under `data/` next to `qxide.toml`, or `./data`), `qxide qasm` refuses file and window effects, and the REPL and browser playground offer only `std.log`.
- **Desktop apps**: Added desktop app support (design in `docs/design/qx-app.md`) with `qxide app new <NAME>` (scaffolds an app with `.qx` logic and an HTML/JS interface), `qxide app dev` (runs it in a window reading files from disk), `qxide app build` (writes a portable executable to `target/app/` by attaching a bundle of `.qx` sources, locked packages, and `ui/` files to the prebuilt `qxide-app` runtime without compiling anything), app-level error codes `Q0711`–`Q0715`, and the `examples/apps/bell-app` example (measures a Bell pair and saves results with `std.fs`). The runtime (`crates/qxide-app-runtime`, built on `wry` and `tao`, the OS webview) serves the page with a strict Content-Security-Policy, accepts calls only from the app's own origin and only for functions listed in `[app] commands`, and runs each call on a worker thread with cancellation. The runtime is not yet part of the release archives or PyPI wheels and is built separately (`cargo build --release --manifest-path crates/qxide-app-runtime/Cargo.toml`).
- **App permission checks in `qxide check`**: Running `qxide check` inside an app project now checks imports against the app's `[app] permissions`.
- **Qxide Studio**: Added Qxide Studio (`apps/studio`, built with `qxide app build apps/studio`), a minimal desktop IDE for `.qx` built with the app framework itself. It includes an editor with highlighting, line numbers, and error markers; Run (shots, seed), Stop, Check, Format, and a QASM view; a distribution chart and a Problems list; the programs from `examples/qx`; and autosave to the app data folder. It uses the new `std.qxide` module (`check`, `run`, `qasm`, `format`, returning JSON; granted with the `qxide` permission), which runs the edited program like the playground does, with only `std.log`.
- **Language server (`qxide lsp`)**: Added the `qxide lsp` subcommand (`docs/design/lsp.md`), which runs a Language Server Protocol server over stdio and accepts an optional `--stdio` flag. It publishes the same diagnostics and error codes as `qxide check` at the same lines and columns (using UTF-16 columns for the protocol), including in projects with `qxide.toml` and imports—errors inside an imported module are shown on the importing file's `import` lines, and unsaved edits of open modules are seen by the files that import them. Supported features include formatting via `qxide fmt`, hover (showing the `qxide explain` entry of the error under the cursor plus the declaration or signature of the name), document symbols, go to definition for locals, parameters, and functions (including across imported modules), and completion of keywords, types, built-in functions, `PI`, the file's and imported functions, local names in scope, and `BitReg` methods. Built on the `lsp-server` crate behind the default `cli` feature, so the library (`default-features = false`) still has no dependencies.
- **VS Code extension**: Added a VS Code extension in `editors/vscode/` (not published to any marketplace yet) providing `.qx` syntax highlighting through a TextMate grammar, a language configuration, and a client that starts `qxide lsp`. It includes the `qxide.path` (defaulting to `qxide` from `PATH`) and `qxide.trace.server` settings and a `Qxide: Restart Language Server` command, requires VS Code 1.91 or newer, and can be built with `npm install` and `npm run compile` inside `editors/vscode`.

### Changed

- **Random default seed for `qxide run` and `qxide repl`**: Running `qxide run` or `qxide repl` without `--seed` now uses a random seed from OS entropy (via `std` only, with no new dependencies) instead of defaulting to `0`, so repeated runs produce different counts rather than identical output that looked like biased sampling (such as `qxide run bell.qx`). The seed used is printed to `stderr` as `seed: S (pass --seed S to reproduce)` only when output depends on simulated measurement outcomes (for `qxide repl`, once when an input first prints a measured bit). `--seed S` remains deterministic with no `stderr` line; scripts relying on the previous implicit seed `0` must now pass `--seed 0`, while `qxide bell` still uses seed `0`.
- **Random default seed in browser playground**: The Seed field is now empty by default and draws a random `u64` seed with `crypto.getRandomValues` on each run; the results panel shows the seed used, and Copy link stores that seed so the link reproduces the run.

## [0.7.0] - 2026-11-21

`qxide v0.7.0` builds on `v0.6.0` by adding modules (`import`, `public`), a package manager (`qxide.toml`, `qxide.lock`, and registry commands `add`, `remove`, `install`, `publish`, `search`), and new CLI commands (`qxide fmt`, `qxide repl`, `qxide explain`, and an updated `qxide new` project layout). This release also fixes seeded sampling so each shot uses an independent seed, tightens linearity soundness checks (`Q0315`), and rejects non-finite rotation angles (`Q0411`). For Rust library users, `qxide::shot_seed` and `Circuit::simulate_shots` are new, `rx`/`ry`/`rz` now panic on non-finite angles, and package manager dependencies are gated behind the default `cli` feature so `default-features = false` keeps the library dependency-free.

### Breaking changes

- **New `.qx` keywords (`import`, `public`)**: `import` and `public` are now keywords in `.qx` and can no longer be used as identifiers.
- **Non-finite rotation angles in the Rust library**: `Circuit::rx`, `Circuit::ry`, and `Circuit::rz` now panic when the angle is `NaN` or infinite. Previously the state vector silently became `NaN`, every measurement returned `0`, and `to_qasm` printed `rx(NaN)`, which OpenQASM parsers reject.
- **Stricter `.qx` linearity checks**: Programs that consume a qubit in the right operand of `&&` or `||`, or that consume an operand borrowed by the same call or gate (for example `rx(q, eat(q))`), are now rejected at check time with `Q0315` and `Q0301`. Previously these programs passed `check` and then failed at run time or leaked a qubit.
- **Nesting limit on `.qx` operator and index/method chains**: Operator chains (`a + b + ...`) and index/method chains (`r[0][0]...`) now count toward the 128-level nesting limit (`Q0106`), so chains longer than about 120 operators are rejected.
- **Package project layout in `qxide new`**: `qxide new` now creates a package project (`qxide.toml`, `src/main.qx`, `README.md`, and `.gitignore`) instead of `main.qx` and `README.md`. Arguments starting with `-` are rejected as a usage error.
- **Strict CLI argument checks**: `qxide bell`, `--version`, and `--help` now reject extra arguments with exit code 2.

### Added

- **Per-shot seeding API**: Added `qxide::shot_seed(seed, shot)` to derive an independent seed for each shot and `Circuit::simulate_shots(backend, seed, shots)` to iterate over `Result<Shot, SimError>` using `shot_seed`. `Circuit::simulate` and `Circuit::simulate_with` are unchanged and continue to execute a single shot with the exact seed provided.
- **Modules and imports**: Every `.qx` file is now a module supporting `public` functions, Java-style `import pkg.module.function;` and `import pkg.module.*;` declarations, and import errors `Q0501`–`Q0510`. Inside a project, `qxide run`, `qxide check`, and `qxide qasm` without a file argument use `src/main.qx`.
- **Package manager**: Added a package manager (`docs/design/qx-packages.md`) with `qxide.toml` manifests, `qxide.lock`, a GitHub-hosted index registry (template in `registry-template/`), commands `qxide add`, `remove`, `install`, `publish`, and `search`, `--offline`/`QXIDE_OFFLINE=1`, `--registry`/`QXIDE_REGISTRY`, cache storage in `QXIDE_HOME` (default `~/.qxide`), and package errors `Q0601`–`Q0617`. The package manager is gated behind the default `cli` feature used by the binary, so the library (`--no-default-features --lib`) still has no dependencies.
- **New CLI commands**: Added `qxide new <NAME>` (project scaffold with the package name derived from the folder name), `qxide fmt <FILE> [--check]`, `qxide repl [--seed S]`, and `qxide explain <CODE>` (prints the entry from `docs/error-codes.md`, including `Q05xx` and `Q06xx` package errors).
- **New error codes**: Added `Q0315` (qubit consumed in the right operand of `&&` or `||`), `Q0411` (non-finite rotation angle), and `Q0412` (internal error when the checker and the interpreter disagree; replaces a misleading `Q0311`).
- **Browser playground**: The `.qx` front end and simulator compile to WebAssembly (`crates/qxide-wasm`, not published) for the playground at `qxide.dev`, and CI checks that it prints exactly what the CLI prints.

### Fixed

- **Biased and under-dispersed sampling across shots**: Shot `k` of `--seed S` previously used seed `S + k`, causing runs with nearby seeds to reuse nearly all of the same shots (over 200 seeds of 1,000-shot Bell runs, `00` counts had a standard deviation of 2.94 instead of ~15.8 for a binomial distribution). Each shot now receives an independent seed mixed from `(S, k)` via `qxide::shot_seed` and `Circuit::simulate_shots`, bringing the 200-seed Bell standard deviation to 16.7 (mean 497.6 vs. 506.1 before). Outputs remain reproducible for any given `--seed`, though counts and single-shot results differ from 0.6.0 across `qxide run`, `qxide bell`, `qxide repl`, and the browser playground.
- **Non-finite rotation angles**: Non-finite rotation angles are now rejected instead of silently corrupting the state vector or producing invalid OpenQASM, with `.qx` reporting `Q0411` at the angle (see Breaking changes).
- **Linearity soundness holes**: Fixed check-time gaps that allowed consuming a qubit in the right operand of `&&`/`||` or consuming an operand borrowed by the same call or gate, now caught with `Q0315` and `Q0301` (see Breaking changes).
- **Stack overflows on long expression chains**: Fixed stack overflows where a 4 KB program with a long `+` chain inside recursion passed `check` but crashed `run`/`qasm`, and where chains of 200,000 terms crashed `check`.
- **Register size overflow on wasm32**: `qreg(4294967297)` in the browser playground no longer wraps to a 1-qubit register and now fails with `Q0409` like the CLI.
- **Remainder of `int.MIN % -1`**: `int.MIN % -1` now yields `0` as in Java instead of reporting overflow `Q0401`.
- **Bare qubit statements**: A statement that only names an existing qubit (`q;`) is now a no-op like `x;` instead of reporting `Q0312`.
- **CRLF string literals in `qxide fmt`**: `qxide fmt` no longer refuses a valid CRLF file containing a line break inside a string literal.
- **Nested `return` in `qxide repl`**: A `return` nested in `if` or `for` is now rejected like a top-level `return` instead of silently ending the session.
- **Trailing line comments in `qxide repl`**: A trailing `//` comment on a REPL expression no longer fails with `Q0105` at `<repl>:0:1`.
- **Unmatched closing brackets in `qxide repl`**: Input closing a bracket it did not open (for example `}`) is now rejected instead of closing `main` and breaking the session.
- **UTF-8 BOM in piped REPL input**: A UTF-8 byte order mark at the start of piped input is now ignored.
- **REPL diagnostic snippets**: REPL diagnostics now show the typed input instead of the internal `println("{}", …)` wrapper.
- **Flag ordering in `qxide run`**: `qxide run` now accepts flags before or after the file path (for example `qxide run --shots 2 file.qx`).
- **Failed scaffold cleanup in `qxide new`**: `qxide new` no longer leaves an empty folder behind when writing a file fails.
- **Package archive path hardening**: Archive entries with trailing dots or spaces, Windows device names (`CON`, `NUL`, …), Windows-invalid characters, or case-only duplicates could overwrite a reviewed file on Windows or macOS; they are now rejected before anything is unpacked (`Q0610`) and refused by `qxide publish` (`Q0614`) and registry CI. `qxide publish` also skips symlinked `README*`/`LICENSE*` files.
- **Semver build metadata in package resolution**: Versions with build metadata (`1.0.0+x`) could take over an exact `=1.0.0` pin; they are never selected or published, and registry CI rejects same-precedence duplicates.
- **Registry index validation and parsing**: Registry CI (`validate.py`) now matches the client's parser exactly across semver limits, requirements, field types, line splitting, and strict JSON. An index line the client cannot use is skipped instead of breaking every version of the package, reporting `Q0609` only when no usable line remains.
- **Registry download failures and temp-file safety**: Any non-zero `curl` exit (including a transfer cut off after HTTP 200) is now reported as `Q0608` instead of being accepted and cached. Downloads pass `--globoff` to prevent `{}`/`[]` expansion in URLs and write to an exclusively created file under `$QXIDE_HOME/registry/tmp` instead of a predictable name in the shared temp directory.
- **Whole-graph lockfile freshness**: Lock freshness now checks the whole graph (transitive requirements, path manifests on disk, sources, and unreachable entries), returning `Q0612` for a stale lock in `qxide run` and `qxide install --locked`.
- **Lockfile package name validation**: Package names in `qxide.lock` are validated (`Q0603`), preventing a crafted name from pointing cache paths outside `QXIDE_HOME`.
- **Registry graph and CI checks**: Self-dependencies of registry packages are rejected (`Q0606`), `build_search.py` orders pre-releases by semver precedence, and `validate.py` rejects `"v": true` and changes to `config.json` in package PRs.

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
