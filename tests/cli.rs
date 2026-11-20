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
    let main = dir.join("main.qx");
    let main_arg = main.to_string_lossy().into_owned();
    assert!(String::from_utf8(out.stdout).unwrap().contains(&main_arg));
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
    assert!(readme.starts_with("# new_project_demo\n"), "{readme}");
    assert!(qxide(&["check", &main_arg]).status.success());
    assert!(qxide(&["fmt", "--check", &main_arg]).status.success());
    let run = qxide(&["run", &main_arg, "--shots", "50"]);
    assert!(run.status.success());
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
