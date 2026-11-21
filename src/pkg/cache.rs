// The local cache under `~/.qxide` (or `$QXIDE_HOME`):
//   registry/index/<registry id>/...           copies of fetched index files, for `--offline`
//   registry/cache/<registry id>/<n>-<v>.tar.gz downloaded archives
//   registry/src/<registry id>/<n>-<v>/         unpacked packages, read by run/check/qasm
//   registry/tmp/                               downloads in progress

use std::path::{Path, PathBuf};

use super::{archive, msg, sha256_hex, Code, Error, Result};

// Written last when a package is unpacked; holds the archive checksum.
const MARKER: &str = ".qxide-checksum";

#[derive(Debug, Clone)]
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    /// `$QXIDE_HOME`, else `.qxide` in the user's home directory.
    pub fn from_env() -> Result<Home> {
        if let Some(h) = std::env::var_os("QXIDE_HOME").filter(|h| !h.is_empty()) {
            return Ok(Home {
                root: PathBuf::from(h),
            });
        }
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
            .ok_or_else(|| Error::new(Code::Io, msg::no_home()))?;
        Ok(Home {
            root: PathBuf::from(home).join(".qxide"),
        })
    }

    fn registry(&self) -> PathBuf {
        self.root.join("registry")
    }

    /// Directory under the qxide home where `curl` writes in-progress downloads; private to the
    /// current user, unlike the shared system temporary directory.
    pub fn download_dir(&self) -> PathBuf {
        self.registry().join("tmp")
    }

    pub fn index_dir(&self, id: &str) -> PathBuf {
        self.registry().join("index").join(id)
    }

    pub fn archive_file(&self, id: &str, name: &str, version: &semver::Version) -> PathBuf {
        self.registry()
            .join("cache")
            .join(id)
            .join(format!("{}.tar.gz", archive::prefix(name, version)))
    }

    /// Directory of an unpacked package (it holds `qxide.toml` and `src/`).
    pub fn src_dir(&self, id: &str, name: &str, version: &semver::Version) -> PathBuf {
        self.registry()
            .join("src")
            .join(id)
            .join(archive::prefix(name, version))
    }

    /// The unpacked package, if it is complete and its files are still exactly those of the cached
    /// archive with this checksum.
    pub fn installed(
        &self,
        id: &str,
        name: &str,
        version: &semver::Version,
        cksum: &str,
    ) -> Option<PathBuf> {
        let dir = self.src_dir(id, name, version);
        let marker = std::fs::read_to_string(dir.join(MARKER)).ok()?;
        if marker.trim() != cksum {
            return None;
        }
        // B-P10: the marker can be rewritten along with the files, the lock's checksum cannot, so
        // the files are compared with the archive that matches it, unpacked afresh.
        let bytes = self.cached_archive(id, name, version, cksum)?;
        let root = archive::prefix(name, version);
        let fresh = dir
            .parent()?
            .join(format!(".verify-{root}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fresh);
        let same = archive::unpack(&bytes, &root, &fresh).is_ok() && same_files(&dir, &fresh);
        let _ = std::fs::remove_dir_all(&fresh);
        same.then_some(dir)
    }

    /// A cached archive whose checksum still matches.
    pub fn cached_archive(
        &self,
        id: &str,
        name: &str,
        version: &semver::Version,
        cksum: &str,
    ) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.archive_file(id, name, version)).ok()?;
        (sha256_hex(&bytes) == cksum).then_some(bytes)
    }

    /// Verifies `bytes` against `cksum`, keeps a copy of the archive and unpacks it.
    pub fn install(
        &self,
        id: &str,
        name: &str,
        version: &semver::Version,
        cksum: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        let actual = sha256_hex(bytes);
        if actual != cksum {
            return Err(Error::new(
                Code::ChecksumMismatch,
                msg::checksum_mismatch(name, &version.to_string()),
            )
            .note(msg::note_expected_checksum(cksum))
            .note(msg::note_actual_checksum(&actual)));
        }
        let file = self.archive_file(id, name, version);
        write_atomic(&file, bytes)?;
        let dest = self.src_dir(id, name, version);
        let parent = dest.parent().expect("src dir has a parent");
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, &e))?;
        // Unpack next to the destination, then rename, so a crash never leaves a half package.
        let tmp = parent.join(format!(
            ".tmp-{}-{}",
            archive::prefix(name, version),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let unpacked =
            archive::unpack(bytes, &archive::prefix(name, version), &tmp).and_then(|()| {
                let m = tmp.join(MARKER);
                std::fs::write(&m, cksum).map_err(|e| Error::io(&m, &e))
            });
        if let Err(e) = unpacked {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest).map_err(|e| Error::io(&dest, &e))?;
        Ok(dest)
    }
}

/// Whether `installed` (apart from its marker) and `fresh` hold the same regular files with the
/// same contents. Symbolic links and other special files never match.
fn same_files(installed: &Path, fresh: &Path) -> bool {
    fn files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else { return false };
            let Ok(kind) = entry.file_type() else {
                return false;
            };
            let path = entry.path();
            if kind.is_dir() {
                if !files(root, &path, out) {
                    return false;
                }
            } else if kind.is_file() {
                let rel = path.strip_prefix(root).expect("walk stays below root");
                if rel != Path::new(MARKER) {
                    out.push(rel.to_path_buf());
                }
            } else {
                return false;
            }
        }
        true
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    if !files(installed, installed, &mut a) || !files(fresh, fresh, &mut b) {
        return false;
    }
    a.sort();
    b.sort();
    a == b
        && a.iter().all(|rel| {
            matches!(
                (std::fs::read(installed.join(rel)), std::fs::read(fresh.join(rel))),
                (Ok(x), Ok(y)) if x == y
            )
        })
}

/// Writes through a temporary file and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().expect("file has a parent");
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, &e))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| Error::io(&tmp, &e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, &e))
}

/// Short, filesystem-safe id of a registry URL: readable host part plus a hash.
pub fn registry_id(url: &str) -> String {
    let host: String = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    let host = if host.is_empty() {
        "local".into()
    } else {
        host
    };
    format!("{host}-{}", &sha256_hex(url.as_bytes())[..16])
}
