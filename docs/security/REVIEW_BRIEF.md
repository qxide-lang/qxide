# Qxide Security Review Brief

> **Note (2026-11-26):** This brief was written for the review of 0.7.0 (`b8c385e`), and that review is done (`MYTHOS_FINDINGS.md`). Since then the `app-framework` branch has been merged into `main`, so read it from `main`. For a new review, start from the current `main` and use the `Status on main` sections of `BASELINE_FINDINGS.md` and `MYTHOS_FINDINGS.md` to see which findings are still open.

Follow these instructions when conducting a defensive security review of the Qxide repository (`github.com/qxide-lang/qxide`, Rust crate `qxide` 0.7.0, tag `v0.7.0`, released 2026-11-21). These rules apply to any AI security reviewer (for example, Claude Mythos) starting with no prior context and define the contract with the human maintainer.

## Mission

Conduct a defensive security review of Qxide's own code:

- Find security vulnerabilities and robustness bugs.
- Reproduce findings locally whenever possible.
- Report every finding clearly with concrete evidence.
- Do not apply fixes during the review.

## Read first

Read these documents before analyzing code or writing findings:

1. `docs/security/THREAT_MODEL.md`
2. `docs/security/BASELINE_FINDINGS.md` — Do not merely restate baseline items. Confirm, refute, or extend each baseline finding you examine, and state explicitly which (`confirms B-xx`, `refutes B-xx`, or `extends B-xx`).
3. `fuzz/README.md`
4. `docs/design/qx-language.md`
5. `docs/design/qx-packages.md`
6. `docs/design/qx-app.md` — Located on the `app-framework` branch; read it with `git show app-framework:docs/design/qx-app.md` or from a separate worktree.
7. `docs/error-codes.md`

## Code under review

Review exactly these two code baselines:

- **Branch `main`**, baseline commit `b8c385e` (`qxide` 0.7.0 / tag `v0.7.0`).
- **Branch `app-framework`**, commit `eea71a6` (unreleased desktop app framework, not merged into `main`). Inspect it using `git show app-framework:<path>` or a separate worktree.

This review may start long after these commits. If `main` or `app-framework` has moved, review the current tips instead, record the commit hashes you reviewed at the top of the report, and re-check every `B-xx` `file:line` reference against them before confirming or refuting it.

This review starts from branch `security-review-prep`. Do not review other branches (`lsp`, `random-default-seed`, or others) unless the maintainer explicitly adds them.

## In scope

Review Qxide's own code in the following locations:

On `main` (`b8c385e`):

- Everything under `src/` (simulators, `.qx` front end and interpreter, CLI, and package manager)
- `crates/qxide-wasm`
- `site/src` (playground and share links)
- `site/functions`
- `site/public`
- `registry-template/` (scripts and workflows)
- `.github/workflows`
- `dist-workspace.toml`
- `pyproject.toml`
- `Cargo.toml`

On `app-framework` (`eea71a6`):

- `crates/qxide-app-runtime`
- `src/app`
- `src/lang/stdlib.rs`
- `apps/studio`

## Out of scope and forbidden actions

Obey these boundaries strictly:

- **Reproduce locally only.** Perform all testing and reproduction exclusively on the local machine using local files, a local directory registry (`--registry <dir>`, with `QXIDE_HOME` pointed at a temporary directory), and local builds.
- **Do not touch third-party or production systems.** Never attack, probe, scan, or interact with `qxide.dev` or its Cloudflare account, `github.com/qxide-lang/*` (including opening PRs or issues on the live registry), `raw.githubusercontent.com`, PyPI, crates.io, or any other package registry or CI service.
- **Do not generate external network traffic.** Allow no network traffic beyond what a normal local build fetches from crates.io/npm caches. Do not run `npm install` with network access unless the maintainer agrees.
- **Do not conduct social engineering, DoS against real services, or credential use.** Never perform social engineering, never run denial-of-service tests against live services, and never use credentials.
- **Do not publish, push, or modify remote state.** Never publish, push, tag, release, deploy, or create GitHub content.
- **Do not install tools or toolchains globally.** Never run `rustup toolchain install`, `cargo install`, `npm -g`, or `pip install` outside a venv. If a required tool is missing, record the exact command a human can run and continue the review without installing it.
- **Do not expand branch scope.** Ignore `lsp`, `random-default-seed`, and any other branches unless the maintainer adds them.

## Working on this machine

Host environment: Windows 11, i9 CPU, 32 GB RAM (often ~8 GB free), with Git Bash and PowerShell available.

Follow these operational rules:

- Always build and test with `-j 4`.
- Start a build only when at least 8 GB of physical memory is free.
- Run hostile inputs under both a wall-clock timeout and a memory watch. Some reproduced inputs grow memory by 300-3000 MB per second (see `B-02` and `B-03`).
- Prefer release builds (`--release`) when executing hostile inputs.
- Keep reproduction files under `target/` (git-ignored) or inline inside the findings document. Never place reproduction files in tracked source directories.

## Useful commands

Build the release binary:

```bash
cargo build --release -j 4
```

Run the test suite:

