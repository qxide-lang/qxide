// Package manager unit tests (no network: in-memory indexes and temporary directories).
// Assertions use error codes, never message text.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};

use super::index::{Entry, IndexDep};
use super::lock::{Lock, LockedPackage, Source};
use super::manifest::{self, Manifest};
use super::resolve::{self, join_rel, Provider, Resolver};
use super::*;

const URL: &str = "file:///registry/";

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qxide-pkg-unit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Mem(HashMap<String, Vec<Entry>>);

impl Provider for Mem {
    fn entries(&mut self, name: &str) -> Result<Vec<Entry>> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new(Code::PackageNotFound, name.to_string()))
    }
}

fn entry(name: &str, vers: &str, deps: &[(&str, &str)]) -> Entry {
    Entry {
        name: name.into(),
        vers: Version::parse(vers).unwrap(),
        deps: deps
            .iter()
            .map(|(n, r)| IndexDep {
                name: n.to_string(),
                req: VersionReq::parse(r).unwrap(),
            })
            .collect(),
        cksum: sha256_hex(format!("{name}-{vers}").as_bytes()),
        qxide: None,
        description: None,
        dl: None,
        yanked: false,
    }
}

fn registry(entries: Vec<Entry>) -> Mem {
    let mut m: HashMap<String, Vec<Entry>> = HashMap::new();
    for e in entries {
        m.entry(e.name.clone()).or_default().push(e);
    }
    Mem(m)
}

fn root(deps: &str) -> Manifest {
    let text = format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{deps}");
    manifest::parse(&text, Path::new("/project")).unwrap()
}

fn resolve_with(m: &Manifest, lock: Option<&Lock>, reg: &mut Mem) -> Result<Lock> {
    Resolver::new(m, lock, reg, URL, Version::new(0, 6, 0)).resolve()
}

fn versions(lock: &Lock) -> Vec<(String, String)> {
    lock.packages
        .iter()
        .filter(|p| p.source != Source::Root)
        .map(|p| (p.name.clone(), p.version.to_string()))
        .collect()
}

fn pair(n: &str, v: &str) -> (String, String) {
    (n.to_string(), v.to_string())
}

#[test]
fn names_and_index_paths() {
    assert!(valid_name("bell-utils") && valid_name("a") && valid_name("q2"));
    for bad in [
        "",
        "Bell",
        "bell_utils",
        "-a",
        "a-",
        "a--b",
        "1a",
        &"a".repeat(65),
    ] {
        assert!(!valid_name(bad), "{bad}");
    }
    assert_eq!(import_name("bell-utils"), "bell_utils");
    assert_eq!(index::index_path("a"), "index/1/a");
    assert_eq!(index::index_path("ab"), "index/2/ab");
    assert_eq!(index::index_path("abc"), "index/3/a/abc");
    assert_eq!(index::index_path("bell-utils"), "index/be/ll/bell-utils");
    assert_eq!(join_rel("", "./../a/"), "../a");
    assert_eq!(join_rel("../a", "../b"), "../b");
    assert_eq!(join_rel("vendor/a", "sub"), "vendor/a/sub");
}

#[test]
fn index_entries_round_trip() {
    let mut e = entry("bell-utils", "0.1.0", &[("other", "^1.2")]);
    e.qxide = Some(VersionReq::parse(">=0.6").unwrap());
    e.description = Some("Bell \"pairs\"".into());
    let line = e.to_json();
    assert_eq!(Entry::parse(&line).unwrap(), e);
    assert!(!line.contains('\n'));
    // Unknown keys are ignored (forward compatibility); a bad checksum is not.
    let extra = line.replacen('{', "{\"future\":1,", 1);
    assert_eq!(Entry::parse(&extra).unwrap(), e);
    assert!(Entry::parse(&line.replace(&e.cksum, "abc")).is_err());
    assert!(Entry::parse(&line.replace("bell-utils", "Bell")).is_err());
}

// A directory registry holding one index file with these lines.
fn dir_registry(dir: &Path, name: &str, lines: &[String]) -> index::Registry {
    let file = dir.join("registry").join(index::index_path(name));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, lines.join("\n") + "\n").unwrap();
    let home = cache::Home {
        root: dir.join("home"),
    };
    let reg = dir.join("registry").display().to_string();
    index::Registry::open(Some(&reg), false, home).unwrap()
}

