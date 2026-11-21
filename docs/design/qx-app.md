# Desktop apps with `.qx` (Qxide app framework and Qxide Studio)

Status: design written 2026-11-21 on the `app-framework` branch; MVP implemented on the same branch and tested on Windows; not merged or released.

This document describes a framework for building desktop applications whose UI is written in HTML, CSS, and JavaScript and whose logic is written in `.qx`, without requiring a Rust toolchain or per-app compilation step. Capabilities are exposed through a Qxide standard library provided by a prebuilt runtime under explicit permissions, and the same framework is used to build Qxide Studio, a desktop IDE for `.qx`.

## 1. Goals and non-goals

**Goals**

- Build desktop apps whose UI is HTML, CSS, and JavaScript and whose logic is `.qx`, so app developers never need a Rust toolchain or a compile step.
- Take Tauri's strengths—OS webviews, small executables and low memory, a Rust core, command-style IPC, capability-based least-privilege security, and multi-platform packaging—without depending on Tauri itself.
- Expose system functionality through a Qxide standard library provided by a prebuilt runtime and gated by capability permissions.
- Share one front end, interpreter, sampling path, and diagnostic set across the `qxide` CLI, the `qxide.dev` playground, and desktop apps.
- Build Qxide Studio, a desktop IDE for `.qx`, as an app on this framework.
- Keep a path toward self-hosting the runtime in Qxide once the typed IR, Cranelift native compilation, and FFI exist.

**Non-goals (MVP)**

- Per-app native Rust code or custom native plugins; new native capabilities arrive only through the standard library shipped with the runtime (or FFI later).
- `std.http.post` and per-package permission grants (`std.http.get` is included in the MVP, and dependencies remain limited to `std.log`).
- Installers (MSI, NSIS, macOS DMG), code signing, auto-update, and per-app executable icons or version resources.
- Tested macOS and Linux runtime builds (the code is portable, but the MVP runtime targets Windows).
- Full IDE features in Qxide Studio beyond the minimal editor, run, chart, and QASM view (circuit diagram, multi-file projects, package management, formatting, LSP, and debugger are later work).
- Replacing the Rust runtime or quantum simulator kernels in the MVP.

## 2. What we take from Tauri, and what we leave

| Tauri strength or cost | Qxide app framework |
| --- | --- |
| OS webview | Taken. Uses the OS webview (`WebView2` on Windows, `WKWebView` on macOS, `WebKitGTK` on Linux) directly via `wry` (`0.57`) and `tao` (`0.37`) instead of bundling a browser, keeping executables small and memory usage low. |
| Rust core | Taken. Uses a prebuilt Rust runtime (`qxide-app`) built on `wry` and `tao`, with a long-term plan to move the runtime to Qxide (self-hosting). |
| Command IPC | Taken. The page invokes named commands and receives a `Promise`. |
| Capability security | Taken. Least-privilege model where nothing is allowed unless granted in `[app]` `permissions` (aside from `std.log`). |
| Multi-platform packaging | Taken in design. `qxide app build` produces portable outputs for Windows (`target/app/<name>.exe`), Linux (`target/app/<name>`), and macOS (`target/app/<Title>.app`). |
| Per-app Rust compilation | Left out. Tauri compiles each app as a Rust program, requiring a Rust toolchain, MSVC/Xcode/GCC, minutes of build time, and gigabytes of RAM. `qxide app build` copies a prebuilt runtime and attaches the bundle in seconds. |
| Configuration files | Left out. Replaces `tauri.conf.json`, capabilities JSON, `Cargo.toml`, and `build.rs` with an `[app]` section in `qxide.toml`. |
| Tauri CLI and `npm` | Left out. Does not use the `tauri` crate, Tauri CLI, `npm`, or Node. App workflows live in `qxide app new`, `qxide app dev`, and `qxide app build`. |
| Native plugins per app | Left out. Apps cannot include custom native Rust code; capabilities come only from the runtime's standard library (or FFI later). |

### Alternatives considered

- **(A) Depend on the Tauri CLI**: Generate a `src-tauri` project per app and run `cargo tauri build`. Rejected because every app developer would need a Rust toolchain, the Tauri CLI, and a C toolchain, with builds taking minutes and gigabytes of RAM.
- **(B) Prebuilt runtime on top of the `tauri` crate**: Possible (using `Context::set_assets` to load assets at run time), but pulls in `tauri`'s code generation, configuration, and ACL machinery that would mostly be bypassed, resulting in a larger binary. Rejected in favor of (C).
- **(C) Prebuilt thin runtime on `wry` and `tao` (chosen)**: Ship a prebuilt `qxide-app` runtime built directly on `wry` (`0.57`) and `tao` (`0.37`) with `qxide` releases. `qxide app build` copies the runtime and attaches the app's `.qx` sources and `ui/` files with no compilation, finishing in seconds.
- **(D) Electron**: Bundles Chromium (~100+ MB per app). Rejected.

