# Mythos Security Review Findings

- **Date:** 2026-11-21
- **Reviewer:** Claude Mythos 5.2 in Claude Code
- **Commits reviewed:** `main` (`b8c385e`, `qxide` 0.7.0) and `app-framework` (`eea71a6`, read only). Both tips match the baseline.
- **Inputs:** `docs/security/REVIEW_BRIEF.md`, `THREAT_MODEL.md`, `BASELINE_FINDINGS.md`. The maintainer asked for this file name and for a commit on `security-review-prep` instead of the `REVIEW_FINDINGS.md` and new branch named in the brief.
- **Fixes:** none applied.

All of `src/` on `main` was read in full except test files and message tables, including the files the baseline did not open (`check.rs`, `lexer.rs`, `parser.rs`, `fmt.rs`, `repl.rs`, `diag.rs`, `link.rs`, the simulators, `main.rs`, and all of `src/pkg`), along with `crates/qxide-wasm/src/lib.rs`, the playground sources, `registry-template` scripts and workflows, and the release workflows. On `app-framework`, the runtime (`crates/qxide-app-runtime`), `src/lang/stdlib.rs`, `studio.rs`, `command.rs`, and `src/app/bundle.rs` were read with `git show`; because nothing on that branch was built or run, every statement about it is marked Suspected. Reproductions used the existing release CLI and wasm builds of `b8c385e`, a local directory registry with `QXIDE_HOME` in a scratch folder, Node 24 for the playground module, and a throwaway git repository for `validate.py`. Hostile inputs ran under a time limit and a working-set cap, no request was sent to any network host other than the loopback address, and nothing was fixed.

Severities use the rubric of `REVIEW_BRIEF.md`. No Critical or High finding was found: there is no code execution, no write outside allowed directories, and no bypass of checksum verification. The Medium findings are denial of service through `check` and `install`, and a registry-controlled local path. A finding is marked Reproduced only where the observed output is quoted.

## Summary

| ID | Severity | Status | Location | One-line title |
| --- | --- | --- | --- | --- |
| M-01 | Medium | Reproduced | `src/lang/diag.rs:221-244` | Quadratic `qxide check` output from reprinted diagnostic lines (100 KB, over 3.4 GB) |
| M-02 | Medium | Reproduced | `src/pkg/archive.rs:235-249` | Unbounded memory reads for tar long-name and PAX headers before archive checks |
| M-03 | Medium | Reproduced (local file, loopback UNC); Suspected (remote SMB) | `src/pkg/index.rs:331-345` | Index `dl` value lets the client read any local file or UNC path |
| M-04 | Low | Reproduced | `src/pkg/cli.rs:278-283` | Registry text, archive paths, and echoed source lines retain terminal escape sequences |
| M-05 | Low | Reproduced | `src/lang/check.rs:283-301` | Quadratic checker runtime in `qxide check` (about 1 MB takes 41 s) |
| M-06 | Low | Reproduced (Node); Suspected (browser) | `site/src/pages/Playground.tsx:90` | Playground Counts view trusts stdout shape, allowing spoofed charts and render exceptions |
| M-07 | Low | Suspected | `site/src/pages/Playground.tsx:472-496` | Playground processes large results on main thread outside the 30 s worker limit |
| M-08 | Low | Reproduced | `src/pkg/project.rs:67-80` | Mismatched manifest and index dependencies leave dependents permanently out of date (Q0612) |
| M-09 | Info | Reproduced (gzip members); Suspected (entry size) | `src/pkg/archive.rs:235` | Client and `validate.py` parse archives differently (gzip members, entry sizes) |
| M-10 | Low | Reproduced | `src/lib.rs:720-739` | Conditional nesting across calls: 297 MB `qxide qasm` output and playground wasm stack overflow |
| M-11 | Info | Reproduced | `src/pkg/index.rs:282` | Duplicate-version scan of an index file runs in quadratic time |
| M-12 | Low | Suspected | `.github/workflows/pypi.yml:129` | Branch-pinned PyPI action and cargo-dist installer piped into `sh` without a digest |
| M-13 | Low | Suspected | `app-framework`: `crates/qxide-app-runtime/src/main.rs:302-319` | App runtime and Qxide Studio run with CLI limits (`app-framework` branch) |

Environment for every reproduction unless stated: Windows 11, PowerShell 5.1 and Git Bash, the release CLI `target/release/qxide.exe` (0.7.0, built from `b8c385e`), and the playground module `target/wasm32-unknown-unknown/wasm/qxide_wasm.wasm` loaded in Node 24.21. Commands ran under a wrapper that kills the process at a time limit or a working-set cap and samples the working set every 200 ms, so runs shorter than that report a peak of 0 MB. Scratch files are under `target/mythos-repro/` (ignored, not committed).

## Baseline items re-checked

| Baseline | Result | Note |
| --- | --- | --- |
| B-01, B-02, B-03, B-08, B-09 | confirms | Not re-run, with code locations unchanged; a new random smoke run (seed 20261121, 400 mutants, 2 s limit) had 122 mutants pass `check` and 6 failing commands from 3 mutants with no panics, all time-outs from loop bound 9223372036854775807 duplicating B-01. M-10 and M-01 add two paths that a step budget alone would not stop. |
| B-04 | confirms | Confirmed by reading `share.ts` lines 40 to 54 and not re-run. M-07 describes the related page-side cost after a run. |
| B-05, B-06, B-07 | not re-examined | Not re-examined beyond reading the code, and code locations are unchanged. |
| B-P1 | confirms | Confirmed by reading `validate.yml` and `validate.py` lines 511 to 517, since a pull_request run uses the pull request's workflow file and can skip the step that runs the base copy of the script. Not executed because verification needs GitHub Actions. |
| B-P2 | confirms (READ to Reproduced) | Reproduced in a throwaway repository where a branch deleting `index/de/mo/demo` passes `validate.py --base main` with exit 0. A follow-up branch re-adding version 1.0.0 with a different `cksum` and an external https `dl` also passes with exit 0. |
| B-P3 | confirms (READ to Reproduced) | Reproduced by the same follow-up branch, where an entry with an external `dl` was accepted without any archive check. Although omitted from the baseline, `valid_dl` does restrict `dl` to https URLs or plain relative paths. |
| B-P4 | extends (M-04) | Reproduced and extended by M-04: `qxide search` printed ESC and BEL bytes from `search.json`, and `validate.py` accepted a description containing them. |
| B-P5 | confirms (READ to Reproduced) | Reproduced: a branch that only flips `yanked` to true on an existing line passes with exit 0. |
| B-P6, B-P7, B-P8, B-P10, B-P12, B-P13 | confirms | Confirmed by reading and not executed. For B-P7, the client also reads `index/3/c/con`-style paths for names such as `con` and `nul`; on this Windows 11 machine, that was not a device path. |
| B-P9 | extends (M-03) | Reproduced and extended by M-03 with severity raised to Medium: `file://` works with any registry and UNC paths are honored. |
| B-P11 | confirms (M-02) | Reproduced by M-02, updating status from Suspected to Reproduced and severity from Info to Medium. |
| B-S1 to B-S7 | confirms; extends B-S3 (M-12) | Confirmed by reading, with M-12 extending B-S3. Tag handling in `pypi.yml` held: it validates the tag with a regular expression, passes it through `env`, and checks it against `Cargo.toml`. |
| B-A1 to B-A8, B-A10 to B-A13 | confirms; extends B-A7 (M-13) | Confirmed by reading only, with line references matching `eea71a6` and M-13 extending B-A7. The `link.rs` lines cited in B-A4 were not opened on that branch. |
| B-A9 | neither confirmed nor refuted | Status remains Suspected because the slice can only panic if the request path holds a non-ASCII character after `%` and one hex digit. Whether the `http::Uri` type passed by wry can hold non-ASCII bytes was not verified. |

