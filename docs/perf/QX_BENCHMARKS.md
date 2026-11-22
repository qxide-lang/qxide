# `.qx` benchmark suite

## Purpose

Roadmap stage 2 calls for a benchmark suite before any optimization (`docs/design/qx-language.md`, section 12.6, step 2). This suite records how long each phase of the `.qx` pipeline takes so that the typed IR interpreter and a later Cranelift backend can be compared against measured numbers. No performance claims are made beyond the measured numbers.

## Cases

The suite is an ignored Rust test in `src/lang/bench.rs` inside the `qxide` binary crate (the `.qx` front end in `src/lang` is private to the binary). Benchmark programs live in `bench/qx/*.qx` rather than `examples/qx`, which CI, the wasm tests, and the site enumerate. The first line of each program is a header:

- `// bench: shots=N` — run with `N` shots.
- `// bench: build` — elaborate and produce the OpenQASM text (for circuits too large to simulate).
- `// bench: check` — front end only.

| Case | What it measures | Mode |
| --- | --- | --- |
| `arith` | General computation: counting primes below 60000 by trial division (with an integer square root computed by a Newton iteration on `double`) and 200000 terms of the Leibniz series for pi. | run, 1 shot |
| `loops` | 300 x 300 nested loop updating nine live `int` locals (a linear congruential generator and modular sums); variable lookups dominate. | run, 1 shot |
| `recursion` | Naive recursive `fib(24)` plus 2000 rounds of mutual recursion `isEven`/`isOdd` up to depth 190 (the call-depth limit is 200). | run, 1 shot |
| `qft` | 12-qubit quantum Fourier transform and its inverse built from `h`, `rz`, and `cx` through helper functions; state-vector simulation dominates. | run, 100 shots |
| `ghz_stabilizer` | 400-qubit GHZ state with Clifford layers (`s`, `sdg`); stabilizer (tableau) simulation dominates. | run, 100 shots |
| `circuit_build` | 400 layers over a 512-qubit register through a helper function (about 300 000 gates); too large for a state vector, so it builds the circuit and prints its OpenQASM 3 text. | build |
| `check_flat_8000` | Generated file with 8000 functions (each allocates a qubit, applies `h`, measures, and prints) called from `main`: the public `flat_functions` family of `scripts/perf/checker_large_files.py`, generated in the harness. | check (front end: lexing, parsing, linking, type and ownership checks) |
| `run_flat_8000` | The same generated file as `check_flat_8000`. | run, 1 shot |

## How to run

```sh
cargo test --release --bin qxide bench_suite -- --ignored --nocapture
```

Environment variables:

- `QXIDE_BENCH_REPEAT` — repetitions per case (default `5`).
- `QXIDE_BENCH_ENGINES` — comma-separated elaborators to compare (default `ast`).
- `QXIDE_BENCH_ONLY` — substring filter on case names.
- `QXIDE_BENCH_JSON` — path; one JSON line is appended per case and engine.

## Method

Each case runs once as a warm-up (not recorded), followed by `QXIDE_BENCH_REPEAT` recorded runs. When several engines are compared, their outputs must be identical, or the test fails.

Phases are timed in-process with `std::time::Instant`:

- `front` — lexing, parsing, linking, and checking.
- `elaborate` — running `main` once to build the circuit and the output template.
- `run/qasm` — sampling all shots and rendering the output, or producing the OpenQASM text.
- `total` — total elapsed time across phases.

Tables report `min / median` in milliseconds. Every run uses a fixed seed of `1` and the CLI limits (`Limits::CLI`), which all cases stay within. Measurements execute in a single process on a thread with the interpreter's 256 MiB stack, so they exclude process startup and file reading, unlike the wall-clock measurements in `docs/perf/CHECKER_LARGE_FILES.md`.

## Baseline: tree-walking interpreter (2026-11-22)

Environment:

- Commit: `56285ac` (branch `feat/typed-ir`, `main` `b060c19` plus a roadmap edit)
- Toolchain: `rustc` 1.99.1, release profile
- Host: Windows 11, Intel i9-14900HX laptop, about 32 GB RAM (about 13 GB free)
- Background load: other interactive sessions running on the same machine (CPU load about 15% before the run)
- Repetitions: 5

Expect noise of several percent to tens of percent between runs on this machine; compare engines within the same run.

Each cell is `min / median`; `-` means the phase does not apply.

| Case | front ms | elaborate ms | run/qasm ms | total ms |
| --- | --- | --- | --- | --- |
| `arith` | 0.1 / 0.1 | 682.8 / 714.0 | 0.0 / 0.0 | 682.8 / 714.1 |
| `circuit_build` | 0.1 / 0.1 | 148.7 / 156.2 | 79.6 / 92.7 | 231.8 / 250.8 |
| `ghz_stabilizer` | 0.0 / 0.1 | 0.4 / 0.4 | 932.2 / 1067.5 | 933.0 / 1067.9 |
| `loops` | 0.1 / 0.1 | 211.0 / 221.5 | 0.0 / 0.0 | 211.1 / 221.6 |
| `qft` | 0.1 / 0.1 | 0.5 / 0.5 | 578.7 / 626.7 | 579.4 / 627.4 |
| `recursion` | 0.1 / 0.1 | 186.0 / 206.8 | 0.0 / 0.0 | 186.1 / 206.9 |
| `check_flat_8000` | 87.2 / 94.4 | - | - | 87.2 / 94.4 |
| `run_flat_8000` | 92.3 / 113.9 | 17.4 / 18.6 | 1.0 / 1.2 | 118.1 / 139.7 |

## Notes

- Elaboration (the tree-walking interpreter) dominates `arith`, `loops`, `recursion`, and `circuit_build`.
- Simulation dominates `qft` and `ghz_stabilizer`, which a different elaborator cannot speed up.
- The front end dominates the two `flat_8000` cases.
- Later results (the typed IR interpreter) are appended to this document as new sections, measured in the same run as the tree-walking interpreter so they share the machine state.