### Limits of this direction

- Apps cannot add their own native Rust code (no per-app plugins).
- New native capabilities arrive only through the standard library shipped with the runtime, or later through FFI.
- Code signing, per-app executable icons and version resources, installers, and auto-update are later work.
- The runtime version is tied to the `qxide` version.

## 3. Architecture

The framework consists of five pieces:

1. The `src/lang` front end and interpreter, shared unchanged by the `qxide` CLI, the `qxide.dev` playground (compiled to WebAssembly), and the app runtime.
2. A command engine in `src/lang` that runs `elaborate` on a chosen `public` function with JSON-compatible arguments and returns a typed result instead of rendered stdout.
3. A `Host` trait in `src/lang` through which every standard-library call passes; each embedder (CLI, REPL, playground, and app runtime) supplies the host and its permission set.
4. The `qxide-app` runtime binary (`crates/qxide-app-runtime`), which contains a `tao` window, a `wry` webview, a custom protocol serving `ui/` files, an IPC dispatcher, worker threads, and the app host implementing `std`.
5. The `qxide app new`, `qxide app dev`, and `qxide app build` commands in the `qxide` CLI, keeping GUI dependencies out of the CLI itself.

```text
 ui/index.html + app.js                     qxide-app runtime (prebuilt, Rust: wry + tao)
 ┌──────────────────────────┐  postMessage  ┌──────────────────────────────────────────────┐
 │ qxide.invoke(cmd, args)  │ ────────────▶ │ IPC: origin check, [app] commands allow-list │
 │   Promise<Result>        │ ◀──────────── │ worker threads ─▶ src/lang (link, check,     │
 └──────────────────────────┘  evaluate JS  │   elaborate fn, simulate shots)              │
        served from bundle via qxapp://      │ Host: std.log / time / fs / app, permissions │
                                             └──────────────────────────────────────────────┘
 qxide app build = copy runtime + attach bundle (.qx sources, locked packages, ui/, app.json)
```

When JavaScript calls `qxide.invoke("measure_bell", {}, {shots: 1000})`, the bridge posts a JSON message over `window.ipc.postMessage`. The runtime IPC handler verifies the page origin and checks that the command is listed in `[app]` `commands`. A worker thread then links the app's modules from the bundle, checks them, elaborates the target function with the provided arguments, and simulates the requested shots. Finally, the runtime formats the result as JSON and evaluates a small script in the page that resolves the `Promise`.

## 4. Project layout and the `[app]` section

An app is a normal Qxide project (using the `qxide new` layout) with an `[app]` section in `qxide.toml` and a `ui/` folder. Command functions live in a single module, defaulting to `src/app.qx` (module path `app`), controlled by `module`. Only functions listed in `commands` can be called from the page; each listed function must be `public` and accept only `int`, `double`, `boolean`, or `String` parameters. Both `qxide app dev` and `qxide app build` verify these rules before the window opens or the executable is written, and the runtime checks them again on every call.

```toml
[package]
name = "bell-app"
version = "0.1.0"

[dependencies]
bell-utils = "0.1"

[app]
title = "Bell lab"
identifier = "dev.qxide.app.bell-app"
module = "app"                       # src/app.qx
commands = ["measure_bell", "save_result", "history"]
permissions = ["log", "time", "fs"]
width = 720
height = 520
resizable = true
```

| Key | Meaning | Default |
| --- | --- | --- |
| `title` | Window title. | Package name |
| `identifier` | Reverse-DNS identifier used to name the app data folder. | `dev.qxide.app.<name>` |
| `module` | Module path containing command functions (`src/app.qx` for `app`). | `app` |
| `commands` | List of `public` command function names callable from the page. | Required, non-empty |
| `permissions` | Standard-library modules the app's own code may import (`"log"`, `"time"`, `"fs"`, `"app"`, `"http"`, `"qxide"`). | Empty (`std.log` is always allowed) |
| `http_hosts` | Lowercase host names that `std.http` may reach, matched exactly with no wildcards, scheme, or port; required when `permissions` contains `"http"`. | Empty |
| `width` | Initial window width. | `800` |
| `height` | Initial window height. | `600` |
| `resizable` | Whether the window can be resized. | `true` |

