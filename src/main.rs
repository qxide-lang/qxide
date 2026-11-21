//! `qxide` command-line entry point.
//!
//! The crate is a library first; this binary is a thin wrapper so the project
//! can be installed as a standalone tool (cargo-dist installers, PyPI wheel).

mod lang;
mod lsp;
mod pkg;

use qxide::{Backend, Circuit};
use std::path::Path;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// The tree-walking interpreter recurses per nested call/block; give it more stack than the
// 1 MiB Windows main thread so the call-depth limit, not a stack overflow, is what stops it.
const LANG_STACK: usize = 64 << 20;

fn usage() -> String {
    format!(
        "Usage: qxide <COMMAND>

Commands:
  run [FILE] [--shots N] [--seed S]  Run a .qx program and simulate it (default: 1 shot, seed 0)
  check [FILE]                       Check a .qx program for errors without running it
  qasm [FILE]                        Print the OpenQASM 3 circuit built by a .qx program
  fmt <FILE> [--check]               Format a .qx file in place (--check exits non-zero if it would change)
  repl [--seed S]                    Start an interactive .qx session (default: seed 0)
  explain <CODE>                     Explain an error code with an example and fix (e.g. Q0302)
  new <NAME>                         Create a new project folder with main.qx and README.md
  bell [SHOTS]                       Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)
  lsp                                {}
{}
Options:
  -V, --version                      Print version
  -h, --help                         Print help
",
        LSP_HELP,
        pkg::cli::usage()
    )
}

const LSP_HELP: &str = "Start a .qx language server (LSP) on stdin/stdout";

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
    let mut seed = 0;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let parsed = match flag.as_str() {
            "--seed" => parse_count(flag, rest.next(), 0).map(|n| seed = n),
            _ => {
                eprintln!("qxide: unexpected argument `{flag}` for `repl`");
                Err(ExitCode::from(2))
            }
        };
        if let Err(code) = parsed {
            return code;
        }
    }
    let code = on_lang_stack(move || {
        use std::io::IsTerminal;
        let stdin = std::io::stdin();
        let interactive = stdin.is_terminal();
        let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
        lang::repl::run(stdin.lock(), &mut out, &mut err, seed, interactive)
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

// Runs one `.qx` command: `run`, `check` or `qasm`.
fn lang_command(cmd: &str, args: &[String]) -> ExitCode {
    // Flags may come before or after the file (`run --shots 9 a.qx` or `run a.qx --shots 9`).
    let (mut shots, mut seed, mut file) = (1, 0, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let parsed = match (cmd, arg.as_str()) {
            ("run", "--shots") => parse_count(arg, rest.next(), 1).map(|n| shots = n),
            ("run", "--seed") => parse_count(arg, rest.next(), 0).map(|n| seed = n),
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
    let cmd = cmd.to_string();
    let worker = std::thread::Builder::new()
        .stack_size(LANG_STACK)
        .spawn(move || {
            let input = lang::Input {
                name: &path,
                src: &src,
                project: project.as_ref(),
                read: &lang::read_file,
            };
            match cmd.as_str() {
                "check" => lang::check_input(&input).map(|()| {
                    let mut line = format!("{path}: ok");
                    line.push('\n');
                    line
                }),
                "qasm" => lang::qasm_input(&input),
                _ => lang::run_input(&input, shots, seed),
            }
        });
    match worker.map(|h| h.join()) {
        Ok(Ok(Ok(out))) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Ok(Ok(Err(lang::Failure(diags)))) => {
            eprint!("{diags}");
            ExitCode::from(1)
        }
        // The worker only fails if the interpreter itself panicked; the panic message is already on stderr.
        _ => ExitCode::from(101),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(flag @ ("--version" | "-V" | "--help" | "-h")) if args.len() > 1 => {
            eprintln!("qxide: unexpected argument `{}` for `{flag}`", args[1]);
            return ExitCode::from(2);
        }
        Some("--version" | "-V") => println!("qxide {VERSION}"),
        Some("--help" | "-h") | None => print!("{}", usage()),
        Some(cmd @ ("run" | "check" | "qasm")) => return lang_command(cmd, &args[1..]),
        Some("fmt") => return fmt_command(&args[1..]),
        Some("explain") => return explain_command(&args[1..]),
        Some("new") => return new_command(&args[1..]),
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
