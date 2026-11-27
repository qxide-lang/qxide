# Baseline Security Review Findings

- **Date:** 2026-11-21
- **Reviewer:** Claude Opus 5.5 in Claude Code
- **Commits:** `main` (`b8c385e`) unless noted; `app-framework` branch (`eea71a6`, not merged)
- **Purpose:** First-pass security review recorded as a baseline for comparison with a later review (by Claude Mythos). Nothing was fixed.
- **Method:** Read the code of the surfaces in `docs/security/THREAT_MODEL.md`; built the release CLI (`cargo build --release -j 4`) and the playground wasm (`cargo build -p qxide-wasm --target wasm32-unknown-unknown --profile wasm -j 4`) on Windows 11; ran crafted inputs with time limits and sampled the process working set once per second; ran the new random smoke test and share-link fuzz script. Two read-only sub-reviews (`app-framework` branch; site, CI, and registry template) were done by sub-agents; their claims marked "read" were spot-checked where noted.
- **Status definitions:**
  - `REPRODUCED` = triggered on this machine with the input given.
  - `READ` = confirmed by reading the code, not executed.
  - `SUSPECTED` = plausible from the code, not confirmed.
- **Severity definitions:** `Critical` / `High` / `Medium` / `Low` / `Info` (reviewer's estimate; the threat model's actors decide the real priority).
- **Repro files:** The exact `.qx` inputs are reproduced inline below; scratch copies live under `target/sec-repro/` (ignored, not committed).
- **Paths:** Bare `validate.py`, `build_search.py`, and `scripts/...` paths refer to `registry-template/scripts/`; paths in the app-framework section are on the `app-framework` branch.

## Status on `main` (2026-11-26, `8d61b54`)

The findings below describe `b8c385e` (0.7.0). The fixes recorded in `EXECUTION_LIMITS_FIXES.md`, `PKG_CLIENT_FIXES.md`, `REGISTRY_TEMPLATE_FIXES.md`, and `APP_FRAMEWORK_FIXES.md` have since been merged and released in 0.8.0, and the `app-framework` branch is now part of `main`. Each finding was checked again against the code at `8d61b54` and against the fix records. B-05 and B-06 were also re-run with a debug build of that commit. The `Now` column in the summary gives the result:

- **Fixed:** the issue no longer occurs. The record named in the note has the change and its regression tests.
- **Partial:** part of the issue remains, as the note says.
- **Open:** unchanged.