Packages listed under `[dependencies]` may import only `std.log` in the MVP; per-package permission grants are deferred to later work.

## 5. Command model

Commands are `public` functions declared in the app's command module (default `src/app.qx`) and listed under `[app]` `commands`.

```java
import std.fs;
import std.log;
import std.time;

// Runs on every call of `measure_bell`; the runtime simulates the requested shots.
public BitReg measure_bell() {
    QReg q = qreg(2);
    h(q[0]);
    cx(q[0], q[1]);
    return measure(q);
}

// Build-time effects: runs once per call, before any shot.
public String save_result(String text) {
    String line = time.now_iso() + "  " + text;
    fs.append_text("results.txt", line + "\n");
    log.info("saved " + len(line) + " characters");
    return line;
}

public String history() {
    if (!fs.exists("results.txt")) {
        return "";
    }
    return fs.read_text("results.txt");
}
```

### Calling commands from JavaScript

The page communicates with `.qx` commands through three functions on the `qxide` bridge:

- `qxide.invoke(command, args, options)` runs a named command and returns a `Promise`. `args` is an object keyed by parameter name (like Tauri commands). `options` accepts `shots` (default `1`, maximum `1,000,000` in the app), `seed` (unsigned 64-bit integer; defaults to a fresh random seed, which is returned in the result so the run can be repeated), and `signal` (an `AbortSignal` for cancellation).
- `qxide.commands()` returns the list of available commands together with their parameter names, parameter types, and return type.
- `qxide.on(event, callback)` receives events emitted by `std.app.emit` and log lines in dev mode.

```js
const r = await qxide.invoke("measure_bell", {}, { shots: 1000, seed: 7 });
// r.counts = [{ value: [1, 1], count: 507, output: "" }, { value: [0, 0], count: 493, output: "" }]
const line = await qxide.invoke("save_result", { text: `00=${r.counts[1].count} 11=${r.counts[0].count}` });

try {
  await qxide.invoke("save_result", { text: 42 });
} catch (e) {
  // e.code === "Q0712", e.message: "argument `text` of `save_result` must be a String, got a number"
}

const ctrl = new AbortController();
const p = qxide.invoke("measure_bell", {}, { shots: 1_000_000, signal: ctrl.signal });
ctrl.abort(); // p rejects with e.code === "Q0713"
```

### Argument types

| JavaScript value | `.qx` parameter type |
| --- | --- |
| Integer `Number` within `-(2^53 - 1)` to `2^53 - 1`, `BigInt`, or decimal string within the 64-bit range | `int` |
| `Number`, or the strings `"NaN"`, `"Infinity"`, `"-Infinity"` | `double` |
| `true` or `false` | `boolean` |
| `string` | `String` |

Any other value, a missing argument, or an extra argument fails with error `Q0712` before execution starts. `Qubit`, `QReg`, `Bit`, and `BitReg` parameters are not allowed in commands (`Q0714`), because the page cannot own qubits or measurement results.

### Return values

| `.qx` return type | JavaScript value |
| --- | --- |
| `void` | `null` |
| `int` | `Number` when within `-(2^53 - 1)` to `2^53 - 1`; otherwise a decimal string that the bridge converts to `BigInt` |
| `double` | `Number`; non-finite values are encoded as `"NaN"`, `"Infinity"`, or `"-Infinity"` and converted back by the bridge |
| `boolean` | `boolean` |
| `String` | `string` |
| `Bit` | `0` or `1` |
| `BitReg` | Array of `0` and `1` values in register order (`r[0]` first), of any width |

Only `BitReg` maps to an array today. `.qx` does not have classical arrays yet; when they are added, they will map to JSON arrays.

### Result object

A resolved `qxide.invoke` `Promise` yields an object with the fields `{ command, seed, shots, value, output, counts }`:

- **Single shot (`shots == 1`)**: `value` is the return value (`Bit` or `BitReg` read from that shot), `output` is the `println` text from that shot, and `counts` is `null`.
- **Multiple shots with a `Bit` or `BitReg` return**: `value` is `null`, and `counts` is a list of `{ value, count, output }` entries grouped by the returned bits, ordered most frequent first with ties broken by bit order, matching the CLI table.
- **Multiple shots with a classical return**: `value` is the classical return value, and `counts` groups by the whole measurement record (as `qxide run --shots` does), or is `null` if nothing is measured.

