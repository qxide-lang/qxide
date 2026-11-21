// User-facing text of `qxide app`, kept in one place.

pub fn unknown_subcommand(cmd: &str) -> String {
    format!("unknown subcommand `{cmd}` for `qxide app`")
}

pub fn missing_subcommand() -> String {
    "missing subcommand for `qxide app`: expected `new`, `dev`, or `build`".into()
}

pub fn new_needs_name() -> String {
    "`qxide app new` requires exactly one app name".into()
}

pub fn unexpected_argument(arg: &str) -> String {
    format!("unexpected argument `{arg}` for `qxide app new`")
}

pub fn unexpected_argument_for(arg: &str, cmd: &str) -> String {
    format!("unexpected argument `{arg}` for `qxide {cmd}`")
}

pub fn bad_name(dir: &str) -> String {
    format!("cannot determine an app name from `{dir}`")
}

pub fn exists(dir: &str) -> String {
    format!("destination `{dir}` already exists")
}

pub fn linked_data_dir(path: &str) -> String {
    format!("`{path}` is a symbolic link or junction; `qxide app dev` keeps app data inside the project, so replace it with a plain folder")
}

pub fn io(path: &str, err: &str) -> String {
    format!("file operation on `{path}` failed: {err}")
}

pub fn created(name: &str, dir: &str) -> String {
    format!("Created desktop app `{name}` in `{dir}` with `qxide.toml`, `src/app.qx`, `ui/index.html`, `ui/app.js`, `ui/style.css`, `README.md`, and `.gitignore`")
}

pub fn next_steps(dir: &str) -> String {
    format!(
        "Next:
  cd {dir}
  qxide app dev"
    )
}

pub fn not_a_project(dir: &str) -> String {
    format!("not an app project: no `qxide.toml` found in `{dir}` or any parent directory")
}

pub fn internal() -> String {
    "internal error: checker thread stopped unexpectedly".into()
}

pub fn runtime_env_missing(path: &str) -> String {
    format!("`QXIDE_APP_RUNTIME` points to `{path}`, which is not a file")
}

pub fn runtime_missing(exe: &str) -> String {
    format!("app runtime `{exe}` not found next to the `qxide` executable; install a qxide release that includes it, or set `QXIDE_APP_RUNTIME` to its path")
}

pub fn dev_started(title: &str) -> String {
    format!("Running `{title}` in development mode; close the window to stop")
}

pub fn runtime_failed(path: &str, err: &str) -> String {
    format!("failed to start app runtime `{path}`: {err}")
}

pub fn ui_file_too_large(path: &str, mib: u64) -> String {
    format!("interface file `{path}` exceeds the {mib} MiB limit for bundled files")
}

pub fn runtime_has_bundle(path: &str) -> String {
    format!("runtime `{path}` already contains an app bundle; point `QXIDE_APP_RUNTIME` to a plain `qxide-app` runtime")
}

pub fn built(path: &str, files: usize, bundle_kib: u64, total_kib: u64) -> String {
    format!(
        "Built `{path}` ({files} bundled files, {bundle_kib} KiB app data, {total_kib} KiB total)"
    )
}

pub fn reason_string_list(key: &str) -> String {
    format!("`{key}` must be a list of strings")
}

pub fn reason_string(key: &str) -> String {
    format!("`{key}` must be a string")
}

pub fn reason_size(key: &str) -> String {
    format!("`{key}` must be a whole number of pixels from 200 to 8192")
}

pub fn reason_bool(key: &str) -> String {
    format!("`{key}` must be `true` or `false`")
}

pub fn reason_not_table() -> String {
    "`[app]` must be a table".into()
}

pub fn reason_unknown_key(key: &str) -> String {
    format!("unknown key `{key}`")
}

pub fn reason_no_commands() -> String {
    "`commands` must list at least one function name".into()
}

pub fn reason_bad_command(name: &str) -> String {
    format!("`{name}` in `commands` is not a valid function name")
}

pub fn reason_bad_permission(name: &str, known: &str) -> String {
    format!("unknown permission `{name}` (known permissions: {known})")
}

pub fn reason_bad_module(name: &str) -> String {
    format!("`module = \"{name}\"` is not a valid command module path such as `app` (and cannot be `main` or `lib`)")
}

pub fn reason_bad_identifier(id: &str) -> String {
    format!("`identifier = \"{id}\"` must be a reverse-DNS name such as `dev.qxide.app.demo` (letters, digits, and hyphens between dots)")
}

pub fn reason_no_app_section() -> String {
    "missing `[app]` section; add one or create an app with `qxide app new`".into()
}

pub fn reason_missing_module(path: &str) -> String {
    format!("command module file `{path}` does not exist")
}

pub fn usage_new() -> &'static str {
    "Create a desktop app project with .qx logic and an HTML/JS UI"
}

pub fn usage_dev() -> &'static str {
    "Run the app in a window, loading files directly from disk"
}

pub fn usage_build() -> &'static str {
    "Build a portable app executable in target/app/"
}
