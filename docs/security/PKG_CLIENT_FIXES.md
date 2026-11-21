# Package Client Hardening (M-02, M-03, M-04)

- **Date:** 2026-11-21
- **Branch:** `fix/pkg-client-hardening`, based on `main` (`b8c385e`, `qxide` 0.7.0)
- **Source:** recommendation (2) of `docs/security/MYTHOS_FINDINGS.md` on `security-review-prep` (`7b1f855`): fix M-02 and M-03 in the package client, and M-04 through one sanitising print helper. That report is not changed here; finding IDs below refer to it.
- **Commits:** `98a5fe4` (M-02), `b9f41ec` (M-03), `2e4de98` (M-04), then this record.
- **Method:** each reproduction input from the report became a regression test first. Every test was run against the unfixed code and failed, then passed after the fix. Tests that need the new code (marked *new API*) were added after the fix and have no "before" run.

## Summary

| ID | Change | Files |
| --- | --- | --- |
| M-02 | Tar entries are read raw: GNU long names are read by the client with a 4 KiB limit, and PAX, GNU long-link, and sparse headers are rejected as non-regular entries before their data is read. The decompressed stream is capped at `MAX_UNPACKED + 16 MiB` (66 MiB). | `src/pkg/archive.rs` |
| M-03 | Every expanded download location (index `dl` or the `config.json` template) must pass `index::valid_dl`, a port of `valid_dl` in `registry-template/scripts/validate.py`: an `https://` URL or a relative path of plain, portable segments. Anything else fails with Q0609 before a file is opened. `file://` is no longer accepted for `dl` with any registry. Archives in a directory registry are read only if they are regular files, and at most `MAX_ARCHIVE + 1` bytes are read. | `src/pkg/index.rs`, `src/pkg/msg.rs` |
| M-04 | One function, `lang::printable` (in `src/lang/diag.rs`), replaces every control character except tab (C0, DEL, and C1 such as U+009B) with a `\u{..}` escape. It is applied to the message, notes, and help of package errors (`pkg::Error::render`, which covers the Q0609 reason and Q0610 path), to `qxide search` rows, and to `.qx` diagnostics (message, notes, help, and the echoed source line, with caret padding computed from the escaped text). | `src/lang/diag.rs`, `src/lang/mod.rs`, `src/pkg/mod.rs`, `src/pkg/cli.rs` |

## M-02: tar extension headers and decompressed size

`unpack` used the `tar` crate's default iterator, which reads the data of GNU long-name and PAX extension headers into memory with no limit before returning the next entry. The iterator now runs in raw mode, so extension headers reach `unpack` as entries:

- GNU long name (`L`): accepted only up to `MAX_LONG_NAME` (4096) bytes, then applied to the next entry, as the crate did. A second long name before an entry, or invalid UTF-8, is rejected with Q0610. `qxide publish` writes such entries for paths over 100 bytes, so they must keep working (`long_package_paths_round_trip`).
- PAX local and global headers (`x`, `g`), GNU long link (`K`), and other non-regular types: rejected by the existing entry checks before any data is read. `qxide publish` never writes them.
- `Capped` wraps the gzip decoder and fails once more than `MAX_TAR` (66 MiB) bytes have been decompressed, so data the client skips (for example a directory entry's) cannot be inflated without bound. That failure is reported as the existing "archive exceeds size limit" reason.

| Test | Input | Before fix | After fix |
| --- | --- | --- | --- |
| `pkg::tests::long_name_entries_are_not_read_whole` | The report's M-02 input: a GNU long-name entry declaring 1 GiB of zero bytes, then an empty regular entry, gzip level 9 (same tar stream; the gzip bytes differ from the report's because the compressor differs) | Killed by the memory cap: `killed: memory cap 4000 MB; wall=3.38s; peak working set=4346 MB` | Passes: Q0610, short message; `wall=1.10s; peak working set=8 MB` |
| `pkg::tests::pax_headers_are_rejected_unread` | A PAX header holding one 1 GiB `comment` record, then a valid `qxide.toml` (the report lists PAX as Suspected and gives no input) | Fails: `unpack` returned `Ok` after reading the record; `peak working set=1808 MB` | Passes: Q0610; `peak working set=8 MB` |
| `pkg::tests::decompressed_size_is_capped` | A directory entry carrying 1 GiB of zero data, then a valid `qxide.toml` | Fails: `unpack` returned `Ok` after inflating 1 GiB | Passes: Q0610 |
| `pkg::tests::long_package_paths_round_trip` | Control: a package with a 139-byte archive path, packed by `pack` (asserts the archive contains a long-name entry) | Passes | Passes |

Memory figures come from running the test binary alone under a wrapper that samples the working set every 100 ms and kills the process above the cap, as in the review. Each "after" figure includes generating the 1 GiB stream in the test itself.

## M-03: download locations

`Registry::download` read `file://` URLs from any path for every registry kind, and joined a scheme-less `dl` onto a directory registry's path, where an absolute path, `..`, or backslashes left the registry. `dl_url` now validates the expanded location with `valid_dl`, and `download` handles only `https://` (fetched with `curl` as before) and relative paths (read from the directory registry, or fetched under the https registry root).

`dl` with `file://` is rejected outright rather than confined to a directory registry: `validate.py` already rejects it, no test or documented workflow used it, and a relative `dl` covers the directory-registry case. The registry URL itself may still be a `file://` URL or a path.