#[test]
fn index_lines_the_client_cannot_use_are_skipped() {
    let good = entry("evil", "1.0.0", &[]);
    let newer = entry("evil", "1.1.0", &[]);
    let line = good.to_json();
    let with = |extra: &str| line.replacen('{', &format!("{{{extra},"), 1);
    let mut twin = good.clone();
    twin.cksum = sha256_hex(b"twin");
    let bad = [
        // Build metadata ties with `1.0.0` in precedence.
        entry("evil", "1.0.0+evil", &[]).to_json(),
        line.replace("\"1.0.0\"", "\"1.0.99999999999999999999\""),
        line.replace("\"1.0.0\"", "\"1.0.1-01\""),
        entry("evil", "1.0.2", &[("other", "^1")])
            .to_json()
            .replace("^1", "not a req"),
        with("\"qxide\":\"bogus!\""),
        with("\"qxide\":99"),
        with("\"dl\":5"),
        with("\"description\":[]"),
        line.replace("\"deps\":[]", "\"deps\":{}"),
        "not json".to_string(),
        entry("other", "1.0.3", &[]).to_json(),
    ];
    for b in &bad[..bad.len() - 1] {
        assert!(Entry::parse(b).is_err(), "{b}");
    }
    let mut lines = vec![line.clone()];
    lines.extend(bad.iter().cloned());
    // A second line for a published version is ignored: the first one wins.
    lines.push(twin.to_json());
    lines.push(newer.to_json());
    let dir = tmp_dir("skip");
    let reg = dir_registry(&dir, "evil", &lines);
    assert_eq!(reg.entries("evil").unwrap(), [good.clone(), newer]);
    // `=1.0.0` resolves to the original line, never to a look-alike.
    let mut reg = reg;
    let lock = Resolver::new(
        &root("evil = \"=1.0.0\"\n"),
        None,
        &mut reg,
        URL,
        Version::new(0, 6, 0),
    )
    .resolve()
    .unwrap();
    assert_eq!(lock.get("evil").unwrap().checksum, Some(good.cksum));
    // An index without a single usable line is malformed, not missing.
    let dir = tmp_dir("allbad");
    let reg = dir_registry(&dir, "evil", &bad[..2]);
    assert_eq!(reg.entries("evil").unwrap_err().code, Code::InvalidIndex);
}

#[test]
fn index_json_is_strict() {
    // validate.py rejects the same lines (ci/test_registry_scripts.py).
    let line = entry("evil", "1.0.0", &[]).to_json();
    let with = |extra: &str| line.replacen('{', &format!("{{{extra},"), 1);
    assert!(Entry::parse(&with("\"description\":\"\\ud800\"")).is_err());
    assert!(Entry::parse(&with("\"x\":NaN")).is_err());
    let deep = "[".repeat(130) + &"]".repeat(130);
    assert!(Entry::parse(&with(&format!("\"x\":{deep}"))).is_err());
    // Only `\n` ends a line: two entries joined by a form feed are one unusable line.
    let dir = tmp_dir("formfeed");
    let other = entry("evil", "1.1.0", &[]).to_json();
    let reg = dir_registry(&dir, "evil", &[format!("{line}\u{c}{other}")]);
    assert_eq!(reg.entries("evil").unwrap_err().code, Code::InvalidIndex);
}

#[test]
fn curl_results_need_a_clean_exit() {
    use index::{fetched, Fetched};
    assert_eq!(fetched(true, "200"), Fetched::Body);
    assert_eq!(fetched(true, "404"), Fetched::NotFound);
    assert_eq!(fetched(false, "404"), Fetched::Failed);
    // Exit 18 (partial file) and similar failures after a 200 status line.
    assert_eq!(fetched(false, "200"), Fetched::Failed);
    for code in ["000", "500", "403", "", "200 "] {
        assert_eq!(fetched(true, code), Fetched::Failed, "{code:?}");
    }
}

#[test]
fn manifests_parse_and_validate() {
    let m = root("a = \"0.1\"\nb = { version = \"1\", path = \"../b\" }\nc = { path = \"c\" }\n");
    assert_eq!(
        m.dependencies["a"].req,
        Some(VersionReq::parse("^0.1").unwrap())
    );
    assert_eq!(m.dependencies["b"].path.as_deref(), Some("../b"));
    assert_eq!(m.dependencies["c"].req, None);
    let bad = |text: &str| manifest::parse(text, Path::new("/p")).unwrap_err().code;
    assert_eq!(bad("name = \"x\""), Code::InvalidManifest);
    assert_eq!(
        bad("[package]\nname = \"x_y\"\nversion = \"1.0.0\""),
        Code::InvalidManifest
    );
    assert_eq!(
        bad("[package]\nname = \"x\"\nversion = \"1.0\""),
        Code::InvalidManifest
    );
    assert_eq!(
        bad("[package]\nname = \"x\"\nversion = \"1.0.0\"\nqxide = \"~>1\""),
        Code::InvalidManifest
    );
    assert_eq!(
        bad("[package]\nname = \"x\"\nversion = \"1.0.0\"\n[dependencies]\ny = {}"),
        Code::InvalidManifest
    );
    assert_eq!(bad("[package"), Code::InvalidManifest);
}

