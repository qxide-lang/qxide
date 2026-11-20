# Release Runbook

## Overview

`qxide` (Rust crate and CLI binary) has two distribution channels:

1. **GitHub Releases via `cargo-dist` (`0.34.0`)**
   - Configuration: `dist-workspace.toml`
   - CI workflow: `.github/workflows/release.yml`
   - Regenerate workflow: `dist generate`
   - Validate release plan: `dist plan`
   - Pushing a git tag such as `v0.4.0` (must match the version in `Cargo.toml`) triggers GitHub Actions to build:
     - `.zip` / `.tar.xz` archives for:
       - `x86_64-pc-windows-msvc`
       - `x86_64-apple-darwin`
       - `aarch64-apple-darwin`
       - `x86_64-unknown-linux-gnu`
       - `aarch64-unknown-linux-gnu`
     - Shell and PowerShell installers: `qxide-installer.sh` and `qxide-installer.ps1`
     - Checksums
   - Publishes the artifacts to a GitHub Release.
   - The generated installers place the `qxide` binary in `CARGO_HOME/bin`.

2. **PyPI wheel via `maturin`**
   - Configured in `pyproject.toml` with `bindings = "bin"`, packaging the same `qxide` CLI binary.

---

## Cutting a Release (`cargo-dist`)

Release preparation is manual; nothing is automated beyond the tag push trigger.

1. Bump the version in `Cargo.toml`.
2. Commit the version change.
3. Verify the release plan:
   ```sh
   dist plan
   ```
4. Tag the commit with a matching version (`vX.Y.Z`) and push the tag:
   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

---

## User Install Commands

### Via `qxide.dev` (once installer routes are configured)

**Linux / macOS:**
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"
```

### Direct from GitHub Releases (works without `qxide.dev`)

- Shell installer: `https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.sh`
- PowerShell installer: `https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.ps1`

**Linux / macOS:**
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.sh | sh
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.ps1 | iex"
```

---

## Serving `https://qxide.dev/install.sh` and `/install.ps1`

### Option A: HTTP 302 Redirect (Recommended)

Redirect `/install.sh` and `/install.ps1` with HTTP `302` to the GitHub Releases `latest` URLs.

**Cloudflare Pages / Netlify (`_redirects`):**
```text
/install.sh https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.sh 302
/install.ps1 https://github.com/qxide-lang/qxide/releases/latest/download/qxide-installer.ps1 302
```

Alternatively, configure an equivalent Cloudflare redirect rule.

Notes:
- Clients must follow redirects (`curl` includes `-L` in the install command above; PowerShell `irm` follows redirects by default).
- Always resolves to the latest release with no per-release maintenance.
- GitHub's `latest` endpoint ignores drafts and pre-releases.

### Option B: Static Copy

Download `qxide-installer.sh` and `qxide-installer.ps1` from a GitHub Release and commit them directly to the `qxide.dev` site repository.

- Use this only when redirects are not possible.
- Copied scripts are pinned to that release's version and must be re-copied on every release.

### Verification

After configuring the host, verify redirect resolution and headers:

```sh
curl -sIL https://qxide.dev/install.sh
```

---

## PyPI Distribution (Not Yet Published)

### Current State

- Package name: `qxide`
- `pyproject.toml` uses `dynamic = ["version"]`, so the wheel version always follows `Cargo.toml` (currently `0.4.0`).
  - Both `pip show qxide` and `qxide --version` report `0.4.0`.
- Local verification completed on Windows:
  - `maturin build --release` produced `qxide-0.4.0-py3-none-win_amd64.whl`.
  - After installing the wheel into a fresh virtual environment, `pip show qxide` reported `0.4.0` and `qxide --version` printed `qxide 0.4.0`.
- A locally built wheel only targets the host platform (Windows in the check above). For a real release, build wheels for Linux (`manylinux`), macOS, and Windows using `maturin-action` in CI, along with a source distribution via `maturin sdist`.

### Manual Publishing Steps

1. Create a PyPI account and enable 2FA.
2. *(Optional)* Test uploading against TestPyPI first:
   ```sh
   maturin upload --repository testpypi dist/*
   ```
3. Create a PyPI API token:
   - The first upload for a new package requires an **account-scoped** token.
   - Once the `qxide` project exists on PyPI, replace it with a **project-scoped** token.
4. Export the token in your shell (never commit it to the repository):
   ```sh
   export MATURIN_PYPI_TOKEN="<token>"
   ```
5. Publish the artifacts using one of:
   - `maturin publish`
   - `maturin upload target/wheels/*`
   - `twine upload`

> **Warning:** PyPI versions are immutable. A version number cannot be re-uploaded even if the release is deleted.