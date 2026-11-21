# Packages, modules and the registry

Status: Implemented on the `packages` branch (2026-11-20), not merged into `main`, and not released. The registry repository `qxide-lang/registry` does not exist yet; its contents are prepared in `registry-template/`.

## 1. Goals and non-goals

**Goals**

- Reuse `.qx` code across files within a project and across separate projects.
- Publish and fetch packages through a workflow comparable to crates.io or PyPI.
- Host the initial registry as a GitHub repository index (similar to early Cargo or Homebrew), with no server to operate, no hosting cost, and no user accounts or passwords; publishing happens through pull requests reviewed by maintainers.
- Design the index format and URL layout so the same data can later be served by a dedicated registry server without changing lock file semantics or the index format.

**Non-goals (current implementation)**

- Exporting structs, types, or constants.
- Multiple versions of the same package in a single build.
- Dependency renaming.
- Private or authenticated registries.
- A web UI.
- `qxide yank` and `qxide login` commands.
- Build scripts.

## 2. Project layout and manifest (`qxide.toml`)

A Qxide package is a directory containing a `qxide.toml` manifest and `.qx` source files under `src/`:

```text
my-app/
  qxide.toml
  qxide.lock          (generated)
  src/
    main.qx           program entry (has `void main()`)
    lib.qx            package root module (library packages)
    gates.qx          module `my_app.gates`
    algo/grover.qx    module `my_app.algo.grover`
  target/package/     archives built by `qxide publish` (not committed)
```

An example `qxide.toml`:

```toml
[package]
name = "bell-utils"
version = "0.1.0"
description = "Bell-pair helpers for Qxide"
license = "MIT OR Apache-2.0"
authors = ["Qxide contributors"]
repository = "https://github.com/qxide-lang/qxide"
qxide = ">=0.6"

[dependencies]
other-pkg = "0.3"                                   # caret requirement, like Cargo: ^0.3
local-pkg = { version = "0.1", path = "../local-pkg" }
dev-only  = { path = "../dev-only" }                # path-only: cannot be published
```

**Manifest rules**

- `[package]` with `name` and `version` is required.
- `version` must be a valid semver string (for example `1.2.3`).
- `qxide` is a semver requirement on the `qxide` language and tool version, checked by `run`, `check`, `qasm`, and `install`. This binary is version `0.7.0`; `>=0.6` is recommended because a caret requirement such as `^0.6` would reject `0.7`.
- `description`, `license`, and `qxide` are required when publishing a package.
- `authors` (array of strings) and `repository` are optional metadata.
- Unknown keys are ignored for forward compatibility.
- Package names consist of lowercase ASCII letters, digits, and single hyphens. They must start with a letter, must not end with a hyphen, are at most 64 characters long, and cannot contain underscores.
- In `.qx` source files, a package is addressed by its import name, formed by replacing hyphens with underscores (`bell-utils` becomes `bell_utils`). Because underscores are disallowed in package names, the mapping between package names and import names is one-to-one.
- Dependency values are either a version requirement string or an inline table with `version` and/or `path`. Bare version strings are caret requirements (`"0.1.0"` is equivalent to `^0.1.0`).
- A `path` is relative to the project directory and may use `..` (for example `../local-pkg`). Absolute paths, paths starting with `/` or `\` (including Windows network paths such as `//host/share`), and paths containing `:` (drive letters) are rejected with `Q0602`, and the same `path+` sources in `qxide.lock` with `Q0603`, so a cloned project cannot point `qxide` at an arbitrary directory or network share by an absolute path.
- `qxide add` and `qxide remove` edit `qxide.toml` using a format-preserving TOML editor (`toml_edit`), preserving comments and layout.

## 3. Modules and import syntax

Each `.qx` file under `src/` defines a module. A module path is the package's import name followed by the file's path relative to `src/`, without the `.qx` extension and with `/` replaced by `.`.

- `src/lib.qx` is the package root module and is addressed by the package import name alone (`bell_utils`). `pkg.lib` also resolves to the root module.
- `src/main.qx` is the program entry point and is not intended to be imported.
- A module within the current package is addressed using the current package's own import name (fully qualified, Java style): inside package `my-app`, another file imports `entangle` from `src/gates.qx` as `import my_app.gates.entangle;`.

Implemented import syntax:

```java
import bell_utils.bell_pair;          // one public function from the package root (src/lib.qx)
import bell_utils.gates.entangle;     // one public function from module bell_utils.gates (src/gates.qx)
import bell_utils.gates.*;            // every public function of a module

void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    bell_pair(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}
```

Grammar additions:

```ebnf
program = { import } { fn_decl } ;
import  = "import" IDENT { "." IDENT } ( "." IDENT | "." "*" ) ";" ;
fn_decl = [ "public" ] type IDENT "(" [ param { "," param } ] ")" block ;
```

`import` and `public` are keywords; identifiers named `import` or `public` are no longer accepted (this is the only source-level incompatibility). All `import` declarations must appear before the first function declaration (`Q0510`). An import path must name at least a module and a function or `.*` (`Q0509`), so `import bell_utils;` is rejected.

### Alternatives considered