#[test]
fn add_and_remove_keep_comments() {
    let dir = tmp_dir("edit");
    let text = "# my project\n[package]\nname = \"app\" # the name\nversion = \"0.1.0\"\n";
    std::fs::write(dir.join("qxide.toml"), text).unwrap();
    manifest::add_dependency(&dir, "bell-utils", Some("0.1.0"), None).unwrap();
    manifest::add_dependency(&dir, "local", None, Some("../local")).unwrap();
    let edited = std::fs::read_to_string(dir.join("qxide.toml")).unwrap();
    assert!(edited.starts_with(text), "{edited}");
    let m = manifest::load(&dir).unwrap();
    assert_eq!(m.dependencies.len(), 2);
    assert_eq!(m.dependencies["local"].path.as_deref(), Some("../local"));
    assert!(manifest::remove_dependency(&dir, "bell-utils").unwrap());
    assert!(!manifest::remove_dependency(&dir, "bell-utils").unwrap());
    assert_eq!(manifest::load(&dir).unwrap().dependencies.len(), 1);
}

#[test]
fn resolution_picks_newest_and_respects_the_lock() {
    let mut reg = registry(vec![
        entry("a", "0.1.0", &[]),
        entry("a", "0.1.3", &[("b", "^1.0")]),
        entry("a", "0.2.0", &[]),
        entry("b", "1.0.0", &[]),
        entry("b", "1.4.0", &[]),
    ]);
    let m = root("a = \"0.1\"\n");
    let lock = resolve_with(&m, None, &mut reg).unwrap();
    assert_eq!(versions(&lock), [pair("a", "0.1.3"), pair("b", "1.4.0")]);
    // A pinned older version stays pinned even though a newer one matches.
    let mut pinned = lock.clone();
    for p in &mut pinned.packages {
        if p.name == "b" {
            p.version = Version::new(1, 0, 0);
        }
    }
    let again = resolve_with(&m, Some(&pinned), &mut reg).unwrap();
    assert_eq!(versions(&again), [pair("a", "0.1.3"), pair("b", "1.0.0")]);
    assert!(fresh(&m, &lock, URL, &reg));
    assert!(!fresh(&root("a = \"0.2\"\n"), &lock, URL, &reg));
    assert!(!fresh(&m, &lock, "file:///elsewhere/", &reg));
}

// `is_fresh` with the requirements of registry packages taken from the in-memory index.
fn fresh(m: &Manifest, lock: &Lock, url: &str, reg: &Mem) -> bool {
    resolve::is_fresh(m, lock, url, &mut |p| {
        let e = reg.0.get(&p.name)?.iter().find(|e| e.vers == p.version)?;
        Some(
            e.deps
                .iter()
                .map(|d| (d.name.clone(), d.req.clone()))
                .collect(),
        )
    })
}

fn pkg<'a>(lock: &'a mut Lock, name: &str) -> &'a mut LockedPackage {
    lock.packages.iter_mut().find(|p| p.name == name).unwrap()
}

#[test]
fn freshness_checks_the_whole_graph() {
    let mut reg = registry(vec![
        entry("a", "1.0.0", &[("b", "^1.2")]),
        entry("b", "1.0.0", &[]),
        entry("b", "1.2.0", &[]),
        entry("c", "1.0.0", &[]),
    ]);
    let m = root("a = \"1\"\n");
    let lock = resolve_with(&m, None, &mut reg).unwrap();
    assert!(fresh(&m, &lock, URL, &reg));
    let edit = |f: &dyn Fn(&mut Lock)| {
        let mut l = lock.clone();
        f(&mut l);
        l.sort();
        l
    };
    // A hand-edited downgrade of a transitive dependency breaks `a`'s `^1.2` on it.
    let downgraded = edit(&|l| {
        let b = pkg(l, "b");
        b.version = Version::new(1, 0, 0);
        b.checksum = Some(entry("b", "1.0.0", &[]).cksum);
    });
    assert!(!fresh(&m, &downgraded, URL, &reg));
    // A new resolution moves it back up.
    let again = resolve_with(&m, Some(&downgraded), &mut reg).unwrap();
    assert_eq!(versions(&again), [pair("a", "1.0.0"), pair("b", "1.2.0")]);
    // A locked package nothing depends on would still be fetched by `install`.
    let extra = edit(&|l| {
        l.packages.push(LockedPackage {
            name: "c".into(),
            version: Version::new(1, 0, 0),
            source: Source::Registry(URL.into()),
            checksum: Some(entry("c", "1.0.0", &[]).cksum),
            dependencies: vec![],
        })
    });
    assert!(!fresh(&m, &extra, URL, &reg));
    // A registry package locked as a path package.
    let moved = edit(&|l| {
        let b = pkg(l, "b");
        b.source = Source::Path("../b".into());
        b.checksum = None;
    });
    assert!(!fresh(&m, &moved, URL, &reg));
    // Dependency names that differ from the index entry.
    let renamed = edit(&|l| {
        let a = pkg(l, "a");
        a.dependencies.push("app".into());
    });
    assert!(!fresh(&m, &renamed, URL, &reg));
    // Self-dependencies are stale even where the requirements are not known locally.
    assert!(resolve::is_fresh(&m, &lock, URL, &mut |_| None));
    for target in ["a", "app"] {
        let cyclic = edit(&|l| pkg(l, "a").dependencies.push(target.into()));
        assert!(
            !resolve::is_fresh(&m, &cyclic, URL, &mut |_| None),
            "{target}"
        );
    }
}

