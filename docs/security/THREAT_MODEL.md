# Qxide Threat Model

## 1. Scope and Baseline

- **Project:** Qxide (`github.com/qxide-lang/qxide`), Rust crate `qxide` `0.7.0` (released `2026-11-21`, tag `v0.7.0`).
- **Date:** `2026-11-21`
- **Baseline commits:**
  - `main`: `b8c385e` (review branch: `security-review-prep`)
  - `app-framework` (unreleased, not merged): `eea71a6`
- **In-scope components:**
  - Library simulators: `src/lib.rs`, `src/statevector.rs`, `src/stabilizer.rs`, `src/gpu.rs` (feature `gpu`, off by default).
  - `.qx` front end and interpreter: `src/lang/` (`lexer.rs`, `parser.rs`, `check.rs`, `link.rs`, `interp.rs`, `fmt.rs`, `repl.rs`, `diag.rs`, `msg.rs`, `explain.rs`, `mod.rs`), private to the `qxide` binary and compiled into the WebAssembly crate via `#[path]`.
  - CLI (`qxide`): `src/main.rs` (`run`, `check`, `qasm`, `fmt`, `repl`, `explain`, `new`, `bell`, and package commands).
  - Package manager and registry template: `src/pkg/` (`manifest.rs`, `lock.rs`, `index.rs`, `archive.rs`, `cache.rs`, `resolve.rs`, `cli.rs`) and `registry-template/` (`scripts/validate.py`, `scripts/build_search.py`, `.github/workflows/validate.yml`, `.github/workflows/search.yml`) targeting `github.com/qxide-lang/registry`.
  - Browser playground (`qxide.dev/playground`): `crates/qxide-wasm` (Rust compiled to `wasm32`) and `site/src/playground/` (`worker.ts`, `client.ts`, `share.ts`), `site/src/pages/Playground.tsx`.
  - Site and distribution: `site/` (Vite + React, Cloudflare Pages direct upload), `dist-workspace.toml` (`cargo-dist`), `.github/workflows/release.yml`, `.github/workflows/pypi.yml`, `.github/workflows/ci.yml`, and manual `cargo publish` to `crates.io`.
  - Desktop app framework (`app-framework` branch, `eea71a6`): `crates/qxide-app-runtime` (`wry 0.57` + `tao 0.37`), `src/app/`, `src/lang/stdlib.rs`, and `apps/studio`.

Note: The live `github.com/qxide-lang/registry` repository scripts are known to be older than `registry-template/` and were not re-checked against this baseline.

## 2. Assets

1. **The user's machine:** CPU, memory, and local files.
2. **The user's terminal:** Integrity of terminal state when displaying CLI output and package metadata.
3. **`qxide.dev` visitors' browsers:** Origin integrity (`qxide.dev`), tab responsiveness, and browser memory.
4. **Integrity of registry packages and lock files:** Index entries, downloaded archives, `~/.qxide` cache contents, and `qxide.lock`.
5. **Integrity of released binaries, wheels, and crates:** GitHub Release binaries and installers, PyPI wheels, and `crates.io` crates.
6. **The maintainer's credentials:** GitHub, PyPI, `crates.io`, and Cloudflare accounts and tokens.

## 3. Actors

| ID | Actor | Description and Goals |
| --- | --- | --- |
| **A1** | Author of an untrusted `.qx` file or project | Supplies a shared `.qx` file, cloned repository, pasted snippet, or share link. Goal: crash, hang, exhaust memory, read/write files (via `std.fs` on the `app-framework` branch), or produce misleading output. |
| **A2** | Visitor-targeting web attacker | Crafts a playground share link or embeds `qxide.dev/playground` in a web page. Goal: XSS on `qxide.dev`, freeze the victim's tab, or phishing via misleading output. |
| **A3** | Malicious package publisher | Any GitHub account holder opening a PR to `github.com/qxide-lang/registry`. Goal: path traversal on unpack, name squatting or confusable names, dependency confusion, republishing a version with new content, terminal escape injection, or bypassing CI validation. |
| **A4** | Registry compromise or reviewer error | Compromise of `github.com/qxide-lang/registry` or a human reviewer merging a malicious PR. The registry is trusted only through its index; checksums come from the same index with no cryptographic signatures. |
| **A5** | Network attacker | On-path attacker between the client and `raw.githubusercontent.com` or `github.com`. Mitigated by `https`-only transport with TLS 1.2+. |
| **A6** | Release pipeline supply-chain attacker | Targets GitHub Actions dependencies, the PyPI token, `cargo-dist` installers, the Cloudflare Pages account, or the `crates.io` token on the maintainer's PC. |
| **A7** | Local attacker | Attacker with write access to the user's home directory (`~/.qxide`) or the install directory. Out of scope except where noted (`B-P10`, `B-P12`). |
| **A8** | App framework web/bundle attacker | Malicious web content loaded inside an app built with the app framework, or a tampered `.qxpak` app bundle (`app-framework` branch). |

## 4. Trust Boundaries