### Errors

When a command fails, the `Promise` rejects with a `QxError` object containing `{ code, message, diagnostics, rendered }`:

- `code` and `message` report the primary failure. Compile-time and run-time diagnostics keep their English text and error codes unchanged (for example, `Q0302`).
- `diagnostics` is a list of `{ code, message, file, line, column, endLine, endColumn, help }` objects so editors such as Qxide Studio can mark source locations.
- `rendered` is the exact formatted diagnostic output that the CLI would print.

App-level errors use codes `Q0711` through `Q0716`.

### Threads and cancellation

Each command call runs on one of a fixed pool of worker threads (one per CPU core, at most four; additional calls wait in a queue without a thread of their own), so the window never freezes. A call that fails inside the runtime rejects with `Q0412` instead of leaving its `Promise` pending, and a call id that is still in flight cannot be reused (`Q0712`). Canceling a call through the `signal` option (an `AbortSignal`) sets a flag that the worker checks between shots, inside `std.time.sleep_millis`, and every 4096 interpreter steps; the `Promise` then rejects with `Q0713`. Closing the window cancels all running calls. Every message carries a random token for the page that sent it, so after a reload (F5) a response to a call made by the previous page is ignored.

### Determinism and state

The same command, arguments, `shots`, and `seed` produce the same counts, using the same per-shot seeds as `qxide run` and the playground (`qxide::shot_seed`). Reads from `std.time` and `std.fs` are outside the seed.

Commands are stateless: every call re-links and re-elaborates the module. State that must persist between calls lives in files accessed through `std.fs` or in the page.

## 6. Effects and the standard library

### When effects run

Standard-library calls are build-time effects. They execute while the command function is elaborated, once per call, in program order, before any shot is simulated, regardless of how many shots are requested.

Standard-library functions accept and return only build-time values (`int`, `double`, `boolean`, `String`). Shot-time values (`Bit`, `BitReg`) leave the runtime only through the command's return value or `println`. As a consequence, the example app splits work across two commands: one command measures and returns a `BitReg` (for which the runtime aggregates `counts`), and a second command writes text to disk with `std.fs`.

Standard-library calls are not allowed inside an `if` branch conditioned on a measurement result (`Q0314`, the same error as calling any function that cannot run conditionally), because that body runs per shot.

### Importing standard modules

A standard module can be imported by module path with `import std.fs;` and called in qualified form, such as `fs.write_text("bell.txt", text);`. The existing import forms (`import std.fs.write_text;` and `import std.fs.*;`) also work. Referring to an unknown `std` module or function produces `Q0701`.

Module imports (`import pkg.module;` followed by `module.fn(...)`) are limited to `std` in the MVP; support for other packages may follow. `std` is a reserved package name rejected by `qxide new`, `qxide.toml`, and the registry. `std` imports work both inside a project and in a standalone file.

### Modules

The MVP standard library provides logging, time, confined file access, app window control, and HTTPS GET requests to listed hosts (`std.http.get`); `std.http.post` is designed but not implemented yet.

```text
std.log    info(String msg)  warn(String msg)  error(String msg)
std.time   int now_millis()               // Unix time in milliseconds
           String now_iso()               // UTC, "2026-11-21T02:03:04.005Z"
           sleep_millis(int ms)           // 0..=60000, cancellable
std.fs     String read_text(String path)
           write_text(String path, String text)
           append_text(String path, String text)
           boolean exists(String path)
           remove(String path)            // a file; missing files are not an error
std.app    set_title(String title)
           set_size(int width, int height)
           close()
           emit(String event, String payload)   // qxide.on(event, cb) in the page
std.qxide  String check(String src)       // JSON: diagnostics      (permission "qxide")
           String run(String src, int shots, int seed)   // JSON: counts or diagnostics
           String qasm(String src)        // JSON: OpenQASM 3 or diagnostics
           String format(String src)      // JSON: formatted source or diagnostics
std.http   String get(String url)             // https, [app] http_hosts  (permission "http")
           String post(String url, String body)   // later
```

### Hosts and permissions

Every standard-library call passes through the `Host` trait supplied by the embedder, which enforces the module availability and permission set for that host.

