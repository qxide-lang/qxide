# Changelog

## 0.1.0 - 2026-11-22

- **Syntax highlighting**: Added language ID `qx` for `*.qx` files with a TextMate grammar covering comments, strings with escapes and `{}` placeholders, imports, functions, keywords such as `final`, `var`, `consume`, and `public`, classical and quantum types, `true`, `false`, `PI`, numbers, built-in functions, and `BitReg` methods.
- **Language configuration**: Added `//` comments, brackets, auto-closing pairs, and indentation.
- **Language server client**: Added `qxide lsp` integration for saved and untitled `.qx` files, supporting diagnostics matching `qxide check` (including projects with `qxide.toml` and imports, with errors in imported modules shown on the `import` line), formatting matching `qxide fmt`, hover with `qxide explain` text for errors and declarations or signatures for names, document symbols, go to definition for locals, parameters, and functions across imported modules, and completion for keywords, types, built-in functions, `PI`, functions in the file and imports, local names in scope, and `BitReg` methods.
- **Settings**: Added `qxide.path` (defaults to `qxide` from `PATH`, expands a leading `~` to the home directory, and restarts the server on change) and `qxide.trace.server` (`off`, `messages`, `verbose`).
- **Commands**: Added `Qxide: Restart Language Server`.
- **Requirements**: Requires `qxide` 0.8.0 or newer and Visual Studio Code 1.91 or newer.