| Option | Syntax | Pros | Cons |
| --- | --- | --- | --- |
| A. Single-function and wildcard imports | `import pkg.module.function;`, `import pkg.module.*;`, unqualified calls `f(q)` | Same shape as Java's `import a.b.C;` / `import a.b.*;`, matching the Java-style declarations adopted in 0.5.0; call sites remain unchanged so gate-style calls `bell_pair(a, b)` read like built-ins; all imported names are listed at the top of the file; requires no change to expression parsing. | Names can clash (handled by `Q0506`/`Q0507`); a reader must check the imports at the top of the file to see where a function comes from. |
| B. Module import with qualified calls | `import bell_utils;`, calls `bell_utils.bell_pair(a, b)` | Origin of each function is visible at every call site. | `a.b(...)` already parses as a method call on a value (`r.as_int()`, `r.to_binary()`), requiring a namespace-vs-value resolution rule in the parser and checker; longer call sites. |
| C. Rust-style imports | `use pkg::module::f;`, `mod` | Familiar to Rust users. | `use` and `mod` are reserved words for other plans, and `::` does not fit the Java-style syntax. |
| D. Aliased imports | `import pkg.f as g;` | Avoids name collisions locally. | Not part of Java; deferred. |

**Recommendation and decision:** Option A is implemented. Qualified calls (Option B) can be added later without breaking Option A.

## 4. Visibility, name resolution, conflicts and cycles

### Visibility

Functions are private to their defining module unless marked `public` (for example, `public void prepare(Qubit q) { ... }`).

- Only `public` functions can be imported. Explicitly importing a private function fails with `Q0504`, including a note at the function's declaration and a help message suggesting `public`.
- Wildcard imports (`import pkg.module.*;`) silently skip private functions.
- Marking a function `public` in the entry file is permitted and has no effect.
- Private helper functions with the same name in different modules do not collide.

### Name resolution and conflicts

Inside a module, function calls are resolved in this order:

1. Functions defined in the module itself.
2. Single-function imports.
3. Wildcard imports.

Conflict rules:

- Importing a function whose name matches a function defined in the same module is an error (`Q0506`).
- Two single-function imports bringing different functions under the same name are an error (`Q0506`). Importing the exact same function twice is allowed.
- A call whose name is provided by more than one wildcard import is an error at the call site (`Q0507`), reported only if the ambiguous name is actually called. An explicit single-function import of that name resolves the ambiguity.
- Calls to functions that cannot be resolved remain `Q0202`.

### Import cycles

The module import graph must be acyclic. Cycles are detected with a depth-first search while loading modules. When a cycle is found, `Q0505` is reported at the import that closes the cycle, with `note:` lines pointing to the other imports in the cycle and the cycle chain in the message (for example, `my_app.a -> my_app.b -> my_app.a`).

Diamond dependencies (two modules importing a shared third module) are permitted. When the checked entry file is itself a module (such as `qxide check src/lib.qx`), importing that module back also forms a cycle and is rejected.

Disallowing module cycles keeps module loading order well defined and leaves room for separate compilation with the planned typed IR. Mutual or direct recursion between functions remains allowed within a single module.

### Linking

`src/lang/link.rs` lexes and parses the entry file along with every transitively imported module. It then rewrites each call target to a qualified name (such as `bell_utils.bell_pair` or `bell_utils.gates.entangle`; functions in the entry file keep bare names) and merges all functions into a single program. Because `.` cannot appear in identifiers, generated qualified names cannot collide with user-defined names.

The existing checker, interpreter, and OpenQASM 3 exporter run unchanged on the merged program. Program output and generated OpenQASM 3 are identical to writing the same functions in a single file (a test verifies byte-identical OpenQASM 3 output).

All loaded files share a single byte-offset space for source spans: the entry file starts at offset 0, and each subsequent file starts after the previous file plus a one-byte gap. Diagnostics in imported files therefore report accurate file names, lines, and columns, and diagnostic notes can point across files. Lexer and parser errors in imported files are reported in those files. The loader caps a module graph at 4096 modules. Single-file programs without imports follow the original code path unchanged.

## 5. Linearity across package boundaries

The function signature is the linearity contract across module and package boundaries:

- A `Qubit` or `QReg` parameter without `consume` is borrowed: the callee may only apply gates or `reset`, while the caller retains ownership and must still `measure` or `discard` it (`Q0302` if leaked).
- A parameter declared with `consume` (for example `consume Qubit q`) moves ownership into the callee; subsequent use in the caller fails with `Q0301`.
- A function returning `Qubit` or `QReg` transfers ownership of the returned quantum value to the caller.

Each callee body is checked once in its own module against its own signature. For example, if a dependency discards a borrowed parameter, `Q0313` is reported in the dependency's source file.

The rule restricting calls inside an `if` conditioned on a measured `Bit` (`Q0314`) is computed over the merged call graph, so it sees through imported functions. Duplicate-qubit checks (`Q0303`) apply to imported functions in the same way as local functions.

Because checking currently operates on source, a future separate-compilation mode would export these signature properties (parameter ownership, quantum return types, and conditional-call safety) in an interface file.

## 6. Lock file (`qxide.lock`)

`qxide.lock` pins the resolved package graph:

```toml
# This file is automatically generated by qxide. Do not edit by hand.
version = 1

[[package]]
name = "bell-app"
version = "0.1.0"
dependencies = ["bell-utils"]

[[package]]
name = "bell-utils"
version = "0.1.0"
source = "registry+https://raw.githubusercontent.com/qxide-lang/registry/main/"
checksum = "sha256:d18afc9d9e5b2362b5ddf0db2007096f3556d38f3efe96d02b055e9718f7fc90"
dependencies = []
```

**Format and freshness rules**