#[test]
fn freshness_rereads_path_packages() {
    let dir = tmp_dir("pathfresh");
    let mid = dir.join("mid");
    std::fs::create_dir_all(&mid).unwrap();
    let write_mid = |req: &str| {
        std::fs::write(
            mid.join("qxide.toml"),
            format!(
                "[package]\nname = \"mid\"\nversion = \"0.1.0\"\n\n[dependencies]\nb = \"{req}\"\n"
            ),
        )
        .unwrap()
    };
    write_mid("^1");
    std::fs::create_dir_all(dir.join("app")).unwrap();
    let text = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nmid = { path = \"../mid\" }\n";
    let m = manifest::parse(text, &dir.join("app")).unwrap();
    let mut reg = registry(vec![entry("b", "1.0.0", &[]), entry("b", "2.0.0", &[])]);
    let lock = resolve_with(&m, None, &mut reg).unwrap();
    assert_eq!(versions(&lock), [pair("b", "1.0.0"), pair("mid", "0.1.0")]);
    assert!(fresh(&m, &lock, URL, &reg));
    // The path package now needs `b` 2, which the lock does not have.
    write_mid("^2");
    assert!(!fresh(&m, &lock, URL, &reg));
    let again = resolve_with(&m, Some(&lock), &mut reg).unwrap();
    assert_eq!(versions(&again), [pair("b", "2.0.0"), pair("mid", "0.1.0")]);
}

#[test]
fn lock_names_are_validated() {
    let path = Path::new("qxide.lock");
    let ok = "version = 1\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = []\n";
    assert!(lock::parse(ok, path).is_ok());
    // Names end up in cache paths, so a traversal must be refused when the lock is read.
    let cksum = sha256_hex(b"x");
    for bad in [
        ok.replace("name = \"app\"", "name = \"../../../../outside/x\""),
        ok.replace("dependencies = []", "dependencies = [\"../x\"]"),
        format!(
            "{ok}\n[[package]]\nname = \"..\\\\x\"\nversion = \"1.0.0\"\nsource = \"registry+{URL}\"\nchecksum = \"sha256:{cksum}\"\ndependencies = []\n"
        ),
    ] {
        assert_eq!(
            lock::parse(&bad, path).unwrap_err().code,
            Code::InvalidLock,
            "{bad}"
        );
    }
}

#[test]
fn yanked_versions_are_skipped_unless_locked() {
    let mut yanked = entry("a", "1.1.0", &[]);
    yanked.yanked = true;
    let mut reg = registry(vec![entry("a", "1.0.0", &[]), yanked]);
    let m = root("a = \"1\"\n");
    let lock = resolve_with(&m, None, &mut reg).unwrap();
    assert_eq!(versions(&lock), [pair("a", "1.0.0")]);
    let mut pinned = lock.clone();
    pinned
        .packages
        .iter_mut()
        .find(|p| p.name == "a")
        .unwrap()
        .version = Version::new(1, 1, 0);
    let kept = resolve_with(&m, Some(&pinned), &mut reg).unwrap();
    assert_eq!(versions(&kept), [pair("a", "1.1.0")]);
}

#[test]
fn resolution_errors_have_codes() {
    let mut reg = registry(vec![
        entry("a", "1.0.0", &[("c", "^1")]),
        entry("b", "1.0.0", &[("c", "^2")]),
        entry("c", "1.0.0", &[]),
        entry("c", "2.0.0", &[]),
    ]);
    let code = |deps: &str, reg: &mut Mem| resolve_with(&root(deps), None, reg).unwrap_err().code;
    assert_eq!(code("a = \"9\"\n", &mut reg), Code::NoMatchingVersion);
    assert_eq!(code("zz = \"1\"\n", &mut reg), Code::PackageNotFound);
    assert_eq!(
        code("a = \"1\"\nb = \"1\"\n", &mut reg),
        Code::VersionConflict
    );
    // A registry package that depends on itself (design §7: Q0606), directly or as a dependency.
    let mut selfish = registry(vec![
        entry("s", "1.0.0", &[("s", "^1")]),
        entry("t", "1.0.0", &[("s", "^1")]),
    ]);
    assert_eq!(code("s = \"1\"\n", &mut selfish), Code::VersionConflict);
    assert_eq!(code("t = \"1\"\n", &mut selfish), Code::VersionConflict);
    let mut newer = entry("d", "1.0.0", &[]);
    newer.qxide = Some(VersionReq::parse(">=9").unwrap());
    let mut reg = registry(vec![newer]);
    assert_eq!(code("d = \"1\"\n", &mut reg), Code::LanguageVersion);
}

#[test]
fn lock_files_are_deterministic() {
    let lock = Lock {
        packages: vec![
            LockedPackage {
                name: "zeta".into(),
                version: Version::new(1, 0, 0),
                source: Source::Registry(URL.into()),
                checksum: Some(sha256_hex(b"z")),
                dependencies: vec!["alpha".into()],
            },
            LockedPackage {
                name: "app".into(),
                version: Version::new(0, 1, 0),
                source: Source::Root,
                checksum: None,
                dependencies: vec!["zeta".into(), "alpha".into()],
            },
            LockedPackage {
                name: "alpha".into(),
                version: Version::new(0, 3, 1),
                source: Source::Path("../alpha".into()),
                checksum: None,
                dependencies: vec![],
            },
        ],
    };
    let mut sorted = lock.clone();
    sorted.sort();
    let text = sorted.render();
    assert!(!text.contains('\r'));
    let parsed = lock::parse(&text, Path::new("qxide.lock")).unwrap();
    assert_eq!(parsed, sorted);
    assert_eq!(parsed.render(), text, "render -> parse -> render is stable");
    let names: Vec<&str> = parsed.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["alpha", "app", "zeta"]);
    let bad = text.replace("sha256:", "md5:");
    assert_eq!(
        lock::parse(&bad, Path::new("qxide.lock")).unwrap_err().code,
        Code::InvalidLock
    );
}

