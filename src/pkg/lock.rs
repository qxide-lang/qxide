// `qxide.lock`: the exact version, source and checksum of every package in the dependency graph.
// Written by hand in a fixed order (sorted by name, `\n` line endings) so the same graph always
// produces the same bytes; read back with toml_edit.

use std::path::Path;

use semver::Version;
use toml_edit::{DocumentMut, Item};

use super::{msg, valid_name, Code, Error, Result};

pub const FILE: &str = "qxide.lock";
const FORMAT: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The project itself.
    Root,
    /// A registry, by its URL.
    Registry(String),
    /// A local directory, relative to the project root.
    Path(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub version: Version,
    pub source: Source,
    /// Lowercase hex sha256 of the package archive; registry packages only.
    pub checksum: Option<String>,
    /// Names of the packages it depends on, sorted.
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lock {
    /// Sorted by name; names are unique (one version per package).
    pub packages: Vec<LockedPackage>,
}

impl Lock {
    pub fn get(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    pub fn root(&self) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.source == Source::Root)
    }

    pub fn sort(&mut self) {
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        for p in &mut self.packages {
            p.dependencies.sort();
            p.dependencies.dedup();
        }
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(msg::lock_header());
        s.push('\n');
        s.push_str(&format!("version = {FORMAT}\n"));
        for p in &self.packages {
            s.push_str("\n[[package]]\n");
            s.push_str(&format!("name = {}\n", quote(&p.name)));
            s.push_str(&format!("version = {}\n", quote(&p.version.to_string())));
            match &p.source {
                Source::Root => {}
                Source::Registry(url) => {
                    s.push_str(&format!("source = {}\n", quote(&format!("registry+{url}"))))
                }
                Source::Path(path) => {
                    s.push_str(&format!("source = {}\n", quote(&format!("path+{path}"))))
                }
            }
            if let Some(c) = &p.checksum {
                s.push_str(&format!("checksum = {}\n", quote(&format!("sha256:{c}"))));
            }
            let deps: Vec<String> = p.dependencies.iter().map(|d| quote(d)).collect();
            s.push_str(&format!("dependencies = [{}]\n", deps.join(", ")));
        }
        s
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(FILE);
        std::fs::write(&path, self.render()).map_err(|e| Error::io(&path, &e))
    }
}

// TOML basic string; names, versions, URLs and paths never need more than these escapes.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `None` when the project has no lock file yet.
pub fn load(dir: &Path) -> Result<Option<Lock>> {
    let path = dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, &path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(&path, &e)),
    }
}

pub fn parse(text: &str, path: &Path) -> Result<Lock> {
    let bad = |reason: String| {
        Error::new(
            Code::InvalidLock,
            msg::invalid_lock(&path.display().to_string(), &reason),
        )
    };
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| bad(msg::reason_toml(e.message())))?;
    match doc.get("version").and_then(Item::as_integer) {
        Some(FORMAT) => {}
        _ => return Err(bad(msg::reason_lock_format(FORMAT))),
    }
    let mut lock = Lock::default();
    let Some(tables) = doc.get("package") else {
        return Ok(lock);
    };
    let tables = tables
        .as_array_of_tables()
        .ok_or_else(|| bad(msg::reason_not_table("package")))?;
    for t in tables.iter() {
        let field = |k: &str| t.get(k).and_then(Item::as_str).map(str::to_string);
        let name = field("name").ok_or_else(|| bad(msg::reason_missing("name")))?;
        // Names become cache paths (`<name>-<version>`), so `../x` must never get that far.
        if !valid_name(&name) {
            return Err(bad(msg::reason_bad_name(&name)));
        }
        let vtext = field("version").ok_or_else(|| bad(msg::reason_missing("version")))?;
        let version = Version::parse(&vtext).map_err(|_| bad(msg::reason_bad_version(&vtext)))?;
        let source = match field("source") {
            None => Source::Root,
            Some(s) => match (s.strip_prefix("registry+"), s.strip_prefix("path+")) {
                (Some(url), _) => Source::Registry(url.to_string()),
                (_, Some(p)) => Source::Path(p.to_string()),
                _ => return Err(bad(msg::reason_bad_source(&s))),
            },
        };
        let checksum = match field("checksum") {
            None => None,
            Some(c) => match c.strip_prefix("sha256:") {
                Some(hex) if is_sha256_hex(hex) => Some(hex.to_string()),
                _ => return Err(bad(msg::reason_bad_checksum(&c))),
            },
        };
        if matches!(source, Source::Registry(_)) && checksum.is_none() {
            return Err(bad(msg::reason_missing("checksum")));
        }
        let dependencies = match t.get("dependencies") {
            None => Vec::new(),
            Some(i) => i
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .map(|v| v.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| bad(msg::reason_not_string_array("dependencies")))?,
        };
        if let Some(d) = dependencies.iter().find(|d| !valid_name(d)) {
            return Err(bad(msg::reason_bad_name(d)));
        }
        if lock.get(&name).is_some() {
            return Err(bad(msg::reason_duplicate_package(&name)));
        }
        lock.packages.push(LockedPackage {
            name,
            version,
            source,
            checksum,
            dependencies,
        });
    }
    lock.sort();
    Ok(lock)
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