- Each package in the resolved graph has one `[[package]]` entry, including the root package (which has no `source` field).
- Package entries are sorted by `name`, each package's `dependencies` list is sorted, and the file is written with `\n` line endings in a deterministic field order so identical graphs always produce identical bytes.
- `source` is either `registry+<registry URL>` or `path+<path relative to the project root>`.
- Registry packages must include `checksum = "sha256:<64 hex>"`. Path packages have no `checksum`.
- Applications should commit `qxide.lock`.
- A lock file is considered fresh when all of the following hold:
  1. the root package's direct dependencies match `qxide.toml` exactly,
  2. every locked version satisfies every requirement on it: requirements from the root manifest, from path packages as their `qxide.toml` exists on disk now, and from registry packages (`qxide run`, `qxide check`, and `qxide qasm` read a registry package's requirements from its unpacked `qxide.toml` in the cache, reporting a package that is not yet unpacked as `Q0611` instead; `qxide install` reads them from the unpacked package or otherwise from the registry index before downloading anything),
  3. registry packages come from the active registry URL,
  4. path packages still have the same name, version, and dependency list on disk, and a package is locked as a path package exactly when some package requires it by path, and
  5. every referenced dependency has a corresponding locked entry, every locked package is reachable from the root (so `qxide install` never fetches unused entries), and no package depends on itself or on the root.
- Package names and dependency names in `qxide.lock` must be valid package names; otherwise the lock file is invalid (`Q0603`), because names become paths in the local cache.
- A stale lock file is re-resolved by `qxide install`; `qxide install --locked` fails with `Q0612`.
- `qxide run`, `qxide check`, and `qxide qasm` require a fresh lock whenever the project has dependencies, failing with `Q0611` if the lock file is missing or a package is not unpacked, or `Q0612` if the lock file is stale. These commands never access the network.

## 7. Version resolution

Resolution is implemented in `src/pkg/resolve.rs`. A build contains at most one version per package name.

The resolver iterates over the dependency graph:

1. For each package, collect every version requirement imposed by the root manifest and by all currently chosen dependency versions.
2. Select a version that satisfies every requirement, is not yanked (unless the existing lock file already pins that version), and whose `qxide` requirement accepts the current tool version.
3. Among valid candidates, prefer the version pinned in `qxide.lock`, then the currently chosen version, and otherwise the newest matching version.
4. Repeat until the chosen set stabilizes, up to a maximum of 256 rounds.

The resolver does not backtrack. Resolution errors map to specific codes:

- Unknown package: `Q0604`.
- No version satisfies the requirements: `Q0605` (diagnostic notes list the requirements and available versions).
- Incompatible requirements, or any package depending on itself (the project, a path package, or a registry package whose index entry lists itself, directly or reached transitively): `Q0606`.
- Only versions requiring a different `qxide` version exist: `Q0613`.

Adding a new dependency preserves already locked versions of existing dependencies. Deleting `qxide.lock` and running `qxide install` re-resolves the graph to the newest matching versions. `qxide remove` prunes unreachable packages from `qxide.lock` without contacting the registry.

## 8. Registry index format

A registry uses the same layout whether accessed as a local directory, a `file://` URL, or an `https://` URL:

```text
config.json                      {"dl": "packages/{prefix}/{name}/{name}-{version}.tar.gz", "api": null}
index/<prefix>/<name>            one JSON object per line, one line per published version
packages/<prefix>/<name>/<name>-<version>.tar.gz   archives (GitHub-hosted registry)
owners/<prefix>/<name>           package owners, one JSON object per line (GitHub-hosted registry, see section 10)
search.json                      generated list for `qxide search`
```

`<prefix>` follows Cargo's sparse-index sharding scheme:

| Name length | `<prefix>` | Example path |
| --- | --- | --- |
| 1 | `1` | `index/1/a` |
| 2 | `2` | `index/2/ab` |
| 3 | `3/<first letter>` | `index/3/a/abc` |
| 4 or more | `<first two>/<next two>` | `index/be/ll/bell-utils` |

Each line in `index/<prefix>/<name>` is a JSON object with sorted keys, representing one published version (shown exactly as printed by `qxide publish`):

```json
{"cksum":"d18afc9d9e5b2362b5ddf0db2007096f3556d38f3efe96d02b055e9718f7fc90","deps":[],"description":"Bell-pair helpers for Qxide","name":"bell-utils","qxide":">=0.6","v":1,"vers":"0.1.0","yanked":false}
```

**Index entry fields**

- `name`: package name.
- `vers`: semver version string without build metadata (no `+...` suffix), since `1.0.0+x` has the same precedence as `1.0.0` and could otherwise stand in for a published version.
- `deps`: array of `{"name", "req"}` objects.
- `cksum`: 64-character lowercase hex SHA-256 digest of the package archive.
- `qxide`: optional `qxide` version requirement.
- `description`: optional package description (used by search); registry CI rejects control characters in it.
- `dl`: optional absolute or registry-relative download URL overriding `config.json`'s `dl` template (for example, a GitHub Release asset). Registry CI still requires a copy of the archive in the repository (at the template path, or at the default `packages/...` path when the template is a URL) and checks it; clients verify `cksum`, so they can only install those bytes.
- `yanked`: boolean indicating whether the version is yanked.
- `v`: index schema version (`1`).

Unknown keys in index entries are ignored by the client. Download URL templates expand `{name}`, `{version}`, `{prefix}`, and `{sha256-checksum}`. Relative URLs are resolved against the registry root.

**Decision:** The client skips index lines it cannot use instead of failing the whole package. A line is unusable when it is not valid JSON, has a field of the wrong type (a non-string `qxide`, `description`, or `dl`, or a non-array `deps`), has an invalid name, version, checksum, or version requirement, carries build metadata, names another package, or repeats a version already listed earlier in the file (the first line for a version wins, because published lines are immutable). Skipped lines can never be selected and the package's other versions stay installable; a lock file that pins a skipped version cannot fetch it. Only an index file without a single usable line fails, with `Q0609`. Registry CI rejects such lines before they are merged, and one malformed line must not make every version of a package uninstallable (Cargo also skips index lines it cannot parse).

The default registry URL is `https://raw.githubusercontent.com/qxide-lang/registry/main/`. It can be overridden with `--registry <url|path>` or the `QXIDE_REGISTRY` environment variable. Plain `http://` URLs are refused (`Q0608`).

## 9. Archives and checksums

### Archive storage decision

Package archives are committed directly into the registry repository under `packages/` in the same pull request as the corresponding index line.

- Reviewers inspect the exact archive bytes that users will download.
- Availability is controlled by the registry rather than external author repositories, so deleting an author repository or release cannot break downstream builds.
- GitHub's automatically generated tag tarballs are not used because their bytes (and therefore SHA-256 digests) are not guaranteed to remain stable over time.

The index format also supports a per-entry `dl` URL (such as a GitHub Release asset), though `qxide publish` does not automate that mode. `.qx` packages are small (the `bell-utils` example archive is 642 bytes), and the client caps archive downloads at 10 MiB.

### Archive format

Archives are deterministic gzip-compressed tar files:

- Every entry path sits under `<name>-<version>/`.
- Every path component must be portable: non-empty, with no trailing dot or space, no Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM1`-`COM9`, `LPT1`-`LPT9`, including the superscript-digit forms, with or without an extension), and none of the characters `<>:"/\|?*` or control characters. No two entries may be equal after case folding, and no file may also be a directory of another entry, so that nothing overwrites a reviewed file on case-insensitive file systems such as Windows and macOS. `qxide publish` refuses a package whose files break these rules (`Q0614`) and skips top-level `README*` or `LICENSE*` entries that are symbolic links.
- Included files are `qxide.toml`, every `.qx` file under `src/`, and top-level `README*` and `LICENSE*` files. Nothing else is packaged.
- Entries are sorted by path, with `mtime` 0, `uid` and `gid` 0, file mode `0644`, and gzip header `mtime` 0.
- `.qx` files and `qxide.toml` are normalized to `\n` line endings before archiving.

The same source files produce identical archive bytes and the same SHA-256 checksum on every operating system (publishing the example package twice yields the same digest).

## 10. Publishing flow

### Client workflow

`qxide publish [--dry-run] [--registry <url|path>]` performs the following steps:

1. Validates `qxide.toml`: `description`, `license`, and `qxide` must be present; the package `version` must not carry build metadata; every dependency must specify a `version` requirement (path-only dependencies are rejected); and `src/lib.qx` must exist.
2. Runs `qxide check` on `src/lib.qx` with its dependencies (library packages do not require `main`).
3. Builds the deterministic archive at `target/package/<name>-<version>.tar.gz`.
4. Prints the packaged file list, `sha256 <hex>`, the target index path, and the index JSON line.

If `--dry-run` is passed, `qxide publish` stops after printing this summary. Otherwise:

- **Local directory registry** (for example, a local clone of the registry repository): appends the index line (rejecting duplicate versions with `Q0614`) and writes the archive to its resolved `dl` path.
- **Default GitHub registry with `gh` available and authenticated**: forks `qxide-lang/registry`, shallow-clones the fork into a temporary directory, creates branch `publish/<name>-<version>` from upstream `main`, writes the index line and archive (and, for the first version of a package, the owner file listing the account `gh` is logged in as, with the id and login from `gh api user`), commits, pushes, and opens a pull request via `gh pr create`. This automated `gh` path has not yet been run against GitHub (tests never invoke `gh`).
- **Otherwise**: prints the files to add (index line, archive, and for a new package the owner file) and where to open the pull request manually.

### Registry validation and CI (`registry-template/`)

`registry-template/` contains the automation prepared for the registry repository:

- `scripts/validate.py` (using only the Python standard library) runs on every pull request and push and verifies the following. Pull requests are checked by the `validate.yml` workflow with `pull_request_target`, so the workflow and `validate.py` come from the target branch; the pull request is checked out only as data and nothing from it is executed.
  - Index files live at the correct `<prefix>/<name>` path and use valid package names.
  - Every line is accepted by the client: valid JSON containing the required keys, a valid package name, a 64-hex checksum, `v` equal to the integer `1` (not `true`), `vers` without build metadata, string `description` and `dl` fields, no control characters in `description`, and `vers`, dependency `req`, and `qxide` values checked with a port of the `semver` crate parser the client uses (64-bit numbers, no leading zeros, and the same requirement syntax); the shared case list `tests/data/semver_cases.json` in the `qxide` repository is checked against both parsers.
  - No version appears more than once, compared by semver precedence.
  - Every download location, after template expansion, is an `https://` URL or a plain relative path inside the registry.
  - Every version's archive exists in the repository, also when its `dl` is an `https://` URL; its SHA-256 matches `cksum`; and it is read the way the client reads it: exactly one gzip member with nothing after it, at most 66 MiB decompressed, raw tar headers with valid checksums and plain octal sizes, GNU long names of at most 4096 bytes, and no PAX, GNU long-link, or other extension headers or data after the end-of-archive block. It contains only regular files and directories under `<name>-<version>/`, enforcing the same path rules as the client (no absolute, `..`, drive-letter, or backslash paths, only portable components, and no entries that collide after case folding), with at most 2000 files (the check stops reading at the limit), at most 10 MiB compressed, and at most 50 MiB unpacked.
  - Each archive contains `qxide.toml` whose `name` and `version` match the index entry and which defines `description`, `license`, and `qxide`, whose `[dependencies]` equal the entry's `deps` (names and requirements compared as the client's `semver::VersionReq` values, with the shared pairs in `tests/data/semver_cases.json`), and contains exactly `src/lib.qx`.
  - Every dependency listed in an entry names a package present in the registry, and no entry depends on its own package.
  - Index file names are not Windows device names (`con`, `nul`, `com1`, and so on).
  - Malformed input (invalid UTF-8, JSON or TOML nested deeper than the client accepts, numbers too long to be versions) is reported as an error instead of stopping the script.
  - Owner files are well formed (see Package owners) and exist only for packages that have an index file.
  - Against the pull request's base branch, with the pull request author from the workflow event: every package whose index file, archives, or owner file the pull request changes is owned by the author on the base branch, or is new and gets an owner file listing exactly the author (see Package owners); every file added under `packages/` is the archive of a version in the index, and new versions keep their archive at the default path; a pull request that changes `index/`, `packages/`, or `owners/` changes nothing else.
  - Against the pull request's base branch: previously published lines are unchanged, except that an owner may set `yanked` from false to true; no version or index file is removed; no archive under `packages/` is modified or deleted; `search.json` is not manually edited; `config.json` is not changed in a pull request; every changed path can be checked out on Windows; and a new package name is not a look-alike of another package, existing or added in the same pull request, nor reserved (names are compared after removing hyphens and folding look-alike digits, `i`, `rn`, `vv`, and `cl`; names starting like `qxide` and the name `std` are reserved).
  - Outputs a summary listing the files inside each newly added archive for reviewers; file names and messages are escaped so that they cannot add Markdown or new lines to the summary or the job log.
