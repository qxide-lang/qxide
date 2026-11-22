//! `qxide` command-line entry point.
//!
//! The crate is a library first; this binary is a thin wrapper so the project
//! can be installed as a standalone tool (cargo-dist installers, PyPI wheel).

mod app;
mod bundle;
mod lang;
mod lsp;
mod pkg;
mod qpu;

use qxide::{Backend, Circuit};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// The tree-walking interpreter recurses per nested call/block; give it more stack than the
// 1 MiB Windows main thread so the call-depth limit, not a stack overflow, is what stops it.
const LANG_STACK: usize = lang::STACK_SIZE;

fn usage() -> String {
    format!(
        "Usage: qxide <COMMAND>

Commands:
  run [FILE] [--shots N] [--seed S] [-- ARGS...]
                                     Run a .qx program and simulate it (default: 1 shot, random seed, printed to stderr)
  run [FILE] --backend ibm:DEVICE    {}
                                     {}
  check [FILE]                       Check a .qx program for errors without running it
  qasm [FILE]                        Print the OpenQASM 3 circuit built by a .qx program
  build [FILE] [-o OUT] [--engine E] [--shots N] [--seed S] [--allow fs]
                                     {}
  fmt <FILE> [--check]               Format a .qx file in place (--check exits non-zero if it would change)
  repl [--seed S]                    Start an interactive .qx session (default: random seed, printed to stderr)
  explain <CODE>                     Explain an error code with an example and fix (e.g. Q0302)
  new <NAME>                         Create a new project folder with main.qx and README.md
  bell [SHOTS]                       Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)
  lsp                                {}
{}{}
Options:
  --engine ast|ir|jit                {}
  -V, --version                      Print version
  -h, --help                         Print help
",
        BACKEND_HELP,
        BACKEND_FLAGS_HELP,
        BUILD_HELP,
        LSP_HELP,
        pkg::cli::usage(),
        app::usage(),
        ENGINE_HELP
    )
}

const BACKEND_HELP: &str =
    "Run a .qx program on an IBM quantum computer (prompts before submitting; uses QPU time)";

const BACKEND_FLAGS_HELP: &str = "--dry-run (preview), --yes (no prompt), --max-seconds S (QPU cap, default 120), --device-config FILE (dry-run)";

const BUILD_HELP: &str = "Package a .qx program into a standalone executable";

const LSP_HELP: &str = "Start a .qx language server (LSP) on stdin/stdout";

const ENGINE_HELP: &str = "Select engine for run, qasm, and repl (default: QXIDE_ENGINE or ast; ir: typed IR; jit: IR + native, needs jit feature)";

// `--engine` (or `QXIDE_ENGINE` when the flag is absent): which interpreter builds the circuit.
fn parse_engine(flag: &str, value: Option<&str>) -> Result<lang::Engine, ExitCode> {
    match value.and_then(lang::Engine::parse) {
        // `dual` (every engine, compared) is for tests and not advertised.
        Some(e) => Ok(e),
        None if value == Some("jit") => {
            eprintln!("qxide: `jit` not available for {flag} (this build has no JIT; rebuild with `cargo build --features jit`, or use `--engine ir`)");
            Err(ExitCode::from(2))
        }
        None => {
            let v = value.unwrap_or_default();
            eprintln!("qxide: invalid value `{v}` for {flag} (expected `ast`, `ir`, or `jit`)");
            Err(ExitCode::from(2))
        }
    }
}

// The engine of `run`, `qasm` and `repl` when `--engine` is not given.
fn default_engine() -> Result<lang::Engine, ExitCode> {
    match std::env::var("QXIDE_ENGINE") {
        Ok(v) => parse_engine("QXIDE_ENGINE", Some(&v)),
        Err(_) => Ok(lang::Engine::Ast),
    }
}

fn bell(shots: u64) {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();
    c.h(&mut a);
    c.cx(&mut a, &mut b);
    let ma = c.measure(a);
    let mb = c.measure(b);

    println!("OpenQASM 3");
    print!("{}", c.to_qasm());

    println!("\nSimulation Counts");
    let mut counts = [0u64; 4];
    for shot in c.simulate_shots(Backend::StateVector, 0, shots) {
        let shot = shot.unwrap_or_else(|e| panic!("{e}"));
        counts[(shot.get(ma) as usize) << 1 | shot.get(mb) as usize] += 1;
    }
    for (k, n) in counts.iter().enumerate() {
        println!("{}{}: {n}", k >> 1, k & 1);
    }
}