fn write_package(dir: &Path) -> Manifest {
    std::fs::create_dir_all(dir.join("src/sub")).unwrap();
    std::fs::write(
        dir.join("qxide.toml"),
        "[package]\nname = \"demo\"\nversion = \"1.2.3\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.qx"), "public void f() {}\r\n").unwrap();
    std::fs::write(dir.join("src/sub/x.qx"), "public void g() {}\n").unwrap();
    std::fs::write(dir.join("src/notes.txt"), "not shipped").unwrap();
    std::fs::write(dir.join("README.md"), "# demo\n").unwrap();
    manifest::load(dir).unwrap()
}

#[test]
fn archives_are_deterministic_and_unpack_safely() {
    let dir = tmp_dir("pack");
    let m = write_package(&dir.join("pkg"));
    let (a, files) = archive::pack(&m).unwrap();
    assert_eq!(
        files,
        ["README.md", "qxide.toml", "src/lib.qx", "src/sub/x.qx"]
    );
    let (b, _) = archive::pack(&m).unwrap();
    assert_eq!(sha256_hex(&a), sha256_hex(&b));
    let out = dir.join("out");
    archive::unpack(&a, "demo-1.2.3", &out).unwrap();
    // Line endings were normalised, and only package files were shipped.
    assert_eq!(
        std::fs::read_to_string(out.join("src/lib.qx")).unwrap(),
        "public void f() {}\n"
    );
    assert!(!out.join("src/notes.txt").exists());
    // The wrong top-level directory is rejected.
    let err = archive::unpack(&a, "other-1.0.0", &dir.join("out2")).unwrap_err();
    assert_eq!(err.code, Code::BadArchive);
}

// A gzip tarball with raw header paths and types, bypassing the tar crate's own path checks.
fn evil_archive(path: &str, kind: tar::EntryType) -> Vec<u8> {
    let mut h = tar::Header::new_gnu();
    {
        let name = &mut h.as_old_mut().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
    }
    h.set_size(4);
    h.set_mode(0o644);
    h.set_entry_type(kind);
    h.set_cksum();
    let mut raw = h.as_bytes().to_vec();
    raw.extend_from_slice(b"evil");
    raw.resize(raw.len() + 508, 0);
    raw.resize(raw.len() + 1024, 0);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &raw).unwrap();
    gz.finish().unwrap()
}