- `scripts/build_search.py` regenerates `search.json` with the newest non-yanked version and description for each package, where newest is determined by semver precedence (numeric pre-release identifiers compare as numbers, so `1.0.0-rc.10` is newer than `1.0.0-rc.2`). The `search.yml` workflow runs it after pushes to `main` that touch `index/` and commits only `search.json` as `github-actions[bot]`. The `search.yml` workflow runs it after pushes to `main` that touch `index/` and commits only `search.json` as `github-actions[bot]`.
- `.gitattributes` keeps index files LF-terminated and marks archives as binary.
- Human review on pull requests covers name squatting, malicious or misleading packages, and licensing issues according to the policy documented in the registry `README`.

### Package owners

The GitHub registry records which accounts may change a package in `owners/<prefix>/<name>`, with the same `<prefix>` as the index. Each line names one owner, or one invited account with `"pending": true`:

```json
{"id":295735711,"login":"rigelx2b"}
{"id":1002,"login":"bob","pending":true}
```

- `id` is the GitHub user id, and it is the only value CI compares. `login` is there for people reading the file. A login can be renamed, and a freed login can be registered by another account, so a login grants nothing. `gh api users/<login> --jq .id` prints an account's id.
- A file has 1 to 32 lines, each id once, and at least one owner who is not only invited. `pending` is either absent or `true`; no other keys are allowed. An invited account has no rights in the package until it accepts. An owner file exists only for a package that has an index file. The client does not read owner files.