// A fresh seed for runs without `--seed`, from std only: `RandomState` keys come from the OS RNG,
// and the clock and pid are mixed in too; SplitMix64 finalises the hash.
fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    splitmix64(h.finish())
}

fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn parse_count(flag: &str, value: Option<&String>, min: u64) -> Result<u64, ExitCode> {
    match value.map(|s| (s, s.parse::<u64>())) {
        Some((_, Ok(n))) if n >= min => Ok(n),
        Some((s, _)) => {
            eprintln!("qxide: invalid value `{s}` for {flag}");
            Err(ExitCode::from(2))
        }
        None => {
            eprintln!("qxide: option {flag} requires a value");
            Err(ExitCode::from(2))
        }
    }
}

fn parse_value(flag: &str, value: Option<&String>) -> Result<String, ExitCode> {
    value.cloned().ok_or_else(|| {
        eprintln!("qxide: option {flag} requires a value");
        ExitCode::from(2)
    })
}

// Runs `f` on a thread with the interpreter's stack size; `None` if it panicked (the message is on stderr).
fn on_lang_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let worker = std::thread::Builder::new().stack_size(LANG_STACK).spawn(f);
    worker.ok()?.join().ok()
}

fn read_source(path: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("qxide: failed to read `{path}`: {e}");
        ExitCode::from(1)
    })
}