#[test]
fn unsafe_archive_entries_are_rejected() {
    let dir = tmp_dir("evil");
    // The hand-built format itself is valid: a harmless entry unpacks.
    let fine = evil_archive("demo-1.0.0/qxide.toml", tar::EntryType::Regular);
    archive::unpack(&fine, "demo-1.0.0", &dir.join("fine")).unwrap();
    assert_eq!(std::fs::read(dir.join("fine/qxide.toml")).unwrap(), b"evil");
    for (path, kind) in [
        ("demo-1.0.0/../escape.qx", tar::EntryType::Regular),
        ("/abs/escape.qx", tar::EntryType::Regular),
        ("demo-1.0.0/C:/escape.qx", tar::EntryType::Regular),
        ("demo-1.0.0/link", tar::EntryType::Symlink),
        ("demo-1.0.0/hard", tar::EntryType::Link),
    ] {
        let bytes = evil_archive(path, kind);
        let out = dir.join("out");
        let err = archive::unpack(&bytes, "demo-1.0.0", &out).unwrap_err();
        assert_eq!(err.code, Code::BadArchive, "{path}");
        assert!(!dir.join("escape.qx").exists());
    }
    assert_eq!(
        archive::unpack(b"not gzip", "demo-1.0.0", &dir.join("x"))
            .unwrap_err()
            .code,
        Code::BadArchive
    );
}

// A gzip tarball of regular files with raw header paths, in the given order.
fn raw_archive(files: &[&str]) -> Vec<u8> {
    let mut raw = Vec::new();
    for path in files {
        let mut h = tar::Header::new_gnu();
        {
            let name = &mut h.as_old_mut().name;
            name[..path.len()].copy_from_slice(path.as_bytes());
        }
        h.set_size(4);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        raw.extend_from_slice(h.as_bytes());
        raw.extend_from_slice(b"evil");
        raw.resize(raw.len() + 508, 0);
    }
    raw.resize(raw.len() + 1024, 0);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &raw).unwrap();
    gz.finish().unwrap()
}

#[test]
fn archive_paths_must_be_portable_and_unique() {
    let dir = tmp_dir("portable");
    let ok = raw_archive(&[
        "demo-1.0.0/qxide.toml",
        "demo-1.0.0/src/lib.qx",
        "demo-1.0.0/src/console.qx",
        "demo-1.0.0/src/com10.qx",
        "demo-1.0.0/src/lib/x.qx",
    ]);
    archive::unpack(&ok, "demo-1.0.0", &dir.join("ok")).unwrap();
    // Each path either collides with `src/lib.qx` (or another entry) on Windows or macOS, or cannot
    // be created on those platforms.
    for second in [
        "src/lib.qx.",
        "src/lib.qx ",
        "src/LIB.qx",
        "src/lib.qx",
        "SRC/lib.qx",
        "src/lib.qx/x.qx",
        "src/nul.qx",
        "src/CON",
        "src/aux.tar.gz",
        "src/Com1.qx",
        "src/lpt9",
        "src/conin$.qx",
        "src/com\u{b9}.qx",
        "src/nul .qx",
        "src/a<b.qx",
        "src/a>b.qx",
        "src/a\"b.qx",
        "src/a|b.qx",
        "src/a?b.qx",
        "src/a*b.qx",
        "src/a\u{1}b.qx",
        "src/a\u{7f}b.qx",
        "src./x.qx",
    ] {
        let bytes = raw_archive(&[
            "demo-1.0.0/qxide.toml",
            "demo-1.0.0/src/lib.qx",
            &format!("demo-1.0.0/{second}"),
        ]);
        let out = dir.join("out");
        let err = archive::unpack(&bytes, "demo-1.0.0", &out).unwrap_err();
        assert_eq!(err.code, Code::BadArchive, "{second:?}");
        assert!(!out.exists(), "nothing written for {second:?}");
        assert!(
            archive::check_paths(["qxide.toml", "src/lib.qx", second]).is_err(),
            "{second:?}"
        );
    }
    // A directory first, then a file of the same (case-folded) name.
    let bytes = raw_archive(&[
        "demo-1.0.0/qxide.toml",
        "demo-1.0.0/src/a/x.qx",
        "demo-1.0.0/src/A",
    ]);
    let err = archive::unpack(&bytes, "demo-1.0.0", &dir.join("out")).unwrap_err();
    assert_eq!(err.code, Code::BadArchive);
    assert!(!dir.join("out").exists());
    assert!(archive::check_paths(["qxide.toml", "src/lib.qx", "src/lib/x.qx"]).is_ok());
}

#[test]
fn publishing_skips_symlinked_top_level_files() {
    let dir = tmp_dir("symlink");
    let m = write_package(&dir.join("pkg"));
    std::fs::write(dir.join("secret"), "do not ship").unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(dir.join("secret"), m.dir.join("LICENSE"));
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(dir.join("secret"), m.dir.join("LICENSE"));
    #[cfg(not(any(unix, windows)))]
    let linked: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    if linked.is_err() {
        // Windows without Developer Mode or admin rights cannot create symlinks.
        return;
    }
    let files: Vec<String> = archive::package_files(&m)
        .unwrap()
        .into_iter()
        .map(|(r, _)| r)
        .collect();
    assert!(!files.contains(&"LICENSE".to_string()), "{files:?}");
    assert!(files.contains(&"README.md".to_string()), "{files:?}");
}