`validate.py --base` applies these rules to pull requests:

- **Pull request author.** The workflow passes `github.event.pull_request.user.id` and `.login` from the `pull_request_target` event. GitHub sets them to the account that opened the pull request. Later pushes do not change them, and nothing in the pull request can set them. The workflow does not use `github.actor`, which is whoever triggered the run (for example a re-run), or commit author fields, which anyone can write. Without an author, `--base` fails.
- **New package** (no index file on the base branch). The pull request must add the owner file, listing exactly its author. `qxide publish` writes this file from the next release on; with `qxide` 0.7.0 and earlier, add it by hand (the registry README and pull request template say how). The first account to publish a name owns it.
- **Existing package.** A pull request that changes the package's index file, adds one of its archives, or changes its owner file must be opened by an owner listed in the owner file on the base branch (invited accounts do not count). Owners added by the pull request itself do not count either, so an account cannot add itself and publish in the same pull request.
- **Inviting and accepting.** An account becomes an owner in two steps. First an owner opens a pull request that adds the account with `"pending": true`. After that is merged, the invited account opens its own pull request that removes `pending` from its line (and may update its `login`); that pull request may change nothing else, in the owner file or elsewhere. No pull request can add an owner directly, and an owner cannot accept for someone else.
- **Removing owners and transferring.** An owner can remove owners and withdraw invitations. To transfer a package, the owner invites the new owner, who accepts, and then either of them removes the old owner. The file cannot be left without an owner or deleted.
- **Packages without an owner file.** A package that has an index file but no owner file on the base branch is locked: no pull request can change it until a maintainer adds the owner file.
- **Archives.** Every file that a pull request adds under `packages/` must be the archive of a version in the index, and a version added by a pull request keeps its archive at the default path, which contains its package name (a relative `dl` elsewhere is rejected; an `https` `dl` still needs the copy at the default path). Without this, an account could place a file at the path of another package's next version and block that publication.
- **Bots.** A pull request opened by a GitHub App (such as `dependabot[bot]`) passes the author check but owns no package.
- **Maintainers.** Disputes, lost accounts, abuse, and restoring yanked versions are handled by maintainers in commits to `main`; CI has no maintainer list. The push run checks the format of owner files but not who made the change.