- **TB1:** `.qx` source text -> lexer, parser, checker, linker, interpreter, and simulators (CLI, REPL, LSP branch, playground wasm, app runtime).
- **TB2:** URL hash -> share-link decoder -> playground editor (no auto-run).
- **TB3:** Browser main thread <-> Web Worker <-> wasm linear memory (no privilege difference; robustness only).
- **TB4:** Registry index, `search.json`, and archives (network) -> package manager -> `~/.qxide` cache -> linker.
- **TB5:** Registry pull request -> `validate.py` in GitHub Actions -> human reviewer -> merge.
- **TB6:** Webview page (JS) -> `wry` IPC -> Rust runtime -> `std` hosts (`fs`, `app`, `qxide`) [`app-framework` branch].
- **TB7:** App bundle bytes appended to the runtime executable -> runtime (bundle trailer, `app.json`) [`app-framework` branch].
- **TB8:** Maintainer machine and CI -> GitHub Releases, PyPI, `crates.io`, `qxide.dev` (Cloudflare Pages).
- **TB9:** Command-line arguments and environment (`$QXIDE_HOME`, `$QXIDE_REGISTRY`, `HOME`/`USERPROFILE`) -> CLI; the CLI spawns `curl`, `gh`, and `git`.

### Data-Flow Diagram

```text
[A1 .qx Source / Repo] ---(TB1)---> [CLI / REPL / LSP / Wasm / App Runtime]
                                       ^            ^
[CLI Args & Env:                       |            | (TB4: index, search.json,
 $QXIDE_HOME, $QXIDE_REGISTRY,         |            |  tar.gz -> ~/.qxide -> linker)
 HOME/USERPROFILE] --------(TB9)-------+            |
   | (spawns curl, gh, git)                         |
   v                                   [github.com/qxide-lang/registry]
[curl / gh / git] <--(TB4 / A5 HTTPS)-------------->|
                                                    ^
[A3 Publisher PR] ---(TB5: validate.py -> Reviewer)-+ [A4]

[A2 Web Attacker]
  +-- URL hash (#code=/#src=) --(TB2)--> [share.ts -> Editor (no auto-run)]
  |                                                  |
  |                                          (TB1, user run)
  v                                                  v
[qxide.dev Main Thread] <--(TB3)--> [Web Worker] <--(TB3)--> [qxide-wasm]

[A8 Webview JS] ---(TB6: wry IPC)---> [qxide-app-runtime] ---> [std: fs/app/qxide]
                                              ^
[A8 .qxpak Bundle + app.json] -----(TB7)------+   (branch app-framework, eea71a6)

[A6 / Maintainer PC & CI] ---(TB8)---> [GitHub Releases | PyPI | crates.io | qxide.dev]
```

## 5. Attack Surfaces

### 5.1 Untrusted `.qx` Input

Covers the parser, checker, linker, interpreter, and simulators across stack overflow, infinite loops, memory blow-up, integer overflow, and panics (TB1; actors A1, A2, A3, A8).

#### Untrusted input

- `.qx` source text from local files, cloned repositories, pasted REPL input, playground editor input and share links, dependency package sources, and app `.qx` sources.
- Numeric literals, loop bounds (any `i64`), register sizes (`qreg`), rotation angles, and shot counts supplied by `.qx` programs.

#### Existing controls

- **Stack overflow and nesting:**
  - Parser enforces `MAX_DEPTH = 128` across parenthesized expressions, unary chains, binary chains, blocks, and `else-if` chains, returning error `Q0106` (`src/lang/parser.rs`).
  - Interpreter enforces `MAX_CALL_DEPTH = 200`, returning a recursion-limit diagnostic (`src/lang/interp.rs`).
  - CLI executes language work on a dedicated thread with a `64 MiB` stack (`LANG_STACK` in `src/main.rs`).
  - WebAssembly build configures a `16 MiB` shadow stack (`crates/qxide-wasm/build.rs`).
- **Memory blow-up and simulator limits:**
  - Maximum qubits per program is capped at `MAX_QUBITS = 2^20`, returning error `Q0409` (`src/lang/`).
  - State-vector simulator enforces `DEFAULT_MEMORY_LIMIT = 1 GiB` in the library/CLI (`src/lib.rs`, `src/statevector.rs`) and `MAX_STATE_VECTOR_QUBITS = 20` in the playground (`crates/qxide-wasm/src/lib.rs`).
  - The stabilizer backend (`src/stabilizer.rs`) has no memory limit of its own; the `.qx` runner checks tableau size before simulating via `stabilizer_bytes` (`src/lang/mod.rs`), capping at `1 GiB` in the CLI and `512 MiB` in the playground (`crates/qxide-wasm/src/lib.rs`).
  - Linker caps loaded modules at `MAX_MODULES = 4096` (`src/lang/link.rs`).