| ID | Now | Note |
| --- | --- | --- |
| B-01 | Fixed | Step limit, `Q0413` (`EXECUTION_LIMITS_FIXES.md`). `qxide run --no-budget` turns it off on request. |
| B-02 | Fixed | Operation and output limits, `Q0414` and `Q0415` (`EXECUTION_LIMITS_FIXES.md`). |
| B-03 | Fixed | Output limit counts every shot, `Q0415` (`EXECUTION_LIMITS_FIXES.md`). |
| B-04 | Open | `site/src/playground/share.ts:50-54` still inflates the whole hash before the `MAX_CODE` check. |
| B-05 | Fixed | Fixed 2026-11-27: the binary's `print!` family drops output once the reader closes the pipe, and the command keeps its exit status (`tests/cli.rs::closed_stdout_pipe_is_not_a_panic`). |
| B-06 | Fixed | Fixed 2026-11-27: `9223372036854775808` is accepted right after a unary minus, as in Java (`lang::tests::smallest_int_literal_follows_a_unary_minus`). |
| B-07 | Open | `src/lang/link.rs` still has no device-name check. Windows 11 treats these names as ordinary missing files. |
| B-08 | Fixed | Estimated simulation work limit, `Q0417` (`EXECUTION_LIMITS_FIXES.md`). |
| B-09 | Fixed | Registers are shared slices (`Rc<[T]>`), so indexing no longer copies them (`EXECUTION_LIMITS_FIXES.md`). |
| B-P1 | Fixed | `validate.yml` runs on `pull_request_target` with the base branch's scripts (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P2 | Fixed | Deleted index files are rejected (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P3 | Fixed | Every version needs an archive in the repository that passes the checks (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P4 | Partial | Client output escapes control characters (`PKG_CLIENT_FIXES.md`), and CI rejects control characters in descriptions (`REGISTRY_TEMPLATE_FIXES.md`). One part is still open. |
| B-P5 | Fixed | Only an owner can yank by pull request, and no pull request can un-yank (`REGISTRY_TEMPLATE_FIXES.md`, `registry-template/scripts/validate.py`). |
| B-P6 | Fixed | Hostile input is reported as an error, with depth and size caps (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P7 | Fixed | Look-alike folding table, reserved names, and device names (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P8 | Fixed | Job summary entries are code spans with escaped control characters (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P9 | Fixed | Same as M-03: download locations must pass `index::valid_dl` (`PKG_CLIENT_FIXES.md`). |
| B-P10 | Fixed | Unpacked files are compared with a fresh unpack of the locked archive (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P11 | Fixed | Same as M-02 (`PKG_CLIENT_FIXES.md`). |
| B-P12 | Fixed | `curl`, `gh`, and `git` are looked up on `PATH` only (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-P13 | Fixed | Path dependencies and `path+` sources must be relative (`REGISTRY_TEMPLATE_FIXES.md`). |
| B-S1 | Open | There is still no `site/public/_headers`. |
| B-S2 | Open | `pypi.yml` still uploads with a token; `id-token: write` is already granted. |
| B-S3 | Open | Actions are still pinned to tags or branches (`ci.yml`, `pypi.yml`). |
| B-S4 | Open | `ci.yml` still has no `permissions:` block. |
| B-S5 | Open | `release.yml` (generated by cargo-dist) is unchanged. |
| B-S6 | Open | Unchanged. |
| B-S7 | Open | `site/vite.config.ts:7` still allows `..` (dev server only). |
| B-A1 | Fixed | `APP_FRAMEWORK_FIXES.md`. |
| B-A2 | Fixed | `APP_FRAMEWORK_FIXES.md`. |
| B-A3 | Fixed | Built apps refuse `--dev` (`APP_FRAMEWORK_FIXES.md`). |
| B-A4 | Open | The runtime still trusts `app.json` from the bundle (`crates/qxide-app-runtime/src/app.rs`); only `http_hosts` is checked again. |
| B-A5 | Partial | Empty loop bodies now cost a step, and `Machine::charge` polls for cancellation, so app calls and `std.qxide.run` can be stopped. One Studio command still cannot be cancelled. |
| B-A6 | Open | `src/app/bundle.rs` still expects the bundle trailer at the end of the file. |
| B-A7 | Fixed | Fixed worker pool, panics caught, and call ids that are still running cannot be reused (M-13 in `APP_FRAMEWORK_FIXES.md`). |
| B-A8 | Fixed | `String` text costs steps per byte before it is allocated (`STR_BYTE_STEPS`, part 2 of `EXECUTION_LIMITS_FIXES.md`). |
| B-A9 | Open | `percent_decode` still slices by bytes (`crates/qxide-app-runtime/src/main.rs:125`); whether a request path can hold non-ASCII bytes is still unchecked. |
| B-A10 | Open | Dev-mode `ui_file` still reads with `std::fs::read` (`crates/qxide-app-runtime/src/app.rs:215`). |
| B-A11 | Open | The `RESERVED` list in `src/lang/stdlib.rs` is still shorter than the one in `src/pkg/archive.rs`. |
| B-A12 | Open | Info, kept by design. The token is now capped at 64 characters, but the page still generates it. |
| B-A13 | Open | `src/app/mod.rs` still writes the folder name unescaped into the generated files. |

## Summary

| ID | Now | Severity | Status | Area | Title |
| --- | --- | --- | --- | --- | --- |
| B-01 | Fixed | Medium | REPRODUCED | Interpreter | Unbounded loop/recursion: no step or time limit in the interpreter |
| B-02 | Fixed | Medium | REPRODUCED | Interpreter / Circuit | Unbounded memory from gates or output produced in a loop |
| B-03 | Fixed | Medium | REPRODUCED | Multi-shot runtime | Multi-shot output amplification |
| B-04 | Open | Low | REPRODUCED | Site / Playground | Share link is fully inflated before the size check |
| B-05 | Fixed | Info | REPRODUCED | CLI | CLI panics when stderr/stdout is closed early |
| B-06 | Fixed | Info | REPRODUCED | Parser | Smallest i64 literal cannot be written |
| B-07 | Open | Info | SUSPECTED (Windows before 11) | Linker | Module names that are Windows device names |
| B-08 | Fixed | Medium-Low | REPRODUCED | Simulator | Stabilizer simulation time grows roughly cubically with qubit count inside the memory limit |
| B-09 | Fixed | Medium-Low | REPRODUCED timing; cause READ | Interpreter | Each register index copies the whole register |
| B-P1 | Fixed | Medium | READ | Package system / Registry CI | Registry PR can disable its own validation |
| B-P2 | Fixed | Medium | READ | Package system / Registry CI | Deleting a whole index file passes validation |
| B-P3 | Fixed | Medium-Low | READ | Package system / Registry CI | Externally hosted `dl` skips every archive check in CI |
| B-P4 | Partial | Low-Medium | READ | Package system / CLI | Package descriptions can carry terminal escape sequences |
| B-P5 | Fixed | Low | READ | Package system / Registry | Anyone can flip `yanked` in either direction for any package |
| B-P6 | Fixed | Low | READ | Package system / Registry CI | `validate.py` reads archive members without a per-member cap |
| B-P7 | Fixed | Low/Info | READ | Package system / Registry CI | Look-alike name check only strips hyphens |
| B-P8 | Fixed | Low/Info | READ | Package system / Registry CI | Markdown injection into the CI job summary through archive member names |
| B-P9 | Fixed | Low/Info | READ | Package system / Client | Index `dl` may point at a local file |
| B-P10 | Fixed | Info | READ | Package system / Client | Unpacked packages are trusted by a marker file |
| B-P11 | Fixed | Info | SUSPECTED | Package system / Client | tar GNU long-name / PAX header buffering |
| B-P12 | Fixed | Info | SUSPECTED | Package system / Client | Windows program search for `curl`, `gh`, `git` |
| B-P13 | Fixed | Info | READ | Package system / Client | Path dependencies and locked `path+` sources can point anywhere on disk |
| B-S1 | Open | Low | READ | Site | No security headers on `qxide.dev` |
| B-S2 | Open | Medium | READ | CI / Distribution | PyPI publishing uses a long-lived token |
| B-S3 | Open | Medium-Low | READ | CI / Distribution | Actions pinned to mutable tags |
| B-S4 | Open | Low | READ | CI | `ci.yml` has no `permissions:` block |
| B-S5 | Open | Info | READ | CI / Distribution | `release.yml` interpolates `github.ref_name` in `run:` lines and grants `contents: write` workflow-wide |
| B-S6 | Open | Info | READ | Distribution | Installer integrity cannot be checked from the repo |
| B-S7 | Open | Info | READ | Site (dev) | Vite dev server may serve the repo root |
| B-A1 | Fixed | Medium | READ | App framework (`std.fs`) | `std.fs` escape through a dangling symlink |
| B-A2 | Fixed | Medium | READ | App framework (`std.fs`) | The data root itself may be a symlink |
| B-A3 | Fixed | Low-Medium | READ | App framework (runtime) | Every built app exe accepts `--dev <json>` |
| B-A4 | Open | Low | READ | App framework (runtime) | Runtime trusts `app.json`; bundle has no integrity check |
| B-A5 | Partial | Low-Medium | READ | App framework (interpreter) | Empty loop bodies cannot be cancelled |
| B-A6 | Open | Info | READ | App framework (distribution) | Authenticode signing would break built apps |
| B-A7 | Fixed | Low | READ | App framework (IPC) | IPC worker robustness |
| B-A8 | Fixed | Low-Medium | READ | App framework (interpreter) | Unbounded `String` growth |
| B-A9 | Open | Low | SUSPECTED | App framework (runtime) | `percent_decode` slices `&str` by bytes |
| B-A10 | Open | Info | READ | App framework (dev) | Dev-mode UI serving follows symlinks, has no reserved-name filter and no size cap |
| B-A11 | Open | Low | SUSPECTED | App framework (`std.fs`) | Reserved-name list in stdlib `confine` is shorter than `archive.rs`'s |
| B-A12 | Open | Info | READ | App framework (IPC) | The page token in `bridge.js:11` is page-generated and compared only in JS |
| B-A13 | Open | Info | READ | App framework (CLI) | `qxide app new` puts the folder name unescaped into TOML and HTML templates |

## Reproduced

### B-01: Unbounded loop/recursion: no step or time limit in the interpreter

- **Severity:** Medium
- **Status:** REPRODUCED
- **Location:** `src/lang/interp.rs:218` (`for k in a..b` with any i64 bounds); recursion bounded only in depth (`MAX_CALL_DEPTH` 200), not in total calls.
- **Input:**

`loop.qx`:
```
void main() {
    for i in 0..9223372036854775807 {
    }
}
```

`rec.qx` (2^62 calls):
```
int f(int n) {
    if (n == 0) { return 1; }
    return f(n - 1) + f(n - 1);
}
void main() {
    println("{}", f(62));
}
```

- **Command:** `qxide run loop.qx` (`check` is fine; `qasm` and `run` hang).
- **Observed:** Killed by `timeout 10` (exit 124) for both `loop.qx` and `rec.qx`. In the playground wasm (Node harness), `loop.qx` also ran until the 60 s harness timeout without trapping; the site stops it with its 30 s worker kill.
- **Impact:** CLI and any embedder (LSP branch, REPL, app framework) can be hung by a shared `.qx` file; the playground relies only on the JS worker timeout. On the `app-framework` branch the cancel check runs only per statement, so an empty loop body cannot be cancelled (see B-A5).
- **Fix idea:** A step budget in `Interp` (count loop iterations, calls and gates) with a diagnostic, configurable per host via `Limits`.

### B-02: Unbounded memory from gates or output produced in a loop

- **Severity:** Medium
- **Status:** REPRODUCED
- **Location:** `src/lang/interp.rs:517` (`fn text`, output pieces appended without a cap) and every gate call appending to the `Circuit` (no op-count cap); the loop at `interp.rs:218`.
- **Input:**

`print.qx`:
```
void main() {
    for i in 0..9223372036854775807 {
        println("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    }
}
```

`gates.qx`:
```
void main() {
    Qubit q = qubit();
    for i in 0..9223372036854775807 {
        h(q);
    }
    discard(q);
}
```

- **Observed:** Working set (MB, one sample per second, process then killed):
  - `print.qx`: 785, 1560, 2091, 2643, 3150, 5759, 3447, 3987
  - `gates.qx`: 305, 609, 862, 1011, 1170, 1874
  Nothing is printed until elaboration ends, so the output is held in memory.
- **Observed (playground):** Measured in Node 24 running the playground wasm module (not a browser): `print.qx` trapped after 1.43 s with `RuntimeError: unreachable` (allocation failure aborts because the wasm profile uses `panic=abort`) at about 3,216 MiB of wasm memory, Node working set 2,675 MB; `gates.qx` trapped after 7.38 s at about 3,088 MiB (working set samples: 712, 1376, 2387, 2552, 2725, 2902, 3080 MB). There is no `-zmax-memory` link argument in `crates/qxide-wasm/build.rs`, so only the wasm32 4 GiB address space bounds it. In the site the worker reports the trap and the next run gets a fresh instance (read, not run in a browser).
- **Impact:** A `.qx` file can exhaust the memory of the machine running `qxide run`/`qasm` (no OS-level cap); in the playground a single run makes the worker allocate about 3 GiB within seconds before it traps.
- **Fix idea:** Cap total output bytes and circuit operations in `Limits` (tighter for the playground), and set a wasm max-memory.

### B-03: Multi-shot output amplification

- **Severity:** Medium
- **Status:** REPRODUCED
- **Location:** `src/lang/mod.rs:355-363` (`groups: HashMap<Vec<bool>, (u64, String)>`; each distinct measurement record stores its own fully rendered output).
- **Input:**

`amplify.qx` (about 100 KB): 17 qubits put in superposition, measured, then 100 `println` calls of a 1000-character literal, then the bits (abridged below; `<1000 x 'y'>` stands for a literal of 1,000 `y` characters):
```
void main() {
    QReg r = qreg(17);
    for i in 0..17 { h(r[i]); }
    BitReg b = measure(r);
    for i in 0..100 {
        println("<1000 x 'y'>");
    }
    println("{}", b);
}
```

- **Command:** `qxide run amplify.qx --shots 100000`
- **Observed:** Working set (MB per second): 3220, 6248, 8460, 8780, 8591, 8338, 8258, 8148, 8409, 8060 (then killed; the machine started paging). Up to 100,000 distinct records x 100 KB each = about 10 GB. The playground accepts the same shot count (`MAX_SHOTS` 100,000); in the Node wasm harness the run neither finished nor trapped, and the Node working set grew steadily to 949 MB at 30 s (samples: 271, 392, 465, ... 936, 949 MB), when it was killed (the site's worker limit is also 30 s). An earlier run without memory sampling did not finish within 120 s.
- **Impact:** A short program plus a large `--shots` value uses memory proportional to shots x output size (about 8.7 GB within 4 s in the CLI). Share links carry shots, so a victim who presses Run on a shared link has the playground worker (off the main thread) grow toward about 1 GB and burn CPU until the 30 s kill; a frozen tab was not observed or tested.
- **Fix idea:** Render each row lazily or cap the total rendered bytes; reject when shots x output size exceeds a budget.

### B-04: Share link is fully inflated before the size check

- **Severity:** Low
- **Status:** REPRODUCED
- **Location:** `site/src/playground/share.ts:42-54` (`pipe(...DecompressionStream('deflate-raw'))` buffers everything; `MAX_CODE` is checked afterwards). Runs on page load and on every `hashchange` (`site/src/pages/Playground.tsx:171-197`).
- **Input:** 140 MiB of `a` deflated (142,692 bytes) -> base64url hash of 190,262 characters, under `MAX_HASH` 200,000.
- **Command:** Node 24 importing `share.ts` directly (`target/sec-repro/share_bomb.mjs`).
- **Observed:** Rejected with `"shared program too large"` after 442 ms, but `ArrayBuffer` memory grew about 280 MiB first.
- **Impact:** Transient memory and CPU spike in the victim's tab (bounded by the 200,000-character hash, about 150 MB inflated). Not code execution; the oversize result is discarded.
- **Fix idea:** Read the decompression stream chunk by chunk and abort after 256 KiB.

### B-05: CLI panics when stderr/stdout is closed early

- **Severity:** Info
- **Status:** REPRODUCED
- **Location:** Diagnostics printed with `eprint!`/`print!` in `src/main.rs` (e.g. after `lang_command`), which panic on a broken pipe.
- **Command:** `qxide run chain.qx 2>&1 | head -c 400` where `chain.qx` produces a long diagnostic.
- **Observed:** Exit code 101 instead of 1. Without the pipe the exit code is 1.
- **Impact:** None for security; noisy exit status in scripts.

### B-06: Smallest i64 literal cannot be written

- **Severity:** Info (correctness, not security)
- **Status:** REPRODUCED
- **Input:**
```
int b = -9223372036854775808;
```
- **Observed:** Error `Q0104` `"integer literal is too large for \`int\`"` (the literal is parsed before negation).
- **Impact:** Correctness, not security. Listed only so later reviews do not re-report it as a parser crash.

### B-07: Module names that are Windows device names

- **Severity:** Info
- **Status:** SUSPECTED (Windows before 11)
- **Location:** `src/lang/link.rs:153` (`module_file`)
- **Input:**
```qx
import devp.con.f;
import devp.nul.f;
```
- **Observed:** `module_file` maps `import devp.con.f;` to `src/con.qx`; import segments are identifiers, so no `..` or separators can appear. On this Windows 11 machine, `import devp.con.f;` and `import devp.nul.f;` in a scratch project (`qxide new devp`) failed with `Q0502` ("failed to load module ... tried reading `src\con.qx`: (os error 2)"), i.e. ordinary missing files.
- **Impact:** Older Windows maps `con.qx` to the console device, where the read could block; not tested.
- **Fix idea:** Reject reserved Windows device names as module segments on Windows, or check that the resolved path is a regular file before reading.

### B-08: Stabilizer simulation time grows roughly cubically with qubit count inside the memory limit

- **Severity:** Medium-Low
- **Status:** REPRODUCED
- **Location:** The library's stabilizer backend (`src/stabilizer.rs`, not reviewed, not profiled); the check in `src/lang/mod.rs` bounds memory, not time.
- **Input:** (`ghz.qx`, `n` varied)
```qx
void main() {
    int n = 32768;
    QReg r = qreg(n);
    h(r[0]);
    for i in 1..n {
        cx(r[0], r[i]);
    }
    BitReg m = measure(r);
    println("{}", m[0]);
}
```
- **Command:** `qxide run ghz.qx`
- **Observed:** `qxide run` wall time: `n=8192` 2.0 s, `n=16384` 19.6 s, `n=32768` 97.9 s; `n=65536` is rejected (`Q0406`, 2.0 GiB tableau over the 1 GiB limit).
- **Impact:** The CLI limit admits about 46,000 qubits, so a program of about 200 bytes runs for minutes; the playground's 512 MiB limit admits about 32,768 qubits, so the worker is killed at 30 s.
- **Fix idea:** Include an `n^3` work estimate in the execution budget or lower the stabilizer qubit cap for the playground; profile `src/stabilizer.rs`.

### B-09: Each register index copies the whole register

- **Severity:** Medium-Low
- **Status:** REPRODUCED timing; cause READ
- **Location:** `src/lang/interp.rs:75-83` (`Frame::get` returns `v.clone()`) and the `ExprKind::Index` case of `expr`, which evaluates the base and so clones `Value::QReg(Vec<usize>)` on every `r[i]`; a loop over a register is therefore O(n^2) in time and memory traffic.
- **Input:** (`ghz.qx`, `n` varied up to the `1048576` qubit limit)
```qx
void main() {
    int n = 1048576;
    QReg r = qreg(n);
    h(r[0]);
    for i in 1..n {
        cx(r[0], r[i]);
    }
    BitReg m = measure(r);
    println("{}", m[0]);
}
```
- **Command:** `qxide qasm ghz.qx`
- **Observed:** `qxide qasm` (no simulation) of the same GHZ program: `n=8192` 0.11 s, `n=16384` 0.24 s, `n=32768` 0.46 s, `n=65536` 1.47 s, `n=131072` 86.0 s, `n=262144` more than 120 s (killed); `n=1048576` (the qubit limit) more than 120 s. The jump after 65536 fits the copied register (1 MiB per clone at `n=131072`) no longer fitting in CPU cache. `qxide check` of the `n=1048576` program takes 65 ms. Found by the smoke test.
- **Impact:** Finite, innocent-looking programs within every size limit run for minutes in `qasm`/`run`; the playground worker is killed at 30 s; a limit on loop iterations alone would not catch it (about 1M iterations).
- **Fix idea:** Share register storage (for example `Rc<[usize]>`) or index without cloning, and count work rather than iterations.

## Checked and held

Reproduced checks with no issue found:

- **Parser depth limit:** 100,000 nested parentheses, 100,000 unary minus, a 200,000-term `1+1+...` chain, 5,000 `else if` and 100,000 nested `if (true) {` all stop with `Q0106` at depth 128; no stack overflow.
- **Deepest legal recursion:** 199 nested calls, each returning a call wrapped in 120 parentheses, runs in both the CLI (64 MiB stack) and the wasm build (16 MiB shadow stack).
- **Size limits:** `qreg(1048576)` -> stabilizer tableau 512 GiB exceeds 1 GiB (`Q0406`); two of them -> `Q0409` (> 2^20 qubits); 30 state-vector qubits -> 16 GiB exceeds 1 GiB (`Q0406`); 90,000-qubit Clifford circuit -> 3.8 GiB tableau rejected.
- **Random smoke test (`tests/smoke_random.rs`, new):** The first version kept `.` inside identifier tokens (so `0..17` was one token and loop bounds were never mutated alone) and OR-ed the seed with 1 (so seeds 2 and 3 produced the same sequence); its runs (9,300 mutants, no failures) do not count. In the fixed version, `.` is its own token, half of the edits replace a number with a boundary value (`0, 1, 2, 3, 7, 19, 20, 21, 63, 64, 65, 1000, 1048576, 2147483648, 9223372036854775807`), every non-zero seed gives its own sequence, and it prints how many mutants pass `check`. Results with a 2 s limit per command: seed 1 (1,000 mutants): 307 passed `check`, 12 failing commands; seed 2 (1,000 mutants): 311 passed, 4 failing; seed 3 (1,000 mutants): 307 passed, 8 failing; default seed (300 mutants, 3 s limit): 92 passed, 2 failing. Every failure was a time-out (no panic or crash). Across the 24 saved inputs (12 mutants, each failing `qasm` and `run`), 9 mutants use `2147483648` or `9223372036854775807` as a loop bound, directly or through a helper such as `int twice(int n) { return n * 2147483648; }` called in the `else` branch of an `if` on a measured bit (both branches are elaborated) -- duplicates of B-01; 3 mutants are the GHZ example with `int n = 1048576;` (seed 1 case 253, seed 2 case 17, seed 3 case 799), which led to B-09. Replaying `loop.qx` with `QXIDE_SMOKE_REPLAY` also reports the hang.
- **Share-link fuzz script (`fuzz/js/share_decode.mjs`, new):** Had the same seed bug (fixed). Across 20,000 mutated hashes per seed, seed 1 is unchanged (5,248 decoded, 4,947 without a program, 9,805 rejected with an `Error`) and seed 2 gives 5,304 decoded, 5,048 without a program, 9,648 rejected; all invariants held.
- **Fuzz harness (`fuzz/`):** The replay test passes on stable; the seven `cargo-fuzz` binaries type-check on stable (`cargo check --manifest-path fuzz/Cargo.toml --bins -j 4`, which downloaded `libfuzzer-sys` into the cargo cache) but were never run (nightly and `cargo-fuzz` are not installed).
- **Fuzz harness replay (`fuzz/tests/replay.rs` on stable):** All targets accept the example programs and package manifests.

## Read and suspected: package system

### B-P1: Registry PR can disable its own validation

- **Severity:** Medium
- **Status:** READ (spot-checked)
- **Location:** `registry-template/.github/workflows/validate.yml:3-4, 21-32`; `scripts/validate.py:514-517`.
- **Observed:** On `pull_request`, the workflow file comes from the PR merge ref, so a PR can edit the step that runs the base branch's `validate.py` (or replace it with `true`). The rule "a PR that changes `index/` or `packages/` may not change other files" lives inside `validate.py` (`scripts/validate.py:514-517`), so it never runs.
- **Impact:** Not a secret-exfiltration issue (no secrets, `contents: read`); the impact is a misleading green check for reviewers.
- **Fix idea:** `CODEOWNERS` + required review for `.github/` and `scripts/`, or a trusted `pull_request_target` job that checks out the base and reads the PR only as git data.

### B-P2: Deleting a whole index file passes validation

- **Severity:** Medium
- **Status:** READ (spot-checked)
- **Location:** `registry-template/scripts/validate.py:488-503` (the `packages/` deny-list at `518-525` covers archives only).
- **Observed:** `validate.py:488-503` compares only index files that still exist at HEAD; a deleted `index/...` file is never compared with the base. Repro idea (local throwaway registry): `git rm index/3/f/foo`, run `python scripts/validate.py --base main` -> 0 errors.
- **Impact:** A follow-up PR can re-add the same name and version with a different `cksum` and an external `dl` (B-P3). Clients with an existing `qxide.lock` catch the `cksum` mismatch; fresh resolutions do not.

### B-P3: Externally hosted `dl` skips every archive check in CI

- **Severity:** Medium-Low
- **Status:** READ (spot-checked)
- **Location:** `registry-template/scripts/validate.py:471-472` (`if "://" in target: continue`); client verification in `src/pkg/cache.rs` `install`.
- **Observed:** No sha256, path, size or manifest check and no file list in the reviewer summary; any https host (IP, localhost, any port) is accepted. The client still verifies the `cksum` (`src/pkg/cache.rs` `install`) and unpacks with its own checks.

### B-P4: Package descriptions can carry terminal escape sequences

- **Severity:** Low-Medium
- **Status:** READ (spot-checked)
- **Location:** `validate.py`; `build_search.py` (`search.json`); `src/pkg/cli.rs:282` (`println!("{left:<width$} # {}", r.description)`).
- **Observed:** `validate.py` only checks that `description` is a string; `build_search.py` copies it into `search.json`; `qxide search` prints it unchanged.
- **Impact:** A description with `\u001b]8;;...` (OSC-8 links), screen clears or title changes runs in the terminal of anyone who searches.
- **Fix idea:** Strip control characters when printing registry text in the client, and reject them in `validate.py`.

### B-P5: Anyone can flip `yanked` in either direction for any package

- **Severity:** Low
- **Status:** READ
- **Location:** `validate.py:501` (`n = dict(entries[i], yanked=o.get("yanked"))`).
- **Observed:** There is no ownership model; review is the only control.

### B-P6: `validate.py` reads archive members without a per-member cap

- **Severity:** Low (fails closed)
- **Status:** READ
- **Location:** `validate.py:349-356`.
- **Observed:** Checks the unpacked total only after the loop.
- **Impact:** A compressible multi-GB member in a 10 MiB gzip, or deeply nested JSON (`RecursionError`), crashes the job rather than bypassing it.

### B-P7: Look-alike name check only strips hyphens

- **Severity:** Low/Info
- **Status:** READ
- **Location:** `validate.py:419-420` (`squash()`).
- **Observed:** No `l`/`1` or `rn`/`m` folding, no prefix squatting rule (e.g. `qxide-std`), and two new look-alikes in the same PR are not compared. Windows device names (`con`, `com1`) pass the name regex; with an external `dl` they create `index/3/c/con`, which breaks Windows checkouts.

### B-P8: Markdown injection into the CI job summary through archive member names

- **Severity:** Low/Info
- **Status:** READ
- **Location:** `validate.py:529-531`.
- **Observed:** Writes member names inside backticks; backticks and brackets are allowed in names.

### B-P9: Index `dl` may point at a local file

- **Severity:** Low/Info
- **Status:** READ
- **Location:** `src/pkg/index.rs:331-344`.
- **Observed:** An entry's `dl` can be `file://...` or a relative path, which the client reads from disk.
- **Impact:** The archive must still match the `cksum` from the same index line, so this matters only for a malicious index.

### B-P10: Unpacked packages are trusted by a marker file

- **Severity:** Info
- **Status:** READ
- **Location:** `src/pkg/cache.rs` (`installed()`).
- **Observed:** Checks only `.qxide-checksum`, not the files.
- **Impact:** A local attacker who can write `~/.qxide` can change package sources. Outside the threat model unless `~/.qxide` is shared.

### B-P11: tar GNU long-name / PAX header buffering

- **Severity:** Info
- **Status:** SUSPECTED
- **Location:** `archive.rs` (`unpack`).
- **Observed:** The `tar` crate reads long-name and PAX extension data before our per-entry checks in `archive.rs` `unpack`; a 10 MiB gzip could expand such a header a lot. Not tested.
- **Impact:** Requires a package whose sha256 is in the index (i.e. a published malicious package).

### B-P12: Windows program search for `curl`, `gh`, `git`

- **Severity:** Info
- **Status:** SUSPECTED
- **Observed:** `Command::new("curl")` on Windows searches the directory of `qxide.exe` before `PATH` (Rust std behavior).
- **Impact:** A writable install directory would allow a planted `curl.exe`. Depends on how the user installed `qxide`.

### B-P13: Path dependencies and locked `path+` sources can point anywhere on disk

- **Severity:** Info
- **Status:** READ
- **Location:** `src/pkg/resolve.rs:51` (`join_rel`), `src/pkg/project.rs:56` (joining a lock file's `path+...` source onto the project directory); also checked `src/pkg/lock.rs:142`
- **Observed:** `join_rel` keeps absolute paths and leading `..`, and `src/pkg/project.rs:56` joins a lock file's `path+...` source onto the project directory. Registry packages cannot declare path dependencies (their dependencies come from the index line only, `Choice::Registry` in `src/pkg/resolve.rs`). Lock names are validated so `../x` never becomes a cache path (`src/pkg/lock.rs:142`). `qxide install` downloads from the registry given by `--registry`, `$QXIDE_REGISTRY` or the default, not from the URL recorded in the lock.
- **Impact:** A cloned project can make `qxide run`/`check` read `qxide.toml` and `.qx` files from any directory the user can read; content only reaches the same user's terminal.
- **Fix idea:** Document as expected local path dependency behavior, or restrict or warn on `path+` targets outside the project root when running untrusted projects.

## Read and suspected: site, CI and distribution

### B-S1: No security headers on `qxide.dev`

- **Severity:** Low
- **Status:** READ
- **Location:** No `site/public/_headers`.
- **Observed:** No CSP, `frame-ancestors`/`X-Frame-Options`, `X-Content-Type-Options` or `Referrer-Policy`.
- **Impact:** No XSS sink exists today, so this is defence in depth (a CSP needs `'wasm-unsafe-eval'` and `worker-src 'self'`).

### B-S2: PyPI publishing uses a long-lived token

- **Severity:** Medium
- **Status:** READ
- **Location:** `.github/workflows/pypi.yml:129-131` (`password: ${{ secrets.PYPI_API_TOKEN }}`), although `id-token: write` (line 121) is already granted.
- **Observed:** Whether the `pypi` environment has protection rules is not visible from the repo.
- **Fix idea:** Trusted Publishing would remove the token.

### B-S3: Actions pinned to mutable tags

- **Severity:** Medium-Low
- **Status:** READ
- **Location:** `pypi.yml:82, 102` (`PyO3/maturin-action@v1`); `ci.yml` (`dtolnay/rust-toolchain@stable`).
- **Observed:** Includes the third-party `PyO3/maturin-action@v1`, whose wheels are uploaded to PyPI, and `dtolnay/rust-toolchain@stable` in `ci.yml`.

### B-S4: `ci.yml` has no `permissions:` block

- **Severity:** Low
- **Status:** READ
- **Location:** `ci.yml`.
- **Observed:** Uses the repo default token scope.

### B-S5: `release.yml` interpolates `github.ref_name` in `run:` lines and grants `contents: write` workflow-wide

- **Severity:** Info
- **Status:** READ
- **Location:** `release.yml:80, :279` (cargo-dist generated).
- **Observed:** Interpolates `github.ref_name` in `run:` lines and grants `contents: write` workflow-wide.
- **Impact:** Tags need push access; fork PRs get a read-only token.

### B-S6: Installer integrity cannot be checked from the repo

- **Severity:** Info
- **Status:** READ
- **Observed:** The generated `qxide-installer.sh`/`.ps1` are not committed; any checksum they embed comes from the same GitHub release (integrity, not authenticity); `/install.sh` and `/install.ps1` redirect to `releases/latest`, so trust = Cloudflare Pages account + GitHub account.

### B-S7: Vite dev server may serve the repo root

- **Severity:** Info
- **Status:** READ
- **Location:** `site/vite.config.ts:7` (`server.fs.allow: ['..']`).
- **Impact:** Matters only with `--host`.

## Read and suspected: app framework (branch app-framework, eea71a6)

All findings in this section are on the unmerged `app-framework` branch (`eea71a6`).

### B-A1: `std.fs` escape through a dangling symlink

- **Severity:** Medium
- **Status:** READ (confirmed in code by reviewer)
- **Location:** `src/lang/stdlib.rs` `confine` (around `:479-492`); `write_text`/`append_text` (`:324-341`).
- **Observed:** The deepest existing ancestor is canonicalized and checked, but a symlink whose target does not exist fails `canonicalize`, so the loop pops to the parent (inside the root) and passes; `write_text`/`append_text` (`:324-341`) then follow the link and create the target outside the root. Repro idea (needs symlink rights, so Unix or Windows developer mode): `data/notes.txt -> ~/.zshenv` (absent), `fs.write_text("notes.txt", "...")` under `qxide run --allow fs`.

### B-A2: The data root itself may be a symlink

- **Severity:** Medium
- **Status:** READ (sub-agent)
- **Location:** `src/main.rs:291-293` on the branch (`<project>/data`); `src/app/mod.rs:307` (`qxide app dev`'s `target/app-data`).
- **Observed:** If `data` links to `..` or `$HOME`, every "inside" path is outside. Same for `qxide app dev`'s `target/app-data`.

### B-A3: Every built app exe accepts `--dev <json>`

- **Severity:** Low-Medium
- **Status:** READ (sub-agent)
- **Location:** `crates/qxide-app-runtime/src/main.rs:378`.
- **Observed:** Runs `App::from_dev` whether or not a bundle is attached.
- **Impact:** A shipped (later signed) app can be pointed at any `.qx`/UI with any permissions, any data root (e.g. `C:\`) and devtools.
- **Fix idea:** Honor `--dev` only when no bundle is attached.

### B-A4: Runtime trusts `app.json`; bundle has no integrity check

- **Severity:** Low
- **Status:** READ (sub-agent)
- **Location:** `crates/qxide-app-runtime/src/app.rs:85-146`; `src/lang/link.rs:344/351`.
- **Observed:** `identifier` is joined raw (`..\..\X` escapes `%APPDATA%`); `data`, `dev`, `permissions`, and `deps` indices (out-of-range -> panic at `src/lang/link.rs:344/351`) are not re-validated.

### B-A5: Empty loop bodies cannot be cancelled

- **Severity:** Low-Medium
- **Status:** READ (confirmed in code by reviewer)
- **Location:** `src/lang/interp.rs` on the branch.
- **Observed:** Polls `host.cancelled()` only inside `stmt()` (every `CANCEL_POLL` statements); `for i in 0..9000000000000000000 {}` never polls. Studio's `std.qxide.qasm` uses `NoEffects` whose `cancelled()` is always false.
- **Impact:** Studio's Stop does nothing and the call holds a worker slot.

### B-A6: Authenticode signing would break built apps

- **Severity:** Info
- **Status:** READ (sub-agent)
- **Location:** `src/app/bundle.rs:46`; `docs/design/qx-app.md:381`.
- **Observed:** `src/app/bundle.rs:46` requires the bundle magic in the last 8 bytes; signing appends the certificate table; `docs/design/qx-app.md:381` says to sign after build. The macOS Resources layout promised in the doc is not implemented.

### B-A7: IPC worker robustness

- **Severity:** Low
- **Status:** READ (sub-agent)
- **Location:** `crates/qxide-app-runtime/src/main.rs:302-306`, `:317-319`.
- **Observed:** Acquire/release without a guard (a panic leaks the slot and the Promise never settles); `:317-319` ignores spawn failures; threads with 64 MiB stacks are spawned before acquiring a slot (unbounded queued threads); the `"commands"` request bypasses the slots; duplicate `(page,id)` overwrites the cancel flag.

### B-A8: Unbounded `String` growth

- **Severity:** Low-Medium
- **Status:** READ (confirmed in code by reviewer)
- **Location:** `src/lang/interp.rs:691` on the branch (`Value::Str(a + &concat_text(b, at)?)`).
- **Repro idea (not run):**
```
void main(){ String s="xxxxxxxx"; for i in 0..60 { s = s + s; } println("{}", len(s)); }
```
- **Observed:** Has no length cap; doubles to GBs before any cancel poll and aborts on allocation failure.
- **Impact:** Reachable from the playground once merged.

### B-A9: `percent_decode` slices `&str` by bytes

- **Severity:** Low
- **Status:** SUSPECTED
- **Location:** `crates/qxide-app-runtime/src/main.rs:121` (`&s[i + 1..i + 3]`).
- **Observed:** Panics if `%` is followed by a multi-byte character; likely unreachable because the webview percent-encodes.

### B-A10: Dev-mode UI serving follows symlinks, has no reserved-name filter and no size cap

- **Severity:** Info (dev only)
- **Status:** READ
- **Location:** `crates/qxide-app-runtime/src/app.rs:195`.

### B-A11: Reserved-name list in stdlib `confine` is shorter than `archive.rs`'s

- **Severity:** Low
- **Status:** SUSPECTED (Windows-version dependent)
- **Observed:** No `CONIN$`, `CONOUT$`, `COM0`/`LPT0`, or superscript digits; `"CON .txt"` is not caught (stem taken before the first dot without trimming).

### B-A12: The page token in `bridge.js:11` is page-generated and compared only in JS

- **Severity:** Info
- **Status:** READ
- **Location:** `bridge.js:11`; `main.rs:217`.
- **Observed:** The origin check (`main.rs:217`) is the only gate; matches its documented purpose (ignore stale replies).

### B-A13: `qxide app new` puts the folder name unescaped into TOML and HTML templates

- **Severity:** Info
- **Status:** READ
- **Location:** `src/app/mod.rs:121, :128`.

## Defended (read)

### Site, CI and distribution

- React text rendering for all output, diagnostics and QASM.
- External links use `rel=noreferrer`.
- Share links are not auto-run, use a fixed file name, and accept digits-only `shots`/`seed`.
- Limits enforced in Rust.
- Fresh instance per run.
- Stale worker messages dropped by id.
- `_middleware.ts` is not an open redirect.
- No secrets in history.

### App framework (`app-framework`, `eea71a6`)

- Exact-origin IPC check (rejects `qxapp.localhost.evil.com` and other ports).
- Navigation handler uses the same predicate.
- New windows denied.
- Devtools off unless dev.
- CSP + `nosniff` + `no-store` on every response including 404.
- Strict per-kind IPC key allow-list.
- Commands must be listed in `[app]` `commands` and `public`.
- Shots `1..=1,000,000`.
- All strings sent back through `evaluate_script` escaped by `json_str` (quotes, backslash, control chars, U+2028/2029).
- Bundle parsing uses checked offsets, no decompression, no extraction.
- Build walk skips symlinks.
- `std` permission check per import before caching.
- Dependencies limited to `std.log`.
- `std` reserved as a package name.

## Coverage of this baseline

- **Read in full by the reviewer:** `src/lang/mod.rs`, `src/lang/interp.rs`, `src/pkg/archive.rs`, `src/pkg/cache.rs`, `src/pkg/mod.rs`, `crates/qxide-wasm/src/lib.rs`, `site/src/playground/share.ts`.
- **Read in part:** `src/main.rs` (thread setup, `run`/`check`/`qasm` dispatch, start of `fmt`), `src/lang/parser.rs` (depth limits, blocks, `if`/`for`), `src/pkg/index.rs` (entry parsing, registry open, downloads, `curl`), `src/pkg/manifest.rs` (start of parse), `src/pkg/lock.rs` (parse), `src/pkg/resolve.rs` (path joins, dependency queue), `src/pkg/project.rs` (package dirs, install), `src/lang/link.rs` (module file mapping), `registry-template/scripts/validate.py` and `.github/workflows/validate.yml` (spot checks), and on `app-framework` `src/lang/stdlib.rs` (confine, write path), `src/lang/interp.rs` (loop, cancel, string concat) and the line references in B-A1..B-A4, B-A6, B-A7, B-A9.
- **Covered only by the two read-only sub-agent reviews:** the rest of `site/`, `.github/workflows`, `dist-workspace.toml`, `pyproject.toml`, `registry-template/`, and the `app-framework` branch.
- **Not opened:** `src/lang/check.rs`, `src/lang/lexer.rs`, `src/lang/fmt.rs` (exercised only by the smoke test and fuzz replay), `src/lang/repl.rs`, `src/lang/explain.rs`, `src/lang/diag.rs`, the simulators (`src/lib.rs`, `src/statevector.rs`, `src/stabilizer.rs`, `src/gpu.rs`, `src/groups.rs`, `src/uncompute.rs`, `src/rng.rs`), `qxide new`, the in-place write of `qxide fmt`, most of `src/pkg/cli.rs` (publish via `gh`/`git`, `add`/`remove`), and the `lsp` and `random-default-seed` branches. No browser was used: B-03 and B-04 were measured in Node only.

## Suggested priorities

1. **B-01 / B-02 / B-03 / B-08 / B-09 together:** A work-based execution budget (counting work rather than only loop iterations, plus gates, output bytes, shots x output, and stabilizer `n^3` work) shared by CLI, playground, LSP, REPL and apps, plus a wall-clock limit for the CLI, before the app framework ships.
2. **B-A1 / B-A2 / B-A3:** Before merging `app-framework`.
3. **Registry:** B-P1, B-P2, B-P3, B-P4, and syncing the live registry scripts with the template.
4. **Supply chain:** B-S2, B-S3.