Registries created before owner files have none. The push run does not require them, so a maintainer adds one owner file per existing package (listing the account that opened the pull request that published its first version) in a commit to `main`, together with the updated scripts. Each package stays locked until its owner file exists.

An owner who opens a pull request from a branch that other accounts can push to is responsible for what they push: the author stays the same, so CI accepts their commits too.

### Yanking

An owner yanks a version with a pull request that changes `"yanked": false` to `true` on that version's index line (the ownership rules above apply, as for any change to the index file). CI rejects the other direction: restoring a yanked version, and yanking by anyone else, are done by a registry maintainer in a commit to `main`. Versions are never deleted or overwritten.

Yanked versions are skipped during fresh version resolution and by `qxide add`, while projects whose `qxide.lock` already pins a yanked version continue to install it.

## 11. Installing and the local cache

### `qxide install`, `qxide add`, and `qxide search`

- `qxide install [--locked] [--offline] [--registry <url|path>]`:
  - Uses `qxide.lock` directly if it is fresh. Otherwise resolves the dependency graph and writes `qxide.lock` (unless `--locked` is set, which fails with `Q0612`).
  - Fetches any registry package not yet unpacked locally:
    1. If a cached archive exists and its SHA-256 still matches the lock, it is reused.
    2. Otherwise the client verifies that the registry index entry lists the same checksum as `qxide.lock` (`Q0607` before downloading if they differ), downloads the archive, and checks the downloaded bytes' SHA-256 against the lock (`Q0607` on mismatch, unpacking nothing).
    3. Unpacks the archive defensively into a temporary directory alongside the destination, renames it into place, and writes a `.qxide-checksum` marker file last.
  - A package counts as unpacked only if its marker holds the locked checksum, the cached archive still matches that checksum, and the unpacked files are exactly the archive's files (compared with a fresh unpack of the archive on every `run`, `check`, `qasm`, and `install`). Edited, added, or removed files in the cache, or a missing cached archive, make the package not unpacked (`Q0611` for `run`, `check`, and `qasm`); `qxide install` then unpacks it again, downloading the archive if needed.
- `qxide add <pkg>[@<req>] [--path <dir>]`:
  - Selects the newest non-yanked version compatible with the current `qxide` version (writing, for example, `bell-utils = "0.1.0"`), or adds a path dependency when `--path` is supplied, and then runs installation.
  - If installation fails, the edit to `qxide.toml` is rolled back.
- `qxide search <word>`:
  - Performs a case-insensitive substring match against package names and descriptions using `search.json`, or by scanning all index files for a local directory registry without `search.json`.
  - Prints matches in the form `bell-utils = "0.1.0" # Bell-pair helpers for Qxide`.

### Running inside a project

When running `qxide run`, `qxide check`, or `qxide qasm`:

- If a file argument is given, `qxide` locates the nearest `qxide.toml` above that file.
- If no file argument is given, `qxide` locates the project around the current working directory and uses `src/main.qx`.
- Diagnostics display project module paths relative to the current working directory.
- A standalone `.qx` file outside any project continues to work as before, but cannot use `import` (`Q0508`).

### Local cache layout

The cache root is `$QXIDE_HOME`, defaulting to `~/.qxide` (resolved from `HOME` or `USERPROFILE`):

```text
~/.qxide/registry/index/<registry id>/...                 copies of fetched index files (for --offline)
~/.qxide/registry/cache/<registry id>/<name>-<version>.tar.gz
~/.qxide/registry/src/<registry id>/<name>-<version>/      unpacked packages read by run/check/qasm
~/.qxide/registry/tmp/                                     files being downloaded by curl, removed afterwards
```

`<registry id>` combines a readable host segment with the first 16 hex digits of the SHA-256 of the registry URL so data from different registries never collides.

## 12. Security

- **Checksum verification at three points:** SHA-256 digests are mandatory and verified by registry CI against the committed archive, by the client comparing the index entry against `qxide.lock` before downloading, and by the client hashing the downloaded archive against `qxide.lock` before unpacking.
- **Immutability:** Once published, a version's index entry and archive cannot be overwritten or deleted; CI rejects any change to a published line in a pull request except an owner setting `yanked` to true, and only maintainers restore yanked versions.
- **Defensive extraction:** Archives are verified prior to unpacking and extracted into a temporary directory with path, entry-type, size (10 MiB compressed, 50 MiB unpacked), and file-count (2000 files) restrictions, rejecting `..`, absolute paths, drive letters, backslashes, symlinks, and hard links; entry paths must be portable and must not collide after case folding (see Archive format), and every entry is checked before the first file is written.
- **Transport:** Plain `http://` registries are refused; HTTPS requests require TLS 1.2 or higher.
- **Index trust:** The index is trusted as served by GitHub over TLS. Cryptographic index signatures (such as signed index commits or TUF-style metadata) are not implemented yet and remain a follow-up.
- **Ownership:** Only accounts listed in a package's owner file on the registry's `main` branch can publish its new versions or change its owners; CI matches the numeric GitHub user id that GitHub reports for the pull request author (see Package owners).
- **Moderation:** Name squatting and malicious packages are mitigated through pull-request review, look-alike name checks in CI, and yanking.
- **No install-time code execution:** Packages have no build scripts and cannot execute arbitrary code during installation; `.qx` code runs only inside the `qxide` simulator.

