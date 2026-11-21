// The registry index: one file per package with one JSON object per published version, laid
// out like Cargo's sparse index. A registry is a directory, a `file://` URL or an `https://` URL
// (fetched with the system `curl`); the same layout can later be served by a registry server.
//
//   config.json                    {"dl": "packages/{prefix}/{name}/{name}-{version}.tar.gz"}
//   index/<prefix>/<name>          JSON lines, see `Entry`
//   packages/<prefix>/<name>/...   archives (GitHub-hosted registry only)
//   search.json                    {"packages": [{"name", "version", "description"}]}

use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use serde_json::{json, Map, Value};

use super::cache::{self, Home};
use super::{lock::is_sha256_hex, msg, valid_name, Code, Error, Result};

/// The registry used when neither `--registry` nor `QXIDE_REGISTRY` is set.
pub const DEFAULT_REGISTRY: &str = "https://raw.githubusercontent.com/qxide-lang/registry/main/";
pub const DEFAULT_DL: &str = "packages/{prefix}/{name}/{name}-{version}.tar.gz";
/// Index entries this client understands.
pub const SCHEMA: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDep {
    pub name: String,
    pub req: VersionReq,
}

/// One published version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub vers: Version,
    pub deps: Vec<IndexDep>,
    /// Lowercase hex sha256 of the archive.
    pub cksum: String,
    /// Required `qxide` language version.
    pub qxide: Option<VersionReq>,
    pub description: Option<String>,
    /// Download URL; when absent, `config.json`'s `dl` template applies.
    pub dl: Option<String>,
    pub yanked: bool,
}

