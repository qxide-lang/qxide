// End-to-end package workflow through the `qxide` binary with a local file registry.
// Every command gets its own QXIDE_HOME and QXIDE_REGISTRY, so nothing touches the network,
// `gh`, or the real `~/.qxide`. Assertions use exit codes and error codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Env {
    root: PathBuf,
    home: PathBuf,
    registry: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("pkg-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("registry")).unwrap();
        Env {
            home: root.join("home"),
            registry: root.join("registry"),
            root,
        }
    }

    fn qx(&self, dir: &Path, args: &[&str]) -> Output {
        self.qx_home(dir, &self.home, args)
    }

    fn qx_home(&self, dir: &Path, home: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_qxide"))
            .args(args)
            .current_dir(dir)
            .env("QXIDE_HOME", home)
            .env("QXIDE_REGISTRY", &self.registry)
            .env_remove("QXIDE_OFFLINE")
            .output()
            .expect("run qxide")
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.qx(dir, args);
        assert!(
            out.status.success(),
            "qxide {args:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Exit code 1 and the given error code on stderr.
    fn fails(&self, dir: &Path, args: &[&str], code: &str) -> String {
        let out = self.qx(dir, args);
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(1), "qxide {args:?}:\n{err}");
        assert!(
            err.contains(&format!("error[{code}]")),
            "expected {code}:\n{err}"
        );
        err
    }

    /// Copies `examples/packages/<name>` here, optionally with another version.
    fn package(&self, name: &str, version: Option<&str>) -> PathBuf {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/packages")
            .join(name);
        let dest = self
            .root
            .join(format!("{name}-{}", version.unwrap_or("src")));
        copy_dir(&src, &dest);
        let _ = std::fs::remove_dir_all(dest.join("target"));
        if let Some(v) = version {
            let m = std::fs::read_to_string(dest.join("qxide.toml")).unwrap();
            let m = m.replacen("version = \"0.1.0\"", &format!("version = \"{v}\""), 1);
            std::fs::write(dest.join("qxide.toml"), m).unwrap();
        }
        dest
    }

    /// A project with `src/main.qx` and no dependencies yet.
    fn app(&self, name: &str, main: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("qxide.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nqxide = \">=0.6\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/main.qx"), main).unwrap();
        dir
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}

const BELL_MAIN: &str = include_str!("../examples/packages/bell-app/src/main.qx");

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn publish_add_install_run() {
    let env = Env::new("flow");
    let lib = env.package("bell-utils", None);
    // A dry run builds the archive but leaves the registry untouched.
    let dry = env.ok(&lib, &["publish", "--dry-run"]);
    assert!(dry.contains("\"name\":\"bell-utils\""), "{dry}");
    assert!(!env.registry.join("index").exists());
    env.ok(&lib, &["publish"]);
    let index = read(&env.registry.join("index/be/ll/bell-utils"));
    assert_eq!(index.lines().count(), 1);
    assert!(env
        .registry
        .join("packages/be/ll/bell-utils/bell-utils-0.1.0.tar.gz")
        .is_file());
    // Publishing the same version again is rejected.
    env.fails(&lib, &["publish"], "Q0614");

    let search = env.ok(&env.root, &["search", "bell"]);
    assert!(search.contains("bell-utils = \"0.1.0\""), "{search}");

    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils"]);
    assert!(read(&app.join("qxide.toml")).contains("bell-utils = \"0.1.0\""));
    let lock = read(&app.join("qxide.lock"));
    assert!(lock.contains("checksum = \"sha256:"), "{lock}");
    // `run` without a file uses src/main.qx; the imported Bell pair is always correlated.
    let out = env.ok(&app, &["run", "--shots", "200"]);
    let rows: Vec<&str> = out.lines().filter(|l| l.starts_with('[')).collect();
    assert!(!rows.is_empty());
    for r in rows {
        assert!(r.starts_with("[0, 0") || r.starts_with("[1, 1"), "{out}");
    }
    env.ok(&app, &["check"]);
    let qasm = env.ok(&app, &["qasm", "src/main.qx"]);
    assert!(qasm.contains("cx q[0], q[1];"), "{qasm}");

    env.ok(&app, &["remove", "bell-utils"]);
    assert!(!read(&app.join("qxide.lock")).contains("bell-utils"));
    env.fails(&app, &["remove", "bell-utils"], "Q0617");
    // The import no longer resolves.
    env.fails(&app, &["check"], "Q0501");
}

