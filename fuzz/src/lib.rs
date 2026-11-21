// Fuzz harness: compiles the binary-only `src/lang` and `src/pkg` modules unchanged (the same
// `#[path]` trick as crates/qxide-wasm) and exposes one entry point per fuzz target. Each entry
// point must never panic, hang or exhaust memory on any input; a panic here is a finding.

#[allow(dead_code, unused_imports)]
#[path = "../../src/lang/mod.rs"]
pub mod lang;

#[allow(dead_code, unused_imports)]
#[path = "../../src/pkg/mod.rs"]
pub mod pkg;

// Lexer + parser only, without linking or checking (`lang` keeps them private).
#[allow(dead_code, unused_imports, clippy::duplicate_mod)]
pub mod front;

pub mod targets {
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{front, lang, pkg};

    const NAME: &str = "fuzz.qx";

    fn text(data: &[u8]) -> Option<&str> {
        std::str::from_utf8(data).ok()
    }

    /// Lexer and parser.
    pub fn parse(data: &[u8]) {
        if let Some(src) = text(data) {
            if let Ok(toks) = front::lexer::lex(src) {
                let _ = front::parser::parse(toks);
            }
        }
    }

    /// Lexer, parser, linker (no project, so imports are errors) and static checks.
    pub fn check(data: &[u8]) {
        if let Some(src) = text(data) {
            let _ = lang::check(NAME, src);
        }
    }

    /// `qxide fmt`: formatting must succeed again on its own output and be idempotent.
    pub fn fmt(data: &[u8]) {
        let Some(src) = text(data) else { return };
        let Ok(once) = lang::format(NAME, src) else {
            return;
        };
        match lang::format(NAME, &once) {
            Ok(twice) => assert_eq!(once, twice, "fmt is not idempotent"),
            Err(lang::Failure(d)) => panic!("fmt output does not parse:\n{d}"),
        }
    }

    /// `qxide run` with the playground's limits (20 state-vector qubits, 512 MiB tableau,
    /// at most 100k shots). The first byte picks the shot count (1..=64), the next 8 the seed.
    pub fn run_limits(data: &[u8]) {
        if data.len() < 9 {
            return;
        }
        let shots = u64::from(data[0] % 64) + 1;
        let seed = u64::from_le_bytes(data[1..9].try_into().expect("8 bytes"));
        if let Some(src) = text(&data[9..]) {
            let _ = qxide_wasm::execute(qxide_wasm::Command::Run, NAME, src, shots, seed);
        }
    }

    /// `qxide.toml` parser.
    pub fn manifest(data: &[u8]) {
        if let Some(src) = text(data) {
            let _ = pkg::manifest::parse(src, Path::new("fuzz"));
        }
    }

    /// One registry index line; accepted entries must survive a JSON round trip unchanged.
    pub fn index_line(data: &[u8]) {
        let Some(line) = text(data) else { return };
        if let Ok(entry) = pkg::index::Entry::parse(line) {
            let again = pkg::index::Entry::parse(&entry.to_json())
                .unwrap_or_else(|e| panic!("re-parse of to_json failed: {e}"));
            assert_eq!(entry, again);
        }
    }

    /// Package archive unpacking into a fresh temporary directory (removed afterwards).
    /// Every file written must stay under that directory.
    pub fn archive(data: &[u8]) {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "qxide-fuzz-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let dest = base.join("out");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create fuzz temp dir");
        if pkg::archive::unpack(data, "pkg-0.1.0", &dest).is_ok() {
            for entry in walk(&base) {
                assert!(
                    entry.starts_with(&dest),
                    "wrote outside dest: {}",
                    entry.display()
                );
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    out.extend(walk(&p));
                }
                out.push(p);
            }
        }
        out
    }
}