impl Entry {
    pub fn to_json(&self) -> String {
        let deps: Vec<Value> = self
            .deps
            .iter()
            .map(|d| json!({"name": d.name, "req": d.req.to_string()}))
            .collect();
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name));
        m.insert("vers".into(), json!(self.vers.to_string()));
        m.insert("deps".into(), Value::Array(deps));
        m.insert("cksum".into(), json!(self.cksum));
        if let Some(q) = &self.qxide {
            m.insert("qxide".into(), json!(q.to_string()));
        }
        if let Some(d) = &self.description {
            m.insert("description".into(), json!(d));
        }
        if let Some(dl) = &self.dl {
            m.insert("dl".into(), json!(dl));
        }
        m.insert("yanked".into(), json!(self.yanked));
        m.insert("v".into(), json!(SCHEMA));
        Value::Object(m).to_string()
    }

    /// Parses one index line; `Err` carries the reason.
    pub fn parse(line: &str) -> std::result::Result<Entry, String> {
        let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let str_field = |k: &str| v.get(k).and_then(Value::as_str);
        let name = str_field("name").ok_or_else(|| msg::reason_missing("name"))?;
        if !valid_name(name) {
            return Err(msg::reason_bad_name(name));
        }
        let vtext = str_field("vers").ok_or_else(|| msg::reason_missing("vers"))?;
        let vers = Version::parse(vtext).map_err(|_| msg::reason_bad_version(vtext))?;
        // Build metadata does not change precedence, so `1.0.0+x` could stand in for `1.0.0`.
        if !vers.build.is_empty() {
            return Err(msg::reason_bad_version(vtext));
        }
        let cksum = str_field("cksum").ok_or_else(|| msg::reason_missing("cksum"))?;
        if !is_sha256_hex(cksum) {
            return Err(msg::reason_bad_checksum(cksum));
        }
        let mut deps = Vec::new();
        let dep_values = match v.get("deps") {
            None => &[][..],
            Some(d) => d
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| msg::reason_missing("deps"))?,
        };
        for d in dep_values {
            let dn = d.get("name").and_then(Value::as_str);
            let dr = d.get("req").and_then(Value::as_str);
            let (Some(dn), Some(dr)) = (dn, dr) else {
                return Err(msg::reason_missing("deps"));
            };
            if !valid_name(dn) {
                return Err(msg::reason_bad_name(dn));
            }
            let req = VersionReq::parse(dr).map_err(|_| msg::reason_bad_req(dn, dr))?;
            deps.push(IndexDep {
                name: dn.to_string(),
                req,
            });
        }
        // Optional string fields: absent is fine, any other type is an error, never ignored.
        let opt_str = |k: &str| match v.get(k) {
            None => Ok(None),
            Some(x) => x
                .as_str()
                .map(Some)
                .ok_or_else(|| msg::reason_not_string(k)),
        };
        let qxide = match opt_str("qxide")? {
            None => None,
            Some(q) => Some(VersionReq::parse(q).map_err(|_| msg::reason_bad_req("qxide", q))?),
        };
        Ok(Entry {
            name: name.to_string(),
            vers,
            deps,
            cksum: cksum.to_string(),
            qxide,
            description: opt_str("description")?.map(str::to_string),
            dl: opt_str("dl")?.map(str::to_string),
            yanked: v.get("yanked").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// Cargo's index layout: `1/a`, `2/ab`, `3/a/abc`, `ab/cd/abcd...`.
pub fn prefix(name: &str) -> String {
    match name.len() {
        1 => "1".into(),
        2 => "2".into(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    }
}

pub fn index_path(name: &str) -> String {
    format!("index/{}/{name}", prefix(name))
}

#[derive(Debug, Clone)]
enum Location {
    Dir(PathBuf),
    Http(String),
}

/// One registry plus the local cache it is mirrored into.
pub struct Registry {
    /// Canonical URL, recorded in `qxide.lock`.
    pub url: String,
    pub id: String,
    loc: Location,
    pub offline: bool,
    pub home: Home,
}

/// A row of `qxide search`.
#[derive(Debug, Clone)]
pub struct SearchRow {
    pub name: String,
    pub version: String,
    pub description: String,
}

impl Registry {
    /// `--registry`, else `$QXIDE_REGISTRY`, else the GitHub index.
    pub fn open(flag: Option<&str>, offline: bool, home: Home) -> Result<Registry> {
        let env = std::env::var("QXIDE_REGISTRY")
            .ok()
            .filter(|s| !s.is_empty());
        let raw = flag
            .map(str::to_string)
            .or(env)
            .unwrap_or_else(|| DEFAULT_REGISTRY.into());
        let (url, loc) = if let Some(rest) = raw.strip_prefix("https://") {
            let root = format!("https://{}/", rest.trim_end_matches('/'));
            (root.clone(), Location::Http(root))
        } else if raw.starts_with("http://") {
            return Err(Error::new(
                Code::RegistryUnavailable,
                msg::insecure_registry(&raw),
            ));
        } else {
            let path = file_url_path(&raw);
            let abs = std::path::absolute(&path).map_err(|e| Error::io(&path, &e))?;
            (file_url(&abs), Location::Dir(abs))
        };
        Ok(Registry {
            id: cache::registry_id(&url),
            url,
            loc,
            offline,
            home,
        })
    }

    pub fn is_default(&self) -> bool {
        self.url == DEFAULT_REGISTRY
    }

    /// Local directory of a directory registry (for `publish --registry-dir`-style writes).
    pub fn local_dir(&self) -> Option<&Path> {
        match &self.loc {
            Location::Dir(d) => Some(d),
            Location::Http(_) => None,
        }
    }

    // A file relative to the registry root; `None` if it does not exist there.
    fn fetch(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        match &self.loc {
            Location::Dir(d) => read_optional(&d.join(rel)),
            Location::Http(root) => {
                if self.offline {
                    return Err(Error::new(Code::Offline, msg::offline_uncached(rel)));
                }
                curl(&format!("{root}{rel}"), &self.home.download_dir())
            }
        }
    }

    // Index-like files are mirrored to the cache so `--offline` can read them later.
    fn fetch_cached(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        let cached = self.home.index_dir(&self.id).join(rel);
        if self.offline && matches!(self.loc, Location::Http(_)) {
            return match read_optional(&cached)? {
                Some(b) => Ok(Some(b)),
                None => Err(Error::new(Code::Offline, msg::offline_uncached(rel))),
            };
        }
        let fetched = self.fetch(rel)?;
        if let (Some(bytes), Location::Http(_)) = (&fetched, &self.loc) {
            cache::write_atomic(&cached, bytes)?;
        }
        Ok(fetched)
    }

    /// Returns every usable version of `name` in index order. Unusable lines (malformed, naming
    /// another package, or duplicating an earlier version) are skipped and never selected; only an
    /// index with no usable lines fails.
    pub fn entries(&self, name: &str) -> Result<Vec<Entry>> {
        let not_found = || {
            Error::new(
                Code::PackageNotFound,
                msg::package_not_found(name, &self.url),
            )
        };
        if !valid_name(name) {
            return Err(not_found());
        }
        let Some(bytes) = self.fetch_cached(&index_path(name))? else {
            return Err(not_found());
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut out: Vec<Entry> = Vec::new();
        let mut first_invalid = None;
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed = Entry::parse(line).and_then(|e| {
                if e.name == name {
                    Ok(e)
                } else {
                    Err(msg::reason_wrong_name(&e.name))
                }
            });
            match parsed {
                // The first line of a version wins, as published lines are immutable.
                Ok(e) if out.iter().any(|o| o.vers == e.vers) => {}
                Ok(e) => out.push(e),
                Err(reason) => {
                    first_invalid.get_or_insert_with(|| {
                        Error::new(Code::InvalidIndex, msg::invalid_index(name, i + 1, &reason))
                    });
                }
            }
        }
        if out.is_empty() {
            return Err(first_invalid.unwrap_or_else(not_found));
        }
        Ok(out)
    }

    fn dl_template(&self) -> Result<String> {
        let Some(bytes) = self.fetch_cached("config.json")? else {
            return Ok(DEFAULT_DL.into());
        };
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|e| Error::new(Code::InvalidIndex, msg::invalid_config(&e.to_string())))?;
        Ok(v.get("dl")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_DL)
            .to_string())
    }

    /// Where the archive of `e` is downloaded from, relative to the root unless absolute.
    pub fn dl_url(&self, e: &Entry) -> Result<String> {
        let template = match &e.dl {
            Some(dl) => dl.clone(),
            None => self.dl_template()?,
        };
        Ok(template
            .replace("{name}", &e.name)
            .replace("{version}", &e.vers.to_string())
            .replace("{prefix}", &prefix(&e.name))
            .replace("{sha256-checksum}", &e.cksum))
    }

    /// Downloads the archive of `e` (not yet verified).
    pub fn download(&self, e: &Entry) -> Result<Vec<u8>> {
        let url = self.dl_url(e)?;
        let missing = || {
            Error::new(
                Code::RegistryUnavailable,
                msg::archive_missing(&e.name, &e.vers.to_string(), &url),
            )
        };
        let bytes = if url.starts_with("https://") {
            if self.offline {
                return Err(Error::new(Code::Offline, msg::offline_uncached(&url)));
            }
            curl(&url, &self.home.download_dir())?
        } else if url.starts_with("file://") {
            read_optional(&file_url_path(&url))?
        } else if url.contains("://") {
            return Err(Error::new(
                Code::RegistryUnavailable,
                msg::insecure_registry(&url),
            ));
        } else {
            self.fetch(&url)?
        };
        bytes.ok_or_else(missing)
    }

    /// Every package with its newest non-yanked version, from `search.json` when the registry
    /// has one, else (directory registries only) by reading the whole index.
    pub fn search_rows(&self) -> Result<Vec<SearchRow>> {
        if let Some(bytes) = self.fetch_cached("search.json")? {
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| Error::new(Code::InvalidIndex, msg::invalid_config(&e.to_string())))?;
            let rows = v
                .get("packages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    Some(SearchRow {
                        name: p.get("name")?.as_str()?.to_string(),
                        version: p.get("version")?.as_str()?.to_string(),
                        description: p
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect();
            return Ok(rows);
        }
        let Location::Dir(dir) = &self.loc else {
            return Ok(Vec::new());
        };
        let mut rows = Vec::new();
        let mut names = Vec::new();
        collect_index_names(&dir.join("index"), &mut names)?;
        names.sort();
        for name in names {
            let entries = self.entries(&name)?;
            if let Some(e) = entries
                .iter()
                .filter(|e| !e.yanked)
                .max_by(|a, b| a.vers.cmp(&b.vers))
            {
                rows.push(SearchRow {
                    name: e.name.clone(),
                    version: e.vers.to_string(),
                    description: e.description.clone().unwrap_or_default(),
                });
            }
        }
        Ok(rows)
    }
}

fn collect_index_names(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(dir, &e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, &e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect_index_names(&path, out)?;
        } else if valid_name(&name) {
            out.push(name);
        }
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, &e)),
    }
}

