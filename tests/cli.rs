use std::process::Command;

fn qxide(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(args)
        .output()
        .expect("run qxide")
}

#[test]
fn version_flag_prints_crate_version() {
    let out = qxide(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim(), format!("qxide {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn bell_never_yields_mixed_outcomes() {
    let out = qxide(&["bell", "200"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("OPENQASM 3"));
    assert!(text.contains("01: 0\n"));
    assert!(text.contains("10: 0\n"));
}

#[test]
fn unknown_command_fails() {
    assert_eq!(qxide(&["nope"]).status.code(), Some(2));
}

fn example(name: &str) -> String {
    format!("{}/examples/qx/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn tmp_qx(name: &str, src: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, src).unwrap();
    path.to_string_lossy().into_owned()
}

// M-04 reproduction (docs/security/PKG_CLIENT_FIXES.md): the echoed source line keeps no raw
// ESC or BEL bytes.
#[test]
fn diagnostics_do_not_echo_terminal_escapes() {
    let path = tmp_qx(
        "esc.qx",
        "void main() {\n    undefined_name; // \u{1b}]0;TITLE-SET-BY-QX-FILE\u{7}\u{1b}[2K\u{1b}[1A\u{1b}[2K\n}\n",
    );
    let out = qxide(&["check", &path]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.iter().any(|&b| b == 0x1b || b == 0x07));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("error[Q0201]"), "{err}");
    assert!(err.contains("  |     ^^^^^^^^^^^^^^\n"), "{err}");
}

#[test]
fn run_bell_example_gives_correlated_bits() {
    let out = qxide(&["run", &example("bell.qx"), "--shots", "100", "--seed", "7"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    // Table rows (`[0, 0]  0b00 = 0  ×52`) followed by that outcome's indented output.
    for line in text.lines() {
        match line.strip_prefix("  ") {
            Some(out) => assert!(out == "a=0 b=0" || out == "a=1 b=1", "{text}"),
            None => assert!(
                line.starts_with("[0, 0]  0b00 = 0  ×") || line.starts_with("[1, 1]  0b11 = 3  ×"),
                "{text}"
            ),
        }
    }
}

#[test]
fn check_accepts_examples_and_qasm_prints_openqasm() {
    for name in ["bell.qx", "ghz.qx", "teleport.qx"] {
        let out = qxide(&["check", &example(name)]);
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = qxide(&["qasm", &example("ghz.qx")]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("OPENQASM 3.0;\n"), "{text}");
    assert!(text.contains("qubit[5] q;\nbit[5] c;\n"), "{text}");
}

#[test]
fn check_reports_errors_with_line_and_column() {
    // CRLF line endings, as a Windows checkout may produce.
    let path = tmp_qx(
        "use_after_measure.qx",
        "void main() {\r\n    Qubit q = qubit();\r\n    Bit m = measure(q);\r\n    h(q);\r\n}\r\n",
    );
    for cmd in ["check", "run", "qasm"] {
        let out = qxide(&[cmd, &path]);
        assert_eq!(out.status.code(), Some(1), "{cmd}");
        assert!(out.stdout.is_empty(), "{cmd}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains("error[Q0301]"), "{err}");
        assert!(err.contains("use_after_measure.qx:4:7"), "{err}");
    }
}

#[test]
fn runtime_errors_exit_with_one() {
    let path = tmp_qx(
        "oob.qx",
        "void main() {\n    QReg r = qreg(2);\n    h(r[5]);\n    discard(r);\n}\n",
    );
    let out = qxide(&["run", &path]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("error[Q0403]") && err.contains("oob.qx:3:7"),
        "{err}"
    );
}

#[test]
fn deep_recursion_is_a_diagnostic_not_a_crash() {
    let path = tmp_qx(
        "recurse.qx",
        "int f(int n) {\n    if n > 0 - 1 {\n        return f(n + 1);\n    }\n    return n;\n}\nvoid main() {\n    println(\"{}\", f(0));\n}\n",
    );
    let out = qxide(&["run", &path]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("error[Q0405]"));
}

// The seed from a stderr that is exactly `seed: S (pass --seed S to reproduce)\n`.
fn seed_note(stderr: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(stderr);
    let (seed, rest) = text.strip_prefix("seed: ")?.split_once(' ')?;
    (rest == format!("(pass --seed {seed} to reproduce)\n"))
        .then(|| seed.parse().ok())
        .flatten()
}

#[test]
fn run_without_seed_uses_a_random_seed_and_reports_it() {
    let bell = example("bell.qx");
    let first = qxide(&["run", &bell, "--shots", "100"]);
    let second = qxide(&["run", &bell, "--shots", "100"]);
    assert!(first.status.success(), "{first:?}");
    assert!(second.status.success(), "{second:?}");
    let a = seed_note(&first.stderr).unwrap_or_else(|| panic!("{first:?}"));
    let b = seed_note(&second.stderr).unwrap_or_else(|| panic!("{second:?}"));
    assert_ne!(a, b);
    // The reported seed reproduces the run, and an explicit seed is not noted.
    let again = qxide(&["run", &bell, "--shots", "100", "--seed", &a.to_string()]);
    assert_eq!(again.stdout, first.stdout);
    assert!(again.stderr.is_empty(), "{again:?}");
    // One shot that prints a measured bit also depends on the seed.
    let one = qxide(&["run", &bell]);
    assert!(seed_note(&one.stderr).is_some(), "{one:?}");
}

#[test]
fn explicit_seed_is_deterministic_and_unannounced() {
    let bell = example("bell.qx");
    for shots in ["1", "100"] {
        let x = qxide(&["run", &bell, "--shots", shots, "--seed", "7"]);
        let y = qxide(&["run", "--seed", "7", &bell, "--shots", shots]);
        assert!(x.status.success(), "{x:?}");
        assert_eq!(x.stdout, y.stdout);
        assert!(x.stderr.is_empty() && y.stderr.is_empty(), "{x:?} {y:?}");
    }
}

#[test]
fn seed_is_not_reported_when_nothing_is_simulated() {
    let no_measure = tmp_qx(
        "seed_no_measure.qx",
        "void main() {\n    println(\"{}\", 1 + 1);\n}\n",
    );
    // Measured, but one shot that prints no bit does not depend on the outcome.
    let silent = tmp_qx(
        "seed_silent_measure.qx",
        "void main() {\n    Qubit q = qubit();\n    h(q);\n    Bit m = measure(q);\n    println(\"done\");\n}\n",
    );
    for (path, want) in [(&no_measure, "2\n"), (&silent, "done\n")] {
        let out = qxide(&["run", path]);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), want);
        assert!(out.stderr.is_empty(), "{out:?}");
    }
    // Several shots without any measurement do not simulate either.
    let many = qxide(&["run", &no_measure, "--shots", "5"]);
    assert!(many.status.success() && many.stderr.is_empty(), "{many:?}");
    for cmd in ["check", "qasm"] {
        let out = qxide(&[cmd, &example("bell.qx")]);
        assert!(out.status.success() && out.stderr.is_empty(), "{out:?}");
    }
}

#[test]
fn repl_without_seed_reports_it_once() {
    use std::io::Write;
    let session = |args: &[&str]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qxide"))
            .arg("repl")
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let input = "Qubit q = qubit();\nh(q);\nBit m = measure(q);\nm\nQubit r = qubit();\nBit n = measure(r);\nn\n";
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let first = session(&[]);
    assert!(first.status.success(), "{first:?}");
    let seed = seed_note(&first.stderr).unwrap_or_else(|| panic!("{first:?}"));
    let again = session(&["--seed", &seed.to_string()]);
    assert_eq!(again.stdout, first.stdout);
    assert!(again.stderr.is_empty(), "{again:?}");
    let other = session(&[]);
    assert_ne!(seed_note(&other.stderr), Some(seed));
}

#[test]
fn run_flags_may_come_before_the_file() {
    let bell = example("bell.qx");
    let after = qxide(&["run", &bell, "--shots", "50", "--seed", "3"]);
    let before = qxide(&["run", "--shots", "50", "--seed", "3", &bell]);
    let between = qxide(&["run", "--seed", "3", &bell, "--shots", "50"]);
    assert!(after.status.success());
    assert_eq!(before.stdout, after.stdout);
    assert_eq!(between.stdout, after.stdout);
    // A second file is still an error.
    let two = qxide(&["run", &bell, &bell]);
    assert_eq!(two.status.code(), Some(2));
}

#[test]
fn extra_arguments_are_rejected_everywhere() {
    for args in [
        &["bell", "3", "extra"][..],
        &["--version", "extra"],
        &["-V", "x"],
        &["--help", "x"],
    ] {
        let out = qxide(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
    assert!(qxide(&["bell", "3"]).status.success());
    assert!(qxide(&["-V"]).status.success());
}

// Left-associative chains (`a + b + ...`, `r[0][0]...`) build trees as deep as they are long;
// they count against the nesting limit, so even the deepest accepted chain inside the deepest
// allowed recursion must run without overflowing the stack.
#[test]
fn long_operator_and_index_chains_are_diagnostics_not_crashes() {
    let chain = |n: usize, op: &str| vec!["0"; n].join(op);
    for (name, src) in [
        (
            "chain_recursion.qx",
            format!(
                "int f(int k) {{\n    if k > 0 {{\n        return f(k - 1) + {};\n    }}\n    return 0;\n}}\n\nvoid main() {{\n    println(\"{{}}\", f(198));\n}}\n",
                chain(1000, " + ")
            ),
        ),
        ("chain_or.qx", format!("void main() {{\n    boolean b = {};\n}}\n", vec!["false"; 200_000].join(" || "))),
        ("chain_add.qx", format!("void main() {{\n    int x = {};\n}}\n", chain(200_000, " + "))),
        ("chain_index.qx", format!("void main() {{\n    QReg r = qreg(1);\n    h(r{});\n    discard(r);\n}}\n", "[0]".repeat(200_000))),
    ] {
        let path = tmp_qx(name, &src);
        for cmd in ["check", "run", "qasm"] {
            let out = qxide(&[cmd, &path]);
            let err = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(1), "{name} {cmd}: {err}");
            assert!(err.contains("error[Q0106]"), "{name} {cmd}: {err}");
        }
    }
    // The longest chain that still parses, recursing as deep as allowed, runs to completion.
    let deepest = format!(
        "int f(int k) {{\n    if k > 0 {{\n        return f(k - 1) + {};\n    }}\n    return 0;\n}}\n\nvoid main() {{\n    println(\"{{}}\", f(198));\n}}\n",
        chain(100, " + ")
    );
    let path = tmp_qx("chain_deepest_ok.qx", &deepest);
    let out = qxide(&["run", &path]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

// The costliest nesting kinds per level, at the limit, under the deepest allowed recursion; these
// overflowed the old 64 MiB stack in unoptimized builds (`cargo test --profile dev --test cli`).
#[test]
fn deepest_nesting_of_the_costliest_kinds_runs_in_any_build() {
    let int_fn = |body: String| {
        format!("int g(int x) {{\n    return x;\n}}\n\nint f(int k) {{\n    if k > 0 {{\n        {body}\n    }}\n    return 0;\n}}\n\nvoid main() {{\n    println(\"{{}}\", f(198));\n}}\n")
    };
    // Nested `if m` blocks count across calls (`max_cond_depth`, Q0416), so the bit program
    // nests only in the bottom call (see `deepest_programs` in src/lang/tests.rs).
    let bit_fn = |body: String| {
        format!("void g(int k, Bit m) {{\n}}\n\nvoid f(int k, Bit m) {{\n    if k > 0 {{\n        f(k - 1, m);\n    }} else {{\n        {body}\n    }}\n}}\n\nvoid main() {{\n    Qubit q = qubit();\n    Bit m = measure(q);\n    f(197, m);\n}}\n")
    };
    let nest = |n: usize, open: &str, inner: &str, close: &str| {
        format!("{}{inner}{}", open.repeat(n), close.repeat(n))
    };
    let chain = |n: usize| int_fn(format!("return f(k - 1){};", " + 0".repeat(n)));
    let call_args = |n: usize| int_fn(format!("return {};", nest(n, "g(", "f(k - 1)", ")")));
    let fors = |n: usize| {
        int_fn((0..n).fold("return f(k - 1);".to_string(), |s, i| {
            format!("for i{i} in 0..1 {{ {s} }}")
        }))
    };
    let bit_ifs = |n: usize| bit_fn(nest(n, "if m { ", "g(k - 1, m);", " }"));
    let builtins = |n: usize| {
        int_fn(format!(
            "return {};",
            nest(n, "len(measure(qreg(", "f(k - 1)", ")))")
        ))
    };
    // (kind, deepest accepted program, the same one level deeper)
    let kinds = [
        ("chain", chain(124), chain(125)),
        ("call_args", call_args(122), call_args(123)),
        ("for", fors(122), fors(123)),
        ("bit_if", bit_ifs(61), bit_ifs(62)),
        ("builtins", builtins(40), builtins(41)),
    ];
    for (kind, deepest, over) in kinds {
        let over = tmp_qx(&format!("deepest_{kind}_over.qx"), &over);
        let out = qxide(&["check", &over]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("error[Q0106]"), "{kind}: {err}");
        let path = tmp_qx(&format!("deepest_{kind}.qx"), &deepest);
        for cmd in ["run", "qasm"] {
            let out = qxide(&[cmd, &path]);
            assert!(
                out.status.success(),
                "{kind} {cmd}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn lang_command_usage_errors_exit_with_two() {
    assert_eq!(qxide(&["run"]).status.code(), Some(2));
    let bell = example("bell.qx");
    assert_eq!(
        qxide(&["run", &bell, "--shots", "0"]).status.code(),
        Some(2)
    );
    assert_eq!(qxide(&["run", &bell, "--shots"]).status.code(), Some(2));
    assert_eq!(
        qxide(&["check", &bell, "--shots", "3"]).status.code(),
        Some(2)
    );
    assert_eq!(qxide(&["check", "no/such/file.qx"]).status.code(), Some(1));
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&dir);
    dir
}

#[test]
fn new_creates_a_project_that_checks_and_is_formatted() {
    let dir = fresh_dir("new_project_demo");
    let dir_arg = dir.to_string_lossy().into_owned();
    let out = qxide(&["new", &dir_arg]);
    assert!(out.status.success(), "{out:?}");
    // A package project: manifest, entry point under `src/`, and `target/` ignored by git.
    let main = dir.join("src").join("main.qx");
    let main_arg = main.to_string_lossy().into_owned();
    assert!(main.is_file());
    assert!(!dir.join("main.qx").exists());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("qxide.toml"), "{stdout}");
    let manifest = std::fs::read_to_string(dir.join("qxide.toml")).unwrap();
    // `_` is not allowed in package names, so the folder name is adapted.
    assert!(
        manifest.contains("name = \"new-project-demo\"\n"),
        "{manifest}"
    );
    assert!(manifest.contains("version = \"0.1.0\"\n"), "{manifest}");
    let major_minor: Vec<&str> = env!("CARGO_PKG_VERSION").split('.').take(2).collect();
    assert!(
        manifest.contains(&format!("qxide = \">={}\"\n", major_minor.join("."))),
        "{manifest}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "target/\n"
    );
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
    assert!(readme.starts_with("# new_project_demo\n"), "{readme}");
    assert!(qxide(&["check", &main_arg]).status.success());
    assert!(qxide(&["fmt", "--check", &main_arg]).status.success());
    // Inside the project, `qxide run` finds `src/main.qx` by itself.
    let in_project = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["run", "--shots", "50", "--seed", "0"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(in_project.status.success(), "{in_project:?}");
    let run = qxide(&["run", &main_arg, "--shots", "50", "--seed", "0"]);
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, in_project.stdout);
    let table = String::from_utf8(run.stdout).unwrap();
    assert!(
        table.contains("a=0 b=0") || table.contains("a=1 b=1"),
        "{table}"
    );
    assert!(
        !table.contains("a=0 b=1") && !table.contains("a=1 b=0"),
        "{table}"
    );
}

#[test]
fn new_never_overwrites_an_existing_path() {
    let dir = fresh_dir("new_project_exists");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.qx"), "keep me").unwrap();
    let out = qxide(&["new", &dir.to_string_lossy()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(dir.join("main.qx")).unwrap(),
        "keep me"
    );
    assert!(!dir.join("README.md").exists());
    // A file in the way is refused too.
    let file = fresh_dir("new_project_file");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(
        qxide(&["new", &file.to_string_lossy()]).status.code(),
        Some(1)
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x");
    assert_eq!(qxide(&["new"]).status.code(), Some(2));
}

#[test]
fn new_package_names_are_derived_from_any_folder_name() {
    for (folder, package) in [
        ("New Project", "new-project"),
        ("큐비트", "app"),
        ("2fast", "app-2fast"),
        ("a__b--c-", "a-b-c"),
    ] {
        let dir = fresh_dir(&format!("new_names/{folder}"));
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        let out = qxide(&["new", &dir.to_string_lossy()]);
        assert!(out.status.success(), "{folder}: {out:?}");
        let manifest = std::fs::read_to_string(dir.join("qxide.toml")).unwrap();
        assert!(
            manifest.contains(&format!("name = \"{package}\"\n")),
            "{folder}: {manifest}"
        );
        let check = qxide(&["check", &dir.join("src/main.qx").to_string_lossy()]);
        assert!(check.status.success(), "{folder}: {check:?}");
    }
}

#[test]
fn new_rejects_flags_as_names() {
    for flag in ["--help", "-weird", "--name"] {
        // Older versions created these folders; start clean.
        let _ =
            std::fs::remove_dir_all(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(flag));
        let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(["new", flag])
            .current_dir(env!("CARGO_TARGET_TMPDIR"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{flag}");
        assert!(!std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(flag)
            .exists());
    }
}

// Windows drops a trailing space when creating `sp `, so writing `sp \src` fails; the folder
// `new` just created must not be left behind.
#[cfg(windows)]
#[test]
fn new_removes_its_folder_when_writing_fails() {
    let base = fresh_dir("new_cleanup");
    std::fs::create_dir_all(&base).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["new", "sp "])
        .current_dir(&base)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(std::fs::read_dir(&base).unwrap().count(), 0);
}

#[test]
fn fmt_rewrites_in_place_and_check_reports_changes() {
    let messy = "void main(){\nint x=1;println(\"{}\",x);}\n";
    let path = tmp_qx("fmt_me.qx", messy);
    let check = qxide(&["fmt", "--check", &path]);
    assert_eq!(check.status.code(), Some(1));
    assert!(!check.stderr.is_empty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        messy,
        "--check must not write"
    );
    assert!(qxide(&["fmt", &path]).status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "void main() {\n    int x = 1;\n    println(\"{}\", x);\n}\n"
    );
    assert!(qxide(&["fmt", "--check", &path]).status.success());
    for name in ["bell.qx", "teleport_fn.qx", "conditional_fn.qx"] {
        let out = qxide(&["fmt", "--check", &example(name)]);
        assert!(out.status.success(), "{name}");
    }
}

#[test]
fn fmt_refuses_unparsable_files_without_writing() {
    let src = "void main() {\n    int x = ;\n}\n";
    let path = tmp_qx("fmt_broken.qx", src);
    let out = qxide(&["fmt", &path]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("error[Q0105]"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), src);
    assert_eq!(qxide(&["fmt"]).status.code(), Some(2));
    assert_eq!(qxide(&["fmt", &path, "--shots"]).status.code(), Some(2));
}

#[test]
fn explain_prints_the_catalog_entry() {
    let out = qxide(&["explain", "q0301"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("Q0301: "), "{text}");
    assert!(
        text.contains("```qx") && text.contains("error[Q0301]"),
        "{text}"
    );
    assert_eq!(qxide(&["explain", "Q9999"]).status.code(), Some(2));
    assert_eq!(qxide(&["explain"]).status.code(), Some(2));
}

fn scratch_file(name: &str, src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qxide-cli-limits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    path
}

// M-01: 2000 errors on one 4 kB line printed about 12 MB (16000 errors, 771 MB) before the cap.
#[test]
fn check_of_one_line_with_many_errors_prints_a_bounded_report() {
    let src = format!("void main(){{{}}}\n", "a;".repeat(2000));
    let path = scratch_file("oneline_2000.qx", &src);
    let out = qxide(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(err.matches("error[Q0201]").count(), 50);
    assert!(
        err.contains("error[Q0418]"),
        "{}",
        &err[err.len().saturating_sub(400)..]
    );
    assert!(err.len() < 64 << 10, "{} bytes", err.len());
}

// M-10: this 610-byte program made `qasm` print 297 MB before conditional nesting was limited.
#[test]
fn qasm_refuses_conditionals_nested_too_deeply_across_calls() {
    let src = format!(
        "void f(Qubit q, Bit b, int n){{{}if(n>0){{f(q,b,n-1);}}x(q);{}}}\nvoid main(){{\n Qubit a=qubit(); h(a); Bit b=measure(a);\n Qubit q=qubit();\n f(q,b,198);\n Bit r=measure(q);\n println(\"{{}}\", r);\n}}\n",
        "if(b){".repeat(61),
        "}".repeat(61)
    );
    let path = scratch_file("nest_198_61.qx", &src);
    let out = qxide(&["qasm", path.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{} bytes of stdout",
        out.stdout.len()
    );
    assert!(out.stdout.is_empty());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("error[Q0416]"), "{err}");
}

// B-01 at the CLI's default budget; slow (about a minute), so run it with `--ignored`.
#[test]
#[ignore]
fn endless_loop_stops_at_the_default_step_limit() {
    let path = scratch_file(
        "loop.qx",
        "void main() {\n    for i in 0..9223372036854775807 {\n    }\n}\n",
    );
    let out = qxide(&["run", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("error[Q0413]"), "{err}");
}

fn repl_with(input: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run qxide repl");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn repl_runs_piped_input() {
    let out = repl_with("Qubit q = qubit();\nx(q);\nBit m = measure(q);\nh(q);\nm\n:qasm\n:quit\n");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("1\nOPENQASM 3.0;\n"), "{stdout}");
    // No banner or prompts when input is piped.
    assert!(!stdout.contains("qx> "), "{stdout}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("error[Q0301]") && stderr.contains("<repl>:4:3"),
        "{stderr}"
    );
    let leak = repl_with("Qubit q = qubit();\n");
    assert_eq!(leak.status.code(), Some(1));
    assert!(String::from_utf8(leak.stderr)
        .unwrap()
        .contains("error[Q0302]"));
}

// `qxide args` with `QXIDE_ENGINE` set to `env` (or unset) and `input` on stdin.
fn qxide_engine_env(args: &[&str], env: Option<&str>, input: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new(env!("CARGO_BIN_EXE_qxide"));
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match env {
        Some(e) => c.env("QXIDE_ENGINE", e),
        None => c.env_remove("QXIDE_ENGINE"),
    };
    let mut child = c.spawn().expect("run qxide");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

// Runs `qxide` with `input` on stdin and extra environment variables.
fn qxide_io(args: &[&str], env: &[(&str, &str)], input: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new(env!("CARGO_BIN_EXE_qxide"));
    c.args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().expect("run qxide");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

// Stage 2a: `main(String[] args)` gets the arguments after `--`, `int main()` is the exit code,
// `std.io` reads stdin and writes at once (before the per-shot `println` output), `std.env`
// reads the environment.
#[test]
fn process_io_args_stdin_env_and_exit_code() {
    let src = "import std.io;\nimport std.env;\n\nint main(String[] args) {\n    println(\"deferred\");\n    io.writeLine(\"args \" + len(args) + \" \" + args[1]);\n    String text = io.readAll();\n    io.writeLine(\"read \" + text.trim());\n    io.writeLine(env.get(\"QX_CLI_TEST\").orElse(\"unset\"));\n    return 7;\n}\n";
    let path = tmp_qx("process_io.qx", src);
    for engine in ["ast", "ir"] {
        let out = qxide_io(
            &["run", &path, "--engine", engine, "--", "--shots", "x y"],
            &[("QX_CLI_TEST", "set")],
            "hello\n",
        );
        assert_eq!(out.status.code(), Some(7), "{out:?}");
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "args 2 x y\nread hello\nset\ndeferred\n"
        );
    }
    // The word-count example reads stdin and fails with exit code 2 on an unknown option. It
    // imports `std.fs` (stage 2c), so it always needs `--allow fs`.
    let wc = format!("{}/examples/cli/wc.qx", env!("CARGO_MANIFEST_DIR"));
    let wc_run = |extra: &[&str], input: &str| {
        let mut args = vec!["run", wc.as_str(), "--allow", "fs"];
        args.extend(extra);
        qxide_io(&args, &[], input)
    };
    let out = wc_run(&[], "one two\n\nthree\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "3 3 15\n");
    let out = wc_run(&["--", "-w"], "one two\n");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "2\n");
    // `-f`: word counts, highest first, ties in first-seen order.
    let out = wc_run(&["--", "-f"], "b a b\n c  a b\n");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "2 6 14\n3 b\n2 a\n1 c\n"
    );
    let out = wc_run(&["--", "-q"], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("-q"));
    let out = qxide_io(&["run", &wc], &[], "one\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("Q0703"));
    // Out-of-range exit codes are a run-time error (exit code 1).
    let bad = tmp_qx("exit_256.qx", "int main() {\n    return 256;\n}\n");
    let out = qxide_io(&["run", &bad], &[], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("Q0706"));
}

// Stage 2c: with `--allow fs`, a program reads the files named after `--` (relative to the
// current directory) but cannot write them; other paths stay inside the data folder.
#[test]
fn run_reads_files_named_on_the_command_line() {
    let dir = fresh_dir("named_files");
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("one.txt"), "one two\n\nthree\n").unwrap();
    std::fs::write(dir.join("docs/b.txt"), "b a b\n c  a b\n").unwrap();
    std::fs::write(dir.join("docs/a.txt"), "x\n").unwrap();
    std::fs::write(dir.join("other.txt"), "secret\n").unwrap();
    let wc = format!("{}/examples/cli/wc.qx", env!("CARGO_MANIFEST_DIR"));
    let run_in = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    for engine in ["ast", "ir"] {
        let out = run_in(&[
            "run", &wc, "--engine", engine, "--allow", "fs", "--", "one.txt",
        ]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "3 3 15\n");
        let out = run_in(&[
            "run",
            &wc,
            "--engine",
            engine,
            "--allow",
            "fs",
            "--",
            "-f",
            "one.txt",
            "docs/b.txt",
        ]);
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "5 9 29\n3 b\n2 a\n1 one\n1 two\n1 three\n1 c\n"
        );
        let out = run_in(&["run", &wc, "--allow", "fs", "--", "missing.txt"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8(out.stderr)
            .unwrap()
            .contains("missing.txt"));
    }
    // A folder named on the command line can be listed and read below; nothing is writable,
    // and paths nobody named are refused.
    let prog = tmp_qx(
        "named_files.qx",
        "import std.io;\nimport std.fs;\nimport std.path;\n\nint main(String[] args) {\n    for name in fs.list(args[0]) {\n        String p = path.join(args[0], name);\n        io.writeLine(name + \" \" + fs.isDirectory(p) + \" \" + fs.readString(p).trim());\n    }\n    if args[1] == \"read\" {\n        io.writeLine(fs.readString(\"other.txt\"));\n    } else {\n        fs.writeString(path.join(args[0], \"new.txt\"), \"x\");\n    }\n    return 0;\n}\n",
    );
    let out = run_in(&["run", &prog, "--allow", "fs", "--", "docs", "read"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "a.txt false x\nb.txt false b a b\n c  a b\n"
    );
    // `other.txt` was not named: it means `data/other.txt`, which does not exist.
    assert!(String::from_utf8(out.stderr).unwrap().contains("Q0704"));
    let out = run_in(&["run", &prog, "--allow", "fs", "--", "docs", "write"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8(out.stderr).unwrap().contains("Q0705"));
    assert!(!dir.join("docs/new.txt").exists());
    // A bundle built with `--allow fs` reads the files its own arguments name.
    let exe = dir.join(exe_name("wc"));
    let out = qxide(&["build", &wc, "--allow", "fs", "-o", exe.to_str().unwrap()]);
    assert!(out.status.success(), "{out:?}");
    let out = run_exe(&exe, &dir, &["-l", "one.txt", "docs/a.txt"], "");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4
",
        "{out:?}"
    );
}

// Stage 5: `qxide test` runs every `void testName()` of the given file, of the `.qx` files below
// a folder, or of the project's `tests/` folder, and exits with 1 when any of them fails.
#[test]
fn test_command_runs_test_functions() {
    let dir = fresh_dir("test_command");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("tests/more")).unwrap();
    std::fs::write(
        dir.join("qxide.toml"),
        "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.qx"),
        "void main() {\n    println(\"hi\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/ops.qx"),
        "public int add(int a, int b) {\n    return a + b;\n}\n",
    )
    .unwrap();
    let good = "import std.test;\nimport calc.ops.add;\n\nvoid testAdd() {\n    test.assertEquals(\"5\", \"\" + add(2, 3));\n    test.assertTrue(add(1, 1) == 2, \"one plus one\");\n}\n\nvoid testQubits() {\n    Qubit q = qubit();\n    h(q);\n    discard(q);\n}\n\n// Not tests: a helper with a parameter, and a name that only starts with `test`.\nvoid testWith(int n) {\n    test.fail(\"never run\");\n}\n\nvoid testament() {\n    test.fail(\"never run\");\n}\n";
    std::fs::write(dir.join("tests/add.qx"), good).unwrap();
    let bad = "import std.test;\n\nvoid testWrong() {\n    test.assertEquals(\"4\", \"\" + (2 + 3));\n}\n\nvoid test_overflow() {\n    int big = 9223372036854775807;\n    test.assertTrue(big + 1 > 0, \"wraps\");\n}\n\nvoid testFail() {\n    test.assertFalse(true, \"flag\");\n}\n";
    std::fs::write(dir.join("tests/more/bad.qx"), bad).unwrap();
    let run_in = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    // The project's `tests/` folder by default.
    for engine in ["ast", "ir"] {
        let out = run_in(&["test", "--engine", engine]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        for line in [
            format!("test tests{sep}add.qx::testAdd ... ok"),
            format!("test tests{sep}add.qx::testQubits ... ok"),
            format!("test tests{sep}more{sep}bad.qx::testWrong ... FAILED"),
            format!("test tests{sep}more{sep}bad.qx::test_overflow ... FAILED"),
            format!("test tests{sep}more{sep}bad.qx::testFail ... FAILED"),
        ] {
            assert!(text.contains(&line), "{line}\n{text}");
        }
        assert!(
            !text.contains("testWith") && !text.contains("testament"),
            "{text}"
        );
        assert!(text.contains("Q0422") && text.contains("Q0401"), "{text}");
        assert!(text.contains("expected \"4\", got \"5\""), "{text}");
        assert!(
            text.ends_with("test result: FAILED. 2 passed; 3 failed\n"),
            "{text}"
        );
    }
    // One file, and `--filter`.
    let out = run_in(&["test", "tests/add.qx"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .ends_with("test result: ok. 2 passed; 0 failed\n"));
    let out = run_in(&["test", "tests", "--filter", "Fail"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.ends_with("test result: FAILED. 0 passed; 1 failed\n"),
        "{text}"
    );
    // A file that does not pass the checker fails as a whole.
    std::fs::write(
        dir.join("tests/broken.qx"),
        "void testX() {\n    Qubit q = qubit();\n}\n",
    )
    .unwrap();
    let out = run_in(&["test", "tests/broken.qx"]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("broken.qx ... FAILED") && text.contains("Q0302"),
        "{text}"
    );
    // Usage errors exit with 2.
    assert_eq!(run_in(&["test", "--nope"]).status.code(), Some(2));
}

// The typed IR interpreter (`--engine ir`, docs/design/qx-ir.md) prints exactly what the
// tree-walking interpreter prints, chosen by the flag, the environment variable, or both (`dual`).
#[test]
fn engine_flag_and_env_give_identical_results() {
    let error = tmp_qx(
        "engine_div_zero.qx",
        "void main() {\n    int z = 0;\n    println(\"{}\", 1 / z);\n}\n",
    );
    let mut files: Vec<String> = ["bell.qx", "teleport.qx", "functions.qx", "rotation.qx"]
        .iter()
        .map(|f| example(f))
        .collect();
    files.push(error);
    // Classical functions (native code under `jit`), one of them overflowing.
    files.push(format!(
        "{}/bench/qx/recursion.qx",
        env!("CARGO_MANIFEST_DIR")
    ));
    files.push(tmp_qx(
        "engine_overflow.qx",
        "int grow(int n) {\n    int x = 1;\n    for i in 0..n {\n        x = x * 3;\n    }\n    return x;\n}\n\nvoid main() {\n    println(\"{}\", grow(30));\n    println(\"{}\", grow(50));\n}\n",
    ));
    let repl = "Qubit q = qubit();\nh(q);\nBit m = measure(q);\nm\n:qasm\n:quit\n";
    let mut cases: Vec<(Vec<&str>, &str)> = vec![(vec!["repl", "--seed", "4"], repl)];
    for f in &files {
        cases.push((vec!["run", f, "--shots", "200", "--seed", "5"], ""));
        cases.push((vec!["run", f, "--seed", "9"], ""));
        cases.push((vec!["qasm", f], ""));
    }
    for (args, input) in cases {
        let with = |engine: &str| {
            let mut a = args.clone();
            a.extend(["--engine", engine]);
            qxide_engine_env(&a, None, input)
        };
        let ast = with("ast");
        assert!(!ast.stdout.is_empty() || !ast.stderr.is_empty(), "{args:?}");
        let mut others = vec![
            with("ir"),
            qxide_engine_env(&args, Some("ir"), input),
            qxide_engine_env(&args, Some("dual"), input),
        ];
        // Native code for classical functions (docs/design/qx-jit.md).
        if cfg!(feature = "jit") {
            others.push(with("jit"));
            others.push(qxide_engine_env(&args, Some("jit"), input));
        }
        for other in others {
            assert_eq!(other.status.code(), ast.status.code(), "{args:?}");
            assert_eq!(other.stdout, ast.stdout, "{args:?}");
            assert_eq!(other.stderr, ast.stderr, "{args:?}");
        }
    }
}

#[test]
fn engine_values_are_checked() {
    let bell = example("bell.qx");
    let bad = qxide_engine_env(&["run", &bell, "--engine", "fast"], None, "");
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8(bad.stderr).unwrap().contains("--engine"));
    let missing = qxide_engine_env(&["qasm", &bell, "--engine"], None, "");
    assert_eq!(missing.status.code(), Some(2));
    let bad_env = qxide_engine_env(&["run", &bell], Some("fast"), "");
    assert_eq!(bad_env.status.code(), Some(2));
    assert!(String::from_utf8(bad_env.stderr)
        .unwrap()
        .contains("QXIDE_ENGINE"));
    // The flag wins over the environment variable, which is then never read.
    let args = ["run", &bell, "--engine", "ir", "--seed", "1"];
    assert_eq!(
        qxide_engine_env(&args, Some("fast"), "").status.code(),
        Some(0)
    );
    // `check` builds no circuit, so it takes no engine.
    let check = qxide_engine_env(&["check", &bell, "--engine", "ir"], None, "");
    assert_eq!(check.status.code(), Some(2));
    let repl = qxide_engine_env(&["repl", "--engine", "fast"], None, "");
    assert_eq!(repl.status.code(), Some(2));
    // Without the `jit` feature, `jit` is refused with a note on how to get it.
    let jit = qxide_engine_env(&["run", &bell, "--engine", "jit", "--seed", "1"], None, "");
    if cfg!(feature = "jit") {
        assert_eq!(jit.status.code(), Some(0));
    } else {
        assert_eq!(jit.status.code(), Some(2));
        assert!(String::from_utf8(jit.stderr).unwrap().contains("jit"));
    }
}

// Writes an executable copy: on Linux and macOS a new file has no execute permission.
fn write_exe(path: &std::path::Path, data: &[u8]) {
    std::fs::write(path, data).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

// Runs a bundled executable from `cwd` with `input` on stdin.
fn run_exe(
    exe: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    input: &str,
) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run bundled program");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

// Stage 3a: `qxide build` writes one executable that runs the checked program with the options
// fixed at build time and every command-line argument as `main`'s `args`, without the sources.
#[test]
fn build_writes_an_executable_that_runs_without_the_sources() {
    let dir = fresh_dir("build_basic");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    let src = "import std.io;\n\nint main(String[] args) {\n    println(\"deferred {}\", len(args));\n    String text = io.readAll();\n    io.writeLine(\"read \" + text.trim());\n    for a in args {\n        io.writeLine(a);\n    }\n    return 3;\n}\n";
    let entry = dir.join("src/tool.qx");
    std::fs::write(&entry, src).unwrap();
    let entry_s = entry.to_str().unwrap();
    for engine in ["ast", "ir"] {
        let out_path = dir.join(format!("tool-{engine}"));
        let built = |o: &str| qxide(&["build", entry_s, "-o", o, "--engine", engine]);
        let out = built(out_path.to_str().unwrap());
        assert!(out.status.success(), "{out:?}");
        // `-o tool-ast` gets `.exe` on Windows.
        let exe = dir.join(exe_name(&format!("tool-{engine}")));
        assert!(exe.is_file(), "{}", exe.display());
        // Building again replaces the earlier output.
        assert!(built(exe.to_str().unwrap()).status.success());
        let moved = dir.join("elsewhere").join(exe_name(&format!("t-{engine}")));
        std::fs::rename(&exe, &moved).unwrap();
        std::fs::rename(&entry, dir.join("src/gone.qx")).unwrap();
        // `--` and flags that `qxide run` would take are ordinary arguments here.
        let out = run_exe(
            &moved,
            &dir.join("elsewhere"),
            &["--", "--shots", "x y"],
            "hi\n",
        );
        std::fs::rename(dir.join("src/gone.qx"), &entry).unwrap();
        assert_eq!(out.status.code(), Some(3), "{out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "read hi\n--\n--shots\nx y\ndeferred 3\n"
        );
    }
    // Without `-o` the output is named after the file, in the current directory.
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["build", "src/tool.qx"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(dir.join(exe_name("tool")).is_file());
}

#[test]
fn build_refuses_programs_with_errors_and_unsafe_outputs() {
    let dir = fresh_dir("build_errors");
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.qx");
    std::fs::write(&bad, "void main() {\n    Qubit q = qubit();\n}\n").unwrap();
    let out_path = dir.join(exe_name("bad"));
    let out = qxide(&[
        "build",
        bad.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("error[Q0302]"));
    assert!(!out_path.exists());
    // Never over a source file, a directory, or the input itself.
    let good = dir.join("good.qx");
    std::fs::write(&good, "void main() {\n    println(\"ok\");\n}\n").unwrap();
    let good_s = good.to_str().unwrap();
    for target in [bad.to_str().unwrap(), good_s, dir.to_str().unwrap()] {
        let out = qxide(&["build", good_s, "-o", target]);
        assert_eq!(out.status.code(), Some(1), "{target}");
    }
    assert!(std::fs::read_to_string(&bad).unwrap().contains("qubit()"));
    // Usage errors exit with 2.
    for args in [
        vec!["build", good_s, "--backend", "ibm:x"],
        vec!["build", good_s, "-o"],
        vec!["build", good_s, "--engine", "dual"],
    ] {
        assert_eq!(qxide(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn damaged_bundles_fail_cleanly() {
    let dir = fresh_dir("build_damaged");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("seeded.qx");
    let program =
        "void main() {\n    Qubit q = qubit();\n    h(q);\n    println(\"{}\", measure(q));\n}\n";
    std::fs::write(&src, program).unwrap();
    let exe = dir.join(exe_name("seeded"));
    let out = qxide(&[
        "build",
        src.to_str().unwrap(),
        "-o",
        exe.to_str().unwrap(),
        "--seed",
        "4",
        "--shots",
        "3",
    ]);
    assert!(out.status.success(), "{out:?}");
    // `--seed` and `--shots` are fixed at build time: the output of `qxide run` with them.
    let first = run_exe(&exe, &dir, &[], "");
    assert!(first.status.success(), "{first:?}");
    let run = qxide(&["run", src.to_str().unwrap(), "--seed", "4", "--shots", "3"]);
    assert_eq!(first.stdout, run.stdout);
    assert_eq!(run_exe(&exe, &dir, &[], "").stdout, first.stdout);
    let bytes = std::fs::read(&exe).unwrap();
    let n = bytes.len();
    // A changed payload byte, or a payload cut short under an intact trailer.
    let mut flipped = bytes.clone();
    flipped[n - 40] ^= 1;
    let mut cut = bytes[..n - 60].to_vec();
    cut.extend_from_slice(&bytes[n - 32..]);
    for (name, data) in [("flipped", flipped), ("cut", cut)] {
        let path = dir.join(exe_name(name));
        write_exe(&path, &data);
        let out = run_exe(&path, &dir, &[], "");
        assert_eq!(out.status.code(), Some(1), "{name}: {out:?}");
        assert!(out.stdout.is_empty());
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains("cannot be loaded"), "{name}: {err}");
    }
    // Without its trailer the file is a plain `qxide` again.
    let plain = dir.join(exe_name("plain"));
    write_exe(&plain, &bytes[..n - 1]);
    let out = run_exe(&plain, &dir, &["--version"], "");
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("qxide "));
}

// Stage 4a: `extern` functions need `--allow ffi`, and a build with the `ffi` feature.
#[test]
fn extern_functions_need_allow_ffi() {
    let dir = fresh_dir("ffi_flags");
    std::fs::create_dir_all(&dir).unwrap();
    let lib = if cfg!(windows) {
        "ucrtbase"
    } else if cfg!(target_os = "macos") {
        "libSystem.B.dylib"
    } else {
        "libm.so.6"
    };
    let src = format!(
        "extern \"{lib}\" double cos(double x);\n\nint main(String[] args) {{\n    println(\"{{}} {{}}\", cos(0.0), len(args));\n    return 4;\n}}\n"
    );
    let file = dir.join("native.qx");
    std::fs::write(&file, src).unwrap();
    let f = file.to_str().unwrap();
    // Refused before anything is loaded.
    let out = qxide(&["run", f]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    if !cfg!(feature = "ffi") {
        assert!(err.contains("error[Q0702]"), "{err}");
        let out = qxide(&["run", "--allow", "ffi", f]);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(String::from_utf8(out.stderr)
            .unwrap()
            .contains("`ffi` feature"));
        return;
    }
    assert!(
        err.contains("error[Q0703]") && err.contains("--allow ffi"),
        "{err}"
    );
    let out = qxide(&["check", f]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(qxide(&["check", "--allow", "ffi", f]).status.success());
    for engine in ["ast", "ir"] {
        let out = qxide(&["run", "--allow", "ffi", "--engine", engine, f, "--", "a"]);
        assert_eq!(out.status.code(), Some(4), "{out:?}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "1.0 1\n");
    }
    // A bundle keeps the permission it was built with.
    let exe = dir.join(exe_name("native"));
    let out = qxide(&["build", f, "-o", exe.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(!exe.exists());
    let out = qxide(&["build", f, "--allow", "ffi", "-o", exe.to_str().unwrap()]);
    assert!(out.status.success(), "{out:?}");
    let out = run_exe(&exe, &dir, &["x", "y"], "");
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1.0 2\n");
}

// D7: `--trusted` is a flag of `run` and `qasm` only; `check` and `build` reject it (a bundle
// never carries raised limits).
#[test]
fn trusted_flag_is_accepted_by_run_and_qasm_only() {
    let bell = example("bell.qx");
    for cmd in ["run", "qasm"] {
        let out = qxide(&[cmd, &bell, "--trusted"]);
        assert!(
            out.status.success(),
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for cmd in ["check", "build"] {
        let out = qxide(&[cmd, &bell, "--trusted"]);
        assert_eq!(out.status.code(), Some(2), "{cmd}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("unexpected argument `--trusted`"), "{err}");
    }
}

// Deprecated snake_case names still run: the warning goes to stderr, stdout and the exit code
// are those of the canonical name.
#[test]
fn deprecated_names_warn_on_stderr_only() {
    let old = tmp_qx(
        "old_names.qx",
        "void main() {\n    println(\"{}\", \"ab\".to_upper());\n}\n",
    );
    let new = tmp_qx(
        "new_names.qx",
        "void main() {\n    println(\"{}\", \"ab\".toUpperCase());\n}\n",
    );
    let (o, n) = (qxide(&["run", &old]), qxide(&["run", &new]));
    assert!(o.status.success() && n.status.success());
    assert_eq!(o.stdout, n.stdout);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("warning[Q0801]"), "{err}");
    assert!(
        n.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&n.stderr)
    );
    let checked = qxide(&["check", &old]);
    assert!(checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr).contains("warning[Q0801]"));
}

#[test]
fn doc_command_prints_markdown() {
    let dir = fresh_dir("doc_command");
    std::fs::create_dir_all(dir.join("src/util")).unwrap();
    std::fs::write(
        dir.join("qxide.toml"),
        "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.qx"),
        "void main() {\n    println(\"hi\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/util/ops.qx"),
        "/// Adds.\npublic int add(int a, int b) {\n    return a + b;\n}\n\nint hidden() {\n    return 0;\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("broken.qx"), "public int f( {\n").unwrap();
    let run_in = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let sep = std::path::MAIN_SEPARATOR;
    // The project's `src/` folder by default, files in path order.
    let out = run_in(&["doc"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("# `src{sep}main.qx`\n\n# `src{sep}util{sep}ops.qx`\n\n## `public int add(int a, int b)`\n\nAdds.\n")
    );
    let out = run_in(&["doc", "broken.qx"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr).unwrap().contains("error[Q01"));
    assert_eq!(run_in(&["doc", "--bogus"]).status.code(), Some(2));
}

#[test]
fn bench_command_times_bench_functions() {
    let dir = fresh_dir("bench_command");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("benches")).unwrap();
    std::fs::write(
        dir.join("qxide.toml"),
        "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.qx"),
        "void main() {\n    println(\"hi\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/ops.qx"),
        "public int sum(int n) {\n    int s = 0;\n    for i in 0..n {\n        s += i;\n    }\n    return s;\n}\n",
    )
    .unwrap();
    let benches = "import calc.ops.sum;\n\nvoid benchSum() {\n    int s = sum(100);\n}\n\nvoid benchBroken() {\n    int[] a = new int[2];\n    a[5] = 1;\n}\n\nvoid benchWith(int n) {}\n\nvoid helper() {}\n";
    std::fs::write(dir.join("benches/sum.qx"), benches).unwrap();
    let run_in = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let sep = std::path::MAIN_SEPARATOR;
    let mut steps = Vec::new();
    // The project's `benches/` folder by default; the step count is the same on every engine.
    for engine in ["ast", "ir"] {
        let out = run_in(&["bench", "--engine", engine, "--iterations", "3"]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        let line = text
            .lines()
            .find(|l| l.starts_with(&format!("bench benches{sep}sum.qx::benchSum ... ")))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(line.contains(" ms/iter (min "), "{line}");
        assert!(line.contains(" ms, 3 runs, "), "{line}");
        steps.push(
            line.rsplit(", ")
                .next()
                .unwrap()
                .trim_end_matches(" steps)")
                .to_string(),
        );
        assert!(
            text.contains(&format!("bench benches{sep}sum.qx::benchBroken ... FAILED")),
            "{text}"
        );
        assert!(text.contains("error[Q0403]"), "{text}");
        assert!(
            !text.contains("benchWith") && !text.contains("helper"),
            "{text}"
        );
        assert!(
            text.ends_with("bench result: FAILED. 1 passed; 1 failed\n"),
            "{text}"
        );
    }
    assert_eq!(steps[0], steps[1]);
    assert!(steps[0].parse::<u64>().unwrap() > 100, "{steps:?}");
    let out = run_in(&[
        "bench",
        "benches/sum.qx",
        "--filter",
        "Sum",
        "--iterations",
        "1",
    ]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(", 1 runs, "), "{text}");
    assert!(
        text.ends_with("bench result: ok. 1 passed; 0 failed\n"),
        "{text}"
    );
    assert_eq!(
        run_in(&["bench", "--iterations", "0"]).status.code(),
        Some(2)
    );
}

// `examples/cli/progress.qx` redraws one line with `\r` and an ANSI erase sequence; `std.io`
// output reaches stdout while the program runs (roadmap A3).
#[test]
fn progress_example_redraws_a_line() {
    let progress = format!("{}/examples/cli/progress.qx", env!("CARGO_MANIFEST_DIR"));
    let frame = |pct: &str, filled: usize| {
        format!(
            "\r\u{1b}[K[{}{}] {pct}%",
            "#".repeat(filled),
            ".".repeat(30 - filled)
        )
    };
    let want = format!("{}{}{}\n", frame("0", 0), frame("50", 15), frame("100", 30));
    let mut engines = vec!["ast", "ir"];
    if cfg!(feature = "jit") {
        engines.push("jit");
    }
    for engine in engines {
        let out = qxide_io(
            &["run", &progress, "--engine", engine, "--", "2", "0"],
            &[],
            "",
        );
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), want, "{engine}");
    }
    let out = qxide_io(&["run", &progress, "--", "0"], &[], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("progress.qx"));
}

#[test]
fn experimental_flags_are_named_and_kept_from_bundles() {
    let unknown = qxide_io(&["run", "--experimental", "nope", "x.qx"], &[], "");
    assert_eq!(unknown.status.code(), Some(2));
    let err = String::from_utf8(unknown.stderr).unwrap();
    assert!(err.contains("unknown experimental feature `nope`"), "{err}");
    let file = format!("{}/bench/compare/qx/matmul.qx", env!("CARGO_MANIFEST_DIR"));
    let build = qxide_io(
        &["build", &file, "--experimental", "value-semantics"],
        &[],
        "",
    );
    assert_eq!(build.status.code(), Some(2));
    let err = String::from_utf8(build.stderr).unwrap();
    assert!(err.contains("does not support"), "{err}");
}

// `qxide fix` refuses without `--experimental value-semantics`, and with it rewrites the file in
// place (`--dry-run` only lists the fixes).
#[test]
fn fix_applies_value_semantics_fixes_in_place() {
    let src = "void push(int[] a) {\n    a[0] = 1;\n}\n\nvoid main() {\n    int[] a = {0};\n    push(a);\n    println(\"{}\", a[0]);\n}\n";
    let path = scratch_file("fix_push.qx", src);
    let p = path.to_str().unwrap();
    let off = qxide(&["fix", p]);
    assert_eq!(off.status.code(), Some(2));
    let dry = qxide(&["fix", p, "--dry-run", "--experimental", "value-semantics"]);
    assert_eq!(dry.status.code(), Some(0));
    assert!(String::from_utf8(dry.stdout)
        .unwrap()
        .contains("would apply"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), src);
    let out = qxide(&["fix", p, "--experimental", "value-semantics"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fixed = std::fs::read_to_string(&path).unwrap();
    assert!(
        fixed.contains("void push(inout int[] a)") && fixed.contains("push(inout a)"),
        "{fixed}"
    );
    let run = qxide(&["run", p, "--experimental", "value-semantics"]);
    assert_eq!(String::from_utf8(run.stdout).unwrap(), "1\n");
}

// `run --no-budget` runs past the step limit that stops the same program without it.
#[test]
fn no_budget_runs_past_the_step_limit() {
    // The loop is in a function: `main` itself stays in the interpreter.
    let src = "int count(int n) {\n    int i = 0;\n    while i < n {\n        i += 1;\n    }\n    return i;\n}\n\nvoid main() {\n    println(\"{}\", count(200000000));\n}\n";
    // Without native code the interpreters would take minutes over these steps.
    if !cfg!(feature = "jit") {
        return;
    }
    let path = scratch_file("no_budget.qx", src);
    let p = path.to_str().unwrap();
    let limited = qxide(&["run", p, "--engine", "jit"]);
    assert_eq!(limited.status.code(), Some(1));
    assert!(String::from_utf8(limited.stderr).unwrap().contains("Q0413"));
    let free = qxide(&["run", p, "--no-budget", "--engine", "jit"]);
    assert_eq!(String::from_utf8(free.stdout).unwrap(), "200000000\n");
}

// The Rust comparison programs (bench/compare/qx) at small sizes: every engine prints the result
// the Rust versions (bench/compare/rust) print for the same input.
#[test]
fn rust_comparison_programs_match_the_rust_results() {
    let root = env!("CARGO_MANIFEST_DIR");
    let cases = [
        ("nbody", "200", "-0.16902690858133132"),
        ("binarytrees", "6", "4398"),
        ("matmul", "20", "-33250.0"),
        ("strings", "300", "940"),
    ];
    let mut engines = vec!["ast", "ir"];
    if cfg!(feature = "jit") {
        engines.push("jit");
    }
    // `qx-values` holds the same programs for `--experimental value-semantics`.
    for (dir, flags) in [
        ("qx", &[][..]),
        ("qx-values", &["--experimental", "value-semantics"][..]),
    ] {
        for (name, n, want) in cases {
            let file = format!("{root}/bench/compare/{dir}/{name}.qx");
            for engine in &engines {
                let mut args = vec!["run", &file, "--engine", engine];
                args.extend_from_slice(flags);
                args.extend(["--", n]);
                let out = qxide_io(&args, &[], "");
                assert_eq!(out.status.code(), Some(0), "{dir}/{name} {engine}: {out:?}");
                assert!(out.stderr.is_empty(), "{dir}/{name} {engine}: {out:?}");
                let text = String::from_utf8(out.stdout).unwrap();
                let result = text
                    .strip_prefix("result=")
                    .and_then(|r| r.split(' ').next())
                    .unwrap_or_default();
                assert_eq!(result, want, "{dir}/{name} {engine}: {text}");
            }
        }
    }
}