- **Integer overflow and numeric bounds:**
  - `.qx` `int` is `i64` with checked `add`, `sub`, `mul`, `div`, and `neg` (overflow produces a diagnostic); division by zero produces a diagnostic; non-finite rotation angles are rejected; array/register index bounds are checked (`src/lang/interp.rs`).
  - `%` uses `wrapping_rem` (`int.MIN % -1 == 0`, as in Java).
  - `as int` from `double` saturates (Rust `as`), with `NaN -> 0`.
  - `qreg` sizes pass through `usize::try_from` (relevant on 32-bit `wasm32`).
- **Panic containment:**
  - In the CLI, language work runs on a separate thread (`src/main.rs`); a panic results in exit code `101` and the panic message on stderr.
  - In the playground, the `wasm` release profile uses `panic=abort` (`crates/qxide-wasm`), converting panics into wasm traps reported as crashes by the worker, with a fresh `WebAssembly.Instance` created for the next run (`site/src/playground/worker.ts`).

#### Known gaps

- **B-01** (MEDIUM, REPRODUCED): Unbounded loop/recursion; there is no step or wall-clock time limit in the interpreter (`src/lang/interp.rs`, `src/main.rs`), and loop bounds can be any `i64`.
- **B-02** (MEDIUM, REPRODUCED): Unbounded memory consumption from gates or output produced inside a loop.
- **B-03** (MEDIUM, REPRODUCED): Multi-shot output amplification.
- **B-06** (INFO, correctness not security, REPRODUCED): Smallest `i64` literal cannot be written.
- **B-A5** (LOW-MEDIUM, READ, `app-framework` branch): Empty loop bodies cannot be cancelled because `src/lang/interp.rs` polls `host.cancelled()` only inside `stmt()` (every `CANCEL_POLL` statements); `for i in 0..9000000000000000000 {}` never polls, and `std.qxide.qasm` uses `NoEffects` whose `cancelled()` always returns false.
- **B-A8** (LOW-MEDIUM, READ, `app-framework` branch): Unbounded `String` growth at `src/lang/interp.rs:691` (`Value::Str(a + &concat_text(b, at)?)`) has no length cap; repeated doubling allocates gigabytes before any cancel poll and aborts on allocation failure (reachable from the playground once merged).
- Direct Rust library callers of `src/stabilizer.rs` bypass the `.qx` runner's `stabilizer_bytes` check because the stabilizer backend has no internal memory limit.

#### Open questions for the reviewer

- Should the CLI interpreter introduce a configurable or default step/time limit (`B-01`), or remain unbounded by default for long-running local simulations?
- Should memory limits be enforced inside `src/stabilizer.rs` itself (and evaluated for `src/gpu.rs` when feature `gpu` is enabled) rather than only in `src/lang/mod.rs`?
- What caps on accumulated gates, output size (`B-02`, `B-03`), and `String` length (`B-A8`) are acceptable without breaking intended `.qx` programs?

---

### 5.2 WebAssembly Playground

Covers wasm execution limits, share-link decoding, and output rendering/XSS on `qxide.dev/playground` (TB1, TB2, TB3; actors A1, A2).

#### Untrusted input

- `.qx` source code entered in the editor or loaded from a share link (TB1).
- URL hash fragments `#code=<base64url(deflate-raw(utf8))>&shots=..&seed=..` and `#src=` (uncompressed), decoded on page load and on every `hashchange` (`site/src/pages/Playground.tsx:171-197`) (TB2).
- Output strings, diagnostics, and execution traps returned across the raw C ABI (`qx_alloc`/`qx_exec`) from wasm linear memory to the Web Worker and main thread (TB3).

#### Existing controls

- **Execution and memory limits (`crates/qxide-wasm/src/lib.rs`):** `MAX_STATE_VECTOR_QUBITS = 20`, `STABILIZER_BYTES = 512 MiB`, `MAX_SHOTS = 100,000`.
- **ABI and stack (`crates/qxide-wasm`):** Raw C ABI (`qx_alloc`/`qx_exec`, no `wasm-bindgen`); `crates/qxide-wasm/build.rs` sets a `16 MiB` shadow stack; release profile `wasm` uses `panic=abort`.
- **Worker isolation and timeout:** `site/src/playground/worker.ts` executes the module in a Web Worker using a fresh `WebAssembly.Instance` per run; `site/src/playground/client.ts` enforces `TIME_LIMIT_MS = 30,000` and calls `worker.terminate()` on expiry.
- **Share-link validation and no auto-run (`site/src/playground/share.ts`):** Enforces `MAX_HASH = 200,000` chars and `MAX_CODE = 256 KiB` (checked after inflating). Opening a share link populates the editor without running the code.
- **Output rendering / XSS (`site/src`):** Output is rendered as React text children; `innerHTML` and `dangerouslySetInnerHTML` are not used anywhere in `site/src`.
- **Stdlib confinement (`app-framework` branch, `src/lang/stdlib.rs`):** Playground host grants `log` permission only.

#### Known gaps