| Host | `std.log` | `std.time` | `std.fs` | `std.app` | `std.http` | `std.qxide` |
| --- | --- | --- | --- | --- | --- | --- |
| App runtime | Always (also for dependencies) | Requires `"time"` in `[app]` `permissions` | Requires `"fs"` in `[app]` `permissions` | Requires `"app"` in `[app]` `permissions` | Requires `"http"` in `[app]` `permissions` and a non-empty `http_hosts` | Requires `"qxide"` in `[app]` `permissions` |
| `qxide run` | Always (`stderr`) | Always | Only with `--allow fs` (`data/` or `./data`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) |
| `qxide check` | Checked against app permissions or CLI scope; nothing runs | Checked against app permissions or CLI scope; nothing runs | Checked against app permissions or CLI scope; nothing runs | Checked against app permissions or CLI scope; nothing runs | Checked against app permissions or CLI scope; nothing runs | Checked against app permissions or CLI scope; nothing runs |
| `qxide qasm` | Discarded | Always | Refused (`Q0702`) | Refused (`Q0702`) | Refused (`Q0702`) | Refused (`Q0702`) |
| `qxide repl` | Accepted, not printed (the REPL replays the whole session on every input) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) |
| Playground | Always (printed to stderr as `info: message`, like `qxide run`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) | Unavailable (`Q0702`) |

`std.qxide` is available only to apps granted the `qxide` permission (such as Qxide Studio). Dependencies from `[dependencies]` may import only `std.log` in the MVP. Importing or calling a module unavailable in the current host fails with `Q0702`, and importing or calling a module not granted by `[app]` `permissions` or `--allow` fails with `Q0703`.

### File access rules

`std.fs` confines every operation to a host-defined data root:

- Every path must be relative to the data root: `<data dir>/<identifier>/` for a built app (for example, `%APPDATA%\dev.qxide.app.bell-app` on Windows), `<project>/target/app-data/` in `qxide app dev`, and the project's `data/` folder (or `./data` for a standalone file) in `qxide run --allow fs`.
- Paths are rejected with `Q0705` if they are absolute, contain drive letters or UNC paths, are empty, contain `..` segments, use Windows reserved names (`CON`, `NUL`, `COM1`, and similar), have existing components that resolve through symlinks outside the data root, or pass through a symlink whose target does not exist. The data root itself must not be a symlink or junction, and `qxide app dev` refuses a linked `target/` or `target/app-data/`.
- Files are UTF-8 text only, with a maximum size of 16 MiB per read.
- Failed file operations (not found, exceeding the size limit, or invalid UTF-8) return `Q0704`.
- Parent directories are created automatically on write.

### HTTP access rules

- `std.http.get(String url)` is available only in the desktop app runtime when `permissions` in `[app]` includes `"http"` and `http_hosts` is non-empty; other hosts (`qxide run`, REPL, playground, Studio user code, and dependency packages) never receive it.
- Only `https://` URLs on port 443 without user info whose host exactly matches an entry in `http_hosts` are accepted; disallowed URLs fail with `Q0708` before any network traffic is sent.
- Redirects are not followed, so any 3xx response fails like any other status outside 200-299 (`Q0707`).
- The response body must be valid UTF-8 text of at most 16 MiB, and the request must complete within 30 seconds (`Q0707`).
- Each call runs as a build-time effect once per command invocation before any shot executes, and cancelling the command stops the request (`Q0713`).
- Requests are executed through the operating system's `curl` binary (included with Windows 10 and later and macOS), the same tool used by the package manager.

## 7. Language extensions

### In the MVP

- **`String` type**: a build-time, immutable text value with the following rules:
  - Literals use `"..."` with the existing escape sequences.
  - `+` concatenates when either operand is a `String`; the other operand may be `int`, `double`, `boolean`, or `String`, with numbers formatted the same way as in `println`.
  - `==` and `!=` compare text.
  - `len(s)` counts Unicode characters.
  - `println("{}", s)` prints a `String`.
  - `String` is supported in function parameters, return types, `final` bindings, and `var` type inference.
  - A string literal used as a value cannot contain `{}` placeholders (`Q0214`, reworded for this case); `{{` and `}}` produce literal braces.
- **Module imports for `std`**: `import std.<module>;` enables qualified calls such as `fs.write_text(...)`, alongside `import std.<module>.<fn>;` and `import std.<module>.*;`.
- **Reserved names (minor breaking changes)**: comparable to reserving `import` and `public` in 0.7.0, `String` is now a reserved type name (cannot be used as a variable or function name) and `std` is a reserved package name (rejected by `qxide new`, `qxide.toml`, and the registry).

### Later

