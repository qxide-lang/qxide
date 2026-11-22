# Qxide for Visual Studio Code

Qxide is a programming language for classical and quantum computers with Java/C-style syntax and compile-time qubit safety that compiles quantum code to OpenQASM 3. This extension provides Visual Studio Code support for `*.qx` files (language ID `qx`), including syntax highlighting, language configuration, and integration with the `qxide lsp` language server.

## Features

- **Syntax highlighting**: Registers language ID `qx` for `*.qx` files with a TextMate grammar covering comments, strings with escapes and `{}` placeholders, imports, functions, keywords such as `final`, `var`, `consume`, and `public`, classical and quantum types, `true`, `false`, `PI`, numbers, built-in functions, and `BitReg` methods.
- **Language configuration**: Configures `//` comments, brackets, auto-closing pairs, and indentation.
- **Language server**: Starts `qxide lsp` for saved and untitled `.qx` files.
- **Diagnostics**: Shows diagnostics with the same error codes, lines, and columns as `qxide check`, including in projects with `qxide.toml` and imports. Errors in imported modules are shown on the `import` line.
- **Formatting**: Formats `.qx` files with the same output as `qxide fmt`.
- **Hover**: Displays `qxide explain` text for the error under the cursor, and shows the declaration or signature of a name.
- **Document symbols**: Populates the outline view with symbols in the document.
- **Go to definition**: Jumps to definitions for locals, parameters, and functions, including across imported modules.
- **Completion**: Completes keywords, types, built-in functions, `PI`, functions from the current file and imports, local names in scope, and `BitReg` methods.

## Requirements

- **Visual Studio Code**: Version 1.91 or newer.
- **Qxide CLI**: `qxide` version 0.8.0 or newer, which provides `qxide lsp`.

Install `qxide` using one of the following methods:

- **Linux and macOS**:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh
```

- **Windows**:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"
```

- **pip**:

```sh
pip install qxide
```

- **GitHub Releases**: Download directly from [GitHub Releases](https://github.com/qxide-lang/qxide/releases).

## Settings

| Setting | Description |
| --- | --- |
| `qxide.path` | Path to the `qxide` executable. Defaults to `qxide` from `PATH`. A leading `~` expands to the home directory. Changing this setting restarts the language server. |
| `qxide.trace.server` | Language server trace setting (`off`, `messages`, or `verbose`). |

## Commands

- **`Qxide: Restart Language Server`**: Restarts the `qxide lsp` language server.

## Installing from a .vsix

Download `qxide-vscode-0.1.0.vsix` from [GitHub Releases](https://github.com/qxide-lang/qxide/releases) and install it with:

```sh
code --install-extension qxide-vscode-0.1.0.vsix
```

## Links

- **Website**: [https://qxide.dev](https://qxide.dev)
- **Source**: [https://github.com/qxide-lang/qxide](https://github.com/qxide-lang/qxide) (extension in `editors/vscode`)
- **Issues**: [https://github.com/qxide-lang/qxide/issues](https://github.com/qxide-lang/qxide/issues)

## License

Licensed under `MIT OR Apache-2.0`.