#[test]
fn lock_file_is_reproducible() {
    let env = Env::new("lock");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils@0.1"]);
    let first = read(&app.join("qxide.lock"));
    // A newer compatible release does not move an existing lock.
    let newer = env.package("bell-utils", Some("0.1.1"));
    env.ok(&newer, &["publish"]);
    env.ok(&app, &["install"]);
    assert_eq!(read(&app.join("qxide.lock")), first);
    // A fresh cache installs exactly the locked version, and `--locked` accepts the lock.
    let other_home = env.root.join("home2");
    let out = env.qx_home(&app, &other_home, &["install", "--locked"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(other_home
        .join("registry")
        .read_dir()
        .is_ok_and(|mut d| d.next().is_some()));
    assert_eq!(read(&app.join("qxide.lock")), first);
    // Removing the lock and resolving again picks the newest version.
    std::fs::remove_file(app.join("qxide.lock")).unwrap();
    env.ok(&app, &["install"]);
    assert!(read(&app.join("qxide.lock")).contains("version = \"0.1.1\""));
}

#[test]
fn lock_must_match_the_manifest() {
    let env = Env::new("stale");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    // Dependency declared by hand, nothing installed yet.
    let m = read(&app.join("qxide.toml")) + "\n[dependencies]\nbell-utils = \"0.1\"\n";
    std::fs::write(app.join("qxide.toml"), &m).unwrap();
    env.fails(&app, &["run"], "Q0611");
    env.fails(&app, &["install", "--locked"], "Q0612");
    env.ok(&app, &["install"]);
    env.ok(&app, &["run"]);
    // Editing the requirement makes the lock stale for run and for `--locked`.
    std::fs::write(app.join("qxide.toml"), m.replace("\"0.1\"", "\"0.2\"")).unwrap();
    env.fails(&app, &["run"], "Q0612");
    env.fails(&app, &["install", "--locked"], "Q0612");
    env.fails(&app, &["install"], "Q0605");
    // An unpacked package removed from the cache must be installed again.
    std::fs::write(app.join("qxide.toml"), &m).unwrap();
    std::fs::remove_dir_all(env.home.join("registry/src")).unwrap();
    env.fails(&app, &["run"], "Q0611");
    env.ok(&app, &["install"]);
    env.ok(&app, &["run"]);
}

#[test]
fn checksums_are_enforced() {
    let env = Env::new("cksum");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils"]);
    let archive = env
        .registry
        .join("packages/be/ll/bell-utils/bell-utils-0.1.0.tar.gz");
    // A tampered archive: the download no longer matches the lock.
    let mut bytes = std::fs::read(&archive).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&archive, &bytes).unwrap();
    let fresh = env.root.join("home-tampered");
    let out = env.qx_home(&app, &fresh, &["install"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("error[Q0607]"), "{err}");
    assert!(
        !fresh.join("registry/src").exists(),
        "nothing unpacked:\n{err}"
    );
    // A changed index checksum is refused before anything is downloaded.
    let index = env.registry.join("index/be/ll/bell-utils");
    let line = read(&index);
    let cksum = line.split("\"cksum\":\"").nth(1).unwrap()[..64].to_string();
    std::fs::write(&index, line.replace(&cksum, &"0".repeat(64))).unwrap();
    let out = env.qx_home(&app, &env.root.join("home-index"), &["install"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("error[Q0607]"));
}

#[test]
fn missing_packages_and_versions() {
    let env = Env::new("missing");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    env.fails(&app, &["add", "nope"], "Q0604");
    env.fails(&app, &["add", "bell-utils@9"], "Q0605");
    // Failed adds leave the manifest alone.
    assert!(!read(&app.join("qxide.toml")).contains("[dependencies]"));
    // A yanked release is skipped by new resolutions.
    let newer = env.package("bell-utils", Some("0.1.1"));
    env.ok(&newer, &["publish"]);
    let index = env.registry.join("index/be/ll/bell-utils");
    let text = read(&index);
    let yanked: Vec<String> = text
        .lines()
        .map(|l| {
            if l.contains("\"vers\":\"0.1.1\"") {
                l.replace("\"yanked\":false", "\"yanked\":true")
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(&index, yanked.join("\n") + "\n").unwrap();
    env.ok(&app, &["add", "bell-utils"]);
    assert!(read(&app.join("qxide.lock")).contains("version = \"0.1.0\""));
    // Offline with an empty cache cannot reach an https registry.
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["search", "bell", "--offline"])
        .current_dir(&app)
        .env("QXIDE_HOME", env.root.join("home-offline"))
        .env("QXIDE_REGISTRY", "https://registry.invalid/")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("error[Q0615]"));
    // Usage errors exit with 2.
    assert_eq!(env.qx(&app, &["add"]).status.code(), Some(2));
    assert_eq!(env.qx(&app, &["install", "--bogus"]).status.code(), Some(2));
}

#[test]
fn import_errors_inside_a_project() {
    let env = Env::new("imports");
    let app = env.app("demo", "import demo.a.f;\nvoid main() {\n    f();\n}\n");
    std::fs::write(
        app.join("src/a.qx"),
        "import demo.b.g;\npublic void f() {\n    g();\n}\n",
    )
    .unwrap();
    std::fs::write(
        app.join("src/b.qx"),
        "import demo.a.f;\npublic void g() {}\n",
    )
    .unwrap();
    let err = env.fails(&app, &["check"], "Q0505");
    assert!(err.contains("a.qx") && err.contains("b.qx"), "{err}");
    // A private function of a project module.
    std::fs::write(app.join("src/b.qx"), "void g() {}\n").unwrap();
    env.fails(&app, &["run"], "Q0504");
    std::fs::write(app.join("src/b.qx"), "public void g() {}\n").unwrap();
    env.ok(&app, &["run"]);
    // A lone file outside any project cannot import.
    let lone = env.root.join("lone.qx");
    std::fs::write(&lone, "import x.y;\nvoid main() {}\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["check", lone.to_str().unwrap()])
        .current_dir(&env.root)
        .env("QXIDE_HOME", &env.home)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("error[Q0508]"));
}

#[test]
fn path_dependency_example_runs_from_a_checkout() {
    let env = Env::new("example");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packages/bell-app");
    // The committed lock matches the manifest, so no install is needed.
    let out = env.ok(&root, &["run", "--shots", "50"]);
    assert!(out.contains("a=0 b=0") || out.contains("a=1 b=1"), "{out}");
    env.ok(&root, &["check"]);
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packages/bell-utils");
    env.ok(&lib, &["check", "src/lib.qx"]);
}
