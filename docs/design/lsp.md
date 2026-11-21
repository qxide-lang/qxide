# Language server and VS Code extension

Status: Implemented on the `lsp` branch (2026-11-21), not merged into `main`, and not released. Built while the user was asleep; every design decision below was taken by the implementer (Claude) without asking and is open for review (listed in section 8).

## 1. Goals and non-goals

**Goals**

- Editor support for `.qx` files that produces the same results as the CLI.
- MVP language server features: diagnostics, formatting, hover, document symbols, go to definition, and completion.
- A VS Code extension providing syntax highlighting and an LSP client.

**Non-goals (MVP)**

- Rename, find references, semantic tokens, signature help, and code actions / quick fixes.
- Incremental text synchronization.
- Workspace-wide diagnostics for files that are not open.
- Run or debug integration.
- Publishing or packaging the extension (no `vsce package`, no Marketplace or Open VSX release).

### Preconditions and `std` status

The package system (modules, `import`, `qxide.toml`, `src/pkg`) is already on `main` (released in `0.7.0`), so the server supports multi-file projects and imports from the start.

`std` modules (`import std.fs;`, etc.) currently exist only on the unmerged `app-framework` branch. In this MVP, built-in completion covers the built-in functions (`qubit`, `qreg`, gates `h`, `x`, `z`, `s`, `sdg`, `t`, `tdg`, `reset`, `rx`, `ry`, `rz`, `cx`, `ccx`, `measure`, `discard`, `len`, `println`), the constant `PI`, and the `BitReg` methods `as_int` and `to_binary`. Once `app-framework` merges, `std` module functions should be added to completion and hover.

## 2. CLI command and transport

- `qxide lsp` starts a Language Server Protocol server over stdin/stdout. A `qxide lsp` entry was added to `qxide --help`.
- stdio is the only supported transport.
- The command accepts an optional `--stdio` flag (passed by some LSP clients) and rejects any other argument with exit code `2`.
- stdout carries only protocol messages; error messages are written to stderr.
- The server loop runs on a thread with the same 64 MiB stack used by the interpreter for `run` and `check`, because the front end recurses per nesting level.

## 3. Architecture and dependencies

### Library choice