- `Bytes` type along with `std.fs.read_bytes` and `std.fs.write_bytes`.
- Classical arrays (`int[]`, `double[]`, `String[]`) mapping to JSON arrays.
- Error values (a `Result`-like type or `try`/`catch`) so standard-library failures can be handled in `.qx` instead of aborting the command.
- String slicing and parsing helpers (`substring`, `to_int`, `to_double`).
- Shot-time to build-time results: a sampling expression that runs a circuit and returns counts as classical data, required for hybrid loops such as VQE and for saving measured data from a single function.
- `std.http.post`, request headers, and per-host settings built on top of the existing `http_hosts` allow-list.

## 8. Security model

### Page

The UI is served only from the app bundle over the custom `qxapp` protocol (origin `http://qxapp.localhost` on Windows and Android; `qxapp://localhost` on other platforms). Every response includes the `Content-Security-Policy` header `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'`.

Remote content and inline scripts are disallowed. Navigation to any other origin is blocked, new windows are blocked, and external links do nothing (a `std.shell.open_url` permission may be added later). Developer tools are enabled only in `qxide app dev`.

### IPC

The page communicates with the runtime over a single channel, `window.ipc.postMessage`. The runtime accepts a message only when the sending page's URL matches the app origin, parses the payload strictly (rejecting unknown fields), and permits only `invoke`, `cancel`, and `commands` requests. `invoke` is allowed only for functions listed in `[app]` `commands` with valid argument types. The page has no file, shell, process, or network API of its own.

### `.qx` permissions

Standard-library modules are gated twice for defense in depth:

- At link time, importing a module not granted by `[app]` `permissions` fails with `Q0703`, and the app does not start the command.
- At call time, the host checks permissions again when a call executes.

Permissions are stored in the bundle inside the executable and cannot be changed by the page. Dependencies from `[dependencies]` receive only `std.log` in the MVP.

### Resource limits

- Each call (a command, or `std.qxide.run` in Qxide Studio) may simulate at most 24 qubits on the state-vector backend (256 MiB) and a stabilizer tableau of at most 256 MiB, failing with `Q0406` otherwise. With at most four calls running at once, simulation memory stays within 1 GiB in total. Larger programs run with `qxide run`, which keeps the CLI limits.
- Each call also has its own execution budgets (`Limits::APP`): 2^31 interpreter steps, 2^22 circuit operations, 2^36 units of estimated simulation work, 32 MiB of output, nesting of `if` blocks on measured bits up to 128 levels across calls, and at most 100 diagnostics with source lines echoed in 240-character windows (`Q0413` to `Q0418`). `String` text costs 8 steps per byte. docs/security/EXECUTION_LIMITS_FIXES.md (part 2) records how these values were measured.
- Calls run on worker threads with the same 256 MiB stack as `qxide run` (`lang::STACK_SIZE`).
- Shots are capped at `1,000,000` per command call.
- `std.fs` reads are capped at 16 MiB per read.
- A call result handed to the page is capped at 32 MiB (`Q0716`), and so is a `std.app.emit` event (`Q0706`).
- Only the bare runtime started by `qxide app dev` accepts `--dev`; a built app (a runtime with a bundle attached) refuses it.

## 9. Command-line interface

### `qxide app new <name>`

Creates a `<name>/` directory with a runnable app scaffold and no `npm` dependency:

- `qxide.toml` with package metadata and an `[app]` section.
- `src/app.qx` containing a Bell-pair `measure_bell` command and a `save_result(String text)` command (`src/main.qx` is not created).
- `ui/index.html`, `ui/app.js`, and `ui/style.css`.
- `README.md` and `.gitignore` (ignoring `target/`).

### `qxide app dev [DIR]`

Resolves the project the same way as `qxide run` (packages must be installed and are resolved via `qxide.lock`) and starts the runtime in development mode, reading `.qx` and `ui/` files directly from disk. Edits to `.qx` files apply on the next command call, and edits to `ui/` files apply on reload (`F5`). Developer tools are enabled, `std.fs` uses `<project>/target/app-data/` as its data folder, and the command exits when the window closes.

### `qxide app build [DIR]`

Checks the project (verifying that every listed command is valid and every requested permission is known), packs the app's modules, its locked package modules, and `ui/` files into a bundle, copies the prebuilt runtime for the host platform, and attaches the bundle. No compilation takes place, so the build finishes in seconds.

