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
- **B. `states()` snapshots** (`:401-409`, used at `:547`, `:644`, `:646`, `:653` and `:939`).
  Every `if`, `for`, `&&` and `||` copies the state of every variable declared so far in the
  function, including variables of scopes that have already closed. The cost is O(variables ×
  branches): 40,000 declarations plus 40,000 `if`s take 42 s, and the same shape with `Qubit`
  variables takes 48 s. **Not fixed**, with a proposal below.
- **C. Shadowing check on declaration** (`:473`). This one is new and not part of M-05. Each
  declaration scans every variable of the current scope, so N declarations in one block cost N²/2
  name comparisons: 80,000 declarations take 9.9 s with no branches at all. **Not fixed**.
- **D. `lookup`** (`:392-399`) scans all visible variables. Its own share is the gap between
  `lookup_far` and `lookup_near`, which is 4.0 s at 80,000 declarations. **Not fixed**.
- The measured exponents reach 2.9 to 3.6 at the largest sizes even though the operation counts
  are quadratic. For B the time per unit of work moves with the snapshot size: it jumps between
  21,000 and 22,000 variables, where a snapshot crosses 512 KiB, and the profile shows virtual
  memory calls in the Windows heap (see "Above quadratic"). For A the rise is smooth and matches a
  growing per-lookup cost. Neither explanation was checked with hardware counters.
