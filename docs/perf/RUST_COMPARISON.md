# Qxide versus Rust on the same algorithms

## Purpose

Long-term goal (user, 2026-11-24): Qxide reaches 95 % of Rust's speed on the same algorithm. This suite establishes the baseline against which every later stage (memory model, AOT, optimisation) is measured.

The headline metric is **Rust %** = `(Rust kernel time / Qxide kernel time) × 100`, computed from medians. A score of 100 % means as fast as Rust; the target is at least 95 %.

## Cases

| Case | Algorithm | What it stresses |
| --- | --- | --- |
| `nbody` | 5-body simulation, 1,000,000 steps of 0.01, energy at the end | Structs in an array, field reads and writes, `math.sqrt` |
| `binarytrees` | Complete binary trees (depth 4 to 14, step 2, plus one long-lived tree), node count | Allocation and recursion (Qxide recursive `enum`, Rust `Box`) |
| `matmul` | 400 × 400 dense multiply on flat `double[]` / `Vec<f64>`, i-k-j order | Indexed array loads and stores |
| `strings` | 200,000 pseudo-random words joined, split and counted in a map; sum of `count × length + 1` per distinct word | String building, `split`, `Map` versus `HashMap` |

## How to run

```sh
cargo build --release --features jit
cargo build --release --manifest-path bench/compare/rust/Cargo.toml
python scripts/perf/rust_compare.py --reps 5 --ir-reps 3 [--values]
```

- The benchmark sources are `bench/compare/qx/*.qx` and `bench/compare/rust/src/bin/*.rs`. Each program times only its kernel (`std.time.monotonicMillis` at 1 ms resolution in Qxide; `Instant` in Rust) and prints `result=... ms=...`. Timing and printing stay in `main`, which the JIT leaves to the interpreter, so the kernel functions are the ones compiled (the `ir / jit` column confirms they are, at about 1.4–3.7x).
- `scripts/perf/rust_compare.py` interleaves Rust, Qxide `jit` and Qxide `ir` runs and checks that all results agree.
- `tests/cli.rs` (`rust_comparison_programs_match_the_rust_results`) checks the Qxide programs on `ast`, `ir` and `jit` against the Rust results at small sizes.

## Baseline (2026-11-24)

The table below reports median kernel times and Rust % for each case on Windows.

