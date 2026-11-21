// `qxide app new` and `qxide app build` against a fake runtime; the window itself is exercised
// by the runtime crate.

use std::path::PathBuf;

use super::*;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qxide-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(v: &str) -> String {
    v.to_string()
}

#[test]
fn new_then_build_attaches_a_bundle_to_the_runtime() {
    let base = temp("build");
    let dir = base.join("bell-lab");
    assert!(new(&[dir.display().to_string()]).is_ok());
    for f in [
        "qxide.toml",
        "src/app.qx",
        "ui/index.html",
        "ui/app.js",
        "ui/style.css",
        "README.md",
        ".gitignore",
    ] {
        assert!(dir.join(f).is_file(), "{f}");
    }
    let html = std::fs::read_to_string(dir.join("ui/index.html")).unwrap();
    assert!(html.contains("<title>bell-lab</title>"), "{html}");
    // The generated project passes the same checks as the runtime.
    let app = load(&dir).ok().expect("template loads");
    assert_eq!(app.config.commands, ["measure_bell", "save_result"]);
    assert!(validate(&app).is_ok());

    let runtime = base.join("fake-runtime.exe");
    std::fs::write(&runtime, b"MZ not really a runtime").unwrap();
    std::env::set_var("QXIDE_APP_RUNTIME", &runtime);
    let built = build(&[dir.display().to_string()]);
    std::env::remove_var("QXIDE_APP_RUNTIME");
    assert!(built.is_ok());
    let out = dir
        .join("target/app")
        .join(format!("bell-lab{}", std::env::consts::EXE_SUFFIX));
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"MZ not really a runtime"));
    let files = bundle::read(&bytes).unwrap();
    let keys: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "app.json",
            "pkg/0/bell_lab/src/app.qx",
            "ui/app.js",
            "ui/index.html",
            "ui/style.css"
        ]
    );
    let m: serde_json::Value = serde_json::from_slice(&files["app.json"]).unwrap();
    assert_eq!(m["entry"], "pkg/0/bell_lab/src/app.qx");
    assert_eq!(m["permissions"], serde_json::json!(["log", "time", "fs"]));
    assert_eq!(m["packages"][0]["src"], "pkg/0/bell_lab/src");
    assert_eq!(m["dev"], false);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn load_reports_bad_app_sections_and_commands() {
    let base = temp("bad");
    let dir = base.join("demo");
    assert!(new(&[dir.display().to_string()]).is_ok());
    let toml = dir.join("qxide.toml");
    let good = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(
        &toml,
        good.replace(
            "commands = [\"measure_bell\", \"save_result\"]",
            "commands = []",
        ),
    )
    .unwrap();
    let Err(f) = load(&dir) else {
        panic!("empty commands accepted")
    };
    assert!(f.text.starts_with("error[Q0715]: "), "{}", f.text);

    std::fs::write(
        &toml,
        good.replace("\"save_result\"]", "\"save_result\", \"nope\"]"),
    )
    .unwrap();
    let app = load(&dir).ok().unwrap();
    let Err(f) = validate(&app) else {
        panic!("missing command accepted")
    };
    assert!(f.text.starts_with("error[Q0714]: "), "{}", f.text);

    std::fs::write(
        &toml,
        good.replace(
            "permissions = [\"log\", \"time\", \"fs\"]",
            "permissions = [\"log\"]",
        ),
    )
    .unwrap();
    let app = load(&dir).ok().unwrap();
    let Err(f) = validate(&app) else {
        panic!("std.fs without permission accepted")
    };
    assert!(f.text.contains("error[Q0703]: "), "{}", f.text);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn usage_errors() {
    assert!(new(&[]).is_err());
    assert!(new(&[s("a"), s("b")]).is_err());
    let Err(f) = project_dir(&[s("--x")], "app dev") else {
        panic!()
    };
    assert_eq!(f.code, 2);
}