```bash
cargo test -j 4
```

Run randomized smoke tests (configurable via `QXIDE_SMOKE_CASES`, `QXIDE_SMOKE_SEED`, `QXIDE_SMOKE_TIMEOUT_MS`, and `QXIDE_SMOKE_REPLAY`, which runs one file unchanged before the mutants; time-outs from huge loop bounds are expected duplicates of `B-01` until the interpreter has a step limit):

```bash
cargo test --release --test smoke_random -- --ignored --nocapture
```

Run fuzz crate tests without default features (`cargo-fuzz` requires nightly and is NOT installed; see `fuzz/README.md`):

```bash
cargo test --manifest-path fuzz/Cargo.toml --no-default-features
```

Run the JavaScript share-link decoder harness:

```bash
node fuzz/js/share_decode.mjs 20000 1
```

Build the playground WebAssembly module (the `wasm32-unknown-unknown` target is already installed):

```bash
cargo build -p qxide-wasm --target wasm32-unknown-unknown --profile wasm -j 4
```

Run site parity tests (requires existing `node_modules`; do not run `npm install` with network access unless the maintainer agrees):

```bash
cd site && npm run test:parity
```

Inspect a file on the `app-framework` branch without switching branches:

```bash
git show app-framework:<path>
```

## Priorities

Focus your review effort in this order:

1. **Untrusted `.qx` execution limits:** DoS, unbounded memory growth, and panics across the CLI, playground, REPL, and app runtime.
2. **App-framework privilege boundaries:** IPC origin and navigation controls, `std.fs` confinement (including symlinks and Windows path quirks), `--dev` handling in shipped apps, and `.qxpak` bundle parsing.
3. **Package system integrity:** Index trust, checksum verification flow, archive unpacking, dependency resolver, lock file handling, registry `validate.py` and workflows, name squatting, and description/terminal injection.
4. **Playground web surface:** Share links, output rendering, and CSP/security headers.
5. **Release and supply chain:** GitHub Actions workflows, installers, PyPI token usage, and action pinning.
6. **Everything else:** CLI argument and file handling, `fmt` rewriting files in place, and `explain`/`new`.

## Severity rubric

Classify every finding using this table:

| Severity | Definition |
| --- | --- |
| Critical | Code execution or arbitrary file write/read outside allowed roots without user consent, or a way to publish/alter packages or releases for other users. |
| High | Escape of an intended sandbox/permission (`std.fs` root, app permissions, playground limits) or integrity bypass needing little user action. |
| Medium | Reliable DoS or data exposure needing the victim to open/run attacker content, or a CI/registry control bypass needing a reviewer mistake. |
| Low | Hard-to-reach or bounded impact. |
| Info | Hardening and hygiene. |

## Finding format

Start the findings report with this summary table header:

```markdown
| ID | Severity | Status | Location | One-line title |
| --- | --- | --- | --- | --- |
```

Write one section per finding, numbered sequentially (`M-01`, `M-02`, ...). Mark a finding `Reproduced` only if you actually triggered it locally and include the verbatim observed output; otherwise mark it `Suspected`. Fill in every field using this template:

````markdown
## M-01: <Title>

- **Severity:** Critical | High | Medium | Low | Info
- **Confidence:** High | Medium | Low
- **Status:** Reproduced | Suspected
- **Affected branch and commit:** `<branch>` (`<commit>`)
- **Location:** `path/to/file:line` (list additional `file:line` references if needed)
- **Relation to the baseline:** `new` | `confirms B-xx` | `refutes B-xx` | `extends B-xx`

### Reproduction

Environment notes (OS, shell, build profile, env vars, local registry setup):

```text
<exact input file contents>
```

```bash
<exact command>
```

### Observed result

```text
<verbatim output, exit code, and memory/time numbers>
```

### Impact

Explain who can trigger this issue, which actor and trust boundary in `docs/security/THREAT_MODEL.md` applies, and what the attacker gains.

### Suggested fix

Provide a short description of the remediation (optionally include a diff sketch). Do not apply the fix.
````

## Deliverable and branch rules

- Create a new local branch from `security-review-prep` (for example, `security-review-findings`).
- Write the completed report to `docs/security/REVIEW_FINDINGS.md`.
- Place the summary table (`ID`, `Severity`, `Status`, `Location`, `One-line title`) at the top of `docs/security/REVIEW_FINDINGS.md`.
- Include a list of areas reviewed with no findings.
- Commit `docs/security/REVIEW_FINDINGS.md` locally on your new branch.
- Do not push the branch or commit to any remote.

## Fixes

- Never commit fixes on the review branch.
- Implement each fix on its own separate branch created from `main`, and only after a human maintainer has read the finding and approved the fix.
- Include a regression test using the reproduction input with every fix.
- Do not modify existing tests to make them pass.
- Require human review of the diff before merging.

## When to stop and ask

Stop immediately and ask the maintainer before proceeding if:

- An action would touch the network beyond normal local builds.
- An action would require a global tool or toolchain install.
- An action would require credentials.
- A finding looks **Critical** — report it to the maintainer first before investigating further.
