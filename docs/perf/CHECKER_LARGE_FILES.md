# `qxide check` on large files: measurements and causes (M-05)

This note measures how `qxide check` time grows with input size, as reported in M-05 of the
security review (`docs/security/MYTHOS_FINDINGS.md` on `security-review-prep`). It also names the
functions responsible and records the one fix made on this branch. Line numbers refer to
`src/lang/check.rs` at `b8c385e` (`main`), the same revision M-05 cites, unless a line says
otherwise.

## Summary

- The checker has four separate super-linear paths. Lexing, parsing and linking stayed linear in
  every measurement (`flat_functions` checks 7 MB in 0.4 s).
- **A. `conditional_safety` fixpoint** (`:283-301`). A call chain declared callers first needs one
  sweep per link, so the cost is quadratic in the number of functions: 32,000 functions take 42 s.
  **Fixed on this branch**, and the same file now takes 0.077 s.
- Further super-linear paths in the checker are open. Open items are tracked privately.

## Environment and method

- Intel Core i9-14900HX, Windows 11 Pro, rustc 1.99.1, release profile (`cargo build --release
  -j 8`), and the `qxide` CLI built from this branch. The "before" numbers use `b8c385e` sources
  and the "after" numbers use this branch's fix.
- `scripts/perf/checker_large_files.py` generates the inputs, one family per code path. It runs
  `qxide check FILE` and reports the median wall time of three runs, including process start of
  about 7 to 10 ms, which flattens the slopes of the smallest sizes. A size whose first run
  exceeds the budget (20 s by default) is measured once, and the family stops there. `slope` is
  `log2(t2/t1) / log2(n2/n1)` between consecutive sizes, so 1 means linear and 2 quadratic.
- `scripts/perf/sample_profile.py` is a sampling profiler written for this note because the
  machine has no usable profiler without installs: the Visual Studio collector service is stopped,
  and starting it needs administrator rights. It suspends the target's threads about once per
  millisecond, walks the stacks with the system `dbghelp.dll` (`StackWalk64`), and symbolizes
  frames, including inlined ones, against the PDB of a build with `CARGO_PROFILE_RELEASE_DEBUG=true`.
  Percentages below are shares of the samples taken on the checking thread. In about one run in
  four the stacks lack the binary's frames, the script prints a warning, and a rerun works.

Reproduce:

```bash
cargo build --release -j 8
python -I scripts/perf/checker_large_files.py --json target/perf-checker/results.json
CARGO_PROFILE_RELEASE_DEBUG=true CARGO_TARGET_DIR=target/prof cargo build --release -j 8 --bin qxide
python -I scripts/perf/sample_profile.py target/prof/release/qxide.exe check target/perf-checker/chain_callers_first_16000.qx
```

## Input families

| Family | Shape | Path |
| --- | --- | --- |
| `chain_callers_first` | `f0` calls `f1`, …, `fN` prints, declared in that order (the M-05 repro) | A |
| `chain_callees_first` | the same call graph declared in reverse | control for A |
| `flat_functions` | N independent functions with a qubit, `h`, `measure`, `println` | baseline |

## Results before the fix

Seconds are wall time of `qxide check` (median of 3; values marked ¹ are single runs above the
budget). Every doubling from the first size was measured, but only some rows are shown; `Slope`
is always relative to the previous measured size, which is not necessarily the row above.

| Family | N | Bytes | Seconds | Slope |
| --- | ---: | ---: | ---: | ---: |
| `chain_callers_first` | 1,000 | 20,830 | 0.020 | |
| | 4,000 | 89,830 | 0.233 | 1.89 |
| | 8,000 | 181,830 | 0.912 | 1.97 |
| | 16,000 | 377,832 | 5.546 | 2.60 |
| | 32,000 | 777,832 | 42.277 ¹ | 2.93 |
| `chain_callees_first` | 8,000 | 181,830 | 0.018 | 0.68 |
| | 32,000 | 777,832 | 0.062 | 0.84 |
| | 64,000 | 1,577,832 | 0.128 | 1.05 |
| `flat_functions` | 8,000 | 861,795 | 0.055 | 0.32 |
| | 32,000 | 3,497,795 | 0.242 | 0.71 |
| | 64,000 | 7,017,795 | 0.416 | 0.79 |

