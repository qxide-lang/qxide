# Execution limits: fixes for M-01, B-01, B-02, B-03, B-08, B-09, M-07, M-10

- **Date:** 2026-11-21
- **Branch:** `fix/execution-limits` (from `main` `b8c385e`)
- **Scope:** recommendation (1) of the Mythos review: an execution budget for B-01, B-02, B-03, B-08 and B-09, fixed together with M-01, plus caps for diagnostics, line length, output bytes and conditional nesting (M-07, M-10) in the shared `lang::Limits`.
- **Not touched:** the package client (M-02, M-03, M-04 are handled on another branch), the checker's quadratic time (M-05), `site/` (the M-07 button cap on the page), wasm maximum memory, and a wall-clock limit.

## What changed

`lang::Limits` (src/lang/mod.rs) gained seven fields. The CLI, the REPL, `qxide publish` (through `check_input`) and `qxide fmt` use `Limits::CLI`; the wasm playground uses `Limits::PLAYGROUND` for `check`, `run` and `qasm` (before this change, its `check` and `qasm` ran without limits).

| Field | CLI | Playground | Error |
| --- | --- | --- | --- |
| `max_steps`: interpreter steps (expression, loop iteration, call, variable compared in a lookup, one per element of whole-register work) | 2^30 | 2^26 | Q0413 |
| `max_ops`: circuit operations (gates, measurements, resets, `if` blocks on bits) | 2^24 | 2^20 | Q0414 |
| `max_output_bytes`: `println` output, the multi-shot table, OpenQASM text (estimated before it is built), rendered diagnostics | 256 MiB | 1 MiB | Q0415 (diagnostics: Q0418) |
| `max_cond_depth`: `if` blocks on measured bits nested across calls | 64 | 64 | Q0416 |
| `max_sim_work`: estimated simulation work of all shots | 2^36 | 2^32 | Q0417 |
| `max_diagnostics`: diagnostics rendered | 50 | 50 | Q0418 |
| `max_line_chars`: echoed source line, cut to a window around the column with `...` | 120 | 120 | (cut) |

Other changes:

- **B-09:** `Value::QReg`, `Value::Bits` and printed registers share storage (`Rc<[T]>`), so `r[i]` and variable reads no longer copy the register. Frame names borrow from the AST, so a loop iteration allocates nothing.
- **B-03:** each distinct measurement record adds its row, key and indented output to the output budget as it is created.
- **B-08 / B-03:** `shot_work` estimates one shot in units of about one state-vector amplitude update: state vector `(ops + 1) * 2^n`; stabilizer `16n` per column pass, `16n + n*ceil(n/64)/4` per measurement or reset, plus the tableau allocation; `+128` per shot. The total is multiplied by the shot count and checked before simulating. A state vector over the memory limit still reports Q0406 first.
- **M-01:** `diag::render_all` renders at most 50 diagnostics, stops before the byte limit, and ends with a Q0418 summary; `SourceMap::snippet` shows a 120-character window. Lines within the window render byte-for-byte as before.
- **M-10:** nesting is counted in the interpreter across calls; QASM size is an upper bound summed per recorded line, checked before `to_qasm`.
- New codes Q0413-Q0418 with entries in `docs/error-codes.md` and rows in `docs/design/qx-language.md` section 17.

## Regression tests (red before the fix, green after)

The red run used the branch with the new `Limits` fields and functions present but not enforced. Inputs that would grow without bound before the fix were given finite bounds; the others are the literal reproduction inputs.

