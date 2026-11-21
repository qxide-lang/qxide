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
   - A `CODEOWNERS` file covering `/.github/`, `/scripts/`, and `/config.json`, with "Require review from Code Owners". Without it a pull request can add a second workflow with a job of the same name that reports success, so reviewers must not rely on the check name alone. (No owner is named here because the maintainer team is not known in this repository.)

## Not done here

- B-P5 to B-P8 and B-P10 to B-P13 (yank ownership, look-alike names, Markdown in the job summary, the `RecursionError` on deeply nested index JSON in B-P6, and others) are unchanged.
- The index `qxide` requirement is not compared with `qxide` in `qxide.toml`; a mismatch does not cause the Q0612 loop of M-08 but makes resolution and installed checks use different values.
- Client side of M-09: `fix/pkg-client-hardening` rejects PAX headers, which settles the entry-size part, but the client still stops after the first gzip member without failing on trailing data. CI now rejects such archives, so the registry cannot hold one.
- Control characters other than `Cc` (for example bidirectional overrides, category `Cf`) are still allowed in `description`.