fn fmt_command(args: &[String]) -> ExitCode {
    let (mut path, mut check) = (None, false);
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            p if path.is_none() && !p.starts_with('-') => path = Some(p.to_string()),
            _ => {
                eprintln!("qxide: unexpected argument `{arg}` for `fmt`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = path else {
        eprintln!("qxide: command `fmt` requires a .qx file path");
        eprint!("{}", usage());
        return ExitCode::from(2);
    };
    let src = match read_source(&path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let (name, text) = (path.clone(), src.clone());
    let formatted = match on_lang_stack(move || lang::format(&name, &text)) {
        Some(Ok(out)) => out,
        Some(Err(lang::Failure(diags))) => {
            eprint!("{diags}");
            return ExitCode::from(1);
        }
        None => return ExitCode::from(101),
    };
    if formatted == src {
        return ExitCode::SUCCESS;
    }
    if check {
        eprintln!("qxide: `{path}` is not formatted; run `qxide fmt {path}` to format it");
        return ExitCode::from(1);
    }
    if let Err(e) = std::fs::write(&path, formatted) {
        eprintln!("qxide: failed to write `{path}`: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

const TEMPLATE_MAIN: &str = include_str!("template/main.qx");
const TEMPLATE_README: &str = include_str!("template/README.md");

// Templates are checked out with CRLF on some Windows setups; projects are always written with LF.
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// A valid package name (`[a-z][a-z0-9-]*`, see `pkg::valid_name`) for a project folder name:
// other characters become single hyphens, and a name that cannot start with a letter gets `app`.
fn package_name(folder: &str) -> String {
    let mut name = String::new();
    for c in folder.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let mut name = match name.trim_end_matches('-') {
        "" => "app".to_string(),
        // `std` is reserved for the standard library.
        "std" => "std-app".to_string(),
        n if n.starts_with(|c: char| c.is_ascii_digit()) => format!("app-{n}"),
        n => n.to_string(),
    };
    name.truncate(64);
    let name = name.trim_end_matches('-').to_string();
    debug_assert!(pkg::valid_name(&name), "{name}");
    name
}

fn new_command(args: &[String]) -> ExitCode {
    if let Some(flag) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!("qxide: unexpected argument `{flag}` for `new`");
        return ExitCode::from(2);
    }
    let [dir] = args else {
        match args.get(1) {
            Some(extra) => eprintln!("qxide: unexpected argument `{extra}` for `new`"),
            None => {
                eprintln!("qxide: command `new` requires a project name");
                eprint!("{}", usage());
            }
        }
        return ExitCode::from(2);
    };
    let root = Path::new(dir);
    let Some(name) = root.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        eprintln!("qxide: cannot determine a project name from `{dir}`");
        return ExitCode::from(2);
    };
    if root.exists() {
        eprintln!("qxide: destination `{dir}` already exists");
        return ExitCode::from(1);
    }
    // `create_dir`, not `create_dir_all`: it also fails if the folder appeared in the meantime.
    if let Err(e) = std::fs::create_dir(root) {
        eprintln!("qxide: failed to create directory `{dir}`: {e}");
        return ExitCode::from(1);
    }
    let (major, minor) = VERSION.split_once('.').map_or((VERSION, "0"), |(a, rest)| {
        (a, rest.split('.').next().unwrap_or("0"))
    });
    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nqxide = \">={major}.{minor}\"\n\n[dependencies]\n",
        package_name(&name)
    );
    let files = [
        ("qxide.toml", manifest),
        ("src/main.qx", lf(TEMPLATE_MAIN)),
        ("README.md", lf(TEMPLATE_README).replace("{name}", &name)),
        (".gitignore", "target/\n".to_string()),
    ];
    let written = std::fs::create_dir(root.join("src")).and_then(|()| {
        files
            .iter()
            .try_for_each(|(file, text)| std::fs::write(root.join(file), text))
    });
    if let Err(e) = written {
        // Everything in `root` was created just now, so removing it loses nothing of the user's.
        let _ = std::fs::remove_dir_all(root);
        eprintln!("qxide: failed to create directory `{dir}`: {e}");
        return ExitCode::from(1);
    }
    let main = root.join("src").join("main.qx");
    let main = main.display();
    print!("Created project `{name}` in `{dir}` with `qxide.toml`, `src/main.qx`, `README.md`, and `.gitignore`\nNext:\n  qxide run {main}\n  qxide fmt {main}\n");
    ExitCode::SUCCESS
}

fn repl_command(args: &[String]) -> ExitCode {
    let (mut seed, mut engine) = (None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let parsed = match flag.as_str() {
            "--seed" => parse_count(flag, rest.next(), 0).map(|n| seed = Some(n)),
            "--engine" => {
                parse_engine(flag, rest.next().map(String::as_str)).map(|e| engine = Some(e))
            }
            _ => {
                eprintln!("qxide: unexpected argument `{flag}` for `repl`");
                Err(ExitCode::from(2))
            }
        };
        if let Err(code) = parsed {
            return code;
        }
    }
    let engine = match engine.map_or_else(default_engine, Ok) {
        Ok(e) => e,
        Err(code) => return code,
    };
    let (seed, announce) = seed.map_or_else(|| (random_seed(), true), |s| (s, false));
    let code = on_lang_stack(move || {
        lang::set_engine(engine);
        use std::io::IsTerminal;
        let stdin = std::io::stdin();
        let interactive = stdin.is_terminal();
        let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
        lang::repl::run(
            stdin.lock(),
            &mut out,
            &mut err,
            seed,
            announce,
            interactive,
        )
    });
    ExitCode::from(code.unwrap_or(101))
}

fn explain_command(args: &[String]) -> ExitCode {
    match args {
        [code] => match lang::explain(code) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("qxide: unknown error code `{code}` (codes look like `Q0302`)");
                ExitCode::from(2)
            }
        },
        [] => {
            eprintln!("qxide: command `explain` requires an error code (e.g. `Q0302`)");
            eprint!("{}", usage());
            ExitCode::from(2)
        }
        [_, extra, ..] => {
            eprintln!("qxide: unexpected argument `{extra}` for `explain`");
            ExitCode::from(2)
        }
    }
}

// `--allow fs`: the only permission `qxide run` can grant (`std.log` and `std.time` need none).
fn parse_allow(value: Option<&String>) -> Result<(), ExitCode> {
    match value.map(String::as_str) {
        Some("fs") => Ok(()),
        other => {
            eprintln!(
                "qxide: invalid value `{}` for --allow (expected `fs`)",
                other.unwrap_or_default()
            );
            Err(ExitCode::from(2))
        }
    }
}

fn project_data_dir(entry: &std::path::Path) -> std::path::PathBuf {
    let dir = entry.parent().unwrap_or(std::path::Path::new("."));
    pkg::manifest::find(dir).map_or_else(|| std::path::PathBuf::from("data"), |d| d.join("data"))
}