## 13. Offline use and caching

Passing `--offline` or setting `QXIDE_OFFLINE=1` restricts `add`, `install`, and `search` to cached index files and cached archives under `$QXIDE_HOME`. Any missing index file or archive fails with `Q0615`.

Local directory registries and `file://` registries are always local. `qxide run`, `qxide check`, and `qxide qasm` never perform network access.

### Network transport

HTTPS requests invoke the system `curl` executable with `--proto =https --tlsv1.2`, `--globoff` (so `{}` and `[]` in URLs are taken literally), a 120-second timeout, and a 10 MiB transfer size limit. `curl` writes to a freshly created file under `$QXIDE_HOME/registry/tmp/`, not the shared system temporary directory, and the file is removed afterwards. A response is accepted only when `curl` exits successfully with HTTP 200, so a transfer cut off midway (`curl` exit 18 after a 200 status) fails with `Q0608` and nothing is cached. HTTP 404 is treated as "not found"; all other network or HTTP failures report `Q0608`.

**Decision:** No HTTP or TLS library is linked into `qxide` for now, keeping the binary small and avoiding additional dependencies. `curl` is preinstalled on Windows 10+, macOS, and most Linux distributions. A native HTTP client can be added later behind a Cargo feature.

## 14. Moving to a self-hosted server

Migrating from the GitHub repository to a dedicated registry server does not require changing client lock semantics or the index format:

- A server exposes the same relative endpoints (`config.json`, `index/<prefix>/<name>`, archives, and `search.json`), so clients only change the registry URL.
- Updating the `dl` template in `config.json` redirects archive downloads without rewriting any index entries.
- Existing index lines and archive SHA-256 checksums remain valid, so data migration consists of copying `index/` and `packages/`.
- The `api` field in `config.json` (currently `null`) is reserved for a future HTTP publish API (for example, token-based `qxide publish` instead of pull requests).
- Because `qxide.lock` records the registry URL in `source`, a project moving to a new registry re-resolves on the next `qxide install`; checksums of identical archives remain unchanged.

## 15. CLI reference

| Command | Behavior |
| --- | --- |
| `qxide add <pkg>[@<req>] [--path <dir>] [--registry <url>] [--offline]` | Add a registry or path dependency to `qxide.toml` and install it (reverts `qxide.toml` on failure). |
| `qxide remove <pkg>` | Remove a dependency from `qxide.toml` and prune unreachable packages from `qxide.lock` without contacting the registry. |
| `qxide install [--locked] [--offline] [--registry <url>]` | Resolve and lock dependencies if needed, then download and unpack missing registry packages. |
| `qxide publish [--dry-run] [--registry <url>]` | Validate, check `src/lib.qx`, package into `target/package/`, and publish to a local directory registry, open a GitHub PR via `gh`, or print manual instructions. |
| `qxide search <word> [--registry <url>] [--offline]` | Search package names and descriptions case-insensitively. |
| `qxide run\|check\|qasm [FILE]` | Run, type-check, or export OpenQASM 3 for `FILE`, or `src/main.qx` in the current project if omitted. |

`--registry` accepts either a URL (`https://` or `file://`) or a local directory path.

**Environment variables**

- `QXIDE_REGISTRY`: overrides the default registry URL.
- `QXIDE_HOME`: overrides the local cache directory (default `~/.qxide`).
- `QXIDE_OFFLINE`: when set to `1`, enables offline mode (equivalent to `--offline`).

**Exit codes**

- `0`: success.
- `1`: package or program error.
- `2`: CLI usage error.

**Example session**

```text
$ qxide add bell-utils
       Added bell-utils v0.1.0
      Locked 1 packages
  Downloaded bell-utils v0.1.0
$ qxide run --shots 100
[0, 0, 0]  0b000 = 0  ×31
  a=0 b=0 plus=0
...
$ qxide publish --dry-run
    Packaged 3 files, 642 bytes (.../target/package/bell-utils-0.1.0.tar.gz)
             qxide.toml
             src/gates.qx
             src/lib.qx
sha256 d18afc9d9e5b2362b5ddf0db2007096f3556d38f3efe96d02b055e9718f7fc90
index index/be/ll/bell-utils
{"cksum":"d18afc9d...","deps":[],"description":"Bell-pair helpers for Qxide","name":"bell-utils","qxide":">=0.6","v":1,"vers":"0.1.0","yanked":false}
     Dry-run nothing was uploaded; registry unchanged
```

Package manager errors are formatted without source snippets (with optional `note:` and `help:` lines):

```text
error[Q0605]: no version of `bell-utils` matches `^0.2`
```

## 16. Error codes

### Modules and imports (`Q05xx`)

Rendered as source diagnostics with file name, line, and column.

