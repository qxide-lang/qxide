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
