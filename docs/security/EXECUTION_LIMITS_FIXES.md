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

- `cargo test --workspace`: all 13 test binaries pass (qxide bin 124, wasm crate 113, cli 23 + 1 ignored, plus the library suites); the CLI test for B-01 at default limits is `#[ignore]` (about 75 s).
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p qxide-wasm --target wasm32-unknown-unknown -- -D warnings`, `cargo fmt --all -- --check`: clean.

## Remaining

- M-05 (quadratic checker time) is unchanged; `check` of about 1 MB can still take 40 s.
- Playground limits were checked in Node, not in a browser; page-side rendering of the (now at most 1 MiB) output was not measured.
- Rotation gates cost about twice the calibration unit on the state vector (188.5 s at the CLI limit, 14.4 s in the playground); see the decision below.

## Decision: rotation gates keep weight 1

Decided by the maintainer on 2026-11-21: every state-vector operation, rotations included, keeps a weight of one unit in `shot_work`, and the code is unchanged. A weight of 2 for non-Clifford operations would halve the measured worst cases, but it would also refuse legitimate long simulations that are heavy in rotations at about half the size they are allowed today, and legitimate long runs take priority. The cost of this choice is bounded and known: a run admitted by the defaults takes at most about 3 minutes in the CLI (188.5 s measured) and 14.4 s in the playground, which stays inside the page's 30 s worker limit. Runs at the extreme end are left to a wall-clock limit, planned as separate work.
- No CLI flag to raise the limits; a legitimate simulation above the estimate (up to about 3 minutes of real time) is now refused with Q0417; the state-vector unit weight could be raised for non-Clifford gates.
- No wall-clock limit and no wasm maximum memory; the page still renders up to 1 MiB of output and up to 50 diagnostic buttons on its main thread.
- `app-framework-next` also changes `Limits` (`Limits::APP`, `scope`); merging needs the new fields added to `Limits::APP`. (Done on `app-framework-limits`; see part 2.)
- Step costs are approximate: a step ranges from under 1 ns (a variable compared in a lookup) to about 70 ns (an empty loop iteration).

# Part 2: desktop apps (`Limits::APP`), String and std budgets

- **Date:** 2026-11-21
- **Branch:** `app-framework-limits`, from `app-framework-next` (`2dc2039`), with `main` (`846d18e`: stack, package client and registry fixes) and then `fix/execution-limits` (`5feb456`) merged in. Neither source branch was changed.
- **Scope:** the execution budgets of part 1 applied to desktop apps and Qxide Studio, measured values for `Limits::APP`, budgets for the `String` and `std` paths that `app-framework-next` added, and the interpreter stack for app threads. Decisions taken by the maintainer before the work: measure before choosing `Limits::APP`, budget the String and std paths, use `STACK_SIZE` for app threads, keep rotation weight 1.

## Merge

Conflicts were resolved with the integration branch `integration/check-20261121` (`fa61551`) as the reference.

- `src/lang/interp.rs`: the `fa61551` version (stack-split helpers with the limit checks ported) plus the app pieces: the `Host`, `Value::Str`, std calls and `elaborate_call`, which now takes `&Limits`. Cancellation used to be polled every 4096 statements; it is now polled inside `charge` every 4096 budget steps. A loop with an empty body has no statements, so under the old poll it could not be cancelled.
- `src/lang/mod.rs`: `build_input` takes both the host and the limits. `check_sim_work` runs in the app's `sample`, the B-03 per-record output budget runs in `run_built`, and `qasm` uses the `NoEffects` host together with `check_qasm_size`. `Limits` keeps the app's `scope` field next to the seven new fields; `PLAYGROUND` has scope `Playground`.
- `crates/qxide-wasm`: `LIMITS = Limits::PLAYGROUND`. `check` and `qasm` go through the new `check_input_with`/`qasm_input_with`, so the playground's std permissions still apply.
- `src/lang/diag.rs`: taken from `fa61551` on this branch; `main` uses the version the maintainer decided on (see the merge decisions below).
- `docs/design/qx-language.md`: the app section stays 17 and the limits section becomes 18.
- Code that broke without a conflict: `command.rs:100` (`CallError::from_diags` rendered without limits; it now uses `render_all` with `Limits::APP` and lists at most `max_diagnostics` structured diagnostics) and `studio.rs:78` (`std.qxide.qasm` now builds under `Limits::APP` and checks the QASM estimate before calling `to_qasm`). `studio.rs:104` (`std.qxide.run`) compiles unchanged but now runs with the budgets of `Limits::APP`.

## `Limits::APP`

Limits apply per call: a command call, or `std.qxide.run`/`check`/`qasm` inside Qxide Studio. The runtime runs at most four calls at once (`MAX_WORKERS`), and the page can cancel a call. Cancellation is polled every 4096 budget steps while the program is elaborated and between shots, but not inside a shot. That is why the time budgets of elaboration are larger than the CLI's, the budget for a single shot stays at the CLI's value, and the memory budgets are per call and sized for four calls at once.

| Field | CLI | `Limits::APP` | Basis |
| --- | --- | --- | --- |
| `max_steps` | 2^30 | 2^31 | Cancellable, so twice the CLI. Worst case measured: an empty loop stops after 160.3 s (release build), against 75 s for the CLI at 2^30. String memory is bounded by `max_steps / 8` = 256 MiB per call (see below). |
| `max_ops` | 2^24 | 2^22 | Memory: the recorded circuit takes about 40 bytes per operation (`gates in a loop` stops at 2^22 with a 166 MiB peak working set, 0.52 s). 2^22 keeps one call's circuit near 160 MiB (about 640 MiB for four calls). This is lower than the CLI. |
| `max_sim_work` | 2^36 | 2^36 | A shot cannot be cancelled, so this stays at the CLI value. Worst case at the app's 24-qubit cap: one shot of 24 qubits x 4,094 operations (mostly `t`) took 179.0 s with a 261 MiB peak working set; one operation more is refused at once with `Q0417`. |
| `max_output_bytes` | 256 MiB | 32 MiB | A call result above 32 MiB never reaches the page (`MAX_SCRIPT_BYTES`, `Q0716`), so a larger budget would only build text that is then thrown away. Worst case: `println` in a loop stops at 32 MiB with a 46 MiB peak working set in 0.13 s. This is lower than the CLI. |
| `max_diagnostics` | 50 | 100 | Studio lists diagnostics in a panel rather than a terminal; each one is a short JSON object. |
| `max_line_chars` | 120 | 240 | Studio's editor is wider than a terminal. The cap only shortens the echoed line and rejects nothing. |
| `max_cond_depth` | 64 | 128 | Native app threads have the 256 MiB stack, and the QASM estimate (which includes indentation) is still checked against `max_output_bytes`. |

**Normal use, measured headlessly** (`lang::app_limits_tests::app_limits_keep_their_headroom_over_normal_use`, which prints this table and fails if a growing budget keeps less than 10x headroom). The workload: the seven Studio examples (`examples/qx`, the same files as `apps/studio/ui/examples`) built and run as Studio runs them at 1, 1,000 and 1,000,000 shots (the most Studio's shots field accepts); the Studio command module (`run`, `check`, `qasm`, `format`, `save`, `load`, `set_file`) with each example as the editor buffer and the real `std.qxide` implementation; the bell-app commands, including `history` over a 41 KB results file of 1,000 saves; and each example with every `;` removed, for the diagnostics count. The runtime crate also runs bell-app `measure_bell` at 1,000,000 shots through `call_app` with `CALL_LIMITS` (`tests::example_app_runs_at_the_shot_limit`).

| Measure | Highest normal use | `Limits::APP` | Headroom |
| --- | --- | --- | --- |
| steps | 344,358 (bell-app `history`: importing 41 KB of text at 8 steps per byte) | 2,147,483,648 | 6,236x |
| operations | 14 | 4,194,304 | 299,593x |
| simulation work | 935,000,000 (an example at 1,000,000 shots) | 68,719,476,736 | 73x |
| output of a run | 216 bytes | 33,554,432 | 155,345x |
| QASM estimate | 366 bytes | 33,554,432 | 91,679x |
| diagnostics | 1 | 100 | 100x |
| longest source line | 96 characters | 240 | 2.5x (the window only cuts the echo) |
| conditional nesting | 2 | 128 | 64x |

Studio runs whatever program the user types, so the CLI high-water marks of part 1 apply too: steps 600,009 (3,579x below `Limits::APP`), operations 200,000 (21x), output 600,001 bytes (56x), QASM estimate 6,088,986 bytes (5.5x), simulation work 6.6e7 (1,041x), diagnostics 3, nesting 2. The QASM estimate comes from a 200,000-qubit GHZ stress test. The largest program the CLI admits, the QASM of GHZ on 1,048,576 qubits (53 MB), is refused in apps with `Q0415`; at 32 MiB it would have been refused at the page boundary anyway.

**Worst cases at `Limits::APP`** (release build, Windows 11, the same machine as part 1; the `#[ignore]` tests `app_worst_*` in `src/lang/app_limits_tests.rs`, which print the time and the process's peak working set):

