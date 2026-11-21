// Stable check of the harness: feeds the repository's own inputs (and any crash files saved by
// `cargo fuzz` under fuzz/artifacts/<target>/) to every target entry point.
//   cargo test --manifest-path fuzz/Cargo.toml --no-default-features

use std::path::{Path, PathBuf};

use qxide_fuzz::targets;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                files(&p, ext, out);
            }
        } else if ext.is_empty() || p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn artifacts(target: &str) -> Vec<Vec<u8>> {
    let mut paths = Vec::new();
    files(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("artifacts")
            .join(target),
        "",
        &mut paths,
    );
    paths.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

fn qx_sources() -> Vec<Vec<u8>> {
    let mut paths = Vec::new();
    files(&root().join("examples"), "qx", &mut paths);
    assert!(!paths.is_empty(), "no .qx examples found");
    paths.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

#[test]
fn qx_targets_accept_examples() {
    for src in qx_sources() {
        targets::parse(&src);
        targets::check(&src);
        targets::fmt(&src);
        let mut run = vec![7u8; 9];
        run.extend_from_slice(&src);
        targets::run_limits(&run);
    }
    for t in ["parse", "check", "fmt", "run_limits"] {
        for data in artifacts(t) {
            match t {
                "parse" => targets::parse(&data),
                "check" => targets::check(&data),
                "fmt" => targets::fmt(&data),
                _ => targets::run_limits(&data),
            }
        }
    }
}

#[test]
fn package_targets_accept_examples() {
    let mut manifests = Vec::new();
    files(
        &root().join("examples").join("packages"),
        "toml",
        &mut manifests,
    );
    for p in manifests {
        targets::manifest(&std::fs::read(p).unwrap());
    }
    let line = r#"{"name":"bell-utils","vers":"0.1.0","deps":[],"cksum":"0000000000000000000000000000000000000000000000000000000000000000","yanked":false,"v":1}"#;
    targets::index_line(line.as_bytes());
    targets::archive(b"");
    targets::archive(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff]);
    for data in artifacts("manifest") {
        targets::manifest(&data);
    }
    for data in artifacts("index_line") {
        targets::index_line(&data);
    }
    for data in artifacts("archive") {
        targets::archive(&data);
    }
}
