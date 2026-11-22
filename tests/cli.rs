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
    let src = "import std.io;\nimport std.env;\n\nint main(String[] args) {\n    println(\"deferred\");\n    io.write_line(\"args \" + len(args) + \" \" + args[1]);\n    String text = io.read_all();\n    io.write_line(\"read \" + text.trim());\n    io.write_line(env.get(\"QX_CLI_TEST\").unwrap_or(\"unset\"));\n    return 7;\n}\n";
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
    // The word-count example reads stdin and fails with exit code 2 on an unknown option.
    let wc = format!("{}/examples/cli/wc.qx", env!("CARGO_MANIFEST_DIR"));
    let out = qxide_io(&["run", &wc], &[], "one two\n\nthree\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "3 3 15\n");
    let out = qxide_io(&["run", &wc, "--", "-w"], &[], "one two\n");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "2\n");
    // `-f`: word counts, highest first, ties in first-seen order.
    let out = qxide_io(&["run", &wc, "--", "-f"], &[], "b a b\n c  a b\n");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "2 6 14\n3 b\n2 a\n1 c\n"
    );
    let out = qxide_io(&["run", &wc, "--", "-q"], &[], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("-q"));
    // Out-of-range exit codes are a run-time error (exit code 1).
    let bad = tmp_qx("exit_256.qx", "int main() {\n    return 256;\n}\n");
    let out = qxide_io(&["run", &bad], &[], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("Q0706"));
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
