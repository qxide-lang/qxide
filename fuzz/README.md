# Qxide Fuzzing

## Overview

This directory contains `cargo-fuzz` targets for Qxide along with two stable-Rust/Node tools and a stable replay test that require no nightly toolchain.

All commands in this document are run from the repository root.

## Targets

The `cargo-fuzz` targets live under `fuzz/fuzz_targets/*.rs` and each calls `qxide_fuzz::targets::<name>` in `fuzz/src/lib.rs`.

| Target | What it exercises | Invariant |
| --- | --- | --- |
| `parse` | Lexer and parser only (`fuzz/src/front` includes `src/lang/{ast,diag,lexer,msg,parser}.rs`). | Does not panic or crash. |
| `check` | Lex, parse, link (no project; imports are errors), and static checks via `lang::check`. | Does not panic or crash. |
| `fmt` | `lang::format`. | Output formats again to the same text (idempotence) and still parses; does not panic or crash. |
| `run_limits` | `qxide_wasm::execute(Run)` with the playground limits (20 state-vector qubits, 512 MiB tableau, `<=100k` shots). Input byte `0` selects shots `1..=64`, bytes `1..9` select the seed, and the remaining bytes are the `.qx` source. | Respects playground limits; does not panic or crash. |
| `manifest` | `qxide.toml` parser (`pkg::manifest::parse`). | Does not panic or crash. |
| `index_line` | Registry index line parser. | Accepted entries round-trip through `to_json`; does not panic or crash. |
| `archive` | Package archive unpack into a fresh temporary directory. | Nothing is written outside the temporary directory; does not panic or crash. |

The share-link decoder is written in TypeScript and is therefore not a `cargo-fuzz` target. It is exercised by `fuzz/js/share_decode.mjs` (Node 22.18+/24, no dependencies).

## How the harness is built

`fuzz/` is its own Cargo workspace and is not part of the root workspace. It compiles `src/lang` and `src/pkg` unchanged via `#[path]`, matching `crates/qxide-wasm`.

- The default `libfuzzer` feature pulls in `libfuzzer-sys` for `cargo-fuzz` runs.
- Passing `--no-default-features` builds only the harness library on stable Rust.

## Requirements

Real fuzzing requires a nightly Rust toolchain and `cargo-fuzz`. Nightly and `cargo-fuzz` are **not** installed on the maintainer's PC and must be installed only on a machine where that is allowed (ideally Linux or WSL):

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
```

On Windows, `cargo-fuzz` requires the MSVC toolchain and AddressSanitizer support; Linux or WSL is the easier path.

## Seeding corpora

Run these commands from the repository root to populate initial corpora from the repository examples.

Seed `parse`, `check`, and `fmt`:

```sh
mkdir -p fuzz/corpus/parse fuzz/corpus/check fuzz/corpus/fmt && cp examples/qx/*.qx fuzz/corpus/parse/ && cp examples/qx/*.qx fuzz/corpus/check/ && cp examples/qx/*.qx fuzz/corpus/fmt/
```

Seed `run_limits` (prepends the required 9-byte prefix for shots and seed):

```sh
mkdir -p fuzz/corpus/run_limits && for f in examples/qx/*.qx; do { printf '\001\000\000\000\000\000\000\000\000'; cat "$f"; } > "fuzz/corpus/run_limits/$(basename "$f")"; done
```

Seed `manifest`:

```sh
mkdir -p fuzz/corpus/manifest && for d in examples/packages/*/; do cp "$d/qxide.toml" "fuzz/corpus/manifest/$(basename "$d").toml"; done
```

## Running

List available `cargo-fuzz` targets:

```sh
cargo +nightly fuzz list
```

Run a target (example with `check`; substitute any other target name as needed):

```sh
cargo +nightly fuzz run check -- -max_total_time=600 -timeout=10 -rss_limit_mb=2048
```

Minimize a crash artifact:

```sh
cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/<file>
```

## Stable tools without nightly

### Replay test

Replay the example inputs and every saved crash artifact under `fuzz/artifacts/<target>/` on stable Rust:

```sh
cargo test --manifest-path fuzz/Cargo.toml --no-default-features
```

### Share-link decoder script

Fuzz the TypeScript share-link decoder using Node 22.18+/24 (no external dependencies). Arguments are `<iterations> <seed>`:

```sh
node fuzz/js/share_decode.mjs 20000 1
```

### CLI random smoke test

Run the stable random smoke test against the CLI:

```sh
cargo test --release --test smoke_random -- --ignored --nocapture
```

Configuration environment variables:

- `QXIDE_SMOKE_CASES`: number of test cases to run (default: `300`).
- `QXIDE_SMOKE_SEED`: RNG seed for reproducibility (uses a fixed default).
- `QXIDE_SMOKE_TIMEOUT_MS`: time limit in milliseconds for each `qxide` command (`check`, `fmt --check`, `qasm`, `run`) run on a mutant (default: `3000`).
- `QXIDE_SMOKE_REPLAY=<file.qx>`: runs that file unchanged through the four commands before the mutants (for example, to re-check a saved failure).

Failing inputs are saved under `target/tmp/smoke-failures/`.

## Known results to triage

`run_limits` is expected to report time-outs and RSS-limit hits caused by unbounded loops and unbounded output (`BASELINE_FINDINGS` B-01, B-02, B-03) until the interpreter enforces a step and output budget. Triage these hits as duplicates of B-01, B-02, or B-03. Keep `-timeout` and `-rss_limit_mb` set so fuzzing runs remain bounded.

## Reporting

Crash artifacts produced by `cargo-fuzz` are written to `fuzz/artifacts/<target>/` (ignored by git). Report findings using the format defined in `docs/security/REVIEW_BRIEF.md`.
