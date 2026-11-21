# {name}

This is a Qxide desktop app. The interface is in `ui/index.html`, `ui/app.js`, and `ui/style.css`, and the logic is in `src/app.qx`. Any `public` function in `src/app.qx` listed in `commands` of the `[app]` section of `qxide.toml` can be called from the page with `qxide.invoke`. No Rust toolchain or npm is needed.

## Commands

```sh
qxide app dev           # run the app in a window (edits to .qx apply on the next call, F5 reloads the page)
qxide app build         # build a portable executable in target/app/
qxide check src/app.qx  # check the .qx code
```

## How it works

- `measure_bell` returns a `BitReg`, and with `shots` the runtime returns counts of each result.
- `save_result` uses the standard library (`std.fs`, `std.time`, `std.log`), allowed by `permissions` in `[app]`, and writes `results.txt` in the app's data folder (`target/app-data/` during `qxide app dev`).
- Standard-library calls run once per call, before the shots are simulated.

For more details, see the design document [`docs/design/qx-app.md`](https://github.com/qxide-lang/qxide/blob/main/docs/design/qx-app.md) in the Qxide repository.