/// `file:///C:/x` -> `C:/x`, `file:///srv/x` -> `/srv/x`; other strings are plain paths.
pub fn file_url_path(s: &str) -> PathBuf {
    match s.strip_prefix("file://") {
        None => PathBuf::from(s),
        Some(rest) => {
            let b = rest.as_bytes();
            // `/C:/...`: a Windows drive letter after the URL's leading slash.
            if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
                PathBuf::from(&rest[1..])
            } else {
                PathBuf::from(rest)
            }
        }
    }
}

pub fn file_url(path: &Path) -> String {
    let s = path.display().to_string().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{}/", s.trim_end_matches('/'))
    } else {
        format!("file:///{}/", s.trim_end_matches('/'))
    }
}

/// Outcome of a completed `curl` run, determined from its exit status and printed `%{http_code}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    Body,
    NotFound,
    Failed,
}

/// Treats `200` and `404` as valid only when `curl` exited cleanly; a mid-body disconnect still
/// prints `200` but exits non-zero (`18`) with a truncated file, so it maps to a failure.
pub fn fetched(exit_ok: bool, http_code: &str) -> Fetched {
    match http_code {
        "404" if exit_ok => Fetched::NotFound,
        "200" if exit_ok => Fetched::Body,
        _ => Fetched::Failed,
    }
}

