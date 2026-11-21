// `qxide.toml`: package metadata and dependencies. Edits go through toml_edit so `qxide add`
// and `qxide remove` keep the user's comments and layout.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use toml_edit::{DocumentMut, InlineTable, Item, TableLike};

use super::{msg, valid_name, Code, Error, Result};

pub const FILE: &str = "qxide.toml";

#[derive(Debug, Clone)]
pub struct Manifest {
    /// Directory holding `qxide.toml`.
    pub dir: PathBuf,
    pub name: String,
    pub version: Version,
    pub description: Option<String>,
    pub license: Option<String>,
    // Validated metadata the client does not use yet (shown by a future `qxide info`).
    #[allow(dead_code)]
    pub authors: Vec<String>,
    #[allow(dead_code)]
    pub repository: Option<String>,
    /// Required `qxide` language version, such as `>=0.6`.
    pub qxide: Option<VersionReq>,
    pub dependencies: BTreeMap<String, Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// `None` for a path-only dependency.
    pub req: Option<VersionReq>,
    /// Path as written in the manifest, relative to its directory.
    pub path: Option<String>,
}

impl Dep {
    pub fn req_or_any(&self) -> VersionReq {
        self.req.clone().unwrap_or(VersionReq::STAR)
    }
}

/// The nearest directory at or above `start` that holds a `qxide.toml`.
pub fn find(start: &Path) -> Option<PathBuf> {
    let start = std::path::absolute(start).ok()?;
    start
        .ancestors()
        .find(|d| d.join(FILE).is_file())
        .map(Path::to_path_buf)
}

pub fn load(dir: &Path) -> Result<Manifest> {
    let path = dir.join(FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::new(
                Code::ManifestNotFound,
                msg::manifest_not_found(&dir.display().to_string()),
            )
        } else {
            Error::io(&path, &e)
        }
    })?;
    parse(&text, dir)
}

fn invalid(dir: &Path, reason: String) -> Error {
    Error::new(
        Code::InvalidManifest,
        msg::invalid_manifest(&dir.join(FILE).display().to_string(), &reason),
    )
}

fn string(t: &dyn TableLike, key: &str, dir: &Path) -> Result<Option<String>> {
    match t.get(key) {
        None => Ok(None),
        Some(i) => i
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| invalid(dir, msg::reason_not_string(key))),
    }
}

pub fn parse(text: &str, dir: &Path) -> Result<Manifest> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| invalid(dir, msg::reason_toml(e.message())))?;
    let pkg = doc
        .get("package")
        .and_then(Item::as_table_like)
        .ok_or_else(|| invalid(dir, msg::reason_missing("package")))?;
    let name =
        string(pkg, "name", dir)?.ok_or_else(|| invalid(dir, msg::reason_missing("name")))?;
    if !valid_name(&name) {
        return Err(invalid(dir, msg::reason_bad_name(&name)));
    }
    let version_text =
        string(pkg, "version", dir)?.ok_or_else(|| invalid(dir, msg::reason_missing("version")))?;
    let version = Version::parse(&version_text)
        .map_err(|_| invalid(dir, msg::reason_bad_version(&version_text)))?;
    let qxide = match string(pkg, "qxide", dir)? {
        None => None,
        Some(r) => Some(
            VersionReq::parse(&r).map_err(|_| invalid(dir, msg::reason_bad_req("qxide", &r)))?,
        ),
    };
    let authors = match pkg.get("authors") {
        None => Vec::new(),
        Some(i) => i
            .as_array()
            .and_then(|a| {
                a.iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| invalid(dir, msg::reason_not_string_array("authors")))?,
    };
    let mut dependencies = BTreeMap::new();
    if let Some(item) = doc.get("dependencies") {
        let deps = item
            .as_table_like()
            .ok_or_else(|| invalid(dir, msg::reason_not_table("dependencies")))?;
        for (dep, value) in deps.iter() {
            if !valid_name(dep) {
                return Err(invalid(dir, msg::reason_bad_name(dep)));
            }
            dependencies.insert(dep.to_string(), parse_dep(dep, value, dir)?);
        }
    }
    Ok(Manifest {
        dir: dir.to_path_buf(),
        name,
        version,
        description: string(pkg, "description", dir)?,
        license: string(pkg, "license", dir)?,
        authors,
        repository: string(pkg, "repository", dir)?,
        qxide,
        dependencies,
    })
}

/// Whether a path dependency is relative on every platform: no leading `/` or `\` (a root, or a
/// UNC network path on Windows) and no `:` (a drive such as `C:`). A cloned project could otherwise
/// make `qxide` read any directory or network share (B-P13); `..` stays allowed for sibling
/// packages.
pub fn relative_dep_path(path: &str) -> bool {
    !path.starts_with(['/', '\\']) && !path.contains(':')
}

// `name = "0.1"` or `name = { version = "0.1", path = "../name" }`.
fn parse_dep(name: &str, value: &Item, dir: &Path) -> Result<Dep> {
    let req =
        |r: &str| VersionReq::parse(r).map_err(|_| invalid(dir, msg::reason_bad_req(name, r)));
    if let Some(r) = value.as_str() {
        return Ok(Dep {
            req: Some(req(r)?),
            path: None,
        });
    }
    let t = value
        .as_table_like()
        .ok_or_else(|| invalid(dir, msg::reason_bad_dep(name)))?;
    let version = string(t, "version", dir)?;
    let path = string(t, "path", dir)?;
    if version.is_none() && path.is_none() {
        return Err(invalid(dir, msg::reason_bad_dep(name)));
    }
    if let Some(p) = path.as_deref().filter(|p| !relative_dep_path(p)) {
        return Err(invalid(dir, msg::reason_absolute_path(name, p)));
    }
    Ok(Dep {
        req: version.as_deref().map(req).transpose()?,
        path,
    })
}

fn edit<T>(dir: &Path, f: impl FnOnce(&mut DocumentMut) -> T) -> Result<T> {
    let path = dir.join(FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, &e))?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| invalid(dir, msg::reason_toml(e.message())))?;
    let out = f(&mut doc);
    std::fs::write(&path, doc.to_string()).map_err(|e| Error::io(&path, &e))?;
    Ok(out)
}

/// Adds or replaces `[dependencies] name`: a version string, or an inline table with `path`.
pub fn add_dependency(dir: &Path, name: &str, req: Option<&str>, path: Option<&str>) -> Result<()> {
    edit(dir, |doc| {
        if doc.get("dependencies").is_none() {
            doc["dependencies"] = toml_edit::table();
        }
        let value = match (req, path) {
            (Some(r), None) => toml_edit::value(r),
            (r, p) => {
                let mut t = InlineTable::new();
                if let Some(r) = r {
                    t.insert("version", r.into());
                }
                if let Some(p) = p {
                    t.insert("path", p.into());
                }
                toml_edit::value(t)
            }
        };
        doc["dependencies"][name] = value;
    })
}

/// Removes `[dependencies] name`; `false` if it was not there.
pub fn remove_dependency(dir: &Path, name: &str) -> Result<bool> {
    edit(dir, |doc| {
        doc.get_mut("dependencies")
            .and_then(Item::as_table_like_mut)
            .is_some_and(|t| t.remove(name).is_some())
    })
}
