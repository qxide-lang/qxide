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

    /// Creates a library package directory containing an empty public function in `src/lib.qx` and
    /// the given `[dependencies]` lines.
    fn lib(&self, name: &str, version: &str, deps: &str) -> PathBuf {
        let dir = self.root.join(format!("{name}-{version}"));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("qxide.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"d\"\n\
                 license = \"MIT\"\nqxide = \">=0.6\"\n\n[dependencies]\n{deps}"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.qx"), "public void noop() {}\n").unwrap();
        dir
    }

    /// Rewrites `qxide.lock` in `dir` using closure `f`.
    fn edit_lock(&self, dir: &Path, f: impl FnOnce(String) -> String) {
        let path = dir.join("qxide.lock");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, f(text)).unwrap();
    }
}

const PLAIN_MAIN: &str = "void main() {\n    println(\"ok\");\n}\n";

// The `[[package]]` block of `name` in a lock file.
fn lock_block<'a>(lock: &'a str, name: &str) -> &'a str {
    let start = lock.find(&format!("name = \"{name}\"\n")).unwrap();
    let end = lock[start..].find("\n\n").map_or(lock.len(), |e| start + e);
    &lock[start..end]
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
fn transitive_requirements_keep_the_lock_honest() {
    let env = Env::new("transitive");
    for v in [None, Some("0.1.1"), Some("0.2.0")] {
        let lib = env.package("bell-utils", v);
        env.ok(&lib, &["publish"]);
    }
    let index = read(&env.registry.join("index/be/ll/bell-utils"));
    let cksum = |v: &str| {
        let line = index
            .lines()
            .find(|l| l.contains(&format!("\"vers\":\"{v}\"")))
            .unwrap();
        line.split("\"cksum\":\"").nth(1).unwrap()[..64].to_string()
    };
    // A registry package that needs bell-utils ^0.1.1.
    let top = env.lib("top", "1.0.0", "bell-utils = \"^0.1.1\"\n");
    env.ok(&top, &["install"]);
    env.ok(&top, &["publish"]);
    let app = env.app("demo", PLAIN_MAIN);
    env.ok(&app, &["add", "top"]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock_block(&lock, "bell-utils").contains("version = \"0.1.1\""));
    // Hand-edited to 0.1.0 with its real checksum, which breaks top's requirement.
    let (old, new) = (cksum("0.1.1"), cksum("0.1.0"));
    env.edit_lock(&app, |t| {
        t.replace("version = \"0.1.1\"", "version = \"0.1.0\"")
            .replace(&old, &new)
    });
    env.fails(&app, &["install", "--locked"], "Q0612");
    env.fails(&app, &["run"], "Q0612");
    // With an empty cache the requirements come from the index, before any download.
    let out = env.qx_home(&app, &env.root.join("home2"), &["install", "--locked"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("error[Q0612]"), "{err}");
    env.ok(&app, &["install"]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock_block(&lock, "bell-utils").contains("version = \"0.1.1\""));
    env.ok(&app, &["run"]);

    // A path package whose own requirement changed after locking.
    let mid = env.lib("mid", "0.1.0", "bell-utils = \"^0.1\"\n");
    let app = env.app("demo2", PLAIN_MAIN);
    let m = read(&app.join("qxide.toml")) + "\n[dependencies]\nmid = { path = \"../mid-0.1.0\" }\n";
    std::fs::write(app.join("qxide.toml"), m).unwrap();
    env.ok(&app, &["install"]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock_block(&lock, "bell-utils").contains("version = \"0.1.1\""));
    let manifest = read(&mid.join("qxide.toml")).replace("\"^0.1\"", "\"^0.2\"");
    std::fs::write(mid.join("qxide.toml"), manifest).unwrap();
    env.fails(&app, &["install", "--locked"], "Q0612");
    env.fails(&app, &["run"], "Q0612");
    env.ok(&app, &["install"]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock_block(&lock, "bell-utils").contains("version = \"0.2.0\""));
    env.ok(&app, &["run"]);
}

#[test]
fn lock_entries_cannot_escape_the_cache() {
    let env = Env::new("lockname");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils"]);
    let lock = read(&app.join("qxide.lock"));
    let block = lock_block(&lock, "bell-utils").to_string();
    let with_copy = |name: &str| {
        let copy = block.replace("name = \"bell-utils\"", &format!("name = \"{name}\""));
        format!("{lock}\n[[package]]\n{copy}\n")
    };
    // Lock names become cache paths: a traversal is refused as soon as the lock is read.
    env.edit_lock(&app, |_| with_copy("../../../../outside/x"));
    env.fails(&app, &["install", "--locked"], "Q0603");
    env.fails(&app, &["run"], "Q0603");
    assert!(!env.root.join("outside").exists());
    // A locked package nothing depends on is stale, not fetched.
    env.edit_lock(&app, |_| with_copy("zz-unused"));
    env.fails(&app, &["install", "--locked"], "Q0612");
    env.fails(&app, &["run"], "Q0612");
    env.ok(&app, &["install"]);
    assert_eq!(read(&app.join("qxide.lock")), lock);
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
fn build_metadata_versions_are_never_published_or_selected() {
    let env = Env::new("buildmeta");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let evil = env.package("bell-utils", Some("0.1.0+evil"));
    env.fails(&evil, &["publish"], "Q0614");
    // A look-alike line appended by hand, with another checksum.
    let index = env.registry.join("index/be/ll/bell-utils");
    let original = read(&index);
    let cksum = original.split("\"cksum\":\"").nth(1).unwrap()[..64].to_string();
    let fake = "f".repeat(64);
    let line = original
        .trim_end()
        .replace(&cksum, &fake)
        .replace("\"vers\":\"0.1.0\"", "\"vers\":\"0.1.0+evil\"");
    std::fs::write(&index, format!("{original}{line}\n")).unwrap();
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils@=0.1.0"]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock.contains(&format!("sha256:{cksum}")), "{lock}");
    assert!(!lock.contains("+evil"), "{lock}");
    env.ok(&app, &["run"]);
}

// Compiles `tests/support/fake_curl.rs` with `rustc` into a `curl` binary next to a copy of `qxide`
// and returns the copy. Tests that use it place this directory first on `PATH`, where `qxide`
// looks for `curl` (it no longer runs one just because it is next to the executable, B-P12).
fn fake_curl_qxide(env: &Env) -> PathBuf {
    let bin = env.root.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = std::env::consts::EXE_SUFFIX;
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_curl.rs");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let out = Command::new(rustc)
        .args(["--edition", "2021", "-o"])
        .arg(bin.join(format!("curl{exe}")))
        .arg(&src)
        .output()
        .expect("run rustc");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let qxide = bin.join(format!("qxide{exe}"));
    std::fs::copy(env!("CARGO_BIN_EXE_qxide"), &qxide).unwrap();
    qxide
}

#[test]
fn failed_downloads_are_never_accepted_or_cached() {
    let env = Env::new("curl");
    for v in [None, Some("0.1.1")] {
        let lib = env.package("bell-utils", v);
        env.ok(&lib, &["publish"]);
    }
    let qxide = fake_curl_qxide(&env);
    let app = env.app("demo", BELL_MAIN);
    let m = read(&app.join("qxide.toml")) + "\n[dependencies]\nbell-utils = \"0.1\"\n";
    std::fs::write(app.join("qxide.toml"), m).unwrap();
    let log = env.root.join("curl.log");
    let path = std::env::join_paths(
        std::iter::once(qxide.parent().unwrap().to_path_buf()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap();
    let run = |home: &Path, trunc: &str| {
        Command::new(&qxide)
            .arg("install")
            .current_dir(&app)
            .env("PATH", &path)
            .env("QXIDE_HOME", home)
            .env("QXIDE_REGISTRY", "https://fake.invalid/")
            .env_remove("QXIDE_OFFLINE")
            .env("FAKE_ROOT", &env.registry)
            .env("FAKE_LOG", &log)
            .env("FAKE_TRUNC", trunc)
            .output()
            .unwrap()
    };
    // curl exits 18 after a truncated body but still prints http_code 200.
    for (trunc, home) in [("index/", "home-index"), ("packages/", "home-archive")] {
        let home = env.root.join(home);
        let out = run(&home, trunc);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{trunc}: {err}");
        assert!(err.contains("error[Q0608]"), "{trunc}: {err}");
        assert!(!home.join("registry/src").exists(), "{trunc}: {err}");
        let cached_index = std::fs::read_dir(home.join("registry/index"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|d| d.path().join("index/be/ll/bell-utils").exists());
        assert_eq!(cached_index, trunc != "index/", "{trunc}");
    }
    // The stand-in really ran: same requests, complete bodies, install succeeds.
    let home = env.root.join("home-ok");
    let out = run(&home, "");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_dir(home.join("registry/tmp"))
            .unwrap()
            .count(),
        0,
        "downloads are removed"
    );
    let calls = read(&log);
    let args: Vec<&str> = calls.lines().collect();
    assert!(args.contains(&"--globoff"), "{calls}");
    // Downloads go to a file under QXIDE_HOME, never to a shared temporary directory.
    let outputs: Vec<&str> = args
        .windows(2)
        .filter(|w| w[0] == "--output")
        .map(|w| w[1])
        .collect();
    assert!(outputs.len() >= 4, "{calls}");
    for o in outputs {
        assert!(
            Path::new(o).starts_with(env.root.join("home-index"))
                || Path::new(o).starts_with(env.root.join("home-archive"))
                || Path::new(o).starts_with(&home),
            "{o}"
        );
    }
}

// B-P12: `Command::new("curl")` on Windows runs a `curl.exe` next to `qxide.exe` before looking at
// `PATH`, so a writable install directory could supply one. External tools now come from `PATH`
// only: with no `curl` on `PATH`, the planted one must not run.
#[cfg(windows)]
#[test]
fn tools_next_to_the_executable_are_not_run() {
    let env = Env::new("plantedcurl");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let qxide = fake_curl_qxide(&env);
    let app = env.app("demo", BELL_MAIN);
    let m = read(&app.join("qxide.toml")) + "\n[dependencies]\nbell-utils = \"0.1\"\n";
    std::fs::write(app.join("qxide.toml"), m).unwrap();
    let empty = env.root.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let log = env.root.join("curl.log");
    let out = Command::new(&qxide)
        .arg("install")
        .current_dir(&app)
        .env("PATH", &empty)
        .env("QXIDE_HOME", env.root.join("home-planted"))
        .env("QXIDE_REGISTRY", "https://fake.invalid/")
        .env_remove("QXIDE_OFFLINE")
        .env("FAKE_ROOT", &env.registry)
        .env("FAKE_LOG", &log)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!log.exists(), "the planted curl ran: {err}");
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("error[Q0608]"), "{err}");
}

#[test]
fn failed_add_restores_manifest_and_lock() {
    let env = Env::new("rollback");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils@0.1"]);
    let (manifest, lock) = (read(&app.join("qxide.toml")), read(&app.join("qxide.lock")));
    // 0.2.0 resolves, so the manifest and lock get rewritten, but its archive is corrupt.
    let newer = env.package("bell-utils", Some("0.2.0"));
    env.ok(&newer, &["publish"]);
    let archive = env
        .registry
        .join("packages/be/ll/bell-utils/bell-utils-0.2.0.tar.gz");
    let mut bytes = std::fs::read(&archive).unwrap();
    bytes[10] ^= 0xff;
    std::fs::write(&archive, bytes).unwrap();
    env.fails(&app, &["add", "bell-utils@0.2"], "Q0607");
    assert_eq!(read(&app.join("qxide.toml")), manifest);
    assert_eq!(read(&app.join("qxide.lock")), lock);
    env.ok(&app, &["run"]);
    // Without an earlier lock, a failed add leaves no lock behind.
    let fresh = env.app("fresh", BELL_MAIN);
    let before = read(&fresh.join("qxide.toml"));
    env.fails(&fresh, &["add", "bell-utils@0.2"], "Q0607");
    assert_eq!(read(&fresh.join("qxide.toml")), before);
    assert!(!fresh.join("qxide.lock").exists());
}

#[test]
fn file_url_registries_work() {
    let env = Env::new("fileurl");
    let lib = env.package("bell-utils", None);
    env.ok(&lib, &["publish"]);
    let path = env.registry.display().to_string().replace('\\', "/");
    let url = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    let app = env.app("demo", BELL_MAIN);
    env.ok(&app, &["add", "bell-utils", "--registry", &url]);
    let lock = read(&app.join("qxide.lock"));
    assert!(lock.contains("source = \"registry+file:///"), "{lock}");
    env.ok(&app, &["run"]);
    // The lock records the same canonical URL whether the registry was given as a path or a URL.
    env.ok(&app, &["install", "--locked"]);
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