#[test]
fn installs_verify_the_checksum_first() {
    let dir = tmp_dir("install");
    let m = write_package(&dir.join("pkg"));
    let (bytes, _) = archive::pack(&m).unwrap();
    let home = cache::Home {
        root: dir.join("home"),
    };
    let v = Version::new(1, 2, 3);
    let wrong = sha256_hex(b"something else");
    let err = home.install("r", "demo", &v, &wrong, &bytes).unwrap_err();
    assert_eq!(err.code, Code::ChecksumMismatch);
    assert!(home.installed("r", "demo", &v, &wrong).is_none());
    assert!(!home.src_dir("r", "demo", &v).exists(), "nothing unpacked");
    let cksum = sha256_hex(&bytes);
    let path = home.install("r", "demo", &v, &cksum, &bytes).unwrap();
    assert!(path.join("src/sub/x.qx").is_file());
    assert_eq!(home.installed("r", "demo", &v, &cksum), Some(path));
    assert!(home.cached_archive("r", "demo", &v, &cksum).is_some());
}

// M-02 in docs/security/PKG_CLIENT_FIXES.md: tar entries whose data the `tar` crate would read
// whole before `unpack` can check anything.

const GIB: u64 = 1 << 30;

fn tar_header(name: &str, size: u64, kind: tar::EntryType) -> Vec<u8> {
    let mut h = tar::Header::new_gnu();
    h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    h.set_mode(0o644);
    h.set_uid(0);
    h.set_gid(0);
    h.set_mtime(0);
    h.set_size(size);
    h.set_entry_type(kind);
    h.set_cksum();
    h.as_bytes().to_vec()
}

fn write_repeated(w: &mut impl std::io::Write, byte: u8, mut n: u64) {
    let chunk = vec![byte; 1 << 20];
    while n > 0 {
        let k = n.min(chunk.len() as u64) as usize;
        w.write_all(&chunk[..k]).unwrap();
        n -= k as u64;
    }
}

// Streams `parts` through gzip at the best level, as the review's generator did.
fn gzip_stream(parts: impl FnOnce(&mut flate2::write::GzEncoder<Vec<u8>>)) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    parts(&mut gz);
    gz.finish().unwrap()
}

// A regular `demo-1.0.0/qxide.toml` holding `evil`, then the end-of-archive blocks.
fn manifest_and_end(w: &mut impl std::io::Write) {
    w.write_all(&tar_header(
        "demo-1.0.0/qxide.toml",
        4,
        tar::EntryType::Regular,
    ))
    .unwrap();
    w.write_all(b"evil").unwrap();
    w.write_all(&[0; 508 + 1024]).unwrap();
}

// The M-02 reproduction input: a GNU long-name entry declaring 1 GiB of zero bytes, then one
// empty regular entry.
#[test]
fn long_name_entries_are_not_read_whole() {
    let bytes = gzip_stream(|w| {
        w.write_all(&tar_header(
            "././@LongLink",
            GIB,
            tar::EntryType::GNULongName,
        ))
        .unwrap();
        write_repeated(w, 0, GIB);
        w.write_all(&tar_header("x", 0, tar::EntryType::Regular))
            .unwrap();
        w.write_all(&[0; 1024]).unwrap();
    });
    assert!(bytes.len() as u64 <= archive::MAX_ARCHIVE);
    let dir = tmp_dir("longname");
    let err = archive::unpack(&bytes, "longname-1.0.0", &dir.join("out")).unwrap_err();
    assert_eq!(err.code, Code::BadArchive);
    // Before the fix the 1 GiB name was read into memory and quoted in this message.
    let shown = err.render();
    assert!(shown.len() < 4096, "{} bytes", shown.len());
}

// The same read for a PAX extended header: one 1 GiB `comment` record the crate would parse and
// ignore, followed by a valid manifest.
#[test]
fn pax_headers_are_rejected_unread() {
    let bytes = gzip_stream(|w| {
        let record = format!("{GIB} comment=");
        w.write_all(&tar_header(
            "demo-1.0.0/qxide.toml",
            GIB,
            tar::EntryType::XHeader,
        ))
        .unwrap();
        w.write_all(record.as_bytes()).unwrap();
        write_repeated(w, b'a', GIB - record.len() as u64 - 1);
        w.write_all(b"\n").unwrap();
        manifest_and_end(w);
    });
    let dir = tmp_dir("pax");
    let err = archive::unpack(&bytes, "demo-1.0.0", &dir.join("out")).unwrap_err();
    assert_eq!(err.code, Code::BadArchive);
}