| Input | Result | Wall time | Peak working set |
| --- | --- | --- | --- |
| empty `for` loop | Q0413 | 160.3 s | 4 MiB |
| `s = s + s` 60 times | Q0413 | 0.05 s | 166 MiB |
| `h(q)` in a loop | Q0414 | 0.52 s | 166 MiB |
| `println` of 40 bytes in a loop | Q0415 | 0.13 s | 46 MiB |
| 24 qubits x 4,094 operations, 1 shot (just under the work limit) | ok | 179.0 s (not cancellable) | 261 MiB |
| the same with 4,095 operations | Q0417 | under 0.01 s | - |

## String and std budgets

`app-framework-next` added `String` values and std calls without any budget: concatenation, literals, `==`/`!=`, `len`, and text passed to or returned from std calls all did work and allocated memory in proportion to their length, and every read of a `String` variable copied the text.

- `Value::Str` holds `Rc<str>`, so reading a variable or passing a `String` shares the text, as registers do since B-09.
- `STR_BYTE_STEPS = 8` steps per byte, charged before the work: building a literal (each time it is evaluated), the result of `+`, the shorter operand of `==`/`!=`, `len` (which counts characters), each `String` argument passed to a std call, and each `String` a std call returns. Every live byte was charged when it was built, so string memory per call is at most `max_steps / 8`: 128 MiB for the CLI, 8 MiB for the playground and 256 MiB per app call. The weight 8 is a memory weight, not a time weight: copying a byte costs far less than a step.
- Hosts already cap what they read (16 MiB for `std.fs.read_text` and `std.http.get`), so charging returned text after the call bounds repetition, not one read. `String` arguments of a command call come from the page and are converted before the interpreter starts, so they are not charged (see Remaining). `println` of a `String` copies the text into the output, which `max_output_bytes` already counts.
- Command calls with several shots add each new record's JSON row to `max_output_bytes`, as `qxide run` does for its table (B-03 on the command path).