The server is built on `lsp-server` `0.10` (rust-analyzer's scaffold) rather than `tower-lsp` `0.20`:

- `tower-lsp` requires `tokio`, `tokio-util`, `tower`, `futures`, `async-trait`, `dashmap`, `bytes`, `httparse`, `tracing`, `lsp-types`, and its own macros crate. `lsp-server` requires only `crossbeam-channel`, `log`, `serde`, `serde_derive`, and `serde_json`.
- The server is synchronous and processes one request at a time, which is sufficient because analyzing a file is cheap and avoids an async runtime.
- `lsp-types` is omitted as well: only about ten message shapes are used, so request and response payloads are constructed and read directly with `serde_json` (`json!` and `Value`). This avoids `lsp-types` and its `fluent-uri`, `bitflags`, and `serde_repr` dependencies.
- `Cargo.lock` gained only two new packages (`lsp-server` and `crossbeam-channel`); `serde`, `serde_derive`, and `log` were already present through other crates.
- The dependency is optional and part of the existing default `cli` feature. Building `qxide` as a library (`default-features = false`) remains dependency-free, and the WebAssembly playground build is unaffected.

### Code layout

- `src/lang/ide.rs`: plain-data analysis API with no protocol types, layered over the existing front end. Because `crates/qxide-wasm` compiles `src/lang` via `#[path]`, nothing under `src/lang` may depend on `lsp-server`; the playground build compiles `ide.rs` without calling it.
- `src/lang/diag.rs`: adds `SourceMap::file_at` so spans can be mapped back to the file that contains them.
- `src/lsp/mod.rs`: protocol loop, document store, and JSON conversion.
- `src/lsp/uri.rs`: conversion between `file:` URIs and filesystem paths.
- `src/lsp/tests.rs`: protocol tests.
- `editors/vscode/`: the VS Code extension.

## 4. Document model, positions, and diagnostics

### Document store and synchronization

- The server uses full text synchronization (`TextDocumentSyncKind.Full`).
- Open documents are keyed by the exact URI string sent by the client. URIs are percent-decoded to locate the project (`pkg::project::for_entry`), but paths are never canonicalized: on the development machine `C:\Users\HOYA\dev` is a junction to `D:\dev`, and canonicalizing would produce URIs the client does not know.
- When the linker reads an imported module, open documents take precedence over files on disk so unsaved edits in one module are immediately seen by its importers.
- After every open, change, or close notification, all open documents are re-analyzed and their diagnostics re-published because an edit in a module can change errors in its importers. Analysis results are cached per document until the next edit.
- Closing a document publishes an empty diagnostic list for that URI.

### Positions and encoding

- The server advertises `positionEncoding: utf-16`.
- LSP uses zero-based lines and UTF-16 code units, whereas `qxide check` prints one-based lines and columns counted in Unicode scalar values.
- `ide::position` and `ide::offset` convert byte offsets directly. Columns match `qxide check` for text in the Basic Multilingual Plane (including Korean) and produce valid UTF-16 positions for characters outside it, such as emoji.
- CRLF line endings and a leading byte order mark are handled.

### Diagnostics (`qxide check` parity)

- `ide::analyze` runs the same pipeline as `qxide check`: `link::link` (lexing, parsing, and loading imported modules) followed by `check::check_with` with `require_main` set to `false` for library modules (`lib.qx` and other package modules).
- The server does not elaborate or simulate, so `Q04xx` run-time errors are not reported while typing (matching `qxide check`).
- Each LSP diagnostic sets `code` to the error code (for example `Q0302`), `source` to `qxide`, and `severity` to error. Its message contains the primary message followed by any `note:` lines without a location and the `help:` line, as `qxide check` prints them.
- Notes that carry a source location become `relatedInformation` entries, including notes pointing into another file.
- If `qxide.toml` or the lock file cannot be loaded, `qxide check` prints only that package error (`Q06xx`); the server mirrors this and publishes the error at the start of the file.

### Errors inside imported modules

When a diagnostic's location is in another file (an imported module), it is published on the open file's `import` lines. The diagnostic message is prefixed with `path:line:col:` (the location `qxide check` prints) and accompanied by a `relatedInformation` entry linking to the exact location in the imported file.

Keeping the diagnostic on the open file's `import` lines ensures the problem is visible in the file being edited and avoids publishing diagnostics to the imported file's URI, which would conflict with that file's own diagnostics when it is also open.

## 5. Language features

### Formatting

- `textDocument/formatting` calls `lang::format` (the same formatter used by `qxide fmt`, including its token fingerprint safety check).
- Returns a single edit replacing the whole document, or no edits if the document is already formatted.
- If the document does not parse, the server returns `null` and leaves diagnostics to explain the parse error.
- Editor formatting options such as tab size are ignored because `.qx` has one canonical layout.

### Hover

- Returns Markdown content.
- When the cursor is on a name, a `qx` code block shows its declaration: a local (`Qubit a`, `consume Qubit q`, `final int n`, `var x`, loop variable `int i`), a function signature (imported functions include their qualified name, such as `public void app.gates.prepare(Qubit a, Qubit b)`), a built-in signature, or `PI`.
- If the cursor is inside the range of an error, the hover appends that code's entry from `docs/error-codes.md` (matching what `qxide explain Qxxxx` prints), with one entry per distinct error code at that position.

### Document symbols

- Emits one symbol per function (kind `Function`, `detail` set to the signature, range from `public` or the return type to the closing brace).
- Function parameters and local variables are emitted as children of the function (kind `Variable`, `detail` set to the declaration).
- While the document does not parse, the server returns the outline from the last version that parsed.

### Go to definition

- Resolves locals and parameters using scoped lookup (the innermost declaration visible at the use, handling shadowing).
- Resolves functions in the same file, functions in imported modules via the linker's qualified names (including across packages provided by the project's lock file), import items, and import module segments (jumping to the start of the module file).
- Built-ins and `PI` have no source location.
- If linking fails, an imported name resolves to its `import` line.

### Completion

- Trigger character: `.`.
- After `.`, offers the `BitReg` methods (`as_int`, `to_binary`).
- Otherwise, offers local names in scope at the cursor (innermost first), functions defined in the file, imported functions (single-item imports and `public` functions of wildcard-imported modules), built-in functions with their signatures, `PI`, type names, and keywords.
- Offers nothing on `import` lines or inside `//` comments.
- Mid-edit tolerance: because the line currently being typed usually does not parse, the server re-parses the document with the cursor's line blanked out (preserving byte length so offsets stay valid). If that still fails, it scans the token stream for declarations (`Type name`, `var name`, `for name`, and functions at brace depth `0`).

### Error handling and robustness

- A panic in the front end while handling a request is caught and returned as a JSON-RPC `InternalError` for that request; the server clears its cache and keeps running.
- Unknown requests receive `MethodNotFound`.
- Requests for documents that are not open, or requests missing a position, receive `InvalidParams`.

## 6. VS Code extension

The extension lives in `editors/vscode/` with `private: true`, version `0.1.0`, and publisher ID `qxide-lang` written only in `package.json` (not registered on the Visual Studio Marketplace).

- **Language and grammar**:
  - Registers language `qx` for `*.qx` with a language configuration covering `//` comments, brackets, auto-closing pairs, and indentation rules.
  - Provides a TextMate grammar in `syntaxes/qx.tmLanguage.json` (`source.qx`) covering comments, strings with escapes and `{}` placeholders, invalid escapes, `import` paths, function declarations and calls, control keywords, modifiers (`final`, `var`, `consume`, `public`), primitive and quantum types, reserved words, `true`/`false`/`PI`, numbers with `_` separators and exponents, built-in functions, `BitReg` methods, and operators.
- **Client and configuration**:
  - TypeScript client (`src/extension.ts`) built on `vscode-languageclient` `11.0.0`, requiring VS Code `1.91` or newer.
  - Starts `<qxide.path> lsp` for `file` and `untitled` documents of language `qx`.
  - Settings: `qxide.path` (default `qxide`, resolved via `PATH`; a leading `~` expands to the home directory; changing it restarts the server) and `qxide.trace.server` (`off`, `messages`, or `verbose`, handled by the language client).
  - Command: `Qxide: Restart Language Server`.
  - If the server cannot start, an error notification reports the configured path.