// Runs one `.qx` command: `run`, `check` or `qasm`.
fn lang_command(cmd: &str, args: &[String]) -> ExitCode {
    // Flags may come before or after the file (`run --shots 9 a.qx` or `run a.qx --shots 9`).
    let (mut shots, mut seed, mut file, mut allow_fs) = (1, None, None, false);
    let mut engine = None;
    // `run --backend PROVIDER:DEVICE` (src/qpu): a quantum computer service instead of the simulator.
    let (mut backend, mut dry_run, mut yes) = (None, false, false);
    let (mut max_seconds, mut device_config) = (None, None);
    // `run f.qx -- a b`: everything after `--` goes to `main(String[] args)`.
    let mut program_args = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let parsed = match (cmd, arg.as_str()) {
            ("run", "--") => {
                program_args.extend(rest.by_ref().cloned());
                Ok(())
            }
            ("run" | "qasm", "--engine") => {
                parse_engine(arg, rest.next().map(String::as_str)).map(|e| engine = Some(e))
            }
            ("run", "--shots") => parse_count(arg, rest.next(), 1).map(|n| shots = n),
            ("run", "--seed") => parse_count(arg, rest.next(), 0).map(|n| seed = Some(n)),
            ("run" | "check", "--allow") => parse_allow(rest.next()).map(|()| allow_fs = true),
            ("run", "--backend") => parse_value(arg, rest.next()).map(|v| backend = Some(v)),
            ("run", "--dry-run") => {
                dry_run = true;
                Ok(())
            }
            ("run", "--yes") => {
                yes = true;
                Ok(())
            }
            ("run", "--max-seconds") => {
                parse_count(arg, rest.next(), 1).map(|n| max_seconds = Some(n))
            }
            ("run", "--device-config") => {
                parse_value(arg, rest.next()).map(|v| device_config = Some(v))
            }
            (_, p) if file.is_none() && !p.starts_with('-') => {
                file = Some(p.to_string());
                Ok(())
            }
            _ => {
                eprintln!("qxide: unexpected argument `{arg}` for `{cmd}`");
                Err(ExitCode::from(2))
            }
        };
        if let Err(code) = parsed {
            return code;
        }
    }
    let checked = qpu::check_flags(
        backend.is_some(),
        dry_run,
        yes,
        max_seconds,
        device_config.is_some(),
        seed.is_some(),
        allow_fs,
    )
    .and_then(|()| {
        backend
            .as_deref()
            .map_or(Ok(()), |b| qpu::provider(b).map(drop))
    });
    if let Err(e) = checked {
        eprintln!("qxide: {e}");
        return ExitCode::from(2);
    }
    let settings = qpu::Settings {
        dry_run,
        yes,
        max_seconds: max_seconds.unwrap_or(qpu::DEFAULT_MAX_SECONDS),
        device_config,
    };
    let engine = match engine.map_or_else(default_engine, Ok) {
        Ok(e) => e,
        Err(code) => return code,
    };
    // Without a file, use `src/main.qx` of the project around the current directory.
    let path = match file {
        Some(p) => p,
        None => match pkg::project::default_entry() {
            Ok(p) => p.display().to_string(),
            Err(_) => {
                eprintln!("qxide: command `{cmd}` requires a .qx file path");
                eprint!("{}", usage());
                return ExitCode::from(2);
            }
        },
    };
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("qxide: failed to read `{path}`: {e}");
            return ExitCode::from(1);
        }
    };
    // Inside a project (a `qxide.toml` above the file), imports resolve to its locked packages.
    let project = match pkg::project::for_entry(std::path::Path::new(&path)) {
        Ok(p) => p,
        Err(e) => {
            eprint!("{}", e.render());
            return ExitCode::from(1);
        }
    };
    let (seed, announce) = seed.map_or_else(|| (random_seed(), true), |s| (s, false));
    let cmd = cmd.to_string();
    // `std.fs` paths of `qxide run --allow fs` live in `data/` next to `qxide.toml` (or here).
    let data_root = project_data_dir(std::path::Path::new(&path));
    // `qxide check` of an app's modules uses the app's `[app] permissions`.
    let app_perms = match cmd.as_str() {
        "check" => app::permissions_for(std::path::Path::new(&path)),
        _ => None,
    };
    let worker = std::thread::Builder::new()
        .stack_size(LANG_STACK)
        .spawn(move || {
            lang::set_engine(engine);
            let input = lang::Input {
                name: &path,
                src: &src,
                project: project.as_ref(),
                read: &lang::read_file,
                perms: app_perms.unwrap_or_else(|| lang::stdlib::Permissions::cli(allow_fs)),
            };
            match cmd.as_str() {
                "check" => lang::check_input(&input).map(|()| {
                    let mut line = format!("{path}: ok");
                    line.push('\n');
                    (line, false, 0)
                }),
                "qasm" => lang::qasm_input(&input).map(|out| (out, false, 0)),
                _ if backend.is_some() => {
                    let spec = backend.as_deref().unwrap_or_default();
                    let exported = lang::export_input(&input)?;
                    qpu::provider(spec)
                        .and_then(|p| {
                            qpu::run(&exported, &*p, shots, &settings, &mut qpu::TerminalUi)
                        })
                        .map(|out| (out, false, 0))
                        .map_err(|e| lang::Failure(format!("qxide: {e}\n")))
                }
                _ => {
                    let mut host = lang::cli_host(data_root);
                    host.args = program_args;
                    lang::run_input_report(&input, shots, seed, &mut host)
                        .map(|r| (r.text, r.simulated, r.exit_code))
                }
            }
        });
    match worker.map(|h| h.join()) {
        Ok(Ok(Ok((out, simulated, code)))) => {
            // Only a run whose output depended on the random seed needs it to be reproducible.
            if simulated && announce {
                eprint!("{}", lang::seed_note(seed));
            }
            print!("{out}");
            ExitCode::from(code)
        }
        Ok(Ok(Err(lang::Failure(diags)))) => {
            eprint!("{diags}");
            ExitCode::from(1)
        }
        // The worker only fails if the interpreter itself panicked; the panic message is already on stderr.
        _ => ExitCode::from(101),
    }
}