| Code | Meaning |
| --- | --- |
| `Q0501` | Unknown package in an import (neither the current package nor a declared dependency; help suggests `qxide add`). |
| `Q0502` | Module file not found or unreadable (note shows the path tried; also emitted when exceeding the 4096-module limit). |
| `Q0503` | The target module has no function of that name. |
| `Q0504` | The target function is not `public`. |
| `Q0505` | Module import cycle detected. |
| `Q0506` | An import conflicts with a local function or another single-function import. |
| `Q0507` | A function call is ambiguous between multiple wildcard imports. |
| `Q0508` | `import` used in a file that is not inside a project. |
| `Q0509` | Import path is too short (missing module or item / `.*`). |
| `Q0510` | `import` declaration appears after a function declaration. |

### Package manager (`Q06xx`)

Rendered without a source snippet, with optional `note:` and `help:` lines.

| Code | Meaning |
| --- | --- |
| `Q0601` | No `qxide.toml` found. |
| `Q0602` | Invalid `qxide.toml`. |
| `Q0603` | Invalid `qxide.lock`. |
| `Q0604` | Package not found (registry or path dependency). |
| `Q0605` | No version matches the requirement. |
| `Q0606` | Version conflict (incompatible requirements or self-dependency). |
| `Q0607` | Checksum mismatch (index vs. lock or archive vs. lock). |
| `Q0608` | Registry unreachable, download failed, or insecure URL (`http://`). |
| `Q0609` | Malformed registry JSON (`config.json`, `search.json`) or an index file without a single usable entry. |
| `Q0610` | Unsafe or invalid archive. |
| `Q0611` | Dependency not installed or lock file missing. |
| `Q0612` | Lock file out of date. |
| `Q0613` | `qxide` language version requirement not met. |
| `Q0614` | Publish rejected (for example, version already exists or manifest incomplete). |
| `Q0615` | Offline mode requested and required data is not cached. |
| `Q0616` | Filesystem error. |
| `Q0617` | Package is not a declared dependency (`qxide remove`). |

## 17. Implementation notes

### Code layout

- **Language changes (`src/lang/`):**
  - Lexer keywords (`import`, `public`).
  - `Import` AST node and `public` visibility flag on function declarations.
  - `src/lang/link.rs` for module discovery, cycle detection, name resolution, and call rewriting.
  - Multi-file `SourceMap` in `src/lang/diag.rs`.
  - `check_with(require_main)` so library modules can be checked without requiring `void main()`.
- **Package manager (`src/pkg/`, binary-only):**
  - `manifest.rs`, `lock.rs`, `resolve.rs`, `index.rs`, `archive.rs`, `cache.rs`, `project.rs`, `cli.rs`, `msg.rs`.
- **Library API:** The public `qxide` crate API is unchanged.

### Cargo features and dependencies

The six new dependencies (`semver`, `toml_edit`, `serde_json`, `sha2`, `tar`, `flate2`) are optional and enabled by a default feature `cli` required by the `qxide` binary. Running `cargo build --lib --no-default-features` continues to build the library with zero external dependencies.

### Tests and examples

All tests run without network access, without `gh`, and without touching the user's real home directory:

- `src/lang/module_tests.rs`: module loading, visibility, conflicts, cycles, linking, and diagnostics using in-memory files.
- `src/pkg/tests.rs`: version resolution, yanking, lock determinism, comment-preserving manifest edits, archive determinism, malicious archive rejection (`..`, absolute paths, drive-letter paths, symlinks, hard links, and non-portable or case-colliding paths), checksum-first installs, skipping unusable index lines, build-metadata versions, `curl` exit-status handling, lock name validation, whole-graph lock freshness (transitive requirements, edited path packages, unreachable entries, and self-dependencies), and `tests/data/semver_cases.json`.
- `tests/packages.rs`: end-to-end CLI tests against a temporary file registry covering `publish`, `add`, `install`, `run`, `remove`, lock reproducibility across caches and newer releases, `--locked`, stale locks (including transitive requirement changes), tampered archives and tampered index checksums, missing packages/versions, yanked versions, build-metadata look-alike versions, failed and truncated HTTPS downloads through a stand-in `curl` built from `tests/support/fake_curl.rs`, lock names that try to leave the cache, offline errors, import cycles and private functions inside a project, and the committed path-dependency example.
- `ci/test_registry_scripts.py`: tests `registry-template/scripts` (`validate.py` and `build_search.py`) against throwaway registries, including pull-request checks in a temporary git repository; requires Python 3.11+ and `git`, runs with `python -I ci/test_registry_scripts.py`, and runs in the `registry-scripts` CI job.
- `examples/packages/bell-utils`: example library package containing `src/lib.qx` and `src/gates.qx`.
- `examples/packages/bell-app`: example application using `bell-utils` via a path dependency with a committed `qxide.lock`.

## 18. Limitations and follow-ups

- The automated `gh` publishing path has not been tested against GitHub yet.
- The `qxide-lang/registry` repository must still be created from `registry-template/`.
- `qxide yank`, `qxide owner`, and `qxide login` are not implemented. Owners are changed by pull requests that edit the owner file by hand.
- Only one version per package name can appear in a build (no side-by-side major versions).
- Dependency renaming, optional package features, and dev-dependencies are not supported.
- Version resolution does not perform backtracking.
- The registry index has no cryptographic signatures yet.
- `qxide search` downloads the entire `search.json`.
- Only functions can be exported; exporting constants or types is not implemented yet.
- The `Qx.toml` filename referenced in older design documents is superseded by `qxide.toml`.
- A Korean translation of this document has not been written yet.