- **B-04** (LOW, REPRODUCED): Share links are fully inflated in `site/src/playground/share.ts` before the `MAX_CODE = 256 KiB` check, allowing a compact compressed `#code=` hash to trigger large allocations on page load or `hashchange` (`site/src/pages/Playground.tsx:171-197`).
- **B-02** (MEDIUM, REPRODUCED) and **B-03** (MEDIUM, REPRODUCED): Loop-generated gates/output and multi-shot output amplification can exhaust wasm/browser memory or stall the tab before `TIME_LIMIT_MS = 30,000` terminates the worker.
- **B-A8** (LOW-MEDIUM, READ, `app-framework` branch): Unbounded `String` growth (`src/lang/interp.rs:691`) will become reachable from the playground once `app-framework` is merged, causing rapid allocation failure and wasm trap.
- **B-S1** (LOW, READ): `qxide.dev` has no `site/public/_headers` (no CSP, `frame-ancestors`/`X-Frame-Options`, `X-Content-Type-Options`, or `Referrer-Policy`), leaving the playground embeddable in third-party pages (clickjacking/phishing) and without CSP defense in depth.

#### Open questions for the reviewer

- Should `site/src/playground/share.ts` switch to streaming decompression with a `256 KiB` hard byte cutoff to fix `B-04`?
- Should the wasm runner or worker cap total output bytes returned to the main thread (`B-02`, `B-03`)?
- Is third-party iframe embedding of `qxide.dev/playground` required, or should `frame-ancestors 'none'` be set alongside a CSP (`'wasm-unsafe-eval'`, `worker-src 'self'`) (`B-S1`)?

---

### 5.3 Package System

Covers GitHub index trust, `sha256` verification, extraction path escape, dependency confusion, name squatting, and registry validation scripts (TB4, TB5, TB9; actors A3, A4, A5, A7).

#### Untrusted input

- Pull requests submitted to `github.com/qxide-lang/registry` by any GitHub user (index lines, package `.tar.gz` archives, and PR-modified files) (TB5).
- Registry index entries, `search.json`, package descriptions, `dl` URLs, and `.tar.gz` archives fetched from the default GitHub registry or a `--registry` / `$QXIDE_REGISTRY` URL (TB4).
- Project `qxide.toml` manifests (`src/pkg/manifest.rs`), `qxide.lock` lock files (`src/pkg/lock.rs`), and relative `path` dependencies.

#### Existing controls

- **Download transport and size limits (`src/pkg/index.rs`):** Invokes system `curl` with `--proto =https --tlsv1.2 --max-time 120 --max-filesize 10 MiB`; refuses `http://` registries; creates temporary files exclusively under `~/.qxide/registry/tmp`.
- **Index line validation (`src/pkg/index.rs`):** Validates package names (lowercase ASCII, digits, single `-`, `<= 64` chars), semver versions without build metadata, `cksum` as 64 hex characters, and rejects unknown types on optional fields.
- **Checksum verification (`src/pkg/cache.rs` `install`):** Verifies the archive `sha256` against the index `cksum` before unpacking.
- **Extraction hardening (`src/pkg/archive.rs` `unpack`):** Enforces max `10 MiB` archive size, `50 MiB` unpacked size, and `2000` files; allows only regular files and directories; rejects absolute paths, `..`, drive prefixes, non-UTF-8 names, non-portable names (Windows reserved names including `CONIN$`, `CONOUT$`, `COM¹²³`), and case-folding collisions; validates every entry before writing anything; unpacks into a temporary directory then renames into place.
- **Dependency resolution and isolation (`src/pkg/resolve.rs`, `src/lang/link.rs`, `src/lang/stdlib.rs`):** Single registry URL per project (default GitHub index, or `--registry` / `$QXIDE_REGISTRY`); `path` dependencies resolve from disk relative to the manifest; resolver selects the highest matching non-yanked version; linker caps modules at `MAX_MODULES = 4096`; on the `app-framework` branch, `std` is a reserved package name and dependency packages receive `log`-only stdlib permissions.
- **Registry CI validation (`registry-template/scripts/validate.py`, `registry-template/.github/workflows/validate.yml`):** Never extracts archives; checks paths, symlinks/hardlinks/devices, sizes, `sha256`, and manifest name/version; enforces strict JSON (no `NaN`, depth `<= 100`); executes the base branch's copy of `validate.py` from `$RUNNER_TEMP`; workflow triggers on `pull_request` with `contents: read`.

#### Known gaps

