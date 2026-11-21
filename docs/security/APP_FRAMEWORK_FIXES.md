# App framework fixes for the security review

- **Branch:** `app-framework-next` (from `app-framework` at `eea71a6`)
- **Date:** 2026-11-21
- **Findings addressed:** B-A1, B-A2, B-A3 (`BASELINE_FINDINGS.md`) and M-13 (`MYTHOS_FINDINGS.md`), the four items the review asks to resolve before `app-framework` is merged. Both reports live on the `security-review-prep` branch and are not changed here.

None of the four findings came with an executed reproduction: B-A1 to B-A3 give a reproduction idea and M-13 is marked "Not executed". Each regression test below was written from the finding's description, run against the unfixed code to confirm that it fails, and then run again after the fix.

| Finding | Commit | Regression tests | Before the fix |
| --- | --- | --- | --- |
| B-A1 | `4b90b90` | `lang::stdlib::tests::fs_refuses_dangling_symlink_escape` | `write_text` through a dangling link succeeded and created the file outside the data root |
| B-A2 | `4b90b90` | `lang::stdlib::tests::fs_refuses_symlinked_data_root`, `app::tests::dev_refuses_symlinked_data_folders` | writes went through a symlinked or junction data root; `qxide app dev` accepted a linked `target/` |
| B-A3 | `d8d77ff` | `tests::built_apps_refuse_dev_mode` (runtime crate) | a built app accepted `--dev <json>` |
| M-13 | `0be050b` | `lang::app_tests::studio_runs_use_app_limits`; runtime crate: `tests::app_calls_use_app_limits`, `tests::concurrent_calls_share_one_memory_budget`, `tests::queued_calls_do_not_hold_threads`, `tests::a_panicking_call_keeps_its_worker`, `tests::oversized_results_are_rejected_not_sent` | Studio and commands simulated a 25-qubit (512 MiB) state vector; 16 queued calls used 16 threads; a panicking call lost its slot for good; a 32 MiB result was passed to the webview unchanged |

`fs_allows_symlinks_that_stay_inside` was added alongside to keep the documented behaviour that links resolving inside the data root still work.

## B-A1: dangling symlink inside the data root

`confine` (`src/lang/stdlib.rs`) walked up to the deepest ancestor that `canonicalize` could resolve. A dangling link cannot be resolved, so the walk skipped it and accepted the path, and the following write followed the link. The walk now refuses any entry that exists (by `symlink_metadata`) but does not resolve. The existing re-check after creating parent folders is kept.

## B-A2: linked data root

`confine` now refuses the whole call when the data root itself is a symlink or a Windows junction (`FileType::is_symlink` covers both, which the junction case of the test confirms), and it no longer skips the symlink checks when the root exists but cannot be resolved. This covers every host: `qxide run --allow fs` (`<project>/data`), `qxide app dev`, and built apps. `qxide app dev` also refuses a linked `target/` or `target/app-data/` before starting the runtime, because a linked `target/` would move the data folder out of the project without the root itself being a link.

## B-A3: `--dev` accepted by built apps

The launch decision moved out of `main` into `start` (`crates/qxide-app-runtime/src/main.rs`). `--dev` is honoured only when the executable carries no bundle; any attached bundle, including a damaged one or one of another format version, makes the runtime refuse it. `qxide app dev` keeps working because it starts the bare runtime.

## M-13: app runtime and Studio ran with CLI limits

- `Limits::APP` (`src/lang/mod.rs`): at most 24 state-vector qubits (256 MiB) and a 256 MiB stabilizer tableau per call, used by command calls and by `std.qxide.run` in Qxide Studio. Exceeding it fails with `Q0406` and an app-specific message. `qxide run` keeps the CLI limits.
- At most four calls run at once (`MAX_WORKERS`), so simulation memory across concurrent calls stays within 1 GiB; a test checks that bound.
- Calls run on a fixed pool of worker threads created at start-up. Waiting calls are queued closures instead of threads with 64 MiB stacks, and the `commands` request uses the same pool. A worker survives a panicking call, and that call's `Promise` rejects with `Q0412` instead of staying pending. A call id that is still in flight cannot be reused, so its cancel flag cannot be replaced.
- A call result larger than 32 MiB is not handed to the webview; the call rejects with the new code `Q0716`. `std.app.emit` refuses events above the same size with `Q0706`, and oversized dev log lines are not forwarded to the page.
- The page token of an IPC message, which every answer echoes, is limited to 64 characters (commit `2c7e9e4`); a message with a longer token is not answered, so the token cannot push a script past the size cap. `oversized_results_are_rejected_not_sent` covers it and failed before that commit.
- `docs/design/qx-app.md` (sections 5, 6, 8 and 13), `docs/error-codes.md` and the code table in `docs/design/qx-language.md` describe the new limits and `Q0716`.

Commit `d8d77ff` on its own fails `cargo clippy` for the runtime crate (`large_enum_variant` on the new `Start` enum); `0be050b` boxes the variant, and the branch tip is clean.

## Not addressed here

- The result cap stops oversized results at the boundary to the page, but the result is still built in memory first. The output-amplification findings on `main` (M-01, M-10, B-02, B-03) need their own fixes in the shared interpreter, and Studio inherits those fixes once they land. (Update: on branch `app-framework-limits` the execution budgets apply to apps through `Limits::APP`; see part 2 of `EXECUTION_LIMITS_FIXES.md`.)
- Shots stay capped at 1,000,000 per call, as documented.
- Open items are tracked privately.