**Memory per call, all budgets together.** The budgets add up within one call rather than sharing one cap. While a call elaborates it can hold up to 256 MiB of strings, about 166 MiB of circuit and about 46 MiB of output; while it simulates, a 256 MiB state vector or tableau plus its circuit and output. That is roughly 0.5 GiB per call and roughly 2 GiB across four workers, and more for a Studio call, whose `std.qxide.run` builds the user's program under a second `Limits::APP`. This sum is an estimate from the single-budget measurements above; no combined case was measured. Only the simulation part (four calls within 1 GiB) is enforced by a test (`concurrent_calls_share_one_memory_budget`). Lowering `max_steps` or raising the string weight would lower the string share.

## Interpreter stack in apps

`qxide app` validation (`src/app/mod.rs`) and the runtime's worker pool (`crates/qxide-app-runtime/src/main.rs`) spawned 64 MiB threads. Both now use `lang::STACK_SIZE` (256 MiB). The deepest legal program needs about 100 MiB in an unoptimized build and about 45 MiB optimized (part of the stack-overflow fix on `main`). On Windows, Rust reserves thread stacks with `STACK_SIZE_PARAM_IS_A_RESERVATION`, and Linux and macOS map them lazily, so four workers reserve 1 GiB of address space but each commits only what its call uses. The runtime test `workers_have_the_interpreter_stack` runs the deepest chain program on a worker.

## Regression tests

The red runs used this branch before each fix (the String tests against the merged interpreter without the String budgets, the Studio QASM test with the new size check removed).