| Platform | Output path | Packaging and runtime notes |
| --- | --- | --- |
| Windows | `target/app/<name>.exe` | Single portable executable with the bundle appended. |
| Linux | `target/app/<name>` | Portable executable with the bundle appended; requires WebKitGTK 4.1 and GTK 3 installed at run time. |
| macOS | `target/app/<Title>.app` | App bundle storing the container at `Contents/Resources/app.qxpak` (appending to a Mach-O binary would break code signing). |

### Bundle format

The `.qxpak` container consists of file entries (path, length, and raw bytes), an index, and a footer containing the magic `QXAPPK01`, the format version, and the `qxide` version that built the bundle. On Windows and Linux, the container is appended to the runtime executable (executables ignore trailing data), and the runtime opens its own executable at startup to read the footer. The runtime refuses any bundle built by a different `qxide` version.

### Finding the runtime

The CLI searches for `qxide-app` (or `qxide-app.exe` on Windows) in order:

1. The `QXIDE_APP_RUNTIME` environment variable (development override).
2. Next to the running `qxide` executable (release archives, installer scripts, and PyPI wheels will ship both binaries together).
3. Later: `QXIDE_HOME/runtimes/<version>/<target>/`, downloaded from the GitHub release with a SHA-256 check, which also enables cross-platform builds with `--target`.

### Installers, signing and icons

- **Installers**: the MVP outputs portable executables only and requires no global packaging tools. Windows installers (MSI needs WiX; NSIS needs NSIS) and macOS DMG packages are later work. Candidate approaches include a built-in per-user self-install (`app.exe --install` copying the executable to `%LOCALAPPDATA%\Programs` and adding a Start menu entry) or generating MSI files from Rust.
- **Code signing**: sign executables after `qxide app build`. Windows Authenticode covers the appended bundle, so signing before attaching the bundle would be invalidated.
- **Icons and version resources**: on Windows, the runtime executable has no icon or version resources yet (Windows shows the default application icon); adding the Qxide mark to the runtime and per-app icons through PE resource editing are later work; the window title comes from `[app]`.

### CI and releases

The runtime crate (`crates/qxide-app-runtime`) is excluded from the Cargo workspace for now because `wry` requires WebKitGTK on Linux CI. It will be added to `cargo-dist` and the PyPI wheels once CI installs the Linux webview libraries.

## 10. Packages and the interpreter

### Packages

Apps declare `[dependencies]` in `qxide.toml` the same way any other Qxide project does. When `qxide app build` creates a bundle, it embeds the exact locked package sources from the local cache using `qxide.lock`, so the built app needs no network access or registry at run time.

In the MVP, packages from `[dependencies]` may import only `std.log`; per-package permission grants come later. An app project can also be published as a package: `qxide run` and `qxide publish` ignore the `[app]` section, and both `qxide publish` and the `qxide-lang/registry` registry remain unchanged.

### One interpreter for the CLI, the playground and apps

The `qxide-app` runtime compiles the same `src/lang` module using `#[path]`, matching how `crates/qxide-wasm` builds the WebAssembly playground. As a result, the CLI, the `qxide.dev` playground, and desktop apps share one front end, one interpreter, one sampling path (`Circuit::simulate_shots` and `qxide::shot_seed`), and one set of diagnostics.

The only differences between targets are the `Host` supplied by the embedder (which sets `std` permissions) and the host's resource limits.

## 11. Qxide Studio

Qxide Studio is a desktop IDE for `.qx` built with this framework. It lives in `apps/studio` and is itself a Qxide app built with `qxide app build`.

### MVP

- Editor with syntax highlighting, line numbers, and error markers driven by structured diagnostics.
- Run (`shots` and `seed`), Check, and QASM view.
- Measurement distribution bar chart.
- Example programs.
- Buffer autosave to the app data folder through `std.fs`.
- Privileged `std.qxide` module (permission `"qxide"`, granted only to Studio-like tools) providing `check`, `run`, `qasm`, and `format` functions that operate on a source string and return structured JSON.
- Isolated execution for user code: programs typed into the editor run under a playground-like host that allows only `std.log`, so user code cannot touch files.

### Later

- Circuit diagram drawn from the QASM.
- Multiple files and real projects (requires a file-dialog module).
- Packages (`qxide add` through a `std.qxide` call with network permission).
- Formatting.
- LSP integration (`t16`) for completion and hover.
- Debugger.

### Look and feel

Studio uses the Qxide brand K1 ket mark, fonts and tokens from `site/src/styles.css`, and both light and dark themes. The teal accent is `#0E8A8A` in the light theme and `#3CC8C4` in the dark theme.

## 12. Self-hosting roadmap

The long-term goal is to move the runtime from Rust to Qxide step by step.

