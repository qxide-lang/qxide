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

## PyPI Distribution (GitHub Actions)

### How It Works

- **Workflow:** `.github/workflows/pypi.yml` (workflow name: `PyPI`).
- **Package:** `qxide`. `pyproject.toml` uses `maturin` with `bindings = "bin"` and `dynamic = ["version"]`, so the wheel version always equals the `Cargo.toml` version.
- **Triggers:**
  1. `release` (`published`) — for releases created by a person.
  2. `workflow_run` — runs after the `cargo-dist` `Release` workflow completes successfully for a tag push. This is required because `cargo-dist` creates its GitHub Release with `GITHUB_TOKEN`, and GitHub does not start other workflows from events caused by `GITHUB_TOKEN`, so the `release` trigger alone would not fire for normal tag releases.
  3. `workflow_dispatch` — accepts a `tag` input (default `v0.4.1`) to publish an existing tag manually.
- **Jobs** (every job checks out the tag itself, not the branch):
  - `resolve`: verifies that the tag matches `vX.Y.Z` and equals `v` + the `Cargo.toml` version of the tagged commit; fails otherwise.
  - `wheels`: builds wheels with `PyO3/maturin-action@v1` for Linux `x86_64` and `aarch64` (`manylinux`), macOS `x86_64` and `aarch64`, and Windows `x86_64`.
  - `sdist`: builds the source distribution.
  - `publish`: downloads all artifacts and uploads them with `pypa/gh-action-pypi-publish@release/v1` using `password: ${{ secrets.PYPI_API_TOKEN }}`. Runs in the GitHub environment `pypi` with only `contents: read` and `id-token: write` permissions (`id-token: write` is unused for token uploads and is in place for Trusted Publishing).

### One-Time Setup: Register the Token

1. In the GitHub repository, go to **Settings → Secrets and variables → Actions → New repository secret**.
2. Set **Name** to `PYPI_API_TOKEN` and **Value** to the PyPI API token (starts with `pypi-`). Never commit the token or paste it into issues, logs, or chat.
3. *(Optional)* The `pypi` environment is created automatically on the first run. To require approval before each upload, add required reviewers under **Settings → Environments → `pypi`**.

### Publish Manually (e.g., Existing `v0.4.1` Tag)

The workflow file must be on the default branch (`main`) before it can be run.

**Web:**
1. Go to the **Actions** tab → `PyPI` → **Run workflow**.
2. Keep branch `main`, enter tag `v0.4.1`, and select **Run workflow**.

**CLI:**
```sh
gh workflow run pypi.yml -f tag=v0.4.1
gh run watch
```

**Verify after success:**
```sh
pip install qxide==0.4.1
qxide --version
```
`qxide --version` should print `qxide 0.4.1`.

### Automatic Publishing for Later Releases

1. Bump `Cargo.toml`, tag `vX.Y.Z`, and push the tag (see "Cutting a Release").
2. After the `Release` workflow succeeds, `PyPI` starts automatically.

### After the First Upload: Tighten Credentials

**Option A: Project-Scoped Token**
1. On PyPI, create a new API token scoped to the `qxide` project.
2. In the GitHub repository, go to **Settings → Secrets and variables → Actions → `PYPI_API_TOKEN` → update** and replace the secret value with the new token.
3. Delete the original account-wide token on PyPI.

**Option B: Trusted Publishing (No Stored Secret)**
1. On PyPI, open project `qxide` → **Manage → Publishing** and add a GitHub publisher:
   - Owner: `qxide-lang`
   - Repository: `qxide`
   - Workflow filename: `pypi.yml`
   - Environment: `pypi`
2. In `.github/workflows/pypi.yml`, delete the `password:` line (and its `with:` block) from the publish step (`id-token: write` is already granted).
3. Delete the `PYPI_API_TOKEN` secret in GitHub.
4. Revoke the token on PyPI.

### Notes

- PyPI versions are immutable: a file for a version cannot be uploaded again, even after deletion, so re-running the workflow for an already-published tag fails at upload.
- Local builds are for testing only: `maturin build --release` produces a wheel for the current platform only; do not upload from a local machine.

---

## VS Code Extension (GitHub Actions)

- **Workflow trigger**: `.github/workflows/vscode.yml` (`VS Code extension`) runs automatically after the cargo-dist `Release` workflow succeeds for a `vX.Y.Z` tag push, or manually with `gh workflow run vscode.yml -f tag=vX.Y.Z`.
- **Release asset**: The workflow runs `npm test`, packages `qxide-vscode-<version>.vsix`, and attaches it to the GitHub release for the tag unless a file of that name is already attached.
- **Marketplace publishing**: The `marketplace` and `open-vsx` jobs publish to Visual Studio Marketplace and Open VSX only when credentials (`VSCE_PAT` or `VSCE_TRUSTED_PUBLISHING` set to `true`, and `OVSX_PAT`) exist, and skip already-published versions.
- **Setup and versioning**: One-time registry setup, credential configuration, and extension versioning are documented in `docs/vscode-extension.md`.