| Finding | Test | Red result |
| --- | --- | --- |
| M-01 | `lang::limits_tests::m01_many_errors_on_one_line_render_a_bounded_report` | 2000 diagnostics, about 12 MB |
| M-01 | `m01_long_lines_are_cut_around_the_error_column` | whole 10 kB line echoed |
| M-01 | `m01_rendered_diagnostics_stop_at_the_byte_limit` | no cap |
| M-01 | `m01_lines_within_the_limit_are_echoed_unchanged` | passes (guard) |
| M-01 | `tests/cli.rs::check_of_one_line_with_many_errors_prints_a_bounded_report` | assertion failed |
| M-01, M-07 | `qxide-wasm tests::playground_check_output_is_capped` | 192,946,467 bytes |
| B-01 | `b01_endless_loop_stops_at_the_step_limit`, `b01_exponential_recursion_stops_at_the_step_limit` | did not finish in 20 s |
| B-01 | `tests/cli.rs::endless_loop_stops_at_the_default_step_limit` (`#[ignore]`, about 75 s) | - |
| B-02 | `b02_println_in_a_loop_stops_at_the_output_limit` | 20,200,000 bytes of output |
| B-02 | `b02_gates_in_a_loop_stop_at_the_operation_limit` | no error |
| B-03 | `b03_multi_shot_output_stops_at_the_output_limit` | 8,945,124 bytes of output |
| B-03 | `b03_many_shots_with_few_records_still_run` | passes (guard) |
| B-08 | `b08_huge_stabilizer_run_is_refused_before_simulating` (GHZ 32768) | did not finish in 30 s |
| B-08 | `b08_shot_count_is_part_of_the_simulation_budget` (10^12 shots) | did not finish in 30 s |
| B-09 | `b09_indexing_a_large_register_does_not_copy_it` (GHZ 131072 `qasm`) | did not finish in 30 s |
| B-09 | `b09_register_arguments_are_charged_per_qubit` | did not finish in 30 s |
| M-10 | `m10_conditionals_nested_across_calls_stop_at_the_depth_limit` | 297,382,936 bytes of QASM |
| M-10 | `tests/cli.rs::qasm_refuses_conditionals_nested_too_deeply_across_calls` | 297,382,936 bytes of stdout |
| M-10 | `m10_qasm_text_stops_at_the_output_limit` | 80,081 bytes |
| M-10 | `m10_nesting_within_one_function_stays_valid` | passes (guard) |
| M-07 | `qxide-wasm tests::playground_output_is_capped_before_it_leaves_the_module` | 202,000,000 bytes of stdout |

The catalog test runs the Q0413, Q0414, Q0415 and Q0417 examples under small limits; Q0416 and Q0418 run at CLI defaults.

## How the defaults were chosen

**High-water marks of normal programs.** Counters were logged (temporary instrumentation, not committed) during `cargo test --workspace` without the limit tests, plus `check`, `qasm` and `run --shots 1000` of `examples/qx/*.qx`, the `.qx` programs embedded in `site/src` (playground examples, Home, Learn, posts) and `examples/packages/bell-app`:

| Measure | Highest value | CLI default | Headroom |
| --- | --- | --- | --- |
| steps | 600,009 (`qreg(200000)` test) | 1,073,741,824 | about 1,790x |
| operations | 200,000 | 16,777,216 | about 84x |
| program output | 600,001 bytes | 256 MiB | about 447x |
| QASM estimate | 6,088,986 bytes | 256 MiB | about 44x |
| conditional nesting | 2 | 64 | single functions can still nest 61 levels (parser limit) |
| simulation work of a run that simulates | 6.6e7 (50 shots, 200 stabilizer qubits) | 6.9e10 | about 1,000x |
| diagnostics per run | 3 | 50 | |

The largest legitimate program at the qubit cap, `qxide qasm` of GHZ on 1,048,576 qubits, still runs: 0.46 s, 53 MB of output, peak working set 213 MB.

**Worst case at each CLI default** (release build, Windows 11, the reproduction inputs):

| Input | Result | Wall time | Peak working set |
| --- | --- | --- | --- |
| `loop.qx` (B-01) | Q0413 | 75.0 s | 4 MB |
| `rec.qx` (B-01) | Q0413 | 77.5 s | 5 MB |
| 4-argument call in an endless loop | Q0413 | 66.0 s | 4 MB |
| 1000 variables, endless loop reading the first | Q0413 | 0.45 s | 5 MB |
| `print.qx` (B-02) | Q0415 | 0.50 s | 406 MB |
| `gates.qx` (B-02) | Q0414 | 1.75 s | 646 MB |
| `amplify.qx --shots 100000` (B-03) | Q0415 | 0.22 s | 331 MB |
| GHZ 32768 `run` (B-08, 97.9 s before) | Q0417 | 0.13 s | 3 MB |
| GHZ 16384 `--shots 10^12` | Q0417 | 0.13 s | 4 MB |
| GHZ 131072 `qasm` (B-09, 86 s before) | ok, 6.4 MB | 0.14 s | 4 MB |
| `nest_198_61.qx` `qasm` (M-10) | Q0416 | 0.12 s | 4 MB |
| `oneline_50000.qx` `check` (M-01, over 3.4 GB before) | 15,711 bytes, Q0418 | 0.11 s | 4 MB |

