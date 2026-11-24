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
| `shapes` | Calls through interface values: areas of 300 structs and enums in a `List<Shape>` summed 400 times, and 20000 `scaled` calls that each return a new interface value | run, 1 shot |

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
## Typed IR interpreter versus tree-walking interpreter (2026-11-22)

The typed IR interpreter (`docs/design/qx-ir.md`, enabled with `--engine ir`) was measured against the tree-walking interpreter (`ast`) in the same run so both share the machine state:

```sh
QXIDE_BENCH_ENGINES=ast,ir QXIDE_BENCH_REPEAT=7 cargo test --release --bin qxide bench_suite -- --ignored --nocapture
```

The harness gained a `lower ms` column measuring the time to lower the checked program to the IR (IR engine only). For the IR engine, `elaborate ms` includes `lower ms`. The IR verifier runs only in builds with debug assertions and is not part of these release numbers (it runs as part of every `cargo test` run).

Environment: branch `feat/typed-ir` at commit `8c8dcaa` (final IR code, including frame reuse: the IR interpreter keeps the frames of returned calls per function and reuses them as they are, so repeated calls do not allocate or re-initialise a frame and cost no work in proportion to the function's size, except that a new frame sized for the whole function is still allocated the first time a function is called at a new call depth), `rustc 1.99.1`, release profile, same Windows 11 / i9-14900HX laptop, about 10 GB RAM free, CPU load about 39% before the run from other sessions on the machine (noisier than the baseline run; medians spread more), 7 repetitions. The `ast` numbers differ from the baseline table above due to run-to-run noise and because shared state was refactored into `Machine` (`src/lang/machine.rs`) in between; compare within this table.

A new generated case `many_loops_4000` was added in this round: one `main` with 4000 sequential `for i in 0..2` loops and 4000 `&&` assignments, run with 1 shot; it measures lowering work for long straight-line code that runs once.

Each timing cell reports `min / median`; `-` means not applicable.

| Case | engine | front ms | lower ms | elaborate ms | run/qasm ms | total ms |
| --- | --- | --- | --- | --- | --- | --- |
| arith | ast | 0.1 / 0.1 | - | 751.5 / 1112.3 | 0.0 / 0.0 | 751.6 / 1112.4 |
| arith | ir | 0.1 / 0.1 | 0.0 / 0.0 | 532.5 / 609.7 | 0.0 / 0.0 | 532.5 / 609.8 |
| circuit_build | ast | 0.1 / 0.1 | - | 156.0 / 163.2 | 77.3 / 84.4 | 235.2 / 249.6 |
| circuit_build | ir | 0.1 / 0.1 | 0.0 / 0.0 | 83.4 / 92.2 | 82.8 / 84.0 | 173.1 / 177.9 |
| ghz_stabilizer | ast | 0.0 / 0.1 | - | 0.4 / 0.4 | 939.0 / 947.9 | 939.5 / 948.4 |
| ghz_stabilizer | ir | 0.0 / 0.1 | 0.0 / 0.0 | 0.3 / 0.3 | 834.8 / 945.1 | 835.2 / 945.6 |
| loops | ast | 0.1 / 0.1 | - | 225.8 / 234.2 | 0.0 / 0.0 | 225.9 / 234.3 |
| loops | ir | 0.1 / 0.1 | 0.0 / 0.0 | 60.3 / 65.0 | 0.0 / 0.0 | 60.3 / 65.1 |
| qft | ast | 0.1 / 0.1 | - | 0.4 / 0.5 | 454.4 / 466.4 | 455.0 / 467.0 |
| qft | ir | 0.1 / 0.1 | 0.0 / 0.1 | 0.2 / 0.3 | 457.9 / 463.7 | 458.5 / 464.2 |
| recursion | ast | 0.1 / 0.1 | - | 176.5 / 186.0 | 0.0 / 0.0 | 176.6 / 186.1 |
| recursion | ir | 0.1 / 0.1 | 0.0 / 0.0 | 64.8 / 67.5 | 0.0 / 0.0 | 64.8 / 67.6 |
| check_flat_8000 | ast | 48.5 / 65.4 | - | - | - | 48.5 / 65.5 |
| check_flat_8000 | ir | 63.9 / 65.9 | - | - | - | 63.9 / 65.9 |
| run_flat_8000 | ast | 74.0 / 77.6 | - | 13.3 / 14.5 | 0.9 / 1.0 | 89.3 / 94.7 |
| run_flat_8000 | ir | 74.1 / 78.1 | 21.4 / 25.6 | 41.0 / 45.5 | 0.9 / 1.0 | 117.6 / 125.8 |
| many_loops_4000 | ast | 92.2 / 94.9 | - | 3.2 / 3.5 | 0.0 / 0.0 | 95.5 / 98.5 |
| many_loops_4000 | ir | 78.5 / 88.1 | 13.3 / 14.1 | 17.2 / 18.2 | 0.0 / 0.0 | 95.7 / 107.9 |

### Reading

- Elaboration time, IR versus tree-walking, by minimum: `loops` 60.3 vs 225.8 ms (about 3.7x less), `recursion` 64.8 vs 176.5 ms (about 2.7x less), `circuit_build` 83.4 vs 156.0 ms (about 1.9x less), `arith` 532.5 vs 751.5 ms (about 1.4x less).
- `qft` and `ghz_stabilizer` are dominated by simulation, which the elaborator does not change; their differences are noise. `check_flat_8000` does not elaborate, so both engines run the same code there (the front-end difference is noise).
- Code that runs once is slower on the IR path because every function is lowered before `main` runs: `run_flat_8000` 41.0 vs 13.3 ms elaboration (lowering 21.4 ms for 8000 functions each called once) and `many_loops_4000` 17.2 vs 3.2 ms (lowering 13.3 ms). Lowering is linear in program size but is not cached between runs (for example in the REPL or a desktop app that runs the same program repeatedly); caching or lowering functions on first call are possible follow-ups, not implemented.
- The IR interpreter stores values in typed virtual registers and stack slots indexed by number instead of looking variables up by name, calls functions by index, and reuses call frames; budget accounting (steps, operations, output) uses the same code as the tree-walking interpreter and is charged at the same points.
- Whether to make the IR engine the default is left to the maintainer; these numbers and the equivalence tests are the input for that decision.

## Cranelift JIT (2026-11-22)

The `jit` engine (`docs/design/qx-jit.md`; build with `--features jit`, run with `--engine jit`) compiles build-time classical functions (`int`/`double`/`boolean` only, calling only such functions) to native code with Cranelift and runs everything else in the IR interpreter. A function with a loop or a direct recursive call is compiled on its first call; any other compilable function is compiled on its 16th call, running in the IR interpreter until then. Measured against `ast` and `ir` in the same run (the harness uses the real threshold; tests use 1). For `jit`, `elaborate ms` includes compiling.

```sh
QXIDE_BENCH_ENGINES=ast,ir,jit QXIDE_BENCH_REPEAT=7 cargo test --release --features jit --bin qxide bench_suite -- --ignored --nocapture
```

Added generated case `pure_calls_4000`: 4000 small classical functions, each called once from `main`, to measure compile cost when nothing is hot.

Environment: branch `feat/cranelift-jit` at commit `e9b872d`, `rustc 1.99.1`, release profile, Cranelift 0.138 (`opt_level` `speed`, inline stack probes), same Windows 11 / i9-14900HX laptop, about 11 GB RAM free, CPU load about 49 % before the run from other sessions (noisy; compare engines within the table), 7 repetitions. Values are `min / median`; `-` indicates not applicable.

| Case | engine | front ms | lower ms | elaborate ms | run/qasm ms | total ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| arith | ast | 0.1 / 0.1 | - | 813.2 / 1101.9 | 0.0 / 0.0 | 813.2 / 1102.0 |
| arith | ir | 0.1 / 0.1 | 0.0 / 0.0 | 542.9 / 590.4 | 0.0 / 0.0 | 544.6 / 590.5 |
| arith | jit | 0.1 / 0.1 | 0.0 / 0.0 | 83.7 / 86.9 | 0.0 / 0.0 | 83.8 / 87.0 |
| circuit_build | ast | 0.1 / 0.1 | - | 155.3 / 163.1 | 78.5 / 83.8 | 241.8 / 246.2 |
| circuit_build | ir | 0.1 / 0.1 | 0.0 / 0.0 | 74.8 / 88.8 | 70.7 / 80.1 | 151.1 / 171.4 |
| circuit_build | jit | 0.1 / 0.1 | 0.0 / 0.0 | 74.8 / 91.1 | 68.6 / 85.8 | 153.2 / 178.5 |
| ghz_stabilizer | ast | 0.0 / 0.1 | - | 0.4 / 0.4 | 878.5 / 962.3 | 878.9 / 962.9 |
| ghz_stabilizer | ir | 0.1 / 0.1 | 0.0 / 0.0 | 0.3 / 0.4 | 1021.6 / 1066.1 | 1022.3 / 1066.6 |
| ghz_stabilizer | jit | 0.0 / 0.1 | 0.0 / 0.0 | 0.3 / 0.4 | 957.5 / 1027.3 | 957.9 / 1027.6 |
| loops | ast | 0.1 / 0.1 | - | 253.0 / 260.2 | 0.0 / 0.0 | 253.1 / 260.3 |
| loops | ir | 0.1 / 0.1 | 0.0 / 0.0 | 73.8 / 76.5 | 0.0 / 0.0 | 73.8 / 76.6 |
| loops | jit | 0.1 / 0.1 | 0.0 / 0.0 | 70.0 / 74.9 | 0.0 / 0.0 | 70.0 / 75.0 |
| qft | ast | 0.1 / 0.1 | - | 0.5 / 0.6 | 547.3 / 571.6 | 547.9 / 572.3 |
| qft | ir | 0.1 / 0.1 | 0.0 / 0.0 | 0.3 / 0.4 | 467.7 / 500.2 | 468.3 / 500.7 |
| qft | jit | 0.1 / 0.1 | 0.0 / 0.0 | 1.3 / 1.4 | 474.9 / 547.0 | 476.4 / 548.4 |
| recursion | ast | 0.1 / 0.1 | - | 179.1 / 199.4 | 0.0 / 0.0 | 179.1 / 199.5 |
| recursion | ir | 0.0 / 0.1 | 0.0 / 0.0 | 56.9 / 61.2 | 0.0 / 0.0 | 56.9 / 61.2 |
| recursion | jit | 0.0 / 0.1 | 0.0 / 0.0 | 5.4 / 5.4 | 0.0 / 0.0 | 5.4 / 5.5 |
| check_flat_8000 | ast | 70.9 / 77.6 | - | - | - | 70.9 / 77.6 |
| check_flat_8000 | ir | 64.3 / 66.5 | - | - | - | 64.3 / 66.5 |
| check_flat_8000 | jit | 62.5 / 75.5 | - | - | - | 62.5 / 75.5 |
| run_flat_8000 | ast | 73.2 / 79.3 | - | 11.1 / 13.8 | 0.8 / 0.9 | 86.1 / 94.5 |
| run_flat_8000 | ir | 69.6 / 75.1 | 28.8 / 30.5 | 44.1 / 47.8 | 0.9 / 1.0 | 115.0 / 125.1 |
| run_flat_8000 | jit | 70.4 / 77.7 | 27.1 / 30.7 | 43.1 / 47.8 | 0.9 / 1.0 | 115.0 / 124.3 |
| pure_calls_4000 | ast | 26.4 / 30.8 | - | 3.0 / 5.7 | 0.0 / 0.0 | 29.5 / 36.8 |
| pure_calls_4000 | ir | 24.6 / 28.1 | 12.5 / 13.7 | 16.1 / 20.0 | 0.0 / 0.0 | 44.4 / 48.6 |
| pure_calls_4000 | jit | 24.3 / 30.5 | 10.7 / 12.8 | 15.2 / 19.3 | 0.0 / 0.0 | 41.3 / 49.4 |
| many_loops_4000 | ast | 84.0 / 96.6 | - | 3.2 / 3.7 | 0.0 / 0.0 | 87.9 / 100.3 |
| many_loops_4000 | ir | 76.9 / 85.5 | 14.3 / 14.9 | 17.4 / 18.6 | 0.0 / 0.0 | 96.0 / 104.2 |
| many_loops_4000 | jit | 98.7 / 101.7 | 14.7 / 22.1 | 16.8 / 28.6 | 0.0 / 0.0 | 117.2 / 132.5 |

### Reading

- Work in compiled functions: `arith` 83.7 ms (`jit`) vs 542.9 (`ir`) vs 813.2 (`ast`), minimum (primality test and integer square root compiled); `recursion` 5.4 vs 56.9 vs 179.1 ms (`fib`, `isEven`, `isOdd` compiled).
- `loops` and `circuit_build`: unchanged by the JIT; the loops of `loops` are in `main`, which prints, and `circuit_build`'s layer function applies gates, so neither compiles (function granularity; outlining pure loops is open in `qx-jit.md`).
- `qft`: only `pow2` (it has a loop, so it compiles on its first call) is compiled; compiling costs about a millisecond (1.3 vs 0.3 ms elaboration), more than it saves at this size.
- `pure_calls_4000`: no function reaches 16 calls, so nothing is compiled and `jit` matches `ir`. In an earlier version that compiled every function on its first call, this case took about 1.5 s of elaboration (1504 ms minimum) because each compilation and code finalization costs a few hundred microseconds.
- `qft` and `ghz_stabilizer` stay dominated by simulation; `check_flat_8000` does not elaborate; `run_flat_8000` and `many_loops_4000` compile nothing.
- The machine was busy during this run (about 49 % CPU load); compare engines within this table.

## Interface values in native code (2026-11-24)

Measured on Windows x86-64 with `QXIDE_BENCH_ENGINES=ast,ir,jit QXIDE_BENCH_REPEAT=7 cargo test --release --features jit --bin qxide bench_suite -- --ignored --nocapture`.

| Case | Engine | elaborate ms (min / median) |
| --- | --- | --- |
| `shapes` | `ast` | 217.4 / 224.7 |
| `shapes` | `ir` | 90.0 / 99.6 |
| `shapes` | `jit` | 49.3 / 51.1 |
| `shapes` | `jit`, interface support disabled | 105.4 / 115.4 |

### Reading

- `shapes` is a new benchmark (`bench/qx/shapes.qx`) that sums areas through interface values over a `List<Shape>` of structs and enums and runs repeated `scaled` calls that return interface values.
- Before this change, functions that call through interfaces remained in the IR interpreter, so the disabled-support row reflects the previous JIT behavior (measured on the same build with the two interface classification rules turned off; `ir` measured 101.3 ms in that run).
- The machine was busy with another session's work during these runs, so other cases also measured slower than in the 2026-11-23 tables (for example, `arrays` `jit` 98.3 ms against 43.8 ms); compare engines within a single table only.
