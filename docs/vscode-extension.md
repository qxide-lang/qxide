# VS Code Extension Releases

## Overview

- **Scope**: This guide covers local packaging, the GitHub Actions release workflow, versioning, and one-time registry setup for the extension in `editors/vscode`.
- **Distribution**: Releases attach a `.vsix` package to the GitHub release and can publish to Visual Studio Marketplace and Open VSX under publisher and namespace `qxide-lang`.

## Packaging locally

1. **Install dependencies and package**: Run `npm ci` and `npm run package` in `editors/vscode` to write `qxide-vscode.vsix`. This runs `vscode:prepublish`, which compiles TypeScript and runs `scripts/license.mjs` to generate `LICENSE.md` from the repository's `LICENSE-MIT` and `LICENSE-APACHE`.
2. **List packaged files**: Run `npx vsce ls` to inspect the files included in the package.

```bash
cd editors/vscode
npm ci
npm run package
npx vsce ls
```

- **Pinned dependencies**: `vsce` 4.1.1 and `ovsx` 1.4.0 are pinned `devDependencies`; nothing is installed globally.

## Release workflow

- **Workflow**: `.github/workflows/vscode.yml` (name `VS Code extension`) runs automatically after the cargo-dist `Release` workflow succeeds for a `vX.Y.Z` tag push, or manually via `gh workflow run`.

```bash
gh workflow run vscode.yml -f tag=vX.Y.Z
```

- **Job `package`**: Runs `npm test`, packages `qxide-vscode-<extension version>.vsix`, and uploads it as a workflow artifact.
- **Job `release-asset`**: Attaches the `.vsix` file to the GitHub release for the tag unless a file of that name is already attached.
- **Job `marketplace`**: Publishes with `vsce publish --packagePath <vsix> --skip-duplicate` when secret `VSCE_PAT` exists; otherwise publishes with `--oidc` (Trusted Publishing) when repository variable `VSCE_TRUSTED_PUBLISHING` is `true`; otherwise prints a notice and skips.
- **Job `open-vsx`**: Publishes with `ovsx publish <vsix> --skip-duplicate` when secret `OVSX_PAT` exists; otherwise skips.
- **Token handling**: `vsce` and `ovsx` read tokens from the `VSCE_PAT` and `OVSX_PAT` environment variables, so tokens never appear on command lines.

## Versioning

- **Independent version**: The extension `version` in `editors/vscode/package.json` is independent of the qxide version.
- **Publishing behavior**: Every qxide release attaches the current extension `.vsix` to the GitHub release. The marketplaces only receive a new version when `version` in `editors/vscode/package.json` was raised (with an entry in `editors/vscode/CHANGELOG.md`), because both registries reject re-publishing an existing version and the workflow skips duplicates.

## One-time setup: Visual Studio Marketplace

The publisher ID `qxide-lang` must match `publisher` in `editors/vscode/package.json`.

### Personal access token (PAT)

1. **Create an Azure DevOps organization**: Create an organization at `https://dev.azure.com` if you have none.
2. **Create a PAT**: In Azure DevOps, open User settings > Personal access tokens > New Token. Set Organization to "All accessible organizations", Scopes to "Custom defined", click "Show all scopes", select Marketplace > "Manage", and set an expiration. A token tied to one organization or without Marketplace (Manage) fails with 401/403.
3. **Create the publisher**: At `https://marketplace.visualstudio.com/manage`, sign in with the same Microsoft account, select "Create publisher", set ID to `qxide-lang` (cannot be changed later), and set Name to `Qxide` (or similar).
4. **Verify the token**: From `editors/vscode`, set `VSCE_PAT` in your shell and verify it; the commands prompt for the token without echoing it.

Bash:

```bash
cd editors/vscode
read -rsp 'Marketplace token: ' VSCE_PAT; echo; export VSCE_PAT
npx vsce verify-pat qxide-lang
```

PowerShell:

```powershell
cd editors/vscode
$env:VSCE_PAT = Read-Host -MaskInput 'Marketplace token'
npx vsce verify-pat qxide-lang
Remove-Item Env:VSCE_PAT
```

Note: `-MaskInput` requires PowerShell 7.1 or newer (`pwsh`).

5. **Add the repository secret**: Add `VSCE_PAT` in GitHub under Settings > Secrets and variables > Actions > New repository secret, or run `gh secret set VSCE_PAT` (prompts for the value):

```bash
gh secret set VSCE_PAT
```

### Trusted Publishing (recommended alternative)

- **PAT retirement**: Microsoft retires global Azure DevOps PATs (the "All accessible organizations" kind) on December 1, 2026, and the VS Code docs recommend Trusted Publishing instead.
- **Requirements and precedence**: With Trusted Publishing, no `VSCE_PAT` secret is needed; if both exist, `VSCE_PAT` is used. Creating the publisher itself still requires a Microsoft account, and whether the first publish works without any PAT was not tested.

1. **Create the Trusted Publishing policy**: At `https://marketplace.visualstudio.com/manage`, open publisher `qxide-lang` > Trusted Publishing > Create with Repository owner `qxide-lang`, Repository `qxide`, Workflow `vscode.yml`, and Environment empty.
2. **Set the repository variable**: Enable Trusted Publishing in GitHub Actions:

```bash
gh variable set VSCE_TRUSTED_PUBLISHING --body true
```

## One-time setup: Open VSX

1. **Sign in**: Sign in at `https://open-vsx.org` with a GitHub account.
2. **Sign the publisher agreement**: Link an Eclipse Foundation account and sign the Open VSX Publisher Agreement on the profile page (`https://open-vsx.org/user-settings/profile`).
3. **Create an access token**: Generate a token at `https://open-vsx.org/user-settings/tokens`.
4. **Create the namespace**: Run `ovsx create-namespace` once from `editors/vscode`. Optionally claim ownership of the namespace so extensions show as verified (see the Open VSX "Managing Namespaces" wiki).

Bash:

```bash
cd editors/vscode
read -rsp 'Open VSX token: ' OVSX_PAT; echo; export OVSX_PAT
npx ovsx create-namespace qxide-lang
```

PowerShell:

```powershell
cd editors/vscode
$env:OVSX_PAT = Read-Host -MaskInput 'Open VSX token'
npx ovsx create-namespace qxide-lang
Remove-Item Env:OVSX_PAT
```

5. **Add the repository secret**: Store the token as `OVSX_PAT`:

```bash
gh secret set OVSX_PAT
```

## Publishing an existing release

- **Manual dispatch**: After adding credentials, publish the extension for an existing release without creating a new tag:

```bash
gh workflow run vscode.yml -f tag=v0.8.0
```

## Credential hygiene

- **Environment and secrets**: Pass tokens through shell environment variables or GitHub repository secrets; do not write them into files or pass them on command lines.
- **Leaked tokens**: Never commit tokens or paste them into issues, logs, or chat; rotate any token that leaked.