// Data the client never keeps (here a directory entry's) still counts toward the decompressed
// limit, so a small archive cannot make the client inflate gigabytes.
#[test]
fn decompressed_size_is_capped() {
    let bytes = gzip_stream(|w| {
        w.write_all(&tar_header("demo-1.0.0/d/", GIB, tar::EntryType::Directory))
            .unwrap();
        write_repeated(w, 0, GIB);
        manifest_and_end(w);
    });
    let dir = tmp_dir("inflate");
    let err = archive::unpack(&bytes, "demo-1.0.0", &dir.join("out")).unwrap_err();
    assert_eq!(err.code, Code::BadArchive);
}

// `pack` stores paths over 100 bytes with a GNU long-name entry, which must still unpack.
#[test]
fn long_package_paths_round_trip() {
    let dir = tmp_dir("longpath");
    let m = write_package(&dir.join("pkg"));
    let deep = format!("src/{}/{}.qx", "d".repeat(60), "f".repeat(60));
    std::fs::create_dir_all(m.dir.join(&deep).parent().unwrap()).unwrap();
    std::fs::write(m.dir.join(&deep), "public void h() {}\n").unwrap();
    let (bytes, _) = archive::pack(&m).unwrap();
    let mut tar_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(&bytes[..]),
        &mut tar_bytes,
    )
    .unwrap();
    assert!(tar_bytes.windows(13).any(|w| w == b"././@LongLink"));
    let out = dir.join("out");
    archive::unpack(&bytes, "demo-1.2.3", &out).unwrap();
    assert!(out.join(&deep).is_file());
}

/// Shared with ci/test_registry_scripts.py, so the registry checker accepts exactly what we parse.
const SEMVER_CASES: &str = include_str!("../../tests/data/semver_cases.json");

#[test]
fn semver_cases_match_the_registry_checker() {
    let cases: serde_json::Value = serde_json::from_str(SEMVER_CASES).unwrap();
    let list = |k: &str| -> Vec<(String, bool)> {
        cases[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c[0].as_str().unwrap().into(), c[1].as_bool().unwrap()))
            .collect()
    };
    let cksum = sha256_hex(b"x");
    for (text, ok) in list("versions") {
        let line = serde_json::json!({"name": "a", "vers": text, "deps": [], "cksum": cksum});
        assert_eq!(Entry::parse(&line.to_string()).is_ok(), ok, "vers {text:?}");
    }
    for (text, ok) in list("reqs") {
        let dep = serde_json::json!([{"name": "b", "req": text}]);
        let line = serde_json::json!({"name": "a", "vers": "1.0.0", "deps": dep, "cksum": cksum, "qxide": text});
        assert_eq!(Entry::parse(&line.to_string()).is_ok(), ok, "req {text:?}");
    }
}

// Every variant once, in order: the exhaustive `match` breaks the build when a variant is added
// without being linked into this chain.
fn all_codes() -> Vec<Code> {
    use Code::*;
    let next = |c: Code| match c {
        ManifestNotFound => Some(InvalidManifest),
        InvalidManifest => Some(InvalidLock),
        InvalidLock => Some(PackageNotFound),
        PackageNotFound => Some(NoMatchingVersion),
        NoMatchingVersion => Some(VersionConflict),
        VersionConflict => Some(ChecksumMismatch),
        ChecksumMismatch => Some(RegistryUnavailable),
        RegistryUnavailable => Some(InvalidIndex),
        InvalidIndex => Some(BadArchive),
        BadArchive => Some(NotInstalled),
        NotInstalled => Some(LockOutdated),
        LockOutdated => Some(LanguageVersion),
        LanguageVersion => Some(PublishRejected),
        PublishRejected => Some(Offline),
        Offline => Some(Io),
        Io => Some(NotADependency),
        NotADependency => None,
    };
    std::iter::successors(Some(ManifestNotFound), |&c| next(c)).collect()
}

// `qxide explain` covers the package-manager codes too, from the same catalog.
#[test]
fn explain_covers_every_package_code() {
    let codes = all_codes();
    for (i, code) in codes.iter().enumerate() {
        // Consecutive numbers, so a variant skipped in the chain above shows up as a gap.
        assert_eq!(code.as_str(), format!("Q{:04}", 601 + i));
        let entry = crate::lang::explain(code.as_str())
            .unwrap_or_else(|| panic!("no catalog entry for {}", code.as_str()));
        assert!(
            entry.starts_with(&format!("{}: ", code.as_str())),
            "{entry}"
        );
    }
    let next = format!("Q{:04}", 601 + codes.len());
    assert_eq!(
        crate::lang::explain(&next),
        None,
        "catalog has an entry for unknown {next}"
    );
}