// `qxide build`: checks the program, then writes this executable with the program appended
// (src/bundle.rs). The bundle keeps every module file the linker read, so the output needs
// neither the sources nor the package cache at run time.
fn build_command(args: &[String]) -> ExitCode {
    let (mut shots, mut seed, mut file, mut allow_fs) = (1, None, None, false);
    let (mut engine, mut out) = (None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let parsed = match arg.as_str() {
            "-o" | "--output" => parse_value(arg, rest.next()).map(|v| out = Some(v)),
            "--engine" => {
                parse_engine(arg, rest.next().map(String::as_str)).map(|e| engine = Some(e))
            }
            "--shots" => parse_count(arg, rest.next(), 1).map(|n| shots = n),
            "--seed" => parse_count(arg, rest.next(), 0).map(|n| seed = Some(n)),
            "--allow" => parse_allow(rest.next()).map(|()| allow_fs = true),
            p if file.is_none() && !p.starts_with('-') => {
                file = Some(p.to_string());
                Ok(())
            }
            _ => {
                eprintln!("qxide: unexpected argument `{arg}` for `build`");
                Err(ExitCode::from(2))
            }
        };
        if let Err(code) = parsed {
            return code;
        }
    }
    let engine = match engine.map_or_else(default_engine, Ok) {
        Ok(lang::Engine::Dual) => {
            eprintln!("qxide: invalid value `dual` for --engine (expected `ast`, `ir`, or `jit`)");
            return ExitCode::from(2);
        }
        Ok(e) => e,
        Err(code) => return code,
    };
    let path = match file {
        Some(p) => p,
        None => match pkg::project::default_entry() {
            Ok(p) => p.display().to_string(),
            Err(_) => {
                eprintln!("qxide: command `build` requires a .qx file path");
                eprint!("{}", usage());
                return ExitCode::from(2);
            }
        },
    };
    let src = match read_source(&path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let project = match pkg::project::for_entry(Path::new(&path)) {
        Ok(p) => p,
        Err(e) => {
            eprint!("{}", e.render());
            return ExitCode::from(1);
        }
    };
    if project.as_ref().is_some_and(|p| p.entry_module.is_some()) {
        eprintln!("qxide: `{path}` is a library module with no `main`; pass the program's entry file instead (e.g. `src/main.qx`)");
        return ExitCode::from(1);
    }
    // The name as given is checked too: `-o some_dir` must not become `some_dir.exe`.
    let given = PathBuf::from(out.unwrap_or_else(|| default_output(&path)));
    let out = with_exe_suffix(given.clone());
    let entry = Path::new(&path);
    if let Err(e) = check_output(&given, entry).and_then(|()| check_output(&out, entry)) {
        eprintln!("qxide: {e}");
        return ExitCode::from(1);
    }
    let (name, text, proj) = (path.clone(), src.clone(), project.clone());
    let checked = on_lang_stack(move || {
        let files = std::cell::RefCell::new(Vec::new());
        let read = |p: &Path| {
            let r = lang::read_file(p);
            if let Ok(t) = &r {
                files.borrow_mut().push((p.to_path_buf(), t.clone()));
            }
            r
        };
        let input = lang::Input {
            name: &name,
            src: &text,
            project: proj.as_ref(),
            read: &read,
            perms: lang::stdlib::Permissions::cli(allow_fs),
        };
        let checked = lang::check_input(&input);
        checked.map(|()| files.into_inner())
    });
    let files = match checked {
        Some(Ok(f)) => f,
        Some(Err(lang::Failure(diags))) => {
            eprint!("{diags}");
            return ExitCode::from(1);
        }
        None => return ExitCode::from(101),
    };
    let mut bundled = Vec::with_capacity(files.len());
    for (p, t) in files {
        match p.to_str() {
            Some(s) => bundled.push((s.to_string(), t)),
            None => {
                eprintln!("qxide: path `{}` is not valid UTF-8", p.display());
                return ExitCode::from(1);
            }
        }
    }
    let b = bundle::Bundle {
        version: VERSION.to_string(),
        name: path,
        src,
        engine: engine.name().to_string(),
        shots,
        seed,
        allow_fs,
        project,
        files: bundled,
    };
    match write_bundle(&b, &out) {
        Ok(()) => {
            let shown = out.display();
            eprintln!("Wrote executable to `{shown}`");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("qxide: {e}");
            ExitCode::from(1)
        }
    }
}

// `app.qx` -> `app`; a project's `src/main.qx` -> the package name.
fn default_output(entry: &str) -> String {
    let p = Path::new(entry);
    let stem = p
        .file_stem()
        .map_or_else(|| "app".to_string(), |s| s.to_string_lossy().into_owned());
    if stem == "main" {
        let dir = p.parent().unwrap_or(Path::new("."));
        let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        if let Some(m) = pkg::manifest::find(&dir).and_then(|d| pkg::manifest::load(&d).ok()) {
            return m.name;
        }
    }
    stem
}

// Windows runs only files with an executable extension: `-o app` becomes `app.exe` there.
fn with_exe_suffix(out: PathBuf) -> PathBuf {
    if cfg!(windows) && out.extension().is_none() {
        out.with_extension("exe")
    } else {
        out
    }
}

// The output may replace an earlier build, but never a source file, a directory or this binary.
fn check_output(out: &Path, entry: &Path) -> Result<(), String> {
    let shown = out.display();
    if out.is_dir() {
        return Err(format!("output path `{shown}` is a directory"));
    }
    if out.extension().is_some_and(|e| e == "qx") {
        return Err(format!("refusing to write an executable to `.qx` path `{shown}`; pass `-o` with an executable name"));
    }
    let same = |a: &Path, b: &Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    if same(out, entry) || std::env::current_exe().is_ok_and(|exe| same(out, &exe)) {
        return Err(format!(
            "refusing to overwrite `{shown}`; choose a different output path with `-o`"
        ));
    }
    Ok(())
}

// Copies this executable next to the output, appends the bundle, then renames it into place, so
// a failed build never leaves a half-written program behind.
fn write_bundle(b: &bundle::Bundle, out: &Path) -> Result<(), String> {
    use std::io::Write;
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot locate the qxide executable: {e}"))?;
    let payload = b.encode()?;
    let file_name = out
        .file_name()
        .map_or_else(|| "out".into(), |n| n.to_string_lossy().into_owned());
    let tmp = out.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let fail = |what: &str, e: std::io::Error| {
        let _ = std::fs::remove_file(&tmp);
        format!("failed to {what} `{}`: {e}", out.display())
    };
    std::fs::copy(&exe, &tmp).map_err(|e| fail("write", e))?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(&payload).and_then(|()| f.sync_all()))
        .map_err(|e| fail("write", e))?;
    std::fs::rename(&tmp, out).map_err(|e| fail("replace", e))
}

