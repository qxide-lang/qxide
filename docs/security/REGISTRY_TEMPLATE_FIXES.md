# Registry Template Hardening (B-P1, B-P2, B-P3, M-08, M-09)

- **Date:** 2026-11-21
- **Branch:** `fix/registry-template-hardening`, based on `main` (`b8c385e`, `qxide` 0.7.0)
- **Source:** recommendation (3) of `docs/security/MYTHOS_FINDINGS.md` on `security-review-prep` (`7b1f855`): fix the registry template for B-P1, B-P2, B-P3, M-08, and M-09. B-P1 to B-P4 are described in `docs/security/BASELINE_FINDINGS.md` on the same branch. Neither report is changed here; IDs refer to them.
- **Also requested:** the archive limits of the hardened package client (`fix/pkg-client-hardening`, `98a5fe4`, read only and not merged) in `validate.py`, and rejecting control characters in `description` (the `validate.py` half of B-P4 / M-04).
- **Scope:** `registry-template/` and its tests in this repository only. Nothing was pushed to `qxide-lang/registry` or any other remote.
- **Commits:** `1e3e4f9` (fixes and tests), `3fd419e` (this record and the documentation updates), then a follow-up that checks out the merge result instead of the pull request head (see B-P1 below).
- **Method:** every reproduction input from the reports became a regression test in `ci/test_registry_scripts.py` first. The new test file was run against the unfixed template (a copy of `b8c385e`'s `registry-template/` next to the new tests) and the listed cases failed; after the fix all tests pass. The new error strings, this record, and the commit messages were written by hand: no part of this work was sent to an external text service.

## Summary

| ID | Change | Files |
| --- | --- | --- |
| B-P1 | `validate.yml` uses `pull_request_target` instead of `pull_request`, so pull requests always run the target branch's workflow and `scripts/validate.py`. The base branch is checked out to `base/`, GitHub's merge result for the pull request (`refs/pull/<number>/merge`, which a `pull_request` run also used) to `pr/`, both with `persist-credentials: false`; the only command run is `python -I base/scripts/validate.py --root pr --base origin/<target>`. The fallback that ran the pull request's own script when the target branch had none is removed. Token stays `contents: read`. | `registry-template/.github/workflows/validate.yml` |
| B-P2 | With `--base`, every index file of the base (`git ls-tree -r -z --name-only <base> -- index/`, run with `check=True`, so a bad ref fails) must still exist; a missing one is an error. | `registry-template/scripts/validate.py` |
| B-P3 | Every version's archive must be in the repository and pass the archive checks. For an `https://` location (entry `dl` or the `config.json` template) the copy is expected at the template path, or at the default `packages/{prefix}/{name}/{name}-{version}.tar.gz` when the template itself is a URL. Clients verify `cksum`, so the bytes they install from any URL are the bytes CI checked. | `validate.py` |
| M-08 | The `[dependencies]` of the archive's `qxide.toml` (string or `{ version = ... }` form, as `installed_deps` in the client reads them) must equal the entry's `deps` as a multiset of (name, requirement). Requirements are compared as `semver::VersionReq` values: `parse_req` ports the crate's comparator parser and returns `(op, major, minor, patch, pre)` tuples, so `1` equals `^1` and `>= 1.2,<2` equals `>=1.2, <2`, but `1.0` does not equal `1`. Path-only, non-string, and invalid requirements, a non-table `dependencies`, and duplicate names in `deps` are rejected. | `validate.py`, `tests/data/semver_cases.json` (`req_pairs`), `src/pkg/tests.rs` |
| M-09 and archive limits | `tarfile` is replaced by a reader that follows the client (`tar` 0.4.46 in raw mode, as on `fix/pkg-client-hardening`). See below. | `validate.py` |
| description | `description` may not contain any character of Unicode category `Cc` (C0 including tab and newline, DEL, C1). | `validate.py` |

### Archive reading (M-09, client limits)

- **gzip:** `zlib.decompressobj(wbits=31)` with `max_length = MAX_TAR + 1`. Exactly one member is read; bytes after it (a second member or anything else) are rejected, as is a truncated stream. Output over `MAX_TAR` = `MAX_UNPACKED + 16 MiB` (66 MiB, the client's value) is rejected without reading further.
- **tar headers**, read the way the client reads them in raw mode:
  - Types `0` and NUL are regular files and `5` is a directory. `L` (GNU long name) of at most `MAX_LONG_NAME` = 4096 bytes names the next entry, with trailing NULs removed. Every other type, including `x` and `g` (PAX), `K` (GNU long link), `S` (sparse), and `7` (contiguous, which Python's `tarfile` treats as a regular file), is rejected.
  - Header checksum computed as the `tar` crate does (unsigned sum). Size and checksum fields must be plain octal digits: base-256 and other forms are rejected.
  - The path is the 100-byte name field, prefixed by the 155-byte prefix field only for ustar headers (`ustar\0` + `00`), as the client does. A non-ustar header with any data in the prefix area is rejected, because Python's `tarfile` would read it as a prefix.
  - Names are split on `/`. The first segment must be `<name>-<version>`, and no segment may be empty, `.`, or `..`. One trailing `/` is allowed on directories only. The old check used `PurePosixPath`, which removes `./` and `//` that the client does not.
  - Rejected outright: truncated data or padding, two long names in a row, a long name with no entry after it, and non-zero bytes after the first all-zero block.
- **Limits:** the file count includes directories but not `L` entries (> 2000 fails). The unpacked total is checked as each file is read (> 50 MiB fails at once rather than after the loop). The compressed limit stays at 10 MiB. These are the client's boundaries, all strict `>`.
- A `qxide.toml` that is not UTF-8 is now an error rather than an uncaught exception.

This makes `validate.py` stricter than both 0.7.0 on `main` and the hardened client in a few places (trailing bytes, data after the end block, a dangling long name, truncated padding, non-canonical numbers). `qxide publish` writes none of these, and the committed fixture below confirms that its output passes.

## Regression tests

"Before" is the new test file run against the unfixed template (`python -I ci/test_registry_scripts.py`: 29 tests, `failures=35, errors=20`, all in the tests below; the earlier 16 tests passed). "After" is this branch: 30 tests (with `test_branch_behind_the_target`, added later), OK.

| Test | Input | Before fix | After fix |
| --- | --- | --- | --- |
| `ValidateWorkflow.test_pull_requests_run_the_target_branch_workflow` | `validate.yml` triggers | Fails: `pull_request` trigger, no `pull_request_target` | Passes |
| `ValidateWorkflow.test_checker_comes_from_the_target_branch` | every `validate.py` invocation | Fails: runs `$RUNNER_TEMP/validate.py`, falls back to `cp scripts/validate.py` | Passes: only `python -I base/scripts/validate.py`, `--root pr` |
| `ValidateWorkflow.test_pull_request_is_data_only` | permissions, checkouts, `run:` lines | Fails: one checkout, credentials kept | Passes: `contents: read` only, two checkouts with `persist-credentials: false`, the merge ref into `pr`, no `${{ }}` in `run:` |
| `ValidatePullRequests.test_branch_behind_the_target` | A pull request branched before another package was added to `main`, validated as its merge result | Passes (control) | Passes. Validating the pull request head instead fails with `index/ot/he/other: index file was removed`, which is why the workflow checks out the merge ref (found in review of the first version of this branch, which used the head SHA) |
| `ValidatePullRequests.test_deleting_an_index_file` | B-P2 report input: PR runs `git rm index/ev/il/evil` | Fails: exit 0 | Passes: exit 1, `index/ev/il/evil` |
| `ValidatePullRequests.test_readding_a_deleted_version_with_another_archive` | Report follow-up: after the deletion is merged, re-add `evil 1.0.0` with another `cksum` and an external `dl` | Fails: exit 0 | Passes: exit 1 (the published archive's sha256 differs) |
| `ValidatePullRequests.test_unknown_base_fails` | `--base no-such-ref` | Passes (control) | Passes |
| `ValidateEntries.test_external_download_locations_are_checked` | B-P3: https `dl` without an archive; with a good archive; with an archive lacking `src/lib.qx`; the same for an https `config.json` template | Fails: the first case exits 0 | Passes: rejected, accepted, rejected; template: rejected, accepted |
| `ValidateArchives.test_manifest_dependencies_match_the_index` | M-08 report input (`"deps": []`, `qxide.toml` has `other = "1"`), plus other requirement, other name, missing, path-only, non-string version, invalid requirement, non-table, duplicate index dependency; controls `1` vs `^1`, `{ version = "^1", path = ... }`, `>= 1.2,<2` vs `>=1.2, <2` | Fails: all 9 mismatch cases exit 0 | Passes |
| `ValidateEntries.test_requirement_equality_matches_the_client` | 20 `req_pairs` in `tests/data/semver_cases.json` | Errors: no `same_req` | Passes in both directions |
| `pkg::tests::semver_cases_match_the_registry_checker` (Rust) | the same 20 pairs through `VersionReq::parse(a) == VersionReq::parse(b)` | (test extended; the pairs are what `semver` 1.0.28 says) | Passes |
| `ValidateArchives.test_archives_must_use_the_canonical_form` | M-09 report input (two gzip members, the first tar without end blocks), bytes after the gzip member, PAX `size` record larger than the header size, PAX global header, GNU long link before a regular file, contiguous file, long name of 4097 bytes, two long names in a row, long name with no entry, data after the end block, a directory entry carrying 66 MiB + 1 byte placed after the files, leading `./`, empty segment, base-256 size, truncated data | Fails: 13 of 15 cases exit 0 (the dangling long name and truncated data were already rejected by `tarfile`) | Passes: all rejected |
| `ValidateArchives.test_canonical_forms_the_client_reads_pass` | no end block, 4096-byte long name, ustar prefix field, directory entries | Passes (control) | Passes |
| `ValidateArchives.test_archives_written_by_qxide_publish_pass` | `ci/data/published-registry/`: four packages written by `qxide publish --registry` (this branch's debug build), one with a 115-byte path (a GNU `L` entry) and dependencies `"1"`, `">= 1.2, <2"`, `"1.*"` that the index stores as `^1`, `>=1.2, <2`, `1.*` | Passes (control) | Passes |
| `ValidateEntries.test_description_control_characters` | Report input (`plain ESC ]0;TITLE-SET-BY-REGISTRY BEL ESC [2K`), DEL, C1 CSI, LF, CR, tab, NUL; control: non-ASCII text with U+200D | Fails: all 7 exit 0 | Passes; control accepted |

In the "decompressed stream" case the old `validate.py` accepted the archive because `tarfile` does not skip a directory entry's data: it read the zero bytes as the end of the archive. By reading the code (not run against the clients): the hardened client skips the data and fails at 66 MiB, and 0.7.0 on `main` skips it and accepts.

### Changed existing test

`ValidateEntries.test_download_locations` ended with an accept case: an https `dl` with no archive in the repository, exit 0. That is the B-P3 behaviour, so the case now writes the archive and still expects exit 0. No other assertion was removed or weakened.

## Behaviour changes

- **External `dl`:** an entry or template that points at an `https://` URL now also needs the archive in the repository. This keeps per-entry `dl` and moving downloads through `config.json`, but an external location no longer saves repository space. Forbidding external `dl` for new versions would be a stricter policy; it was not chosen here and is a decision for the maintainer.
- **Pull request workflow:** runs as `pull_request_target`. Pull requests from forks now run without the "approve workflows" step, which is acceptable only because nothing from the pull request is executed and the token is read-only.
- Archives in forms `qxide publish` never writes, descriptions with control characters, and manifests whose dependencies differ from the index line are rejected.

## Documentation

`registry-template/README.md` (field table and automated checks) and `docs/design/qx-packages.md` (index fields and "Registry validation and CI") describe the new checks. They were edited by hand for the same reason as the rest of this record.

## Verification

Run on Windows 11 with Python 3.14.10 and rustc 1.99.1 at the tip of this branch:

| Command | Result |
| --- | --- |
| `python -I ci/test_registry_scripts.py` | 30 tests OK, in this worktree and in a clean export of the committed tree (`git archive HEAD`) |
| `cargo fmt --all --check` | clean |
| `cargo clippy -j 8 --workspace --all-targets -- -D warnings` | clean |
| `cargo test -j 8 --workspace` | all passed (binary unit tests 107, `cli` 21, `packages` 13, `qxide-wasm` 94 + 11, others unchanged) |

Not run: the workflow on GitHub Actions (it can only run there; the tests above check its text only, so the checkout of `refs/pull/<number>/merge` under `pull_request_target` was not exercised; for a pull request with merge conflicts GitHub has no merge ref, and the check is then expected to fail at checkout), the `ci.yml` jobs other than the commands listed, and the playground parity scripts. Python 3.12, the version CI uses, was not tried locally; the script uses nothing newer than 3.11.

## Applying this to `qxide-lang/registry`

1. Run the new `validate.py` over the live repository's current contents first (`python -I scripts/validate.py` in a clone). Published lines are immutable, so one existing entry that the new rules reject (an external `dl` without a local copy, a description with control characters, a manifest whose dependencies differ, or an archive in a non-canonical form) would fail every later pull request. Such entries need a maintainer decision before the update.
2. Copy `scripts/validate.py`, `.github/workflows/validate.yml`, and the README changes in one maintainer commit pushed directly to `main` (or merged by an admin). `pull_request_target` only takes effect once the workflow is on the default branch; a pull request that adds it is still checked by the old workflow.
3. Repository settings the template cannot enforce:
   - Branch protection or a ruleset on `main`: require pull requests, at least one approval, and the `index` status check from the `Validate` workflow.
   - A `CODEOWNERS` file covering `/.github/`, `/scripts/`, and `/config.json`, with "Require review from Code Owners". (No owner is named here because the maintainer team is not known in this repository.)

## Not done here

- B-P5 to B-P8 and B-P10 to B-P13 (yank ownership, look-alike names, Markdown in the job summary, the `RecursionError` on deeply nested index JSON in B-P6, and others) are unchanged here; they are handled on `fix/registry-template-hardening-2` (see the second part below).
- Open items are tracked privately.

---

# Part 2: B-P5 to B-P8 and B-P10 to B-P13

- **Date:** 2026-11-21
- **Branch:** `fix/registry-template-hardening-2`, created from the tip of `fix/registry-template-hardening` (`deef8dd`), which is not modified. Neither branch is merged or pushed.
- **Source:** the remaining package-system items of `docs/security/BASELINE_FINDINGS.md` as re-checked in `docs/security/MYTHOS_FINDINGS.md` (both on `security-review-prep` at `7b1f855`, unchanged). B-P5 to B-P8 concern the registry template; B-P10 to B-P13 concern the package client (`src/pkg`), and B-P11 was already reproduced and fixed as M-02 on `fix/pkg-client-hardening`.
- **Scope:** `registry-template/`, `src/pkg/`, their tests, and documentation in this repository. Nothing was sent to `qxide-lang/registry` or any other remote.
- **Commits:** `0002693` (template: B-P5 to B-P8), `ba90642` (B-P12), `97aa843` (B-P13), `25b1728` (B-P10), `03f6d28` (this record), and `2d9eee1` (template follow-up found in review: a quadratic depth scan introduced by `0002693`, numeric pre-release identifiers, unreadable archive paths, manifest `path` parity with B-P13, code spans in the summary), then the update of this record.
- **Method:** as in part 1. Every reproduction input became a regression test first and was run against the unfixed code; the fix followed. Where a fix needed a policy decision, the most restrictive option was chosen and the alternatives are listed below. This record, the error messages, and the commit messages were written by hand; nothing was sent to an external text service.

## Summary

| ID | Change | Files |
| --- | --- | --- |
| B-P5 | Pull requests may no longer change `yanked` on a published line, in either direction. The registry has no record of who owns a package, so yanking becomes a maintainer action (a commit to `main`). The README, pull request template, and design document describe the new process. | `validate.py`, `README.md`, `.github/pull_request_template.md`, `docs/design/qx-packages.md` |
| B-P6 | Hostile input is reported as an error instead of stopping `validate.py` with a traceback. See the list below. | `validate.py` |
| B-P7 | New package names are compared after folding look-alike characters, also with other names added in the same pull request; names starting like `qxide` and the name `std` are reserved; Windows device names are rejected as index files and as any path a pull request changes. | `validate.py`, `README.md`, `docs/design/qx-packages.md` |
| B-P8 | Error messages, file names, and headings in the job summary are each written as one code span (fenced by more backticks than they contain), and non-printable characters are escaped in the summary and in every line printed to the job log, so pull request text cannot add Markdown, new lines, or `::` workflow commands. | `validate.py` |
| B-P10 | An unpacked package counts as installed only if the cached archive still matches the locked checksum and the unpacked files equal a fresh unpack of that archive. | `src/pkg/cache.rs`, `docs/design/qx-packages.md` |
| B-P11 | No new code. The client side is M-02 on `fix/pkg-client-hardening`; the template side was fixed in part 1 (PAX headers rejected, GNU long names capped at 4096 bytes). Part 2 adds a regression test with an 8 MiB GNU long name and an 8 MiB PAX record. | `ci/test_registry_scripts.py` |
| B-P12 | `curl`, `gh`, and `git` are looked up in the absolute `PATH` entries only (`pkg::command`). On Windows `Command::new` would first look next to `qxide.exe` and in the system directories. | `src/pkg/mod.rs`, `src/pkg/index.rs`, `src/pkg/cli.rs` |
| B-P13 | Path dependencies in `qxide.toml` and `path+` sources in `qxide.lock` must be relative: no leading `/` or `\` (roots and Windows network paths) and no `:` (drives). `..` stays allowed. `validate.py` applies the same rule to dependencies in a published package's `qxide.toml`, which the client also parses. | `src/pkg/manifest.rs`, `src/pkg/lock.rs`, `src/pkg/msg.rs`, `validate.py`, `docs/design/qx-packages.md` |

### B-P6: input that stopped `validate.py`

Besides the deeply nested JSON named in the report, every parser that reads pull request bytes was checked for exceptions other than the ones already handled:

- **JSON nesting:** `strict_json` measures the nesting depth on the text in one linear pass over strings and brackets before calling `json.loads`, and rejects more than 100 levels. The recursive `_depth` helper that ran after parsing is gone. In the before run, depth 1000 raised `RecursionError` in `_depth` and depth 100,000 inside `json.loads` (Python 3.14 parses 2000 levels itself). The first version of the scan (`0002693`) removed strings with a regular expression that retried an unterminated string from every quote: 32,000 characters of `\"` took 17 s, and the 400,000-character test line would take hours. `2d9eee1` replaced it.
- **Long numbers:** a major, minor, or patch number or a requirement number with more than 20 digits (more than any `u64`) is rejected before `int()` is called; Python refuses to convert more than 4300 digits and raised `ValueError`. Numeric pre-release identifiers have no size limit in the client (`tests/data/semver_cases.json` accepts 23 digits), so they stay valid and are compared by length and then digits, which is numeric order for numbers without leading zeros, in `validate.py` and `build_search.py` (whose `key` would otherwise fail after merge).
- **Index files that are not UTF-8** are reported instead of raising `UnicodeDecodeError`.
- **`config.json`:** invalid JSON, invalid UTF-8, nesting over 100 levels, and a value that is not an object are reported (changing the file in a pull request was already rejected before part 1, but it was parsed before that check ran).
- **Archive paths:** a valid version can make the archive's file name longer than 255 bytes. Python 3.12's `Path.is_file` raises for some such errors on Linux, while 3.14 returns `False`, so reading the archive is guarded and reported. Not reproduced here (Windows, Python 3.14).
- **`qxide.toml` in an archive:** `RecursionError` and any `ValueError` from `tomllib` (also integers of more than 4300 digits) are reported, and so is a `package` key that is not a table. Nesting deeper than 80 levels is rejected: `toml_edit`, which the client uses, stops at 80 (`LIMIT` in `toml_edit` 0.25.19). This count includes the document and the `[package]` table, so it is up to two levels stricter than the client.
- **Per-member size** (the other half of B-P6): fixed in part 1, where sizes are added up as each member is read. Part 2 adds the regression test (one member of 50 MiB + 1 byte).

## Policies chosen and alternatives

| ID | Chosen (most restrictive) | Alternatives not chosen |
| --- | --- | --- |
| B-P5 | No pull request may change `yanked`; maintainers yank on request from the publishing account and do not un-yank by pull request. Nothing in the client depends on yank pull requests (`qxide yank` does not exist). With the branch protection recommended in part 1 (pull requests required), a yank commit needs a maintainer allowed to bypass it. | (a) Allow `false` to `true` only, by anyone, with review, as the documentation said before. (b) An owner list per package (kept by maintainers or created by the first publishing pull request) checked against the pull request author's login, which `pull_request_target` provides; this would also cover publishing new versions of someone else's package. (c) Allow yanks only in pull requests that change nothing else. |
| B-P6 | JSON over 100 levels (unchanged limit), TOML over 80 levels counted from the document, major/minor/patch and requirement numbers over 20 digits (as the client, which uses `u64`); numeric pre-release identifiers stay unlimited, as in the client. | Catching `RecursionError` instead of measuring depth; the client's exact limits of 128 (JSON) and 80 (TOML, counting only arrays and inline tables); capping pre-release identifiers, which would reject versions the client accepts and break the shared case list. |
| B-P7 | Fold `0`→`o`, `1`→`l`, `2`→`z`, `3`→`e`, `4`→`a`, `5`→`s`, `6`→`b`, `7`→`t`, `8`→`b`, `9`→`g`, `i`→`l`, then `rn`→`m`, `vv`→`w`, `cl`→`d`, after removing hyphens; reserve the prefix `qxide` and the name `std` (also after folding); reject Windows device names. Only names new relative to the base branch are checked, so existing look-alike pairs never block later pull requests. | A smaller table (only `0`/`o` and `1`/`l`/`i`), which rejects fewer honest names; more reserved prefixes (`qx`, `core`); an allowlist file so maintainers can approve a rejected name without bypassing the check. |
| B-P8 | Write each error, file name, and heading as one code span whose fence is longer than any backtick run inside it, so nothing inside is rendered (no links, autolinks, mentions, or HTML); write every non-printable character (Unicode categories C and Z other than the space, which includes U+2028 and U+0085) as `\u{XXXX}` in the summary and the log. Printable non-ASCII text is kept so reviewers can read it. `0002693` first escaped ASCII punctuation in plain text instead; that does not stop GitHub's autolinking of bare URLs and mentions, so `2d9eee1` switched to code spans. | Backslash-escaping punctuation in plain text (the first version); HTML-escaped `<code>` elements; leaving file names out of the summary; escaping all non-ASCII text. |
| B-P10 | Compare the unpacked files with a fresh unpack of the cached archive on every check. The archive is anchored to the checksum in `qxide.lock`, which a writer of `~/.qxide` cannot change, while a hash list in the marker could be rewritten along with the files. Costs one unpack per package per command (packages are at most 50 MiB unpacked; the example packages take milliseconds). A missing cached archive now also means "not installed", and `qxide install` downloads it again. The fresh copy is unpacked next to the package (`.verify-<name>-<version>-<pid>`), not in the shared temporary directory where another user could prepare the path; so a read-only cache now gives `Q0611`. Tampering is reported as "not installed" (`Q0611`) rather than with its own message, and `qxide install` silently restores the files. | Keep the marker only and document the item as outside the threat model; a per-file hash list in the marker (detects mistakes, not a local attacker); verify only during `qxide install`; read-only permissions on the cache. |
| B-P12 | Search only absolute `PATH` entries for `<name>.exe` (as Rust does, without `PATHEXT`); report the tool as missing otherwise. Other platforms are unchanged (`execvp` already searches `PATH` only). A `curl.exe` that is only in `System32` while `System32` is not on `PATH` is no longer found. | Use `%SystemRoot%\System32\curl.exe` explicitly for `curl`; keep Rust's order but skip the executable's directory; a setting for tool paths. |
| B-P13 | Reject `path` values and `path+` lock sources that start with `/` or `\` or contain `:`. This is a behaviour change: absolute path dependencies, including `qxide add --path <absolute dir>`, now fail with `Q0602` (the manifest is left unchanged). | Confine path dependencies to the project directory, which would break the documented `../local-pkg` form, `examples/packages/bell-app` (`path = "../bell-utils"`), and existing tests, so it was not applied; a warning instead of an error; rejecting only Windows network paths. |

## Regression tests

Template: "Before" is the final test file run against the unfixed template from `deef8dd` (an export of `ci/`, `registry-template/`, and `tests/data/` next to the new tests): 50 tests, `failures=50, skipped=1` (counted per failing subtest), all in the tests below; every other test passed. Against the first fix commit `0002693`, the same file gives `failures=9, errors=1`: the linear-scan test times out, and the numeric pre-release, code span, and manifest `path` tests fail; `2d9eee1` fixed these. "After" is this branch: 50 tests OK, `skipped=1`. The skipped test needs a file system that can create `con`, so it ran on neither side here (Windows); CI runs on Linux.

| Test | Input | Before | After |
| --- | --- | --- | --- |
| `ValidatePullRequests.test_yanked_cannot_change_in_a_pull_request` | B-P5 report input: only `"yanked": false` to `true` on a published line | Fails: exit 0 | Passes: exit 1, `index/ev/il/evil` |
| `ValidatePullRequests.test_unyanking_in_a_pull_request` | `true` to `false` on a yanked line | Fails: exit 0 | Passes |
| `ValidatePullRequests.test_look_alike_names` | New `be11-utils`, `beil-utils`, `bell-uti1s` next to `bell-utils`; `rnodem`, `m0dem` next to `modem`; control `bellutils` (already rejected) | Fails for the 5 new cases | Passes |
| `ValidatePullRequests.test_look_alike_names_in_one_pull_request` | `foo-bar` with `foobar`, `foo1` with `fool`, both new in one pull request | Fails: exit 0 | Passes |
| `ValidatePullRequests.test_reserved_names` | `qxide`, `qxide-std`, `qxidetools`, `qx1de-utils`, `std` | Fails: exit 0 | Passes |
| `ValidatePullRequests.test_distinct_new_names_pass` | Controls: `bell-tools`, `qx-utils`, `dep-a` to `dep-c`, `pub-fixture`, `standard`, then a new version of `bell-utils` | Passes | Passes |
| `ValidatePullRequests.test_windows_device_names` | `con`, `nul`, `aux`, `prn`, `com1`, `lpt9` index files committed with `git update-index --cacheinfo` (with `core.protectNTFS=false`, since Git for Windows refuses such paths) | Fails: exit 0 | Passes: rejected as changed paths |
| `ValidatePullRequests.test_windows_device_names_on_disk` | `con`, `com1` index files in a checkout | Skipped (Windows) | Skipped (Windows) |
| `ValidateHostileInput.test_deeply_nested_index_json` | B-P6 report input: 1000 and 100,000 nested arrays on an index line; control 101 | Fails for 1000 and 100,000 (traceback) | Passes: error at `index/ev/il/evil:2`, count line printed, summary written |
| `ValidateHostileInput.test_depth_scan_is_linear` | Unterminated strings of 200,000 `\"`, 200,000 `"`, 100,000 `["` on an index line, each within 60 s | Passes on `deef8dd` (`json.loads` fails fast); times out on `0002693` | Passes |
| `ValidateHostileInput.test_huge_numbers` | 5000-digit `vers`, `qxide`, and dependency `req`; a 5000-digit numeric pre-release in `vers` and in a dependency `req` | Fails: traceback (`ValueError`) for the first four | Passes; the pre-release cases are valid and reported only for the missing archive |
| `ValidateHostileInput.test_long_archive_names` | `vers` `1.0.0-` plus 300 letters, so the archive name exceeds 255 bytes | Passes on Python 3.14 (expected to fail on Linux with Python 3.12; not run) | Passes |
| `ValidateManifestPaths.test_paths_the_client_rejects` | An archived `qxide.toml` with `path` = `"C:/x"`, `"/x"`, `"//host/share/x"`, `"\\x"`, `"C:x"`, `5` | Fails: exit 0 (the client would refuse the manifest for every user) | Passes |
| `BuildSearch.test_long_numeric_prerelease` | `1.0.0-` plus 5000 digits next to `1.0.0-2` and `1.0.0-10` | Fails: `build_search.py` traceback | Passes: the long one is newest |
| `ValidateHostileInput.test_index_file_that_is_not_utf8` | A `0xff` byte in an index file | Fails: traceback | Passes |
| `ValidateHostileInput.test_bad_config_json` | `{`, `[]`, an unterminated object, 100,000 nested arrays, `"\ud800"`, a `0xff` byte | Fails: traceback for all 6 | Passes |
| `ValidateHostileInput.test_hostile_manifests` | `qxide.toml` with 300 and 100,000 nested arrays, a 5000-digit integer, `package = 1`, `package = []`; control with 70 nested arrays | Fails: 300 accepted (the client rejects it), the others with a traceback | Passes; control accepted |
| `ValidateHostileInput.test_large_members_and_long_names` | One member of 50 MiB + 1 byte; an 8 MiB GNU long name; an 8 MiB PAX record (B-P6, B-P11) | Passes (fixed in part 1) | Passes |
| `ValidateOutput.test_member_names_cannot_add_markdown` | B-P8 report input, a member name with backticks and brackets: ``x` [click](https%3A%2F%2Fevil.invalid) __bold__ `.qx`` | Fails: the name's backticks end the code span, so the link and emphasis are rendered | Passes: the summary line is one code span containing the whole name |
| `ValidateOutput.test_line_breaks_cannot_add_lines` | A member name and an index `name` containing `\n`, `\r`, U+2028, or U+0085 followed by `::error::`, `::warning::`, `# Injected`, `<img>` | Fails: `::warning::injected` printed as its own log line | Passes |

Client (`tests/packages.rs`). "Before" is the test run against this branch's code before the corresponding fix commit.

| Test | Input | Before | After |
| --- | --- | --- | --- |
| `tools_next_to_the_executable_are_not_run` (Windows only) | A stand-in `curl.exe` (the existing `tests/support/fake_curl.rs`) next to a copy of `qxide.exe`, `PATH` set to an empty directory, `qxide install` | Fails: the planted `curl` ran and the install succeeded | Passes: `Q0608`, the stand-in did not run |
| `path_dependencies_must_be_relative` | `path` = the absolute path of a real package, `/mid`, `//host/share/mid`, `C:/mid`, `C:mid`; `qxide add --path <absolute>`; lock `path+<absolute>` and `path+//host/share/mid` | Fails: the absolute path installed with exit 0 | Passes: `Q0602` for the manifest (`install`, `run`, and `add`, which leaves the manifest unchanged), `Q0603` for the lock; `../mid-0.1.0` still works |
| `modified_package_files_are_not_used` | In the cache: append a function to `src/lib.qx`; add `src/extra.qx`; delete `src/lib.qx`; delete the cached archives | Fails: `qxide run` used the edited package (exit 0) | Passes: `run` fails with `Q0611`, `install` restores the files, `run` succeeds |

### Changed existing test code

The comment on `fake_curl_qxide` in `tests/packages.rs` said that Windows finds the stand-in next to the executable; it now says that tests place its directory first on `PATH`, which `failed_downloads_are_never_accepted_or_cached` already did. No assertion was removed or weakened.

## Verification

Run on Windows 11 with Python 3.14.10 and rustc 1.99.1 at the tip of this branch:

| Command | Result |
| --- | --- |
| `python -I ci/test_registry_scripts.py` | 50 tests OK (1 skipped, see above) |
| `cargo clippy -j 8 --target x86_64-linux-android --bin qxide -- -D warnings` | clean (type-checks the non-Windows branch of `pkg::command`; nothing was run on that target) |
| `cargo fmt --all --check` | clean |
| `cargo clippy -j 8 --workspace --all-targets -- -D warnings` | clean |
| `cargo test -j 8 --workspace` | all passed (binary unit tests 107, `cli` 21, `packages` 16, `qxide-wasm` 94 + 11, others unchanged) at `25b1728`, and at the tip in a clean build (`CARGO_INCREMENTAL=0`, separate target directory). In the incremental build of the tip, `cli` `long_operator_and_index_chains_are_diagnostics_not_crashes` overflowed the interpreter's stack 5 times in a row, with no Rust change since the passing run; it passes in the clean build. This is the known unoptimized-build stack margin that `fix/cli-test-stack-overflow` fixes (its diff touches this test); the interpreter code in `src/lang` is not changed here. |
| `git merge-tree --write-tree HEAD fix/pkg-client-hardening` | no conflicts |
| Tests of that combined tree (at `25b1728`, before `2d9eee1`, which changes only the template), checked out in a temporary detached worktree from an unreferenced commit (removed afterwards; no branch was changed) | binary unit tests 114, `cli` 22, `packages` 18, and the registry tests: all passed |

Not run: the workflow on GitHub Actions, so how GitHub renders the summary was not seen; Python 3.12 (the CI version), although nothing used is newer than 3.11; `test_windows_device_names_on_disk` (Linux only); the non-Windows branch of `pkg::command` (only type-checked); the `gh` and `git` calls of `qxide publish` (they now go through the same lookup, but publishing needs GitHub); a merge check against the other unmerged branches (`fix/execution-limits`, `fix/cli-test-stack-overflow`, `integration/check-20261121`).

## Applying this to `qxide-lang/registry`

The steps of part 1 apply. In addition:

1. Before the update, check the live index for names that the new rules reject as Windows device names (the look-alike and reserved-name checks apply only to new names, so existing names are not affected).
2. Announce that yanking is now done by maintainers, and decide how a yank request is verified (for example, an issue opened by the account that published the version).
3. B-P10, B-P12, and B-P13 are client changes; they take effect with the next `qxide` release.

## Not done here

- Package ownership as a whole: anyone can still open a pull request that publishes a new version of an existing package, and only review prevents it (alternative (b) for B-P5 would cover both). Addressed in part 3 (owner files): owners publish and yank; only maintainers un-yank.
- B-P10 costs one archive unpack per package on every `run`, `check`, and `qasm`; the comparison is not cached.
- Open items are tracked privately.
- B-P10: a read-only cache now fails with `Q0611`, and tampering has no message of its own (see the policy table).
- The commit messages of `ba90642`, `97aa843`, and `25b1728` name the weakness each fixes in one sentence; they were not rewritten.

# Part 3: package owners

- **Date:** 2026-11-21
- **Branch:** `feat/registry-owners`, created from local `main` at `846d18e` (parts 1 and 2 merged). Not merged or pushed.
- **Decision:** the user chose alternative (b) of B-P5 for publishing: an owner list per package, created by the first publishing pull request and enforced by CI instead of by review alone. Follow-up decisions (2026-11-21 23:01): owners may yank their versions; new owners join by invitation and acceptance; maintainers act only by commits to `main` (no maintainer list in CI); `qxide publish` writes the owner file and the README and pull request template keep manual steps for 0.7.0 users; `bell-utils` starts with `rigelx2b` as owner; the CHANGELOG is updated at the next release.
- **Scope:** `registry-template/`, `src/pkg/`, their tests, and documentation in this repository. Nothing was sent to `qxide-lang/registry` or any other remote. This record and the commit messages were written by hand; nothing was sent to an external text service.
- **Commits:** `9ca1222` (validator, workflow, tests), `dbbe8b7` (`qxide publish` writes the owner file), `d224ea6` (documentation and this record), `00c47b2` (follow-up found in review: new versions keep their archive at the default path, and GitHub App authors are accepted), then the commit with owner yanks and invitations.

## Design

| Item | Choice | Reason |
| --- | --- | --- |
| Storage | `owners/<prefix>/<name>`, one `{"id": <int>, "login": <str>}` JSON object per line, 1 to 32 lines | Same sharding and line format as `index/`; the client never reads it, so the index format and old clients are unaffected. |
| Identity | GitHub numeric user id; `login` only for readers | A login can be renamed and then registered by someone else; the id cannot be reused. |
| Author source | `github.event.pull_request.user.id` and `.login`, passed through `env` to `--author-id` and `--author-login` | Set by GitHub when the pull request is opened and not changed by later pushes. Not used: `github.actor` (whoever triggered the run, such as a re-run by a maintainer), `sender`, the head repository owner, commit author or committer (written by the pull request), title or body. |
| Authority | Owner file on the base branch (`git show <base>:owners/...`), never the merge result | Otherwise a pull request could add its author and publish in the same change. |
| New package | No index file in `git ls-tree <base> -- index/`; the pull request must add the owner file listing exactly its author | First come, first served, as before. Defined by the base listing, which fails closed, rather than by a missing owner file. |
| Published package without owner file | Locked for pull requests until a maintainer adds the file | Migration-safe: a missing file cannot be claimed by the next pull request. |
| Missing author | `--base` without a valid `--author-id` and `--author-login` is an error | Fail closed if the workflow is changed or run without the event. |
| Changes to owners | An owner can invite (`"pending": true`), withdraw invitations, and remove owners; no pull request adds a confirmed owner except the invited account's own, which may only drop `pending` from its own line (and set its current login) and change nothing else; the file keeps at least one confirmed owner and cannot be deleted | Ownership needs the new owner's consent, given by a pull request that GitHub attributes to that account. Invited accounts have no rights until they accept. |
| Maintainer override | Commit to `main` (admin bypass); the push run checks the format only | No maintainer list inside CI, which would be one more thing to protect. |
| Files under `packages/` | Every added file must be the archive of a version in the index, and a version added by the pull request keeps its archive at the default path (no relative `dl` elsewhere) | Without this, a non-owner could add `packages/<prefix>/<name>/<name>-<next>.tar.gz`, with no index line or as the `dl` of a package of its own; the owner's publication of that version would then modify an existing archive and be rejected. |
| Bot authors | `--author-login` also accepts `<name>[bot]`; owner files list users only | A Dependabot pull request that updates workflow actions would otherwise fail; a bot cannot own a package. |
| Diff parsing | `git diff --name-status -z` | Without `-z`, git quotes paths with unusual characters, and a quoted path does not start with `packages/`. |
| Yanking | An owner may change `yanked` from false to true; un-yanking stays with maintainers (commit to `main`) | The ownership check already covers every change to the index file. Restoring a version that was yanked, possibly by an owner whose account was taken over, is left to people. |

`qxide publish` (the `gh` path) now asks `gh api user --jq '.id, .login'` and writes the owner file into the pull request for the first version of a package. Without `gh`, it prints the owner file as a third file to add. A local directory registry gets no owner file.

## Regression tests

`ValidateOwners` in `ci/test_registry_scripts.py` (the base branch is owned by `alice`, id 1001):

| Test | Input | Expected |
| --- | --- | --- |
| `test_owner_publishes_a_new_version` | `alice` adds `evil 1.0.1` | passes |
| `test_non_owner_cannot_publish_a_new_version` | `mallory` adds `evil 1.0.1` | `index/ev/il/evil` rejected |
| `test_same_login_with_another_id_is_not_the_owner` | author login `alice` with id 4242 | rejected |
| `test_new_package_by_anyone` | `mallory` adds `mallory-tools` with an owner file listing `mallory` | passes; summary names the author |
| `test_owner_line_written_by_qxide_publish` | owner line in the exact form `Owner::to_json` writes | passes |
| `test_new_package_needs_an_owner_file` | new package, no owner file | rejected |
| `test_new_package_owner_file_lists_exactly_the_author` | someone else, author plus another, matching login with another id | rejected |
| `test_non_owner_cannot_add_themselves` | `mallory` adds its own account (with and without a new version), or replaces the owners | rejected |
| `test_owner_invites_and_invitee_accepts` | `alice` invites `bob`; `bob` publishes while invited; `bob` accepts; `bob` publishes and becomes the only owner | passes; rejected; passes; passes |
| `test_owners_cannot_add_owners_without_acceptance` | `alice` lists `bob` as owner directly; `alice` confirms `bob`'s invitation | rejected |
| `test_invitee_pull_request_only_accepts` | `bob` (invited) accepts `mallory`'s invitation, both, removes `alice`, adds another owner, accepts and publishes; an account with login `bob` and another id accepts; `bob` accepts under a renamed login | rejected (6 cases); passes |
| `test_owner_withdraws_an_invitation` | `alice` removes `bob`'s invitation | passes |
| `test_owner_yanks` | `alice`, invited `bob`, and `mallory` each set `yanked` to true | passes; rejected; rejected |
| `test_owner_cannot_leave_a_package_without_owners` | empty owner file; deleted owner file | rejected |
| `test_package_without_owner_file_is_locked` | no owner file on base; `alice` or `mallory` publish and add themselves; then a maintainer adds the file | rejected, then passes |
| `test_archive_without_index_line_in_another_package` | archive at the path of `evil 1.0.1` with no index line; a non-ASCII file name | rejected (the unquoted path is reported) |
| `test_relative_dl_into_another_package` | new package `mallory-tools` whose `dl` is the path of `evil 1.0.1`, with a valid archive there; then the same package with the default path as `dl` | rejected, then passes (without the rule: 0 errors) |
| `test_bot_authors` | `dependabot[bot]` changes `README.md`; publishes `evil 1.0.1`; adds a package listing itself | passes; rejected; rejected |
| `test_author_is_required_with_base` | no author, only one of the two, leading zero, non-digit id, invalid login | rejected |
| `test_malformed_owner_files` | 16 malformed files (empty, invalid JSON, `true`/string/float/zero/oversized id, extra key, bad or long login, duplicate id, invalid UTF-8, nesting 5000 deep, 33 owners) | rejected without a traceback |
| `test_owner_files_need_a_package` | owner file without an index file; owner file at a wrong prefix | rejected |
| `test_owner_files_are_optional_outside_pull_requests` | package without owner file, no `--base` | passes |
| `test_owner_pull_request_may_not_change_other_files` | owner file change plus `README.md` | rejected |

`ValidateWorkflow.test_author_comes_from_the_pull_request_event` pins the two `pull_request.user` expressions and the arguments, and rejects `github.actor`, `triggering_actor`, `head_commit`, `sender`, and head or commit author fields in the workflow.

Existing tests now pass an author (`alice`) with `--base`, and `Registry.add` writes an owner file for a package's first version; no assertion was removed or weakened. With the `check_ownership` call removed (before `test_owner_line_written_by_qxide_publish` was added), 9 of the 16 `ValidateOwners` tests failed, 14 cases counting subtests; the others are cases that must pass, or checks that run without it. With the old diff parsing, the non-ASCII case fails.

Rust: `publishing_a_new_package_writes_its_owner_file` and `gh_user_output_must_be_an_id_and_a_login` in `src/pkg/tests.rs`.

## Verification

Run on Windows 11 with Python 3.14.10 and rustc 1.99.1 at the tip of this branch:

| Command | Result |
| --- | --- |
| `python -I -m unittest discover -s ci -p test_registry_scripts.py` | 74 tests OK (1 skipped: `test_windows_device_names_on_disk`, Linux only). `python -I -m unittest ci.test_registry_scripts` cannot import the module because `-I` removes the current directory from `sys.path`; without `-I` the same 74 tests pass. |
| `cargo fmt --check` | clean |
| `cargo clippy -j 8 --all-targets -- -D warnings` | clean |
| `cargo test -j 8 --workspace` | all passed (binary unit tests 117) |

Not run: the workflow on GitHub Actions, so the event values were not observed live; the `gh api user` call of `qxide publish` (it needs GitHub); Python 3.12.

## Applying this to `qxide-lang/registry`

1. Make sure parts 1 and 2 are applied first (the live `scripts/validate.py` is older than both).
2. List the live packages and, for each, the account that opened the pull request that published its first version, with its id (`gh pr list --repo qxide-lang/registry --state merged --json number,author,title`, then `gh api users/<login> --jq .id`). At the time of writing the registry has `bell-utils` 0.1.0, published by `rigelx2b` in pull request #1. Confirmed on 2026-11-21: `gh api users/rigelx2b --jq .id` printed `295735711`, so the file is `owners/be/ll/bell-utils` with the single line `{"id":295735711,"login":"rigelx2b"}`.
3. In one maintainer commit pushed directly to `main`: the new `scripts/validate.py`, `.github/workflows/validate.yml`, README and pull request template, an `owners/.gitkeep`, and one owner file per package. Run `python -I scripts/validate.py` in the clone before pushing. `pull_request_target` uses the workflow from the default branch, so the rules apply from the next pull request on.
4. Packages left without an owner file are locked until a maintainer adds one; nothing else breaks.
5. `qxide` 0.7.0 does not write owner files, so its `qxide publish` opens pull requests for new packages that fail the check until the next release. Until then, publishers add the owner file to their pull request by hand (the README describes it), or maintainers add it.

## Not done here

- Un-yanking by owners (maintainers only).
- An invited account cannot decline by pull request; an owner withdraws the invitation instead. Invitations do not expire.
- No `qxide owner` command; owner files are edited by hand.
- Open items are tracked privately.
- `qxide publish` does not check ownership before opening the pull request; a non-owner learns from CI.
- If GitHub ever reports a different `pull_request.user` than the account that opened the pull request (for example, transferred or deleted accounts, shown as `ghost`), the pull request fails the check; this was not tested live.