| Path | Test | Red result |
| --- | --- | --- |
| concatenation | `lang::limits_tests::str_doubling_stops_at_the_step_limit` | ran to the end and printed `16777216` |
| String copy on read | `str_variable_reads_do_not_copy_the_text` | did not finish in 20 s |
| `len(String)` | `str_len_is_charged_per_byte` | ran to the end |
| `==` on strings | `str_comparison_is_charged_per_byte` | ran to the end |
| literals in a loop | `str_literals_are_charged_each_time_they_are_built` | ran to the end |
| std results (`fs.read_text`) | `std_results_are_charged_per_byte` | ran to the end |
| std arguments (`fs.write_text`) | `std_arguments_are_charged_per_byte` | ran to the end |
| multi-shot command result | `lang::app_tests::command_counts_stop_at_the_output_limit` | 7,054,075 bytes of JSON |
| diagnostics sent to a page | `command_errors_list_at_most_max_diagnostics` | 500 structured diagnostics |
| Studio `qasm` | `studio_qasm_uses_the_app_output_limit` | 41,777,839 bytes of JSON |
| cancellation without statements | `cancelling_stops_a_loop_without_statements` | not run red: on `app-framework-next`, a loop with an empty body is never polled and has no step budget |
| app limit headroom | `lang::app_limits_tests::app_limits_keep_their_headroom_over_normal_use` | guard |
| worker stack | runtime `tests::workers_have_the_interpreter_stack` | guard; the test profile is optimized, so 64 MiB would also pass |
| bell-app at the shot cap | runtime `tests::example_app_runs_at_the_shot_limit` | guard |

## Merge decisions

Decided by the maintainer on 2026-11-21 and applied when `fix/execution-limits` was merged into `main`; `app-framework-limits` then takes the `main` versions.

1. **`src/lang/diag.rs`: control characters and lines over `max_line_chars`.** The line is escaped first and then windowed. Positions count displayed characters, an escaped control character counts as its whole `\u{..}` escape, and the window only cuts between characters, so it never ends inside an escape. The echoed text stays within `max_line_chars` displayed characters, and the carets cover the escapes inside the span. Test: `diag::tests::long_lines_are_windowed_after_escaping`.
2. **Depth tests for `bit_if`/`bit_else`.** `max_cond_depth` counts nesting across calls, so the old shape (61 nested `if m` blocks in each of 199 recursive calls) fails with `Q0416`. The tests in `src/lang/tests.rs` (also compiled by `qxide-wasm`) and `tests/cli.rs` recurse to the call limit and nest the 61 levels only in the bottom call, so the total nesting stays within 64. The comment on `lang::STACK_SIZE` now says that `if` on a `Bit` nests at most `max_cond_depth` deep across all calls and that the stack measurements predate that limit.

## Results

- `cargo test --workspace -j 8`: all 13 test binaries pass (qxide bin 184 + 5 ignored, `qxide-wasm` 156 + 5 ignored, cli 25 + 1 ignored, plus the library suites). The ignored tests are the `app_worst_*` measurements and the B-01 CLI test of part 1.
- `cargo test --manifest-path crates/qxide-app-runtime/Cargo.toml`: 164 pass, 5 ignored (the same `app_worst_*` measurements, which the runtime crate also compiles).
- `cargo clippy --workspace --all-targets -- -D warnings`, the same for the runtime crate, `cargo clippy -p qxide-wasm --target wasm32-unknown-unknown -- -D warnings`, and `cargo fmt --check` for both: clean.
- No desktop window was opened; everything above ran headless.

## Remaining

- `max_sim_work` and `max_output_bytes` (and `max_ops`) are not larger than the CLI's: one shot cannot be cancelled, a result over 32 MiB cannot reach the page, and four calls share memory. If longer uncancellable shots are acceptable, raising `max_sim_work` is a one-line change.
- A shot is still not cancellable, and there is still no wall-clock limit.
- The string weight applies to the CLI and the playground too: strings there are now bounded by 128 MiB and 8 MiB, and one 16 MiB `std.fs.read_text` in the CLI uses about 12% of its step budget.
- The combined memory of one app call (about 0.5 GiB, about 2 GiB for four) is an estimate, not a measurement.
- Text arriving from the page as a command argument is not charged, and the runtime does not cap the size of incoming IPC messages itself (only whatever the webview imposes, which was not measured).
- Studio's `std.qxide.run` builds the inner program under its own `Limits::APP`, so a Studio call can use the budgets twice: once for the Studio command and once for the user's program.
