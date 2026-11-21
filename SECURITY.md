# Security Policy

## Reporting a vulnerability

Report security vulnerabilities privately through GitHub private vulnerability reporting for the [qxide-lang/qxide](https://github.com/qxide-lang/qxide) repository:

1. Open the repository page on GitHub.
2. Go to the **Security** tab.
3. Select **Report a vulnerability**, or open https://github.com/qxide-lang/qxide/security/advisories/new directly.

Do not open public issues, discussions, or pull requests for security vulnerabilities.

## What to include

Include the following details in your report:

- The affected component and version (for the CLI, include the output of `qxide --version`).
- The input or steps required to reproduce the issue.
- The impact you expect.
- Whether the issue is already public elsewhere.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.7.x (latest release: 0.7.0) | Yes |
| < 0.7.0 | No |

Qxide is pre-1.0. Security fixes are released in a new patch or minor version and are not backported to older minor versions.

## Scope

### In scope

- The `qxide` crate and CLI (crates.io, PyPI wheels, and GitHub Release installers)
- The `.qx` language toolchain
- The package manager
- The playground at `qxide.dev`
- The registry tooling

Problems in a package published to the registry (malicious or vulnerable package content) should be reported through the same private vulnerability reporting process and may lead to the package being yanked.

### Out of scope

- Social engineering
- Physical attacks
- Denial-of-service against `qxide.dev` or GitHub infrastructure
- Findings that require an already-compromised machine

## Testing guidelines

- Do not test against `qxide.dev` or the live registry beyond normal use.
- Reproduce issues locally.

## Disclosure and credit

- There is no bug bounty program.
- Credit will be included in the advisory if the reporter wants it.