Calibration of `shot_work` on the same machine: a 22-qubit state vector ran 463 operations in 2.7 s (about 1.4 ns per amplitude update); stabilizer column passes cost about 12 ns per row at n = 20,000 (400,000 gates: 193 s), and GHZ runs measured 0.03-0.13 ns per estimated row-reduction word. Measured just under the CLI work limit 2^36: 22 qubits x 16,303 operations (mostly `t`) took 188.5 s (about 2.7 ns per unit, since rotations cost more than the calibration gates), 200,001 stabilizer gates on 20,001 qubits took 90.0 s, and GHZ 16384 took 16.7 s; the worst case is therefore about 3 minutes; programs just over it (22 qubits x 16,522 operations; 220,002 stabilizer gates on 20,001 qubits, 193 s before) are refused at once. The playground limits are 16x (steps, work) and 256x (output) smaller.

**Playground** (`qxide_wasm.wasm` built with `--profile wasm`, run in Node 24.21 through the same raw ABI calls as `site/src/playground/engine.ts`; no browser was used):

| Input | Result | Time | wasm memory |
| --- | --- | --- | --- |
| `nest_31_2.qx` (64 levels across 32 calls), `qasm` and `run` | ok | 4 ms | 16 MiB |
| `nest_64_1.qx` (65 levels) | Q0416 | 4 ms | 16 MiB |
| `nest_100_60.qx` `qasm` (stack overflow `RangeError` before), `nest_198_61.qx` `run` | Q0416 | 4 ms | 16 MiB |
| `oneline_50000.qx` `check` (trap at 4 GiB after 291 s before) | 13,511 bytes, Q0418 | 27 ms | 36 MiB |
| `bigout.qx` (M-07, 202 MB before) | Q0415 | 7 ms | 19 MiB |
| `loop.qx`, `rec.qx`, endless 4-argument calls | Q0413 | 1.2 s, 1.3 s, 2.0 s | 16 MiB |
| `print.qx` / `gates.qx` (B-02, trap at about 3 GiB before) | Q0415 / Q0414 | 7 ms / 83 ms | 19 / 65 MiB |
| `amplify.qx` with 100,000 shots (B-03) | Q0415 | 5 ms | 19 MiB |
| 20 qubits x 4,081 `t` operations, just under the work limit | ok | 14.4 s | 32 MiB |
| GHZ 7000 (under the limit) / GHZ 8192 (over) | ok / Q0417 | 1.0 s / 8 ms | 41 / 17 MiB |
| GHZ 131072 `qasm` | Q0415 (7.2 MB estimate) | 32 ms | 32 MiB |

The slowest run the playground limits admit took 14.4 s, inside the page's 30 s worker limit.

## Results

- `cargo test --workspace`: see the commit; all suites pass (the CLI test for B-01 at default limits is `#[ignore]`).
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p qxide-wasm --target wasm32-unknown-unknown -- -D warnings`, `cargo fmt --all -- --check`: clean.

## Remaining

- M-05 (quadratic checker time) is unchanged; `check` of about 1 MB can still take 40 s.
- Playground limits were checked in Node, not in a browser; page-side rendering of the (now at most 1 MiB) output was not measured.
- Rotation gates cost about twice the calibration unit on the state vector (188.5 s at the CLI limit, 14.4 s in the playground). Weighting non-Clifford operations by 2 would halve those worst cases but refuse rotation-heavy simulations earlier; left for a decision.
- No CLI flag to raise the limits; a legitimate simulation above the estimate (up to about 3 minutes of real time) is now refused with Q0417; the state-vector unit weight could be raised for non-Clifford gates.
- No wall-clock limit and no wasm maximum memory; the page still renders up to 1 MiB of output and up to 50 diagnostic buttons on its main thread.
- `app-framework-next` also changes `Limits` (`Limits::APP`, `scope`); merging needs the new fields added to `Limits::APP`.
- Step costs are approximate: a step ranges from under 1 ns (a variable compared in a lookup) to about 70 ns (an empty loop iteration).