The full step lists (every doubling, 1,000 or 2,500 upward) come from the script, and the table
keeps the informative rows.

### Above quadratic

- `chain_callers_first` at 16,000, 20,000, 24,000, 28,000 and 32,000 functions takes 6.07, 11.79,
  20.09, 30.14 and 39.99 s, a smooth rise with no step. The sweep performs about 1.5·N² hash map
  lookups of `String` keys (SipHash), which works out to about 16 ns per lookup at 16,000 and
  26 ns at 32,000. A per-lookup cost that grows with the working set points to cache misses, but
  this was not measured with hardware counters.

## Profiles (before the fix)

Samples on the checking thread with the final version of the profiler (system-DLL leaves
included); "inclusive" counts a sample for every `check.rs` line on its stack. The binary was
built from `b8c385e` sources with full debug info.

| Input | Where the time goes |
| --- | --- |
| `chain_callers_first`, 16,000 | `conditional_safety`: `:286` (`contains_key(name)` per sweep) 66 %, `:291` (`find` over calls) 32 %; leaves are SipHash and hashbrown probing |

## Causes

**A. `conditional_safety`.** The loop at `:283-301` repeats a sweep over all functions until
nothing changes. A function is marked only when, at the moment it is visited, a callee is already
marked. In a chain declared callers first, each sweep marks one more function from the end of the
chain, so N functions take N sweeps of N entries, each costing a `String` hash lookup.
`chain_callees_first`, with the same graph in the other order, finishes in one sweep and is linear.

Further causes are open. Open items are tracked privately.

## Fix on this branch: A

Commit `perf(check): compute conditional safety in near-linear time` replaces the sweep with
`propagate`. It computes, for each function, the time (sweep number, declaration index) at which
the old sweep would mark it, using Dijkstra over reverse call edges. In sweep q, the function at
index e sees a callee marked in an earlier sweep, or in sweep q at a smaller index, and
functions with a direct reason count as sweep 0. The recorded callee and span are the first call
the old sweep would have found, so the `Q0314` notes are unchanged. The cost is O(E log E) for E
call sites.

Verification:

- The new unit test `check::tests::propagate_records_what_the_sweep_records` keeps the old sweep as
  a reference and compares the two results on 3,000 random call graphs. The graphs include several
  callees per function, cycles, self-calls, duplicate names, calls to undefined names and both
  declaration orders. Two deliberately wrong variants of `propagate` fail it: ignoring visibility
  within a sweep fails at round 35, and recording the last matching call instead of the first fails
  at round 2.
- `cargo test -j 8`: all suites pass (108 tests in the `qxide` binary, including the new one).
  `cargo clippy -j 8 --all-targets -- -D warnings` and `cargo fmt --check` are clean.

Before and after (median of 3, release):

| Family | N | Before (s) | After (s) |
| --- | ---: | ---: | ---: |
| `chain_callers_first` | 8,000 | 0.912 | 0.020 |
| | 16,000 | 5.546 | 0.040 |
| | 32,000 | 42.277 ¹ | 0.077 |
| | 64,000 | not run | 0.159 |
| `chain_callees_first` | 32,000 | 0.062 | 0.075 |
| | 64,000 | 0.128 | 0.151 |
| `flat_functions` | 64,000 | 0.416 | 0.379 |

The slope of `chain_callers_first` is now 0.95 to 1.04 from 16,000 upward. `chain_callees_first`
got slightly slower. An A/B run with the same build flags (release with debug info, median of 7)
gives 0.063 s before and 0.069 s after at 32,000 functions, and 0.120 s against 0.144 s at
64,000. The old code needed a single sweep for this order, while `propagate` always builds the
caller map and the heap, so it costs about 20 % more on this best case for the old code.

## Status of M-05 after this branch

The call-chain half of M-05 is fixed. The rest of M-05 is open. Open items are tracked privately.