Regression test `download_locations_stay_inside_the_registry` (`tests/packages.rs`) runs `qxide add` against a directory registry whose only index line carries each `dl` below. The archive outside the registry is a valid package with the checksum in the index line, so a redirected read installs it.

| `dl` | Before fix | After fix |
| --- | --- | --- |
| `file:///C:/Windows/win.ini` (report input) | exit 1, Q0607 (file read and hashed) | exit 1, Q0609 |
| `file:////localhost/C$/Windows/win.ini` (report input) | exit 1, Q0607 (read over the loopback admin share) | exit 1, Q0609 |
| `file:///<absolute path of the outside archive>` | exit 0 (installed) | exit 1, Q0609 |
| `<absolute path of the outside archive>` | exit 0 (installed) | exit 1, Q0609 |
| `../elsewhere/packages/.../outside-1.0.0.tar.gz` | exit 0 (installed) | exit 1, Q0609 |
| the same with backslashes | exit 0 (installed) | exit 1, Q0609 |
| `packages/../../elsewhere/...` | exit 0 (installed) | exit 1, Q0609 |
| `config.json` template `../elsewhere/packages/{prefix}/{name}/{name}-{version}.tar.gz` | exit 0 (installed) | exit 1, Q0609 |
| control: relative `dl` inside the registry | exit 0 | exit 0 |

The per-case "before" results were recorded with the unfixed debug binary and a copy of the test registry; the test itself stops at the first failing case. `pkg::tests::download_locations_follow_the_registry_checker` (*new API*) checks `valid_dl` on accepted and rejected forms, including an `https` port, `http`, UNC, drive letters, empty, `.`, and `..` segments, reserved device names, and spaces.

The new error text `msg::invalid_dl` was written by hand instead of through the usual Gemini workflow, because this change was kept off external services until it is fixed.

## M-04: terminal control characters

| Test | Report input | Before fix | After fix |
| --- | --- | --- | --- |
| `registry_text_is_printed_without_terminal_escapes` (`tests/packages.rs`) | `search.json` row with the injected text in `name`, `version`, and `description`; index line `badline` whose `name` holds it | Fails: raw ESC/BEL on stdout | Passes |
| `pkg::tests::package_errors_do_not_echo_terminal_escapes` | Archive member `escpath-1.0.0/src/a<inj>.qx` (Q0610), plus a C1 CSI and CR in a message, note, and help | Fails | Passes |
| `diagnostics_do_not_echo_terminal_escapes` (`tests/cli.rs`) | `esc.qx` with escapes in a comment after `undefined_name;` | Fails: raw ESC/BEL on stderr | Passes; the carets still sit under `undefined_name` |
| `lang::diag::tests::rendered_diagnostics_hide_terminal_escapes` | Control characters (ESC, BEL, CSI) and a tab before the error column, and in the message | Fails | Passes; the caret is under the same column as in the escaped line |

Output after the fix:

```text
2 |     undefined_name; // \u{1b}]0;TITLE-SET-BY-QX-FILE\u{7}\u{1b}[2K\u{1b}[1A\u{1b}[2K
  |     ^^^^^^^^^^^^^^
good\u{1b}]0;TITLE-SET-BY-REGISTRY\u{7}\u{1b}[2K = "1.0.0\u{1b}]0;TITLE-SET-BY-REGISTRY\u{7}\u{1b}[2K" # plain \u{1b}]0;TITLE-SET-BY-REGISTRY\u{7}\u{1b}[2K
error[Q0609]: invalid index entry for `badline` on line 1: invalid package name `x\u{1b}]0;TITLE-SET-BY-REGISTRY\u{7}\u{1b}[2K` (...)
```

Because `src/lang` is also compiled into `crates/qxide-wasm`, the playground shows the same escaped text for diagnostics.

## Verification

Run on Windows 11 with rustc 1.99.1, `-j 8`, at the tip of this branch:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo clippy -p qxide-wasm --target wasm32-unknown-unknown -- -D warnings` | clean |
| `cargo test --workspace` | all passed (binary unit tests 114, `cli` 22, `packages` 15, `qxide-wasm` 95 + 11, others unchanged) |
| `python -I ci/test_registry_scripts.py` | 16 tests OK (registry template not changed) |

Not run: the playground parity scripts (`site/scripts/build-wasm.mjs`, `parity.mjs`), the `gpu` feature clippy job, and the Qiskit QASM job. Intermediate commits were not built separately; only the branch tip was.

## Not done here

- Registry template, recommendation (3): the same limits in `validate.py` (extension headers, decompressed size), rejecting control characters in `description`, and M-09. The client now refuses PAX and GNU long-link headers that `validate.py` still accepts, so CI can pass an archive the client will not install until the template matches.
- M-04 items beyond the print helper: rejecting control characters in `Entry::parse`, validating `name` and `version` of `search.json` rows like index lines, and filtering program output when stdout is a terminal.
- Documentation of the new behaviour: the `dl` rule in `docs/design/qx-packages.md` ("absolute or registry-relative download URL") and a CHANGELOG entry.
- Related observation, not fixed: `qxide publish` into a directory registry still writes the archive to the unvalidated `config.json` template path (`cli::write_into_registry`). `valid_dl` can be applied there the same way.
- A directory registry may contain symbolic links. Downloads now read only regular files within the size limit, but a link to a regular file elsewhere is still followed.