// A bundled executable: runs its program like `qxide run` with the options fixed at build time.
// Every command-line argument goes to `main(String[] args)`; without `--seed` at build time each
// run draws a fresh seed and, unlike `qxide run`, does not print it.
fn run_bundle(b: bundle::Bundle) -> ExitCode {
    if b.version != VERSION {
        eprintln!(
            "qxide: this program was bundled by qxide {} but carries the qxide {VERSION} runtime; rebuild it with `qxide build`",
            b.version
        );
        return ExitCode::from(1);
    }
    let Some(engine) = lang::Engine::parse(&b.engine).filter(|e| *e != lang::Engine::Dual) else {
        eprintln!(
            "qxide: this program needs the `{}` engine, which its runtime does not have; rebuild it with `qxide build`",
            b.engine
        );
        return ExitCode::from(1);
    };
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let seed = b.seed.unwrap_or_else(random_seed);
    let result = on_lang_stack(move || {
        lang::set_engine(engine);
        let files: std::collections::HashMap<PathBuf, String> = b
            .files
            .into_iter()
            .map(|(p, t)| (PathBuf::from(p), t))
            .collect();
        // Only the files read while building exist: the bundle never reads module sources.
        let read = |p: &Path| {
            files.get(p).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "not in the bundle")
            })
        };
        let input = lang::Input {
            name: &b.name,
            src: &b.src,
            project: b.project.as_ref(),
            read: &read,
            perms: lang::stdlib::Permissions::cli(b.allow_fs),
        };
        // `std.fs` of a bundle built with `--allow fs` uses `data/` in the current directory.
        let mut host = lang::cli_host(PathBuf::from("data"));
        host.args = args;
        lang::run_input_report(&input, b.shots, seed, &mut host).map(|r| (r.text, r.exit_code))
    });
    match result {
        Some(Ok((out, code))) => {
            print!("{out}");
            ExitCode::from(code)
        }
        Some(Err(lang::Failure(diags))) => {
            eprint!("{diags}");
            ExitCode::from(1)
        }
        None => ExitCode::from(101),
    }
}