**Warning: these numbers were measured on a shared Windows PC while other heavy jobs (another session's experiments) were running, so the spread is large (up to about ±24 %). They are a provisional baseline and must be re-measured on a quiet machine before being used as the reference.**

- qxide `87b7a77` (`cargo build --release --features jit`), `--engine jit|ir --trusted`; rustc 1.99.1 (58cf8f9c3 2026-10-13), `cargo build --release` (opt-level 3)
- Windows 11 AMD64; 5 interleaved rounds (ir: 3); times are the kernel only, ms: median (min-max, ± half the range as % of the median)

| Benchmark | Input | Rust ms | Qxide jit ms | Qxide ir ms | Rust % (jit) | jit / Rust | ir / jit | Same result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `nbody` | 1000000 | 30.2 (22.7-35.9, ±22%) | 11216.0 (10713.0-12763.0, ±9%) | 28175.0 (28144.0-30688.0, ±5%) | 0.27 % | 371.4x | 2.51 | yes |
| `binarytrees` | 14 | 212.1 (195.6-257.3, ±15%) | 1438.0 (1383.0-1862.0, ±17%) | 2520.0 (2491.0-2777.0, ±6%) | 14.75 % | 6.8x | 1.75 | yes |
| `matmul` | 400 | 12.1 (11.1-16.9, ±24%) | 8502.0 (7717.0-8998.0, ±8%) | 31378.0 (30140.0-31589.0, ±2%) | 0.14 % | 702.6x | 3.69 | yes |
| `strings` | 200000 | 28.3 (26.1-35.1, ±16%) | 568.0 (460.0-656.0, ±17%) | 772.0 (623.0-936.0, ±20%) | 4.98 % | 20.1x | 1.36 | yes |

## What differs between the two sides

No tuning was done to remove these differences; they are part of what later stages are meant to address.

- Qxide checks every `int` operation for overflow, checks every array index, and charges the step budget in compiled code (`--trusted` raises the budget to `2^36` steps); Rust release builds do not check overflow, and Rust bounds checks may be removed by the optimiser.
- Qxide arrays, structs and collections are shared references (`Rc`, with arrays backed by `Rc<RefCell<...>>`); native code retains and releases on loads and stores and reads elements through helper calls. Rust uses plain values (`Vec<Body>` of `Copy` structs, `Box` trees).
- The Qxide JIT uses Cranelift; Rust uses LLVM, which can vectorise loops such as the `matmul` inner loop, though this was not checked here as we did not inspect the generated Rust machine code.
- Qxide `Map` preserves insertion order; Rust uses `std::collections::HashMap` (the checksum does not depend on order).
- Qxide's measured time includes JIT compilation of the kernel functions on their first call; Rust is compiled ahead of time.

## Reading

- On Windows (the only platform measured), the JIT reaches 0.14 % (`matmul`, 702.6x slower) to 14.75 % (`binarytrees`, 6.8x slower) of Rust.
- The gap is largest in the kernels dominated by array and struct field access (`matmul`, `nbody`), where Qxide goes through helper calls and reference counting. The baseline measures the gap only, not how much each cause contributes; the hypothesis (not yet measured) is that this is a main cost, and it is what the memory-model proposal (`docs/design/memory-model.md`, H1–H2) and the open 3c item "inline array access" aim at.
- `binarytrees` and `strings` are allocation-bound on both sides, so the gap is smaller.
- Reaching 95 % is expected to need changes such as the memory model, removing the step budget from compiled code, and better code generation (possibly an LLVM backend; that Cranelift generates slower code than LLVM is the user's premise, not measured here); which of these matter most has not been measured.

## Measured changes (2026-11-24 and 2026-11-25, branch `worktree-mm`)

**Measured on the same shared Windows PC while other jobs were running.** Spreads reach about ±46 % (`binarytrees`), so differences under about 20 % are not meaningful. Re-measure on a quiet machine before relying on them. Each row is one run of `python scripts/perf/rust_compare.py --reps 5 --values` (5 interleaved rounds, medians); "values" runs `bench/compare/qx-values` with `--experimental value-semantics`.

Rust % on the `jit` engine (Rust time / Qxide time × 100):

| Commit | What changed | `nbody` | `binarytrees` | `matmul` | `strings` |
| --- | --- | --- | --- | --- | --- |
| `87b7a77` | baseline (table above) | 0.27 % | 14.75 % | 0.14 % | 4.98 % |
| `cb2c00b` | value semantics behind a flag (M0, M1, M3, M4); flag off | 0.26 % | 15.04 % | 0.15 % | 5.79 % |
| `cb2c00b` | same, flag on | 0.20 % | 14.44 % | 0.16 % | 5.56 % |
| `69ad055` | JIT: scalar stores without the interpreter, borrowed loads; flag off | 0.39 % | 13.44 % | 0.30 % | 5.72 % |
| `69ad055` | same, flag on | 0.26 % | 13.83 % | 0.30 % | 5.56 % |
| `753819a` | JIT: numbers in arrays and fields loaded and stored natively, addresses kept per slot, inline reference counts, native `sqrt`; IR: charges merged across instructions that cannot fail; flag off | 3.95 % | 15.92 % | 3.20 % | 5.75 % |
| `753819a` | same, flag on | 2.03 % | 15.92 % | 3.20 % | 5.65 % |
| `2fb866e` | JIT: step count kept in a register; flag off | 5.80 % | 16.30 % | 6.16 % | 5.53 % |
| `2fb866e` | same, flag on | 2.74 % | 16.33 % | 6.29 % | 5.59 % |

Kernel times behind the `69ad055` rows (ms, median of 5): Rust `nbody` 28.2, `binarytrees` 210.0, `matmul` 11.9, `strings` 21.7; Qxide jit 7316 / 1563 / 3979 / 380 with the flag off and 11007 / 1519 / 3912 / 391 with it on. Against `cb2c00b` (12539 / 1928 / 8488 / 416 ms) the JIT change made `matmul` 2.1x and `nbody` 1.7x faster; `binarytrees` and `strings` moved within the noise.

Kernel times behind the 2026-11-25 rows (ms, median of 5, `--ir-reps 1`): at `753819a` Rust 28.4 / 236.7 / 10.3 / 23.5 and Qxide jit 718 / 1487 / 323 / 408 (flag on 1400 / 1487 / 323 / 415); at `2fb866e` Rust 39.6 / 260.6 / 11.7 / 22.8 and Qxide jit 684 / 1599 / 190 / 412 (flag on 1445 / 1596 / 186 / 407). The machine was busier during the `2fb866e` run: the Rust `nbody` time rose from 28.4 to 39.6 ms with a spread of ±50 %, so its Rust % moved more than the Qxide time did. Rust % compares times taken in the same interleaved rounds, which cancels part of the load. Against `69ad055` (7316 / 1563 / 3979 / 380 ms) `matmul` is about 21x and `nbody` about 11x faster on the jit engine; `binarytrees` and `strings` did not change.

What these rows say:

- Value semantics as implemented now does not make anything faster, and it was not expected to: it runs on the same shared-reference representation and adds copies and write-back cells (`docs/design/memory-model.md` §12). The flag-on `nbody` is slower (0.26 % against 0.39 %). A likely reason, not measured: the value-semantics version reads `bs[i].x` through the array on every access instead of through a local `Body` reference, so it makes more helper calls.
- The JIT change is independent of the memory model and is the larger effect so far.
- From `753819a` on, the flag-on `nbody` is about half as fast as the flag-off one (2.74 % against 5.80 %). The value-semantics version reads `bs[i].x`. The base of that field access is an array element, not a local variable, so it gets none of the per-slot addresses and still makes a helper call per access. `matmul`, `binarytrees` and `strings` are the same with the flag on or off.

## Where the time goes (2026-11-25, `2fb866e`)

This section replaces the conclusions of the 2026-11-24 profile below: element and field access no longer calls a helper on the paths the kernels use, so most of the time is now in the native code itself. The profiler cannot split native code by instruction, so the remaining costs were measured with throwaway builds (not committed) that leave one kind of check out, on top of `2fb866e`. Three runs each, Windows only, measured while other jobs were running; the full build ran slower here than in the table above, so compare within a row only.

| Build | `matmul` n = 400 ms | `nbody` 1M ms |
| --- | --- | --- |
| as committed | 189-223 | 596-629 |
| no step charges | 123-139 | 468-530 |
| no step charges, no `int` overflow checks | 109-112 | 432-476 |
| also no bounds checks | 107-112 | 436-476 |

- **Step charges**: removing them would save about a third of `matmul` and about a sixth of `nbody`. Before the count was kept in a register (`2fb866e`) the same experiment saved about two thirds of `matmul` (300 to 107 ms), which is why that change came first. This is the ceiling for removing the budget from compiled code (`docs/design/step-budget.md`). Nothing measured here says the budget must go: what is left is one add, one compare and one branch per charge.
- **`int` overflow checks**: about 15 % of `matmul` (index arithmetic `i * n + j`) and a few percent of `nbody`. Taking them out would change the language (overflow is an error), so this only marks what bounds-check style elimination of provably safe index arithmetic could give.
- **Bounds checks**: no measurable cost any more.
- **What is left in `nbody`** (about 440 ms against 30-40 ms for Rust): each `Body b = bs[j]` takes a new reference out of the array through `qx_elem_ref`, refreshes the field addresses of the slot through `qx_fields`, and gives up the previous value's count; the interpreter's `charge` per expression node also stays. Closing this needs structs stored inline in the array, which is the M5 layout of the memory-model proposal.
- **What is left in `matmul`** (about 110 ms against 10-12 ms for Rust): Cranelift does not vectorise loops, which `rustc` can do for this one, and every access still checks its bounds and computes its index with overflow checks. Checking once per loop instead would need loop analysis the JIT does not have.

## Earlier profile (2026-11-24, `69ad055`)

Kept for the record; its conclusions no longer hold after the changes above.

`scripts/perf/sample_profile.py` on a release build with debug symbols (`69ad055`), 1 ms sampling, `--engine jit --trusted`, Windows only. `matmul` at n = 300 and `nbody` at 200,000 steps.

| | `matmul` | `nbody` |
| --- | --- | --- |
| Worker samples in helper functions (`qx_heap` and what it calls) | 440 | 300 |
| Worker samples whose innermost frame is native JIT code | 44 | 31 |
| Share in helpers | about 91 % | about 91 % |

Samples of other threads (the main thread waits for the worker) are left out. A sample counts only if the profiler could tell where it was, so these shares are approximate.

Inside the helpers, `matmul`: `machine::binary` (the `+=` of `c[i * n + j] += ...`) 37 %, `store_fast` 24 %, `to_word` (turning a read element into a native word) 12 %, `ArrayData::set`/`get` 10 %, `qx_heap` dispatch 4 %, `checked_index` (bounds check) 2 %. `nbody` adds allocator time (`RtlSizeHeap`, `RtlFreeHeap`, about 8 %) from reference counts of `Body` values read out of the array.

What this answers:

- **Native code against interpreter code**: the kernels (`multiply`, `advance`, `energy`) are compiled (they have loops, so they compile on their first call); `main` stays in the IR interpreter but only times the kernel. Every array element and struct field access still calls a helper written in Rust, and those calls are about nine tenths of the kernel time. The native code itself, arithmetic and step checks included, is about one tenth.
- **Step charging**: the inline check (three loads, an add, two compares, a branch and a store per charge site) belongs to the native tenth, so it costs at most about 9 % here. The slow path (`qx_charge`) did not appear in the samples. Removing the budget from compiled code therefore cannot close the gap by itself; it becomes worth doing once element access is inline.
- **Bounds checks**: about 2 % of the helper time.
- **Boxing**: numbers are passed as raw words, so there is no boxing in native code. Inside the helpers, each read builds a `Value` and converts it (`to_word`, `from_word`, drops), about 15 %. Struct elements (`nbody`) also touch reference counts and the allocator.

At the time, the conclusion was that reading and writing elements and fields without a call needs a new memory layout (M5). It turned out not to: an array never changes length and a field never changes type, so native code can keep the addresses for as long as a local variable holds the value (`SlotCache` in `src/lang/jit/mod.rs`), with no change to the representation. M5 is still what would remove the remaining per-element reference counts and let struct arrays be stored inline.