- **Index trust and lack of signatures (A4):** Checksums are sourced from the same GitHub-hosted index with no cryptographic signatures; compromise of the registry repo or reviewer error compromises package integrity.
- **Dependency confusion and name ownership:** No namespaces or scopes exist; the first publisher of a package name owns it by convention (human review) only.
- **Live registry drift:** Scripts in the live `github.com/qxide-lang/registry` repository are known to be older than `registry-template/` and were not re-checked.
- **B-P1** (MEDIUM, READ spot-checked): A registry PR can disable its own validation.
- **B-P2** (MEDIUM, READ spot-checked): Deleting an entire index file passes validation.
- **B-P3** (MEDIUM-LOW, READ spot-checked): An externally hosted `dl` skips every archive check in CI.
- **B-P4** (LOW-MEDIUM, READ spot-checked): Package descriptions can carry terminal escape sequences.
- **B-P5** (LOW, READ): Anyone can flip `yanked` in either direction for any package (`registry-template/scripts/validate.py:501`: `n = dict(entries[i], yanked=o.get("yanked"))`); there is no ownership model and review is the only control.
- **B-P6** (LOW, fails closed, READ): `registry-template/scripts/validate.py:349-356` checks the unpacked total only after the loop with no per-member cap; a compressible multi-GB member inside a `10 MiB` gzip, or deeply nested JSON (`RecursionError`), crashes the CI job rather than bypassing it.
- **B-P7** (LOW/INFO, READ): Look-alike name check (`registry-template/scripts/validate.py:419-420`, `squash()`) only strips hyphens — no `l`/`1` or `rn`/`m` folding, no prefix-squatting rule (e.g., `qxide-std`), and two new look-alike names in the same PR are not compared. Windows device names (`con`, `com1`) pass the name regex and with an external `dl` create `index/3/c/con`, breaking Windows checkouts.
- **B-P8** (LOW/INFO, READ): Markdown injection into the CI job summary via archive member names (`registry-template/scripts/validate.py:529-531` embeds member names inside backticks while allowing backticks and brackets in names).
- **B-P9** (LOW/INFO, READ): Index `dl` may point at a local file (`src/pkg/index.rs:331-344` allows `file://...` or a relative path, read from disk; archive must still match `cksum` from the same index line, so this applies only to a malicious index).
- **B-P10** (INFO, READ): Unpacked packages are trusted solely by the `.qxide-checksum` marker file (`src/pkg/cache.rs` `installed()`) without re-checking extracted files (outside threat model unless `~/.qxide` is shared; A7).
- **B-P11** (INFO, SUSPECTED, not tested): The `tar` crate reads GNU long-name and PAX extension headers before per-entry checks in `src/pkg/archive.rs` `unpack`; a `10 MiB` gzip could expand header memory significantly (requires a published malicious package whose `sha256` is in the index).
- **B-P12** (INFO, SUSPECTED): On Windows, `Command::new("curl")` (and `gh`, `git`) searches the directory of `qxide.exe` before `PATH` (Rust `std` behavior), allowing a planted `curl.exe` in a writable install directory (A7).

#### Open questions for the reviewer

- How far behind `registry-template/` is the live `github.com/qxide-lang/registry` repository, and how will `B-P1` through `B-P8` be rolled out there?
- Should external `dl` URLs (`B-P3`) and local `file://`/relative `dl` paths (`B-P9`) be rejected outright?
- What package ownership model is intended to block unauthorized `yanked` changes (`B-P5`), index deletions (`B-P2`), and name squatting (`B-P7`)?
- Are signed index entries or package signatures planned to mitigate single-point trust in the GitHub registry (A4)?

---

### 5.4 App Framework (`app-framework` Branch, `eea71a6`, Unmerged)

Covers the `wry` IPC bridge, `std.fs` and `std.http` permissions, page token, and `.qxpak` bundle integrity (TB1, TB6, TB7, TB9; actors A1, A7, A8).

#### Untrusted input

- JavaScript IPC messages and custom-protocol resource paths sent from the webview page (`crates/qxide-app-runtime/src/bridge.js`, `main.rs`) (TB6; actor A8).
- `.qxpak` bundle bytes appended to the runtime executable, including the bundle trailer and `app.json` fields (`identifier`, `data`, `dev`, `permissions`, `deps`) (TB7; actor A8).
- `--dev <json>` command-line argument accepted by built app executables (TB9; actor A8/A1).
- `.qx` application code calling `std` modules (`log`, `time`, `fs`, `app`, `qxide`) (TB1; actor A1).
- Folder name passed to `qxide app new` (`src/app/mod.rs`) (TB9).

#### Existing controls

- **Per-host `std` permissions (`src/lang/stdlib.rs`):** Enforced via a `Host` trait — CLI grants `log`/`time` (plus `fs` only with `qxide run --allow fs`), playground and REPL grant `log` only, apps grant permissions declared in `[app] permissions`, and dependency packages grant `log` only. `std` is a reserved package name.
- **`std.http` status:** `std.http` does not exist yet (listed as later in `docs/design/qx-app.md`).
- **Local dev server surface:** `qxide app dev` opens no network socket.
- **Webview and IPC bridge (`crates/qxide-app-runtime/src/main.rs`, `bridge.js`):** Custom protocol serves UI files with a CSP header set on every response; runtime enforces an origin check on IPC calls (`crates/qxide-app-runtime/src/main.rs:217`); `crates/qxide-app-runtime/src/bridge.js:11` generates a page token compared in JS to ignore stale replies.
- **`std.fs` path confinement (`src/lang/stdlib.rs`):** `confine` (around `:479-492`) canonicalizes the deepest existing ancestor against the configured data root (`<project>/data` for CLI at `src/main.rs:291-293`, `target/app-data` for `qxide app dev` at `src/app/mod.rs:307`) and checks a reserved-name list.

