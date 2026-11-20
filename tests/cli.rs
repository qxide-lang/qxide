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
    for line in text.lines().filter(|l| !l.starts_with("==")) {
        assert!(line == "a=0 b=0" || line == "a=1 b=1", "{text}");
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
        "fn main() {\r\n    let q = qubit();\r\n    let m = measure(q);\r\n    h(q);\r\n}\r\n",
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
        "fn main() {\n    let r = qreg(2);\n    h(r[5]);\n    discard(r);\n}\n",
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
        "fn f(n: i64) -> i64 {\n    if n > 0 - 1 {\n        return f(n + 1);\n    }\n    n\n}\nfn main() {\n    println(\"{}\", f(0));\n}\n",
    );
    let out = qxide(&["run", &path]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("error[Q0405]"));
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