- **Build and local development**:
  - Built inside `editors/vscode` with `npm install` followed by `npm run compile` (`tsc`); nothing is installed globally.
  - All dependency versions are pinned exactly and `package-lock.json` is committed; `node_modules/` and `out/` are ignored.
  - Not packaged (no `vsce`). To run locally, open `editors/vscode` in VS Code and press `F5` (Extension Development Host) with `qxide` on `PATH` or `qxide.path` pointing to a built binary (for example `target/debug/qxide`).

## 7. Testing

- **Rust unit tests**:
  - `src/lang/ide.rs`: UTF-16 positions and round trips, keyword/type/built-in lists checked against the lexer and checker, and cursor-line blanking.
  - `src/lsp/uri.rs`: percent decoding, Windows drive letters, and round trips.
- **In-memory protocol tests (`src/lsp/tests.rs`)**:
  - Drives the real server loop over `lsp_server::Connection::memory()`.
  - Covers initialize capabilities; diagnostics compared with rendered `qxide check` output (code, line, column, and caret width) for both blocks of every entry in `docs/error-codes.md` that has `qx` examples; UTF-16 columns on a CRLF line containing an emoji; located notes as `relatedInformation` and the `help:` line; diagnostic clearing on close; hover (explain entry plus declarations); go to definition; document symbols including the stale outline while broken; completion in scope, mid-edit, and after `.`; formatting (whole-document edit, already formatted, unparsable); and error responses.
  - Uses a temporary two-module project on disk to verify cross-file definition, an unsaved private-function edit reaching the importer (`Q0504`), an error inside the module shown on the import line (`Q0303` with `gates.qx:3:11:`), and a malformed `qxide.toml` reported as `Q06xx`.
- **Binary integration tests (`tests/lsp.rs`)**:
  - Starts the real binary, exchanges Content-Length framed JSON-RPC over stdio, verifies diagnostic positions and clean exit after `shutdown`/`exit`, and checks CLI argument rejection.
- **VS Code extension tests (`editors/vscode`)**:
  - `npm test` compiles the client and runs `node:test` suites: grammar scopes evaluated through `vscode-textmate` and `vscode-oniguruma` (the engine VS Code uses) on samples and every `examples/qx` program, plus a `vscode-jsonrpc` session (the transport `vscode-languageclient` uses) against the real `qxide lsp` binary (skipped when `target/debug/qxide` is not built and `QXIDE_BIN` is unset).
- **Not tested**:
  - Running the extension inside a real VS Code instance (`@vscode/test-electron` would download VS Code and was left out).
  - Extension tests are not wired into CI yet.

## 8. Decisions taken without review

Every item below was chosen by the implementer without prior confirmation and is open to revision:

- **`lsp-server` over `tower-lsp`**: avoids an async runtime and a large dependency tree; synchronous single-request handling is sufficient for per-file analysis.
- **No `lsp-types`**: only about ten message shapes are needed, so `serde_json` avoids `lsp-types` and its `fluent-uri`, `bitflags`, and `serde_repr` dependencies.
- **`check`-level diagnostics only**: matches `qxide check` and avoids elaboration or simulation while typing (`Q04xx` errors are omitted).
- **Imported-module errors reported on `import` lines**: surfaces errors in the active file without colliding with diagnostics published for the imported file when it is open.
- **Full document synchronization**: simpler than incremental sync for the MVP.
- **Whole-document formatting edit ignoring editor indentation options**: matches `qxide fmt` and enforces `.qx`'s single canonical layout.
- **Hover combines declaration and `qxide explain` entry**: shows symbol declarations alongside inline error catalog explanations without duplicating prose.
- **Extension metadata (`0.1.0`, `private: true`, publisher `qxide-lang`)**: keeps extension versioning independent of the crate version, uses `qxide-lang` only as a placeholder publisher ID in `package.json`, and sets `private: true` to prevent accidental publishing.
- **`vscode-languageclient` `11.0.0` (minimum VS Code `1.91`)**: targets the current client version.
- **Extension not packaged**: defers `vsce` and Marketplace/Open VSX setup until packaging is reviewed.
- **English design document**: matches `qx-packages.md` and the `app-framework` branch's `qx-app.md`.

## 9. Limitations and follow-ups

- No incremental text synchronization.
- Every edit re-analyzes all open documents; acceptable for small projects, but may need debouncing or dependency tracking later.
- Diagnostics are reported only for open files.
- The document symbol outline of an unparsable file can be stale until it parses again.
- Completion does not filter candidates by expected type.
- Module-segment completion inside `import` lines is not offered.
- Hover shows signatures only for built-ins, with no prose documentation.
- No `Q04xx` run-time errors are reported while editing.
- Add `std` module functions to completion and hover after `app-framework` merges.
- Add a CI job for `editors/vscode`.
- Package and publish the extension once a Marketplace publisher and an Open VSX namespace are decided.