#### Known gaps

- **B-A1** (MEDIUM, READ confirmed): `std.fs` escape through a dangling symlink (`src/lang/stdlib.rs` `confine` `:479-492`). Because `canonicalize` fails on a symlink whose target does not exist, the loop pops to the parent inside the root and passes; `write_text`/`append_text` (`:324-341`) then follow the symlink and create the target outside the root (requires symlink creation rights, i.e., Unix or Windows developer mode).
- **B-A2** (MEDIUM, READ): The data root itself may be a symlink (`<project>/data` at `src/main.rs:291-293` and `target/app-data` at `src/app/mod.rs:307`); linking it to `..` or `$HOME` places all "confined" paths outside the project.
- **B-A3** (LOW-MEDIUM, READ): Every built app executable accepts `--dev <json>` (`crates/qxide-app-runtime/src/main.rs:378` invokes `App::from_dev` whether or not a bundle is attached), allowing a shipped (and later signed) app exe to load arbitrary `.qx`/UI files with arbitrary permissions, arbitrary data root (e.g., `C:\`), and devtools enabled.
- **B-A4** (LOW, READ): Runtime trusts `app.json` and the `.qxpak` bundle has no integrity check (`crates/qxide-app-runtime/src/app.rs:85-146`): `identifier` is joined raw (`..\..\X` escapes `%APPDATA%`); `data`, `dev`, `permissions`, and `deps` indices (out-of-range indices panic at `src/lang/link.rs:344/351`) are not re-validated.
- **B-A5** (LOW-MEDIUM, READ confirmed): Empty loop bodies cannot be cancelled (`src/lang/interp.rs` polls `host.cancelled()` only inside `stmt()` every `CANCEL_POLL` statements), so Studio's Stop button does nothing and the call holds a worker slot; Studio's `std.qxide.qasm` uses `NoEffects` whose `cancelled()` is always false.
- **B-A6** (INFO, READ): Authenticode signing breaks built apps because `src/app/bundle.rs:46` requires the bundle magic in the last 8 bytes while Authenticode appends a certificate table, conflicting with `docs/design/qx-app.md:381` (sign after build). The macOS `Resources` layout documented there is not implemented.
- **B-A7** (LOW, READ): IPC worker robustness (`crates/qxide-app-runtime/src/main.rs:302-306` acquires/releases slots without a guard, so a panic leaks the slot and the Promise never settles; `:317-319` ignores spawn failures; threads with `64 MiB` stacks are spawned before acquiring a slot, allowing unbounded queued threads; `"commands"` bypasses slots; duplicate `(page, id)` overwrites the cancel flag).
- **B-A8** (LOW-MEDIUM, READ confirmed): Unbounded `String` growth (`src/lang/interp.rs:691`) doubles memory to gigabytes before any cancel poll and aborts on allocation failure.
- **B-A9** (LOW, SUSPECTED): `percent_decode` (`crates/qxide-app-runtime/src/main.rs:121`) slices `&str` by byte indices (`&s[i + 1..i + 3]`), panicking if `%` is followed by a multi-byte character (likely unreachable because the webview percent-encodes).
- **B-A10** (INFO, dev only, READ): Dev-mode UI serving (`crates/qxide-app-runtime/src/app.rs:195`) follows symlinks, has no reserved-name filter, and has no file size cap.
- **B-A11** (LOW, SUSPECTED, Windows-version dependent): Reserved-name list in `src/lang/stdlib.rs` `confine` is shorter than `src/pkg/archive.rs` (omits `CONIN$`, `CONOUT$`, `COM0`/`LPT0`, superscript digits; misses `"CON .txt"` because the stem before the first dot is not trimmed).
- **B-A12** (INFO, READ): The page token in `crates/qxide-app-runtime/src/bridge.js:11` is page-generated and compared only in JS; the origin check at `crates/qxide-app-runtime/src/main.rs:217` is the sole runtime gate (consistent with its documented purpose of ignoring stale replies).
- **B-A13** (INFO, READ): `qxide app new` interpolates the folder name unescaped into TOML and HTML templates (`src/app/mod.rs:121`, `:128`).

#### Open questions for the reviewer

- Should `--dev <json>` be disabled whenever a `.qxpak` bundle is attached (`B-A3`), and how should the bundle format be updated to remain compatible with Authenticode signing and macOS `Resources` layouts (`B-A6`)?
- When `std.http` is designed and added (`docs/design/qx-app.md`), what permission granularity and SSRF/TLS/size controls will be required before merge?
- Should the runtime verify a bundle integrity digest and strictly re-validate `identifier`, `permissions`, `data`, `dev`, and `deps` indices from `app.json` (`B-A4`)?
- Should `std.fs` forbid symlinks at every path component including the data root (`B-A1`, `B-A2`) and share reserved-name checks with `src/pkg/archive.rs` (`B-A11`)?

---

### 5.5 CLI and File Handling

Covers CLI command execution, file/directory handling, environment variables, subprocess invocation, and terminal output (TB1, TB4, TB9; actors A1, A3, A7).

#### Untrusted input

- Command-line arguments, file/project paths, and environment variables (`$QXIDE_HOME`, `$QXIDE_REGISTRY`, `HOME`/`USERPROFILE`) (TB9).
- Untrusted repository contents (`qxide.toml`, `qxide.lock`, `.qx` files, `<project>/data` on `app-framework` branch) (TB1, TB4; actor A1).
- Registry package metadata displayed to the user's terminal (TB4; actor A3).
- Early closure of stdout/stderr pipes by downstream processes.

#### Existing controls

- **Thread isolation (`src/main.rs`):** Language work executes on a dedicated `64 MiB` stack thread (`LANG_STACK`); panics exit with code `101` and print the panic message to stderr.
- **Opt-in filesystem access (`app-framework` branch, `src/lang/stdlib.rs`):** CLI grants `log`/`time` by default and requires explicit `qxide run --allow fs` for `std.fs`; REPL grants `log` only.
- **Safe archive and temp-file handling (`src/pkg/index.rs`, `src/pkg/archive.rs`):** `http://` registries are refused; download temp files are created exclusively under `~/.qxide/registry/tmp`; archive entries are validated prior to writing and moved via temp-directory rename.

#### Known gaps

- **B-05** (INFO, REPRODUCED): CLI panics when stdout or stderr is closed early.
- **B-P4** (LOW-MEDIUM, READ spot-checked): Package descriptions can inject terminal escape sequences into CLI output.
- **B-P10** (INFO, READ): `~/.qxide` cache trusts extracted package files solely by the `.qxide-checksum` marker file (`src/pkg/cache.rs` `installed()`); out of scope unless `~/.qxide` is shared (A7).
- **B-P12** (INFO, SUSPECTED): On Windows, `Command::new("curl")` (and `gh`, `git`) searches the directory containing `qxide.exe` before `PATH` (Rust `std` behavior), allowing binary planting if the install directory is writable (A7).
- **B-A1** (MEDIUM, READ confirmed), **B-A2** (MEDIUM, READ), and **B-A11** (LOW, SUSPECTED) (`app-framework` branch): `qxide run --allow fs` confines to `<project>/data` (`src/main.rs:291-293`), which is bypassable via a dangling symlink (`B-A1`), a symlinked `data` directory (`B-A2`), or gaps in Windows reserved-name filtering (`B-A11`).
- **B-A13** (INFO, READ, `app-framework` branch): `qxide app new` places the folder name unescaped into TOML and HTML templates (`src/app/mod.rs:121`, `:128`).

#### Open questions for the reviewer

- Should CLI output strip or escape terminal control sequences when printing untrusted package metadata (`B-P4`)?
- Should Windows subprocess lookups for `curl`, `gh`, and `git` avoid searching the directory of `qxide.exe` (`B-P12`)?
- Should `qxide run --allow fs` reject symlinked data roots and require an explicit data-root flag or confirmation when running untrusted repositories (`B-A2`)?

---

### 5.6 Install Scripts (`cargo-dist` and PyPI)

Covers binary installers, GitHub Actions CI/release workflows, PyPI wheel publishing, and `crates.io` distribution (TB8; actors A5, A6).

#### Untrusted input

- Third-party GitHub Actions referenced by mutable tags in CI and release workflows (TB8; actor A6).
- Git ref names (`github.ref_name`) interpolated into shell steps in `.github/workflows/release.yml` (TB8; actor A6).
- Installer scripts (`/install.sh`, `/install.ps1` via `qxide.dev` redirect to GitHub `releases/latest`), release artifacts, and PyPI wheels fetched by users (TB8; actors A5, A6).

#### Existing controls

- **Auto-updater disabled (`dist-workspace.toml`):** `install-updater = false` for `cargo-dist` shell and PowerShell installers.
- **Release trigger and fork token isolation (`.github/workflows/release.yml`):** Creating release tags requires repository push access, and fork PRs receive only a read-only token.
- **OIDC readiness (`.github/workflows/pypi.yml:121`):** `id-token: write` is already granted in the PyPI publish workflow.
- **Secret hygiene:** No secrets, `.env` files, or `wrangler` configuration files are tracked in the repository or its full git history (verified by token-pattern scan).

#### Known gaps

- **B-S2** (MEDIUM, READ): `.github/workflows/pypi.yml:129-131` publishes wheels using a long-lived `secrets.PYPI_API_TOKEN` instead of PyPI Trusted Publishing despite `id-token: write` (`:121`) already being granted. Whether the `pypi` GitHub environment has protection rules is not visible from the repository.
- **B-S3** (MEDIUM-LOW, READ): GitHub Actions are pinned to mutable tags, including third-party `PyO3/maturin-action@v1` (`.github/workflows/pypi.yml:82`, `:102`), whose built wheels are published to PyPI, and `dtolnay/rust-toolchain@stable` in `.github/workflows/ci.yml`.
- **B-S4** (LOW, READ): `.github/workflows/ci.yml` defines no `permissions:` block and therefore inherits the repository's default token scope.
- **B-S5** (INFO, READ): `.github/workflows/release.yml` (`cargo-dist` generated) interpolates `github.ref_name` in `run:` lines (`:80`, `:279`) and grants `contents: write` workflow-wide.
- **B-S6** (INFO, READ): Generated `qxide-installer.sh` and `qxide-installer.ps1` scripts are not committed to the repository, preventing integrity verification from source. Any checksum embedded in them originates from the same GitHub release (integrity only, not authenticity). Because `site/public/_redirects` redirects `/install.sh` and `/install.ps1` to GitHub `releases/latest`, user trust depends on both the Cloudflare Pages account and the GitHub account.

#### Open questions for the reviewer

- Are environment protection rules enabled on the GitHub `pypi` environment, and can `.github/workflows/pypi.yml` migrate to Trusted Publishing to remove `PYPI_API_TOKEN` (`B-S2`)?
- Should all third-party and first-party Actions in `ci.yml`, `pypi.yml`, and `release.yml` be pinned to immutable commit SHAs (`B-S3`), and how will `cargo-dist` regeneration be managed for `release.yml` (`B-S5`)?
- How is the maintainer's `crates.io` token protected for manual `cargo publish`, and should release artifacts and installers adopt signed provenance (`B-S6`)?

---

### 5.7 Site Deployment

Covers `qxide.dev` static site hosting, Cloudflare Pages direct upload, redirects, middleware, HTTP headers, and the local Vite dev server (TB2, TB8; actors A2, A5, A6).

#### Untrusted input

- Incoming browser HTTP requests, Host headers, and URL fragments routed to `qxide.dev` and `site/functions/_middleware.ts` (TB2, TB8; actors A2, A5).
- Network requests to the Vite dev server if exposed with `--host` (`site/vite.config.ts`).
- Manually built `dist/` bundles uploaded to Cloudflare Pages from a maintainer machine (TB8; actor A6).

#### Existing controls

- **Minimal edge function surface (`site/functions/_middleware.ts`):** Redirects the apex host only.
- **XSS-safe React rendering (`site/src`):** Output is rendered as React text children, with no `innerHTML` or `dangerouslySetInnerHTML` anywhere in `site/src`.
- **No tracked deployment secrets:** No secrets, `.env`, or `wrangler` configuration files are tracked in the repository or its git history.

#### Known gaps

- **B-S1** (LOW, READ): No `site/public/_headers` file is present on `qxide.dev`; responses lack `Content-Security-Policy`, `frame-ancestors`/`X-Frame-Options`, `X-Content-Type-Options`, and `Referrer-Policy`. Although no XSS sink exists today, these headers are missing as defense in depth (a CSP for the playground requires `'wasm-unsafe-eval'` and `worker-src 'self'`).
- **B-S6** (INFO, READ): `site/public/_redirects` maps `/install.sh` and `/install.ps1` to GitHub `releases/latest`; compromise of the Cloudflare Pages account enables redirecting installer downloads without compromising GitHub.
- **B-S7** (INFO, READ): `site/vite.config.ts:7` sets `server.fs.allow: ['..']`, allowing the Vite dev server to serve files from the repository root (relevant only when run with `--host`).
- Deployment uses manual `npx wrangler pages deploy dist --project-name qxide` direct uploads rather than a git-connected or CI-backed pipeline, tying site integrity directly to the maintainer's workstation and Cloudflare account (TB8; actor A6).

#### Open questions for the reviewer

- What exact security header policy (including CSP with `'wasm-unsafe-eval'` and `worker-src 'self'`, plus `frame-ancestors`) should be added in `site/public/_headers` (`B-S1`)?
- Should `qxide.dev` deployment move from manual `wrangler pages deploy` direct uploads to a reviewed CI workflow?
- Can `server.fs.allow: ['..']` in `site/vite.config.ts:7` be narrowed to the specific build-output directory needed by the site (`B-S7`)?

## 6. Out of Scope

- **Local attacker with write access to user/install paths (Actor A7):** Write access to the user's home directory (`~/.qxide`) or the `qxide` installation directory is out of scope, except where specifically noted (`B-P10` for shared `~/.qxide` caches and `B-P12` for Windows binary search order in writable install directories).
- **Unimplemented features:** `std.http` (does not exist yet; deferred in `docs/design/qx-app.md`) and the unimplemented macOS `Resources` app-bundle layout (`B-A6`).
- **Live `github.com/qxide-lang/registry` repository scripts:** Known to be older than `registry-template/` and not re-checked against this baseline.
- **Non-repository configuration and external artifacts:** GitHub `pypi` environment protection rules (`B-S2`), generated `cargo-dist` installer scripts not committed to git (`B-S6`), and off-repo credential/account controls for GitHub, PyPI, `crates.io`, and Cloudflare.
- **Correctness-only bugs without security impact:** Such as `B-06` (smallest `i64` literal cannot be written).