/// Downloads `url` via system `curl` into a new file under `dir`, returning `None` on 404 and an
/// error on any other failure.
fn curl(url: &str, dir: &Path) -> Result<Option<Vec<u8>>> {
    let unavailable =
        |reason: String| Error::new(Code::RegistryUnavailable, msg::fetch_failed(url, &reason));
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, &e))?;
    let tmp = dir.join(format!(
        "dl-{}-{}",
        std::process::id(),
        &super::sha256_hex(url.as_bytes())[..12]
    ));
    // Created fresh and exclusively, so curl never writes through a file planted beforehand.
    let _ = std::fs::remove_file(&tmp);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| Error::io(&tmp, &e))?;
    let out = super::command("curl").and_then(|mut curl| {
        curl.args([
            "--silent",
            "--show-error",
            "--location",
            "--globoff",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "120",
            "--max-filesize",
        ])
        .arg(super::archive::MAX_ARCHIVE.to_string())
        .arg("--output")
        .arg(&tmp)
        .args(["--write-out", "%{http_code}"])
        .arg(url)
        .output()
    });
    let out = match out {
        Ok(out) => out,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(unavailable(msg::reason_curl_missing(&e.to_string())));
        }
    };
    let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let body = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    match fetched(out.status.success(), &status) {
        Fetched::Body => body.map(Some).map_err(|e| Error::io(&tmp, &e)),
        Fetched::NotFound => Ok(None),
        Fetched::Failed => Err(unavailable(
            String::from_utf8_lossy(&out.stderr).trim().to_string() + " " + &status,
        )),
    }
}