- Nesting depth is not a factor of its own: with the number of declarations and `if`s held fixed,
  depths 1 to 60 all take 0.16 to 0.23 s. The parser caps nesting at 63 `if` levels
  (`MAX_DEPTH = 128`).

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
python -I scripts/perf/checker_large_files.py --family decls_then_ifs --sizes 20000 21000 22000
CARGO_PROFILE_RELEASE_DEBUG=true CARGO_TARGET_DIR=target/prof cargo build --release -j 8 --bin qxide
python -I scripts/perf/sample_profile.py target/prof/release/qxide.exe check target/perf-checker/decls_then_ifs_20000.qx
```

## Input families

| Family | Shape | Path |
| --- | --- | --- |
| `chain_callers_first` | `f0` calls `f1`, …, `fN` prints, declared in that order (the M-05 repro) | A |
| `chain_callees_first` | the same call graph declared in reverse | control for A |
| `flat_functions` | N independent functions with a qubit, `h`, `measure`, `println` | baseline |
| `decls_then_ifs` | N `int` declarations, then N `if (true) {}` (the M-05 repro) | B, C |
| `ifs_only` | one variable and N `if`s | control for B |
| `decls_then_fors` | N declarations, then N `for i in 0..1 {}` | B, C |
| `decls_then_andand` | N declarations, then N `boolean c = true && true;` | B, C |
| `qubits_then_ifs` | N `Qubit` declarations, N `if`s, N `discard`s | B, C, D |
| `lookup_near` | N declarations, each reading the previous variable | C |
| `lookup_far` | N declarations, each reading the first variable | C, D |
| `nested_ifs` | 200 nests of depth d, one declaration per level | B (total size grows with d) |
| `depth_only` | 6,000 declarations and `if`s arranged as nests of depth d | depth alone |

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
| `decls_then_ifs` | 2,500 | 58,905 | 0.185 | |
| | 10,000 | 238,905 | 1.012 | 1.27 |
| | 20,000 | 488,905 | 4.067 | 2.01 |
| | 40,000 | 988,905 | 42.245 ¹ | 3.38 |
| `ifs_only` | 20,000 | 300,024 | 0.035 | 0.95 |
| | 80,000 | 1,200,024 | 0.093 | 0.89 |
| `decls_then_fors` | 10,000 | 298,905 | 0.663 | 1.64 |
| | 20,000 | 608,905 | 3.874 | 2.55 |
| | 40,000 | 1,228,905 | 33.714 ¹ | 3.12 |
| `decls_then_andand` | 10,000 | 427,795 | 0.762 | 1.88 |
| | 20,000 | 877,795 | 2.767 | 1.86 |
| | 40,000 | 1,777,795 | 33.003 ¹ | 3.58 |
| `qubits_then_ifs` | 10,000 | 477,795 | 0.930 | 2.47 |
| | 20,000 | 977,795 | 6.116 | 2.72 |
| | 40,000 | 1,977,795 | 47.728 ¹ | 2.96 |
| `lookup_near` | 20,000 | 397,788 | 0.237 | 1.42 |
| | 40,000 | 817,788 | 1.122 | 2.24 |
| | 80,000 | 1,657,788 | 9.942 | 3.15 |
| `lookup_far` | 20,000 | 328,902 | 0.306 | 1.64 |
| | 40,000 | 668,902 | 1.555 | 2.35 |
| | 80,000 | 1,348,902 | 13.932 | 3.16 |
| `nested_ifs` (d) | 8 | 35,215 | 0.023 | 1.37 |
| | 32 | 145,215 | 0.256 | 1.85 |
| | 60 | 274,015 | 0.889 | 1.98 |
| `depth_only` (d) | 1 | 132,015 | 0.192 | |
| | 8 | 132,015 | 0.172 | 0.14 |
| | 60 | 137,015 | 0.229 | 0.05 |

The full step lists (every doubling, 1,000 or 2,500 upward) come from the script, and the table
keeps the informative rows. `nested_ifs` is quadratic only because its total size grows as 200·d
and `vars` is never truncated when a scope closes; `depth_only` shows that depth alone costs
nothing.

### Above quadratic

Finer steps of `decls_then_ifs`:

| N | 14,000 | 16,000 | 18,000 | 20,000 | 21,000 | 22,000 | 23,000 | 24,000 | 28,000 | 32,000 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Seconds | 1.58 | 5.79 / 5.93 | 2.88 / 2.85 | 3.88 | 4.62 | 10.58 | 11.35 | 12.92 | 18.56 | 23.74 |

- Between 21,000 and 22,000 the time more than doubles for 5 % more input. A snapshot is a
  `Vec<State>` of 24 bytes per variable, which is 504,000 bytes at 21,000 and 528,000 bytes at
  22,000, so the step sits where a snapshot crosses 512 KiB. That is consistent with the Windows
  process heap (the Rust default allocator here) handing large blocks to `VirtualAlloc`, after
  which each snapshot maps fresh pages, faults them in on first write and unmaps them on free.
  The exact heap threshold was not looked up. The profile agrees: `NtFreeVirtualMemory` is 16 % of
  the samples at 24,000 against 6 % at 20,000, and the copy loop that touches the new pages stays
  near 60 %.
- 16,000 is reproducibly slower than 18,000 (two runs of three each). Its profile also shows 17 %
  in `NtFreeVirtualMemory`, so the heap decommits and recommits at that size too. This was not
  pursued further, since it is allocator policy rather than checker logic.
- The same effect shows at small sizes. Time per (`if` × variable) is about 30 ns at 2,500,
  10 ns at 10,000 and 20,000, 23 ns at 16,000 and 26 ns at 40,000. The low slopes of the small
  `decls_then_ifs` steps come from this varying cost more than from process start.
- `chain_callers_first` at 16,000, 20,000, 24,000, 28,000 and 32,000 functions takes 6.07, 11.79,
  20.09, 30.14 and 39.99 s, a smooth rise with no step. The sweep performs about 1.5·N² hash map
  lookups of `String` keys (SipHash), which works out to about 16 ns per lookup at 16,000 and
  26 ns at 32,000. A per-lookup cost that grows with the working set points to cache misses, but
  this was not measured with hardware counters.
- `lookup_near` at 40,000, 48,000, 56,000, 64,000, 72,000 and 80,000 declarations takes 1.08,
  1.63, 2.19, 2.89, 4.78 and 8.87 s: quadratic up to 64,000, then a sharp rise. This was not
  investigated.

## Profiles (before the fix)

Samples on the checking thread with the final version of the profiler (system-DLL leaves
included); "inclusive" counts a sample for every `check.rs` line on its stack. The binary was
built from `b8c385e` sources with full debug info.

| Input | Where the time goes |
| --- | --- |
| `chain_callers_first`, 16,000 | `conditional_safety`: `:286` (`contains_key(name)` per sweep) 66 %, `:291` (`find` over calls) 32 %; leaves are SipHash and hashbrown probing |
| `decls_then_ifs`, 20,000 | `states()` `:402` 64 % (calls at `:644`, `:646`, `:653` about 19 to 23 % each), `restore` `:407` 13 %, merge loop `:660` 8 %; 7.5 % of leaves in `ntdll` (`NtFreeVirtualMemory` 6.5 %) |
| `decls_then_fors`, 20,000 | `for` snapshot `:547` 35 %, `restore` `:569` 34 %, comparison loop `:556` 18 %, `:473` scan 5 % |
| `decls_then_andand`, 20,000 | `short_circuit_rhs` snapshot `:939` 34 %, comparison loop `:942` 23 %, `:473` scan 36 % |
| `qubits_then_ifs`, 20,000 | `states()` `:402` 51 %, `restore` 11 %, merge loop `:660` 15 %, `consume` lookups 6 % |
| `lookup_near`, 40,000 | `:473` shadowing scan 98.5 % (34 % of the leaves are `memcmp` in `vcruntime140`) |
| `lookup_far`, 40,000 | `:473` 60 %, `lookup` (`:398`, reached from `expr` `:809`) 39 % |

## Causes

**A. `conditional_safety`.** The loop at `:283-301` repeats a sweep over all functions until
nothing changes. A function is marked only when, at the moment it is visited, a callee is already
marked. In a chain declared callers first, each sweep marks one more function from the end of the
chain, so N functions take N sweeps of N entries, each costing a `String` hash lookup.
`chain_callees_first`, with the same graph in the other order, finishes in one sweep and is linear.

**B. Snapshots of linearity state.** `states()` returns the state of every entry of `self.vars`,
and `self.vars` holds every variable declared in the current function, including variables of
blocks that have already closed. Each `if` takes three snapshots (`before`, `s1`, `s2`), restores
twice and compares `before.len()` entries. `for`, `&&` and `||` take one snapshot each and compare
it. Only quantum variables ever change state (`consume`, `let_init`, `return` and shadowing all
check `is_quantum()` first), so almost all of the copying is of entries that cannot change.
`ifs_only`, with one variable, is linear.

**C. Shadowing check.** `stmt` for `Stmt::Decl` (`:473`) looks for a live quantum variable of the
same name in the current scope by scanning the whole scope and comparing names before checking
the type. A block with N declarations performs N²/2 string comparisons.

**D. `lookup`.** Name resolution walks every visible variable from the innermost scope outwards. It
is fast when the name is recent (`lookup_near`) and linear when it is old (`lookup_far`). The gap
between the two families, 0.43 s at 40,000 and 4.0 s at 80,000, is the cost of `lookup` itself.

Three more scans are linear per use and were not measured: the `passed` list when checking call
arguments (bounded by the number of parameters), the duplicate-parameter check in `function`, and
`return`, which scans every visible variable (`:592-598`). A function with many variables and many
`return`s would hit the last one, but every `return` inside a branch also pays for B.

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

## Proposals (not implemented)

1. **B, complete fix: an undo log for linearity state.** Route every write of `Var::state` through
   one setter that pushes `(id, old state)` onto a trail. A snapshot becomes a trail length;
   restoring pops back to it; the `if` merge, the `for` check and the `&&`/`||` check iterate only
   the ids written since the mark, in ascending id order so diagnostics keep their order. The cost
   becomes proportional to the state changes inside each construct. This touches the core of the
   linearity checker (`if_stmt`, the `for` arm, `short_circuit_rhs` and every state write), so it
   needs its own review. A differential test against the current snapshot code, run over the
   existing test programs and random mutations, would be the safety net.
2. **B, partial: snapshot only quantum variables.** Keep a per-function list of quantum variable
   ids and snapshot only those. This removes the cost for classical-heavy code such as the M-05
   `int` repro, but `qubits_then_ifs` (48 s at 40,000) would stay quadratic. On its own it does
   not close M-05.
3. **B, constant factor only: reuse snapshot buffers.** Keeping a stack of preallocated vectors
   would avoid the `VirtualAlloc` churn above 512 KiB and remove the step at 22,000, but the cost
   stays quadratic.
4. **C and D: a name index.** Keep `HashMap<String, Vec<usize>>` from name to the stack of visible
   ids, and pop entries when a scope closes. `lookup` reads the top, and the shadowing check
   compares the top's scope depth with the current one. The order-sensitive callers (`lookup`
   returns the innermost and latest declaration) keep their behavior if the stack is kept in
   declaration order.
5. **Input size limit.** Reject source files above a documented size before lexing, as M-05
   suggests. A limit is a policy change, so it is left to the maintainers. Even 1 MiB would not
   bound B or C on its own, since `decls_then_ifs` at 40,000 is under 1 MB.
6. **Regression guard.** Once B and C are fixed, a test can check `chain_callers_first`,
   `decls_then_ifs` and `qubits_then_ifs` at about 50,000 elements within a generous time bound,
   or count operations instead of measuring time to avoid flaky CI.

## Status of M-05 after this branch

The call-chain half of M-05 is fixed. The declaration-and-branch half (B) and the new
shadowing-scan path (C) remain. On the fixed binary, `decls_then_ifs` still takes 3.89 s at 20,000
and 38.2 s at 40,000 (988,905 bytes), and `lookup_near` takes 1.08 s at 40,000 and 8.87 s at
80,000. `qubits_then_ifs` (1.9 MB, 48 s before) goes through the same unchanged code.