Output of the `validate.py` reproduction (`python -I`, throwaway repository made from `registry-template/`, package `demo` 1.0.0 on `main`):

```text
[PR deletes index/de/mo/demo] validate.py exit 0: 0 packages, 0 versions, 0 errors
[follow-up PR re-adds demo 1.0.0 with a new cksum and external dl] validate.py exit 0: 1 packages, 1 versions, 0 errors
[PR flips yanked on an existing version] validate.py exit 0: 1 packages, 1 versions, 0 errors
[PR adds a version whose description holds ESC and BEL] validate.py exit 0: 1 packages, 2 versions, 0 errors
```

Smoke run (`QXIDE_SMOKE_SEED=20261121 QXIDE_SMOKE_CASES=400 QXIDE_SMOKE_TIMEOUT_MS=2000 cargo test --release -j 4 --test smoke_random -- --ignored --nocapture`):

```text
400 mutants x 4 commands, seed 20261121: 122 passed `check`, 6 failure(s)
```

## M-01: Quadratic `qxide check` output from reprinted diagnostic lines (100 KB, over 3.4 GB)

- **Severity:** Medium
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`); the same code is compiled into `crates/qxide-wasm` and the `app-framework` runtime
- **Location:** `src/lang/diag.rs:221-244` (`snippet` prints the whole line and builds the caret padding from it), `src/lang/diag.rs:246-262` (`render`), `src/lang/mod.rs:144-152` (all diagnostics joined into one `String`), `src/lang/check.rs:168-186` (every error is returned)
- **Relation to the baseline:** `new`

Every diagnostic prints the complete text of the referenced source line along with a caret line padded to the error column via `SourceMap::snippet`, while the checker reports every error it finds with no cap. Because `render` joins all rendered diagnostics into a single `String` before anything is written, E errors on one line of L bytes yield roughly 2 x E x L bytes of output, so output grows with the square of the input size. In measurement, a single-line source of 32,014 bytes with 16,000 unknown-name errors produced 771,114,467 bytes of stderr.

### Reproduction

One line of source with `n` unknown names (`n` = 2000, 4000, 8000, 16000, 50000):

```bash
python -I -c "import sys; n=int(sys.argv[1]); open(f'oneline_{n}.qx','wb').write(('void main(){' + 'a;'*n + '}\n').encode())" 16000
qxide check oneline_16000.qx
node target/mythos-repro/wasm_run.mjs check oneline_16000.qx   # same ABI calls as site/src/playground/engine.ts
```

### Observed result

```text
[oneline_2000] exited; exit=1; wall=0.23s; peak working set=0 MB; stdout=0 B; stderr=12385461 B      (input 4014 B)
[oneline_4000] exited; exit=1; wall=0.22s; peak working set=0 MB; stdout=0 B; stderr=48771461 B      (input 8014 B)
[oneline_8000] exited; exit=1; wall=0.43s; peak working set=190 MB; stdout=0 B; stderr=193546467 B   (input 16014 B)
[oneline_16000] exited; exit=1; wall=1.29s; peak working set=810 MB; stdout=0 B; stderr=771114467 B  (input 32014 B)
[oneline_50000] killed: memory cap 3000 MB; exit=-1; wall=1.27s; peak working set=3459 MB; stdout=0 B; stderr=0 B   (input 100014 B)
```

Playground module in Node:

```text
done in 2078 ms; result JSON 192978500 bytes; wasm memory 735 MiB; source 16014 bytes
done in 64531 ms; result JSON 769962500 bytes; wasm memory 2884 MiB; source 32014 bytes
trap after 291334 ms: RuntimeError: unreachable; wasm memory 4096 MiB; source 100014 bytes
```

First diagnostic of the 4 KB input (the echoed line is 4,017 characters and repeats for each of the 2,000 errors):

```text
error[Q0201]: cannot find value `a` in this scope
 --> oneline_2000.qx:1:13
  |