fn main() -> ExitCode {
    // A bundled program (`qxide build`) runs instead of the command-line interface.
    match bundle::current() {
        Ok(Some(b)) => return run_bundle(b),
        Ok(None) => {}
        Err(e) => {
            eprintln!("qxide: the program bundled into this executable cannot be loaded ({e}); the file may be truncated or modified, so rebuild it with `qxide build`");
            return ExitCode::from(1);
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(flag @ ("--version" | "-V" | "--help" | "-h")) if args.len() > 1 => {
            eprintln!("qxide: unexpected argument `{}` for `{flag}`", args[1]);
            return ExitCode::from(2);
        }
        Some("--version" | "-V") => println!("qxide {VERSION}"),
        Some("--help" | "-h") | None => print!("{}", usage()),
        Some(cmd @ ("run" | "check" | "qasm")) => return lang_command(cmd, &args[1..]),
        Some("build") => return build_command(&args[1..]),
        Some("fmt") => return fmt_command(&args[1..]),
        Some("explain") => return explain_command(&args[1..]),
        Some("new") => return new_command(&args[1..]),
        Some("app") => return app::command(&args[1..]),
        Some("repl") => return repl_command(&args[1..]),
        // Language clients may pass `--stdio`; stdio is the only transport anyway.
        Some("lsp") if args.len() > 1 && !(args.len() == 2 && args[1] == "--stdio") => {
            eprintln!("qxide: unexpected argument `{}` for `lsp`", args[1]);
            return ExitCode::from(2);
        }
        Some("lsp") => return lsp::main(LANG_STACK),
        Some(cmd) if pkg::cli::COMMANDS.contains(&cmd) => return pkg::cli::main(cmd, &args[1..]),
        Some("bell") if args.len() > 2 => {
            eprintln!("qxide: unexpected argument `{}` for `bell`", args[2]);
            return ExitCode::from(2);
        }
        Some("bell") => {
            let shots = match args.get(1) {
                None => 1000,
                Some(s) => match s.parse::<u64>() {
                    Ok(n) if n > 0 => n,
                    _ => {
                        eprintln!("qxide: invalid shot count `{s}`");
                        return ExitCode::from(2);
                    }
                },
            };
            bell(shots);
        }
        Some(other) => {
            eprintln!("qxide: unknown command `{other}`");
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