### Prerequisites

1. Typed IR (`docs/design/qx-ir.md`).
2. Native compilation of classical code with Cranelift.
3. FFI (`extern` functions with a C ABI).
4. Language features: `Bytes`, arrays, structs, error values, and `String` operations.
5. A memory model for classical heap data.

### Migration order

1. Rewrite `std` modules in `.qx` over a few host intrinsics (`std.fs` path confinement, logging, and time formatting).
2. Rewrite the bundle tool (`qxide app build`: archive writing and file walking) in `.qx`.
3. Move IPC message parsing and JSON handling into `.qx`.
4. Drive the window and webview through FFI to a small C shim over `wry` and `tao` (the webview shim stays native the longest).
5. Compile the interpreter and command engine from `.qx` once the compiler bootstraps.

### What stays in Rust and parity rule

The quantum simulator kernels (state vector, stabilizer, and GPU) stay in Rust until `.qx` has SIMD and GPU support, and the C shim over `wry` and `tao` remains native the longest.

Each step must keep WebAssembly parity and app behavior identical, verified by the same tests.

## 13. Error codes

The `Q07xx` range is new, and its compile-time codes appear in `qxide explain`.

| Code | Meaning | Reported at |
| --- | --- | --- |
| `Q0701` | Unknown `std` module or function | Link / check |
| `Q0702` | `std` module not available in this host (for example, `std.app` in `qxide run`) | Link / check, `qxide qasm` |
| `Q0703` | `std` module not permitted by `[app]` `permissions` or `--allow` | Link / check |
| `Q0704` | File operation failed (not found, larger than 16 MiB, not UTF-8, or I/O error) | Run time |
| `Q0705` | Path not allowed (absolute, `..`, drive or UNC prefix, reserved name, or escapes the data folder) | Run time |
| `Q0706` | `std` argument out of range (for example, `sleep_millis` above `60000`) | Run time |
| `Q0707` | HTTP request failed (network error, status outside 2xx, body over 16 MiB, or not UTF-8) | Run time |
| `Q0708` | URL not allowed (not `https://`, port other than 443, user info, or host not in `[app]` `http_hosts`) | Run time |
| `Q0711` | Unknown command (not listed in `[app]` `commands`) | App call |
| `Q0712` | Bad command arguments (missing, extra, or wrong type) | App call |
| `Q0713` | Call canceled | App call |
| `Q0714` | Invalid command signature (not `public`, or uses a quantum or measurement parameter) | `qxide app dev` / `qxide app build`, and on each call |
| `Q0715` | Invalid `[app]` section in `qxide.toml` | `qxide app dev` / `qxide app build` |
| `Q0716` | Call result larger than 32 MiB | App call |

## 14. MVP status and follow-ups

### MVP implemented on this branch

- `String` type and `std` module imports.
- `Host` trait with CLI, REPL, `qasm`, playground, and app hosts.
- Standard-library modules: `std.log`, `std.time` (`now_millis`, `now_iso`, `sleep_millis`), `std.fs` (`read_text`, `write_text`, `append_text`, `exists`, `remove`), and `std.app` (`set_title`, `set_size`, `close`, `emit`).
- `std.http.get` for apps granted `"http"`, restricted to `https://` hosts listed in `[app]` `http_hosts` and executed through the system `curl`.
- `qxide run --allow fs`.
- Command engine in `src/lang`.
- `qxide-app` runtime on `wry` and `tao` for Windows.
- `qxide app new`, `qxide app dev`, and `qxide app build`.
- Example app in `examples/apps/bell-app`.
- Qxide Studio (`apps/studio`, built with `qxide app build`): editor with syntax highlighting, line numbers, and error markers; Run with shots and seed, Stop, Check, Format, QASM view, distribution chart, and Problems list; examples from `examples/qx`, autosave through `std.fs`, and window title through `std.app.set_title`, using system font stacks instead of the website's web fonts.

### Not in the MVP

- `std.http.post` and request headers.
- Installers.
- Code signing.
- Per-app icons.
- macOS and Linux builds (the code is portable, but untested).
- Per-package permissions.
- Circuit diagram.
- LSP.
- CI does not build or lint the runtime yet, and the runtime is not included in release archives or PyPI wheels.
- Keeping the runtime version equal to the `qxide` version is manual.
- A built app whose bundle is missing or from a different version exits without a visible message because built apps have no console window (an error dialog is later work).
- `qxide --help` does not mention `--allow fs` yet.