1 | void main(){a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a;a; ...
  |             ^
```

### Impact

Actor A1 (author of an untrusted `.qx` file) or A2 (via a share link) can trigger this issue across trust boundary TB1 using only `qxide check` rather than `run`. Because the baseline described `check` as safe, tools that check a file on open (editors, CI, the unmerged `lsp` branch, and `qxide publish`, which checks `src/lib.qx`) are exposed. In the CLI, a 100 KB file passed 3.4 GB of memory in 1.3 s before being killed by the test harness cap, and there is no limit other than the machine's memory. In the playground, a 16 KB source produces a 193 MB result within about 2 s, well inside the 30 s worker limit and far below the 256 KiB share-link limit, while a 100 KB source grows wasm memory to 4 GiB and traps. A step budget or an output cap in the interpreter (as proposed for B-01 and B-02) does not cover this path because the interpreter never runs.

### Suggested fix

Cap the number of rendered diagnostics (for example, rendering the first 50 followed by a line stating how many more were omitted) and truncate each echoed source line to a window around the error column (for example, 120 columns with an ellipsis) without building caret padding from the whole line. Write diagnostics to the output incrementally instead of joining them into one string, and add a total byte cap for rendered diagnostics in `Limits` for the playground and the app runtime. A regression test can supply a one-line input with a few thousand errors and assert that the output stays under a fixed size.

## M-02: Unbounded memory reads for tar long-name and PAX headers before archive checks

- **Severity:** Medium
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/archive.rs:235-249` (entries are checked only after the `tar` crate has yielded them); `tar` 0.4.46 (`Cargo.lock:2039`)
- **Relation to the baseline:** `confirms B-P11` (status Suspected to Reproduced, severity Info to Medium)

`archive::unpack` validates each entry only after `tar::Archive::entries()` yields it, whereas the `tar` crate (0.4.46) reads the data of a GNU long-name (`L`) or PAX extended header fully into memory with no size limit before yielding the following entry. The 10 MiB limit applies only to the compressed archive, and `MAX_UNPACKED` and `MAX_FILES` are checked only after the header has already been read. A long-name header declaring 1 GiB of zero bytes compresses to about 1 MiB.

### Reproduction

Local directory registry, `QXIDE_HOME` set to a scratch folder, project `p-longname` with an empty `[dependencies]`. The archive is one GNU long-name header (`././@LongLink`, type `L`, size 1 GiB) followed by 1 GiB of zero bytes and one empty regular entry, written through `zlib.compressobj(9, zlib.DEFLATED, 31)`; its sha256 goes into `index/lo/ng/longname` (generator: `target/mythos-repro/build_reg.py`, section 2).

```bash
QXIDE_HOME=target/mythos-repro/home qxide add longname --registry target/mythos-repro/reg   # run in proj/p-longname
```

### Observed result

```text
longname archive bytes: 1043760
[add_longname] killed: memory cap 5000 MB; exit=-1; wall=2.66s; peak working set=5067 MB; stdout=0 B; stderr=29 B
       Added longname v1.0.0
```

Control with a normal archive from the same registry:

```text
[add_good] exited; exit=0; wall=0.24s; peak working set=0 MB; stdout=0 B; stderr=74 B
```

Not tested: how `validate.py` (Python `tarfile`) behaves on the same archive.

### Impact

Actor A3 (a package publisher) or A4 (via a registry compromise or reviewer error) can trigger this defect across trust boundary TB4 once a crafted archive is published with a matching index checksum. Anyone running `qxide add` or `qxide install` for such a version loses several gigabytes of memory within seconds: a 1,043,760-byte archive exceeded 5,067 MB of working set in 2.7 s and was killed by the test cap, and a 10 MiB archive can declare about ten times as much. In the registry template, the same archive also causes `validate.py` to read the header into memory via Python `tarfile`, which fails closed. However, an entry with an external `dl` skips the archive check in CI (B-P3), so CI would not see it.

### Suggested fix

Wrap the gzip decoder in a reader that fails after `MAX_UNPACKED` plus a small allowance of decompressed bytes so no header or entry can expand beyond that threshold. Reject archives containing GNU long-name, long-link, or PAX extension entries (since `qxide publish` writes plain GNU headers with short paths), or read raw entries and cap extension data at a few kilobytes, applying the same cap in `validate.py`. A regression test should confirm that the 1 GiB long-name archive is rejected with Q0610 while memory stays near the archive size.

## M-03: Index `dl` value lets the client read any local file or UNC path

- **Severity:** Medium
- **Confidence:** Medium
- **Status:** Reproduced for local files and for a UNC path over the loopback admin share; Suspected for credential exposure to a remote SMB host (not tested, by the review rules) and for `/dev/zero` on Unix
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/index.rs:331-345` (`download`: `file://` at `:336-337`, scheme-less `dl` at `:343-344`), `src/pkg/index.rs:224` (directory registry join), `src/pkg/index.rs:427-440` (`file_url_path`), `src/pkg/index.rs:418-424` (`read_optional`, no size limit); compare `registry-template/scripts/validate.py:101-108` (`valid_dl`)
- **Relation to the baseline:** `extends B-P9`

An index entry's `dl` value (or the `dl` template in `config.json`) is used as given. `Registry::download` reads `file://` URLs from the local disk for every kind of registry, including the default https registry, and joins a `dl` without a scheme onto a directory registry's path, where an absolute path or `..` leaves the registry. On Windows, `file:////host/share/x` becomes the UNC path `\\host\share\x`, and the file is read whole with no size limit before the size check runs. Because `registry-template/scripts/validate.py` (`valid_dl`) accepts only https URLs or plain relative paths, the client is more permissive than the registry CI.

### Reproduction

Index lines in the local directory registry (the `cksum` is that of an unrelated valid archive):

```text
{"name": "localfile", "vers": "1.0.0", "deps": [], "cksum": "d41b55a7b00a48390038ecbeb8aac1f8dc3922473b686902b69b2bce4d40210e", "yanked": false, "v": 1, "dl": "file:///C:/Windows/win.ini"}
{"name": "uncpath", "vers": "1.0.0", "deps": [], "cksum": "d41b55a7b00a48390038ecbeb8aac1f8dc3922473b686902b69b2bce4d40210e", "yanked": false, "v": 1, "dl": "file:////localhost/C$/Windows/win.ini"}
```

```bash
qxide add localfile --registry target/mythos-repro/reg   # in proj/p-localfile
qxide add uncpath --registry target/mythos-repro/reg     # in proj/p-uncpath
```

### Observed result

```text
[add_localfile] exited; exit=1; wall=0.24s
       Added localfile v1.0.0
error[Q0607]: checksum mismatch for downloaded archive `localfile 1.0.0`
note: recorded in qxide.lock: sha256:d41b55a7b00a48390038ecbeb8aac1f8dc3922473b686902b69b2bce4d40210e
note: downloaded file: sha256:6b3d6e268dcb76e175a7db3d9e031349ab2c32654c7e57581a851e64dd6214ab
help: do not use this package; report it to the registry maintainers

sha256 of win.ini: 6b3d6e268dcb76e175a7db3d9e031349ab2c32654c7e57581a851e64dd6214ab   (Get-FileHash C:\Windows\win.ini)

[add_unc2] exited; exit=1; wall=0.21s
       Added uncpath v1.0.0
error[Q0607]: checksum mismatch for downloaded archive `uncpath 1.0.0`
note: downloaded file: sha256:6b3d6e268dcb76e175a7db3d9e031349ab2c32654c7e57581a851e64dd6214ab
```

A missing target gives a different code:

```text
error[Q0608]: archive for `uncpath 1.0.0` not found at `file:////127.0.0.1/qx-no-such-share/a.tar.gz`
```

### Impact

Actor A4 (a compromised registry or a merged malicious index line) or the operator of any third-party registry pointed to with `--registry` or `QXIDE_REGISTRY` can exploit this across trust boundary TB4. Although a registry should only supply package contents, it can cause the client to open any local path: checksum errors display the file's sha256 to the local user only, and error codes Q0607 and Q0608 distinguish existing files from missing ones. On Windows, a UNC path makes the operating system contact the named host over SMB; a loopback test confirmed that UNC paths are honored, and Windows normally authenticates such connections with the user's NTLM credentials, which was not tested against a remote host under the review rules. On Unix, a `dl` of `file:///dev/zero` would be read without limit, though this was not tested.

### Suggested fix

Restrict the client to accept only what `validate.py` accepts, namely an `https://` URL or a relative path made of plain segments without `..`, drive letters, leading slashes, or backslashes. Allow `file://` only when the registry itself is a local directory, resolve it inside that directory, and stat the file to refuse anything larger than `MAX_ARCHIVE` before reading it. Regression tests should verify that index lines with `file:///`, UNC, absolute, and `..` values of `dl` fail with Q0609 before any file is opened.

## M-04: Registry text, archive paths, and echoed source lines retain terminal escape sequences

- **Severity:** Low
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/cli.rs:278-283` (search rows), `src/pkg/index.rs:360-369` (`name`/`version` of `search.json` not validated), `src/pkg/index.rs:285-287` (Q0609 reason), `src/pkg/archive.rs:203-217` (Q0610 path), `src/lang/diag.rs:237` (echoed source line), `registry-template/scripts/validate.py:280-283` (`description` only type-checked)
- **Relation to the baseline:** `extends B-P4`

Text coming from a registry, an archive, or a `.qx` file is printed to the terminal byte for byte. Besides the package description named in B-P4, the same applies to the unvalidated `name` and `version` fields of `search.json`, the reason text of invalid-index errors (Q0609) quoting the bad package name, the entry path quoted in invalid-archive errors (Q0610), and the source line echoed by every `.qx` diagnostic. In addition, `validate.py` accepts control characters in `description`, which was reproduced with a throwaway repository.

### Reproduction

Local directory registry. `ESC` and `BEL` below stand for the bytes 0x1B and 0x07; the injected text is `ESC ]0;TITLE-SET-BY-REGISTRY BEL ESC [2K`.

```text
search.json:        {"version": 1, "packages": [{"name": "good<inj>", "version": "1.0.0<inj>", "description": "plain <inj>"}]}
index/ba/dl/badline: {"name": "x<inj>", "vers": "1.0.0", "deps": [], "cksum": "000...0", "yanked": false, "v": 1}
escpath archive:    member `escpath-1.0.0/src/a<inj>.qx`
esc.qx:             void main() {
                        undefined_name; // ESC ]0;TITLE-SET-BY-QX-FILE BEL ESC [2K ESC [1A ESC [2K
                    }
```

```bash
qxide search good --registry target/mythos-repro/reg
qxide add badline --registry target/mythos-repro/reg
qxide add escpath --registry target/mythos-repro/reg
qxide check esc.qx
```

### Observed result

Captured bytes, with 0x1B shown as `<ESC>` and 0x07 as `<BEL>`:

```text
good<ESC>]0;TITLE-SET-BY-REGISTRY<BEL><ESC>[2K = "1.0.0<ESC>]0;TITLE-SET-BY-REGISTRY<BEL><ESC>[2K" # plain <ESC>]0;TITLE-SET-BY-REGISTRY<BEL><ESC>[2K

error[Q0609]: invalid index entry for `badline` on line 1: invalid package name `x<ESC>]0;TITLE-SET-BY-REGISTRY<BEL><ESC>[2K` (must start with a lowercase ASCII letter, use only lowercase ASCII letters, digits, and single hyphens, and be at most 64 characters)

error[Q0610]: invalid package archive: unsafe or invalid archive entry path `escpath-1.0.0/src/a<ESC>]0;TITLE-SET-BY-REGISTRY<BEL><ESC>[2K.qx`

error[Q0201]: cannot find value `undefined_name` in this scope
 --> esc.qx:2:5
  |
2 |     undefined_name; // <ESC>]0;TITLE-SET-BY-QX-FILE<BEL><ESC>[2K<ESC>[1A<ESC>[2K
  |     ^^^^^^^^^^^^^^
```

`validate.py` accepting the description is shown under "Baseline items re-checked".

### Impact

Actors A3 and A4 can trigger this through the registry across trust boundary TB4, and actor A1 can trigger it through a `.qx` file across TB1. Escape sequences can change the window title, erase or rewrite lines that were already printed (for example, to hide an error or a warning), and in terminals that support them create hyperlinks or write the clipboard. No code execution was shown, so the impact is on the integrity of what the user reads. Program output from `println` is also printed unchanged, which is expected for program output but matters when a user runs an untrusted file to see what it does.

### Suggested fix

Route every string originating outside the binary (registry fields, archive paths, error reasons that quote input, and echoed source lines) through one function that replaces control characters other than tab with a visible escape such as `\u{1b}`. Reject control characters in `description` in both `validate.py` and the client's `Entry::parse`, and validate `name` and `version` from `search.json` using the same rules as index lines. Program output could be filtered only when stdout is a terminal.

## M-05: Quadratic checker runtime in `qxide check` (about 1 MB takes 41 s)

- **Severity:** Low
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/lang/check.rs:283-301` (`conditional_safety` fixpoint), `src/lang/check.rs:401-403` (`states`, called at `:547`, `:644`, `:646`, `:653`, `:939`), `src/lang/check.rs:392-399` (`lookup`)
- **Relation to the baseline:** `new` (the baseline measured `check` as fast and did not open `check.rs`)

Three parts of the checker are quadratic in the size of the program. `conditional_safety` repeats a pass over all functions until nothing changes and marks at most one new function per pass when a call chain is declared callers first, `states()` copies the state of every variable declared so far in the function at each `if`, `for`, `&&`, and `||`, and `lookup` scans the variables in scope linearly. Measured on the release CLI, a 777,827-byte file of 32,000 chained functions took 41.3 s in `qxide check` and a 988,905-byte function with 40,000 declarations and 40,000 `if` statements took 41.5 s, with halving the input cutting the time by a factor of about eight.

### Reproduction

```bash
# chain_N.qx: f0 calls f1, ..., fN prints
python -I -c "import sys; n=int(sys.argv[1]); open(f'chain_{n}.qx','wb').write((''.join(f'void f{i}(){{f{i+1}();}}\n' for i in range(n)) + f'void f{n}(){{println(\"x\");}}\nvoid main(){{}}\n').encode())" 32000
# states_N.qx: N declarations, then N empty ifs
python -I -c "import sys; n=int(sys.argv[1]); open(f'states_{n}.qx','wb').write(('void main(){\n' + ''.join(f'int a{i}=0;\n' for i in range(n)) + 'if(true){}\n'*n + '}\n').encode())" 40000
qxide check chain_32000.qx
qxide check states_40000.qx
```

### Observed result

```text
[chain_4000] exited; exit=0; wall=0.44s    (input 89825 B)
[chain_8000] exited; exit=0; wall=1.10s    (input 181825 B)
[chain_16000] exited; exit=0; wall=5.46s   (input 377827 B)
[chain_32000] exited; exit=0; wall=41.32s; peak working set=59 MB   (input 777827 B)
[states_10000] exited; exit=0; wall=1.09s  (input 238905 B)
[states_20000] exited; exit=0; wall=4.14s  (input 488905 B)
[states_40000] exited; exit=0; wall=41.52s; peak working set=34 MB  (input 988905 B)
```

### Impact

Actor A1 can trigger this across trust boundary TB1 with a source file of about 1 MB, keeping `qxide check` and every command that checks first (`run`, `qasm`, `publish`) busy for tens of seconds with no diagnostic while memory stays small and larger files scale worse. Within the playground's 256 KiB share-link limit the cost is about one to two seconds natively, so this mainly concerns the CLI and tools that check files automatically. The interpreter budget proposed for B-01 does not apply because the time is spent before elaboration.

### Suggested fix

Compute conditional safety with a worklist over reverse call edges so it is linear in the number of calls, and avoid copying all states by recording only the variables whose state changed inside a branch or keeping quantum variables in a separate short list since only they have a state. Use a hash map per scope for name lookup, and reject source files above a documented size limit before parsing. Regression tests can assert a time bound on the two generated inputs.

## M-06: Playground Counts view trusts stdout shape, allowing spoofed charts and render exceptions

- **Severity:** Low
- **Confidence:** Medium
- **Status:** Reproduced in Node (parsing and the `RangeError`); Suspected in a browser (page unmount, Safari limit)
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `site/src/playground/output.ts:30-44` (`parseCounts`), `site/src/pages/Playground.tsx:210-213` and `:233` (table detected without regard to shots), `site/src/pages/Playground.tsx:90` (`Math.max(...rows.map((r) => r.count))`)
- **Relation to the baseline:** `new`

The playground decides whether a run produced a multi-shot table solely from the shape of stdout via `parseCounts` in `output.ts`, rather than from the shot count, so a single-shot program that prints lines formatted like table rows gets a Counts chart with numbers of its own choosing. The `Counts` component then evaluates `Math.max(...rows.map((r) => r.count))`, where spreading more than about 125,000 arguments throws `RangeError` in V8 (measured in Node 24, where 100,000 works and 125,000 throws). In addition, `site/src` has no error boundary.

### Reproduction

`spoof.qx` (shots = 1) and `spoof_many.qx` (shots = 1); `×` is U+00D7:

```text
void main() {
    println("[1, 1]  0b11 = 3  ×1000");
}
```

```text
void main() {
    for i in 0..200000 {
        println("[0]  0b0  ×1");
    }
}
```

```bash
node --stack-size=984 target/mythos-repro/play_test.mjs spoof.qx 1        # imports engine.ts and output.ts from site/src/playground
node --stack-size=984 target/mythos-repro/play_test.mjs spoof_many.qx 1
node --stack-size=984 target/mythos-repro/play_test.mjs real17.qx 100000   # qreg(17), h on each, measure
```

### Observed result

```text
== spoof shots=1
page side parseCounts + parseDiagnostics: 0 ms; rows 1
first row: {"bits":"[1, 1]","binary":"0b11","value":"3","count":1000,"lines":[]}
== spoof_many shots=1
worker side (wasm + decode + JSON.parse): 33 ms; exit 0; stdout 2600000 chars; stderr 0 chars; source 80 chars
page side parseCounts + parseDiagnostics: 37 ms; rows 200000
Math.max(...rows.map((r) => r.count)) throws RangeError: Maximum call stack size exceeded
== real17 shots=100000
page side parseCounts + parseDiagnostics: 19 ms; rows 70013
Math.max(...rows.map((r) => r.count)) = 7
```

`Math.max(...new Array(n).fill(1))` in Node 24.21: `65536 ok`, `100000 ok`, `125000 RangeError: Maximum call stack size exceeded`. A search of `site/src` for `ErrorBoundary`, `componentDidCatch` and `errorElement` found nothing.

### Impact

Actor A2 can trigger this issue across trust boundaries TB1 and TB2 via a share link once the victim clicks Run. A shared program can display a fabricated measurement distribution in the Counts tab that looks like a simulation result. An 80-byte program that prints 200,000 row-shaped lines makes the render throw; without an error boundary, React is expected to unmount the page, though this was not run in a browser. A genuine run with 17 qubits and 100,000 shots produced 70,013 rows, below the V8 limit but above the 65,536-argument limit reported for Safari's engine, which was also not tested.

### Suggested fix

The wasm side should return the rows as structured data, or the playground should at least parse a table only when the request asks for more than one shot. The maximum should be computed with a loop or `reduce` instead of spread, and the number of parsed rows should be capped. Adding an error boundary around the result panel would ensure that a render error leaves the editor usable.

## M-07: Playground processes large results on main thread outside the 30 s worker limit

- **Severity:** Low
- **Confidence:** Medium
- **Status:** Suspected (sizes and timings measured in Node; no browser was used)
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `site/src/pages/Playground.tsx:472-496` (diagnostic buttons and `<pre>` blocks), `site/src/playground/worker.ts:45-46`, `crates/qxide-wasm/src/lib.rs:119-122` (no size limit on `stdout`/`stderr`)
- **Relation to the baseline:** `extends B-02`

The playground's 30 s limit and Stop button terminate the worker, but everything after the worker replies runs on the page's main thread with no size limit, including the structured clone of the result, `parseCounts` and `parseDiagnostics`, one button per diagnostic, and a single `<pre>` holding the entire stdout or stderr. Programs can produce hundreds of megabytes well inside the time limit: a 169-byte program produced 202 MB of stdout in about 1 s, and a 16 KB program produced 193 MB of stderr in about 2 s through M-01.

### Reproduction

`bigout.qx` (the string literal is 100 `x` characters), shots = 1:

```text
void main() {
    for i in 0..2000000 {
        println("xxxxxxxxxx...x");
    }
}
```

```bash
node --stack-size=984 target/mythos-repro/play_test.mjs bigout.qx 1
node target/mythos-repro/wasm_run.mjs check oneline_8000.qx
```

### Observed result

```text
== bigout shots=1
worker side (wasm + decode + JSON.parse): 1034 ms; exit 0; stdout 202000000 chars; stderr 0 chars; source 169 chars
structuredClone (postMessage to the page): 64 ms
page side parseCounts + parseDiagnostics: 104 ms; rows null

done in 2078 ms; result JSON 192978500 bytes; wasm memory 735 MiB; source 16014 bytes
```

### Impact

Actor A2 can trigger this behavior across trust boundary TB3 with one click on Run or Check. Although the baseline noted for B-02 and B-03 that a frozen tab was not observed, this path is the likely way to one because the main thread must lay out a text node of hundreds of megabytes and the worker timeout no longer applies. Only the worker and page-side JavaScript steps were measured in Node, and rendering in a browser was not tested, so the freeze itself remains suspected.

### Suggested fix

Stdout and stderr should be capped in the wasm binding, for example at 1 MiB each with a line indicating that the output was truncated, so the limit holds before the data leaves the worker. The playground should also cap the number of diagnostics shown as buttons. In addition, a wasm maximum memory should be set as suggested for B-02.

## M-08: Mismatched manifest and index dependencies leave dependents permanently out of date (Q0612)

- **Severity:** Low
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/project.rs:67-80` (`installed_deps` reads the unpacked manifest), `src/pkg/project.rs:122` and `src/pkg/resolve.rs:468-479` (freshness check), `src/pkg/cache.rs:91-132` (`install` does not compare), `registry-template/scripts/validate.py:366` (name and version only)
- **Relation to the baseline:** `new`

The dependency list of a registry package exists twice: in the index line, which is used by the resolver and written to `qxide.lock`, and in the `qxide.toml` inside the archive, which `installed_deps` uses for the freshness check of `qxide run`, `check`, and `qasm`. Nothing requires them to agree, because `validate.py` compares only the manifest's name and version with the index line and the client does not compare them at install time. When they differ, `resolve::is_fresh` fails for every project that depends on the package.

### Reproduction

Local directory registry with packages `other` 1.0.0 and `mismatch` 1.0.0. The index line of `mismatch` has `"deps": []`; the `qxide.toml` inside its archive has:

```text
[dependencies]
other = "1"
```

Project `p-mismatch`, `src/main.qx`:

```text
import mismatch.f;
void main() {
    Qubit q = qubit();
    f(q);
    discard(q);
}
```

```bash
qxide add mismatch --registry target/mythos-repro/reg
qxide check src/main.qx
qxide install --registry target/mythos-repro/reg
qxide check src/main.qx
```

### Observed result

```text
[add_mismatch] exited; exit=0
       Added mismatch v1.0.0
      Locked 1 packages
  Downloaded mismatch v1.0.0
[mismatch_check] exited; exit=1
error[Q0612]: `qxide.lock` is out of date with `qxide.toml`
help: run `qxide install` to update `qxide.lock`
[mismatch_install] exited; exit=0
[mismatch_check2] exited; exit=1
error[Q0612]: `qxide.lock` is out of date with `qxide.toml`
help: run `qxide install` to update `qxide.lock`
```

### Impact

Actor A3 or a publisher's mistake can introduce this mismatch across trust boundary TB4. Every project that depends on such a version can install it, but `qxide run`, `check`, and `qasm` then always fail with Q0612 and instruct the user to run `qxide install`, which changes nothing. The effect is a persistent failure for dependents until the package is removed or yanked, while nothing is executed or disclosed.

### Suggested fix

In `validate.py`, the `[dependencies]` names and requirements from the archive's `qxide.toml` should be compared with `deps` in the index line. In the client, `Home::install` should compare them once and reject the archive with Q0610 on a mismatch, or the freshness check should use only the index data already recorded in `qxide.lock`.

## M-09: Client and `validate.py` parse archives differently (gzip members, entry sizes)

- **Severity:** Info
- **Confidence:** High for gzip members, Low for entry sizes
- **Status:** Reproduced (gzip members); Suspected (entry size, read only)
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/archive.rs:235` (`GzDecoder`), `src/pkg/archive.rs:271-280` (`entry.header().size()` with `take(size)`), `registry-template/scripts/validate.py:314` (`tarfile.open(..., mode="r:gz")`)
- **Relation to the baseline:** `new`

The client and the registry CI do not parse archives the same way. The client uses `flate2::read::GzDecoder`, which stops after the first gzip member, whereas Python's `tarfile` reads all members, so an archive made of two gzip members shows three files to `validate.py` while installing two. Separately, `unpack` limits each file by the size field of the tar header rather than the entry's effective size, so a PAX `size` record larger than the header value would make the client keep a prefix of a file that Python or GNU tar extracts in full, though this second case was read in the code and not tested.

### Reproduction

Archive `twogz-1.0.0.tar.gz` = `gzip(tar without end-of-archive blocks: qxide.toml, src/lib.qx)` followed by `gzip(tar: src/second.qx)` (generator: `target/mythos-repro/build_reg.py`, section 7).

```bash
python -I -c "import tarfile,sys; print([m.name for m in tarfile.open(sys.argv[1], 'r:gz')])" twogz-1.0.0.tar.gz
qxide add twogz --registry target/mythos-repro/reg   # then list the unpacked package under $QXIDE_HOME/registry/src
```

### Observed result

```text
python tarfile sees: ['twogz-1.0.0/qxide.toml', 'twogz-1.0.0/src/lib.qx', 'twogz-1.0.0/src/second.qx']

[add_twogz] exited; exit=0
local-a03d835cbd8b5f5c\twogz-1.0.0\.qxide-checksum
local-a03d835cbd8b5f5c\twogz-1.0.0\qxide.toml
local-a03d835cbd8b5f5c\twogz-1.0.0\src\lib.qx
```

### Impact

Actor A3 can trigger this discrepancy across trust boundary TB5 so that the file list in the CI job summary that reviewers read differs from what clients install. In the reproduced case, the client installs fewer files than CI lists, which is harmless by itself. Because the client applies its own path and size rules to whatever it unpacks, no check is bypassed. It matters because review of package contents is the main control in this registry design, and it will matter more once packages can use `std` modules in apps.

### Suggested fix

Both sides should enforce one canonical archive form. In `validate.py`, archives with more than one gzip member, trailing bytes, or PAX and GNU extension headers should be rejected, while the client should fail if input remains after the first gzip member and should use each entry's effective size. This matches `qxide publish`, which already writes a single member with plain headers.

## M-10: Conditional nesting across calls: 297 MB `qxide qasm` output and playground wasm stack overflow

- **Severity:** Low
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/lib.rs:720-739` (`write_qasm`, `"  ".repeat(depth)` per call), `src/lib.rs:583-596` (`branch`), `src/lang/interp.rs:250-266` (both branches elaborated through closures), limits at `src/lang/parser.rs:9` and `src/lang/interp.rs:13`
- **Relation to the baseline:** `extends B-02`

An `if` on a measured `Bit` records a nested conditional block, and a function called inside such an `if` may contain further ones, so conditional nesting in the circuit can reach roughly the call-depth limit (200) times the per-function nesting limit (about 61 `if` levels fit under `MAX_DEPTH = 128`). Because `write_qasm` indents every line by two spaces per level, output size grows with depth times lines, while elaboration, `first_non_clifford`, the simulators, and dropping the nested operation list all recurse once per level.

### Reproduction

`nest_C_P.qx`, where `P` is the number of nested `if(b){` and `C` the recursion count (shown for `P` = 2, `C` = 198; the tests used `P` = 61 or 60):

```text
void f(Qubit q, Bit b, int n){if(b){if(b){if(n>0){f(q,b,n-1);}x(q);}}}
void main(){
 Qubit a=qubit(); h(a); Bit b=measure(a);
 Qubit q=qubit();
 f(q,b,198);
 Bit r=measure(q);
 println("{}", r);
}
```

```bash
python -I target/mythos-repro/gen_nest.py 198 61 nest_198_61.qx
qxide qasm nest_198_61.qx > NUL
python -I target/mythos-repro/gen_nest.py 100 60 nest_100_60.qx
node target/mythos-repro/wasm_run.mjs run nest_100_60.qx
```

### Observed result

```text
[nest_198_61.check] exited; exit=0; wall=0.22s; stdout=94 B        (input 610 B)
[nest_198_61.run] exited; exit=0; wall=0.22s; stdout=2 B
[nest_198_61.qasm] exited; exit=0; wall=0.43s; peak working set=290 MB; stdout=297382936 B
[nest_100_60.qasm] exited; exit=0; wall=0.21s; stdout=74187450 B   (input 603 B)

trap after 4 ms: RangeError: Maximum call stack size exceeded; wasm memory 16 MiB; source 603 bytes     (wasm, qasm)
trap after 4 ms: RangeError: Maximum call stack size exceeded; wasm memory 16 MiB; source 603 bytes     (wasm, run)
done in 5 ms; result JSON 37 bytes; wasm memory 16 MiB; source 462 bytes                                (wasm, run, 40 calls x 40 levels)
```

### Impact

Actor A1 or A2 can trigger this issue across trust boundary TB1. A 610-byte program makes `qxide qasm` print 297,382,936 bytes (peak working set 290 MB) in 0.4 s without any large loop bound, so a budget counting loop iterations or interpreter steps would not stop it and only an output cap would. In the playground, a 603-byte program with 100 calls of 60 levels each ends in a call-stack `RangeError` after 4 ms in Node, which the worker reports as a stack trap; the browser case is handled, but the documented limits do not prevent it. The CLI's 64 MiB stack survived the deepest case that the limits allow.

### Suggested fix

The interpreter should track the conditional nesting level across calls and report a diagnostic above a fixed limit, such as 128 levels in total. The size of the QASM text should also be capped as part of the output cap proposed for B-02. Writing nested blocks iteratively should also be considered.

## M-11: Duplicate-version scan of an index file runs in quadratic time

- **Severity:** Info
- **Confidence:** High
- **Status:** Reproduced
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `src/pkg/index.rs:282` (`out.iter().any(|o| o.vers == e.vers)` per line)
- **Relation to the baseline:** `new`

`Registry::entries` checks every index line against all earlier lines for a duplicate version, which is quadratic in the number of lines. A directory-registry index file with 80,000 versions (11.6 MB) took 5.6 s to read in `qxide add`, while 40,000 lines took 0.9 s. Over https, the 10 MiB download limit keeps a single file below about 70,000 such lines. Directory registries and `file://` downloads have no size limit at all (see M-03).

### Reproduction

Index files `many20000`, `many40000`, `many80000` in the local directory registry, each line a yanked version `0.<i>.0` (generator: `target/mythos-repro/build_reg.py`, section 6).

```bash
qxide add many80000 --registry target/mythos-repro/reg
```

### Observed result

```text
many20000 index bytes: 2888890     [add_many20000] exited; exit=1; wall=0.23s
many40000 index bytes: 5788890     [add_many40000] exited; exit=1; wall=0.87s
many80000 index bytes: 11588890    [add_many80000] exited; exit=1; wall=5.64s
error[Q0605]: no version of `many80000` matches `*`
```

### Impact

Actor A4 or the operator of a third-party registry can trigger this cost across trust boundary TB4. The impact is a few seconds of CPU per package lookup, bounded by the download limit for https registries. This finding is listed for completeness, as it would only matter together with a resolver that fetches many packages.

### Suggested fix

Seen versions should be tracked in a hash set. In addition, the 10 MiB limit should be applied to index files read from directories as well.

## M-12: Branch-pinned PyPI action and cargo-dist installer piped into `sh` without a digest

- **Severity:** Low
- **Confidence:** Medium
- **Status:** Suspected (read only)
- **Affected branch and commit:** `main` (`b8c385e`)
- **Location:** `.github/workflows/pypi.yml:129`, `.github/workflows/release.yml:67`, `.github/workflows/ci.yml:62`
- **Relation to the baseline:** `extends B-S3`

This finding is based on reading only and was not executed. In addition to the tag-pinned actions listed in B-S3, the job in `.github/workflows/pypi.yml` that holds `secrets.PYPI_API_TOKEN` runs `pypa/gh-action-pypi-publish@release/v1`, a branch reference that can move. Line 67 of `.github/workflows/release.yml` pipes the cargo-dist installer script for version 0.34.0 from a GitHub release into `sh` without a digest check in a workflow with `contents: write`. `.github/workflows/ci.yml` also installs `qiskit-qasm3-import` without a version pin.

### Reproduction

Not executed; these are lines of workflow configuration:

```text
.github/workflows/pypi.yml:129      - uses: pypa/gh-action-pypi-publish@release/v1
.github/workflows/pypi.yml:131          password: ${{ secrets.PYPI_API_TOKEN }}
.github/workflows/release.yml:67    run: "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.34.0/cargo-dist-installer.sh | sh"
.github/workflows/ci.yml:62         - run: pip install "qiskit==2.6.2" qiskit-qasm3-import
```

### Observed result

```text
(none: read only)
```

### Impact

This issue involves actor A6 in the release pipeline across trust boundary TB8. Whoever can move the `release/v1` branch of the publishing action, or replace the cargo-dist 0.34.0 release asset, runs code in a job that can upload to PyPI or write to the repository's releases. Although this is the same class as B-S3, the point added here is that the action nearest to the secret has the weakest pin.

### Suggested fix

Every action should be pinned to a full commit SHA with the version in a comment, letting Dependabot update the pins. PyPI publishing should switch to Trusted Publishing as suggested for B-S2, which removes the long-lived token. Artifact attestations should be enabled in `dist-workspace.toml` so release binaries and installers carry provenance. Finally, `qiskit-qasm3-import` should be pinned.

## M-13: App runtime and Qxide Studio run with CLI limits (`app-framework` branch)

- **Severity:** Low
- **Confidence:** Medium
- **Status:** Suspected (read with `git show`; the branch was not built or run)
- **Affected branch and commit:** `app-framework` (`eea71a6`)
- **Location:** `crates/qxide-app-runtime/src/main.rs:302-319` (thread spawned, then slot acquired), `crates/qxide-app-runtime/src/main.rs:366` (`limits: lang::Limits::CLI`), `src/lang/studio.rs:100-106` (`run_diag(..., Limits::CLI, ...)`), `src/lang/command.rs:15` (`MAX_SHOTS = 1_000_000`), `crates/qxide-app-runtime/src/main.rs:169-176` (`settle` builds one script string)
- **Relation to the baseline:** `extends B-A7` (and carries B-02, B-03, M-01, M-10 into the app runtime)

This finding is based on reading only on the `app-framework` branch, which was not built or run. App command calls and `std.qxide.run` in Qxide Studio use `Limits::CLI`, the limits of the command-line tool: up to 1 GiB per state vector or stabilizer tableau and at most 1,000,000 shots per call. Up to one call per CPU core runs at once, each further call already holds a thread with a 64 MiB stack reservation while waiting for a slot, and the `commands` request does not use slots (B-A7). The diagnostics and QASM text of M-01 and M-10 are returned through `std.qxide.check`, `run`, and `qasm` and then passed to the page via `evaluate_script` as one script string.

### Reproduction

Not executed.

### Observed result

```text
(none: read only)
```

### Impact

Actors A1 (a program typed or pasted into Studio) and A8 (script running in an app's page) can trigger this issue across trust boundaries TB1 and TB6. Page script can start as many calls as the machine has cores, each allowed 1 GiB, so the app can request many gigabytes at once, and the output amplification findings on `main` (M-01, M-10, B-02, and B-03 with ten times the playground's shot limit) apply to Studio under CLI limits rather than playground limits. Nothing here crosses the origin check or the permission checks, which held in reading.

### Suggested fix

The app runtime should be given its own `Limits` closer to the playground's, together with a total memory budget across concurrent calls, slot acquisition before spawning the worker thread, and a size cap on any string handed to `evaluate_script`. M-01 and M-10 should be fixed before the branch is merged so Studio inherits those caps.

## Areas reviewed with no findings

- Lexer and parser: byte offsets always advance by whole characters, non-ASCII outside strings is rejected with Q0101, and every recursive parser path counts against `MAX_DEPTH`.
- Checker and interpreter agreement: built-in calls index their arguments only after the checker's arity check, and integer arithmetic is checked. Qubit use after measurement, aliasing through registers, and measurement inside conditional blocks end in diagnostics, and the library's assertions for these cases were not reachable in hand-written tests or in the smoke run.
- Simulators: state-vector allocation is gated by `check_memory` and tableau allocation by `stabilizer_bytes`, both computed in `u128`. The slot count reported by the interpreter matches the circuit's, and no index arithmetic depends on untrusted values beyond those limits.
- `qxide fmt` and the REPL: `fmt` writes only when the formatted text lexes to the same tokens and comments, and REPL offset mapping stays on character boundaries.
- Package client: lock and index names are validated before becoming cache paths; the lock checksum takes precedence and a disagreeing index is refused before download; and cache entries are keyed by registry id, name, and version and trusted only with a matching marker. `curl` always receives an `https://` URL as a separate argument with `--proto =https`; archive paths, entry types, case collisions, and sizes are checked before anything is written; and a manifest with 100,000 nested arrays ends in Q0602.
- Playground: share links are size-limited, validated, and never auto-run, and all output is rendered as text. The worker creates a fresh instance per run, the wasm ABI is called only by same-origin code, and the middleware redirect cannot be steered to another host.
- Registry and release workflows: `validate.yml` runs with a read-only token and executes the base branch's script, `search.yml` runs only code from `main`, and `pypi.yml` filters `workflow_run` to successful tag pushes.
- App runtime (by reading): IPC is accepted only from the app origin with a per-kind key allow-list, commands must be listed and public, and values sent to the page are escaped for script context including U+2028 and U+2029. The bundle parser uses checked offsets; UI paths reject `..`, backslashes, and colons; and `apps/studio/ui/studio.js` has no HTML-injection sink.

Hand-written boundary programs (`target/mythos-repro/hand.py`: measurement and allocation reached through helpers inside an `if` on a `Bit`, use after measurement, aliasing through dynamic indices and calls, out-of-range and negative indices and sizes, `int` minimum, NaN angles and casts, BOM with CRLF and tabs, NUL byte, multi-byte columns), each run through `check`, `qasm`, `run`, `run --shots 3` and `fmt --check`:

```text
27 programs x 5 commands, 0 unexpected exit codes
```

Not covered: `src/gpu.rs` (feature `gpu`, off by default), `src/uncompute.rs` and `src/groups.rs` (not reachable from `.qx`), `src/lang/explain.rs` and the message tables, `site/` pages other than the playground, the generated cargo-dist installers (not in the repository), the live registry repository, a real browser, and any execution of the `app-framework` branch. The cargo-fuzz targets were not run (nightly is not installed).

## Recommended next steps

(1) Address M-01 together with the execution budget already proposed for B-01, B-02, B-03, B-08, and B-09, adding caps for diagnostics, output bytes, and conditional nesting (M-07, M-10) in the shared `Limits`. (2) Update the package client for M-02 and M-03, plus M-04 via one sanitising print helper, each on its own branch with the reproduction input as a regression test. (3) Fix the registry template for B-P1, B-P2, B-P3, M-08, and M-09, and then sync the live registry scripts. (4) Before merging `app-framework`, resolve B-A1, B-A2, B-A3, and M-13. The fuzz targets under `fuzz/` have still never run under cargo-fuzz, and running the parser, checker, and archive targets on a machine with nightly installed would be the next source of findings.
