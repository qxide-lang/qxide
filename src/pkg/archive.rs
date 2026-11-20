// Package archives: deterministic `.tar.gz` files with every path under `<name>-<version>/`.
// Unpacking trusts nothing: the caller verifies the sha256 first, and every entry is checked for
// type, path and size before anything is written.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::{Compression, GzBuilder};

use super::manifest::{Manifest, FILE as MANIFEST};
use super::{msg, Code, Error, Result};

/// Largest archive accepted for download or publishing.
pub const MAX_ARCHIVE: u64 = 10 << 20;
/// Largest total unpacked size and file count of one package.
pub const MAX_UNPACKED: u64 = 50 << 20;
pub const MAX_FILES: usize = 2000;

/// The files a package ships, as (path inside the package with `/`, file on disk), sorted:
/// `qxide.toml`, every `.qx` file under `src/`, and top-level `README*` / `LICENSE*` files.
pub fn package_files(m: &Manifest) -> Result<Vec<(String, PathBuf)>> {
    let mut files = vec![(MANIFEST.to_string(), m.dir.join(MANIFEST))];
    let top = std::fs::read_dir(&m.dir).map_err(|e| Error::io(&m.dir, &e))?;
    for entry in top {
        let entry = entry.map_err(|e| Error::io(&m.dir, &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let upper = name.to_ascii_uppercase();
        // `DirEntry::file_type` does not follow symlinks: a linked README could ship any file.
        let regular = entry.file_type().is_ok_and(|t| t.is_file());
        if regular && (upper.starts_with("README") || upper.starts_with("LICENSE")) {
            files.push((name, entry.path()));
        }
    }
    walk_qx(&m.dir.join("src"), "src", &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn walk_qx(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(dir, &e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let ty = entry.file_type().map_err(|e| Error::io(&path, &e))?;
        if ty.is_dir() {
            walk_qx(&path, &format!("{rel}/{name}"), out)?;
        } else if ty.is_file() && name.ends_with(".qx") {
            out.push((format!("{rel}/{name}"), path));
        }
    }
    Ok(())
}

pub fn prefix(name: &str, version: &semver::Version) -> String {
    format!("{name}-{version}")
}

/// Builds the archive: fixed entry order, mtime 0, uid/gid 0, mode 0644, gzip mtime 0.
pub fn pack(m: &Manifest) -> Result<(Vec<u8>, Vec<String>)> {
    let files = package_files(m)?;
    let root = prefix(&m.name, &m.version);
    let gz = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    let mut tar = tar::Builder::new(gz);
    for (rel, path) in &files {
        let mut data = std::fs::read(path).map_err(|e| Error::io(path, &e))?;
        // Archives are byte-for-byte the same on every OS: text files (every shipped file that is
        // valid UTF-8) use `\n`.
        if let Ok(text) = std::str::from_utf8(&data) {
            data = text.replace("\r\n", "\n").into_bytes();
        }
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(0);
        h.set_uid(0);
        h.set_gid(0);
        h.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut h, format!("{root}/{rel}"), data.as_slice())
            .map_err(|e| Error::io(path, &e))?;
    }
    let gz = tar.into_inner().map_err(|e| Error::io(&m.dir, &e))?;
    let bytes = gz.finish().map_err(|e| Error::io(&m.dir, &e))?;
    Ok((bytes, files.into_iter().map(|(r, _)| r).collect()))
}

fn bad(reason: String) -> Error {
    Error::new(Code::BadArchive, msg::bad_archive(&reason))
}

// Device names Windows reserves in every directory, with or without an extension (`nul.qx`).
const RESERVED: [&str; 30] = [
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
];

/// Whether `s` names the same file on every OS: not empty, no character Windows rejects or
/// strips (control characters, `<>:"/\|?*`, a trailing `.` or space) and no device name.
pub fn portable_component(s: &str) -> bool {
    if s.is_empty() || s.ends_with(['.', ' ']) {
        return false;
    }
    let invalid = |c: char| {
        c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
    };
    if s.chars().any(invalid) {
        return false;
    }
    let stem = s.split('.').next().unwrap_or(s).trim_end_matches(' ');
    !RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
}

/// Files of one package compared the way case-insensitive file systems compare names: no two may
/// fold to the same path, and no file may also be a parent directory of another.
#[derive(Default)]
pub struct PathSet {
    files: HashSet<Vec<String>>,
    dirs: HashSet<Vec<String>>,
}

impl PathSet {
    /// Adds a file given by its components; `false` if it collides with an earlier one.
    pub fn insert(&mut self, parts: &[&str]) -> bool {
        let folded: Vec<String> = parts
            .iter()
            .map(|p| p.to_lowercase().to_uppercase())
            .collect();
        if self.files.contains(&folded) || self.dirs.contains(&folded) {
            return false;
        }
        for k in 1..folded.len() {
            if self.files.contains(&folded[..k]) {
                return false;
            }
        }
        for k in 1..folded.len() {
            self.dirs.insert(folded[..k].to_vec());
        }
        self.files.insert(folded);
        true
    }
}

/// Checks package paths (`/`-separated, relative to the package root) the way `unpack` will.
pub fn check_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<(), String> {
    let mut set = PathSet::default();
    for p in paths {
        let parts: Vec<&str> = p.split('/').collect();
        if !parts.iter().all(|c| portable_component(c)) {
            return Err(msg::reason_bad_path(p));
        }
        if !set.insert(&parts) {
            return Err(msg::reason_duplicate_entry(p));
        }
    }
    Ok(())
}

// `<root>/a/b.qx` -> `a/b.qx`; rejects absolute paths, `..`, drive prefixes and names that are
// not portable (see `portable_component`).
fn entry_path(path: &Path, root: &str) -> Result<PathBuf> {
    let shown = path.display().to_string();
    let mut comps = path.components();
    match comps.next() {
        Some(Component::Normal(c)) if c == root => {}
        _ => return Err(bad(msg::reason_outside_root(&shown, root))),
    }
    let mut rel = PathBuf::new();
    for c in comps {
        match c {
            Component::Normal(s) => {
                let s = s
                    .to_str()
                    .ok_or_else(|| bad(msg::reason_bad_path(&shown)))?;
                if !portable_component(s) {
                    return Err(bad(msg::reason_bad_path(&shown)));
                }
                rel.push(s);
            }
            Component::CurDir => {}
            _ => return Err(bad(msg::reason_bad_path(&shown))),
        }
    }
    Ok(rel)
}

/// Unpacks a verified archive into `dest` (which must not exist yet), keeping only regular files
/// and directories under `<root>/`. Every entry is checked before the first file is written.
pub fn unpack(bytes: &[u8], root: &str, dest: &Path) -> Result<()> {
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(bad(msg::reason_too_large(MAX_ARCHIVE)));
    }
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let entries = archive
        .entries()
        .map_err(|e| bad(msg::reason_corrupt(&e.to_string())))?;
    let (mut total, mut count) = (0u64, 0usize);
    let mut saw_manifest = false;
    let mut seen = PathSet::default();
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|e| bad(msg::reason_corrupt(&e.to_string())))?;
        let path = entry
            .path()
            .map_err(|e| bad(msg::reason_corrupt(&e.to_string())))?
            .into_owned();
        let rel = entry_path(&path, root)?;
        let kind = entry.header().entry_type();
        count += 1;
        if count > MAX_FILES {
            return Err(bad(msg::reason_too_many_files(MAX_FILES)));
        }
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(bad(msg::reason_not_regular(&path.display().to_string())));
        }
        if rel.as_os_str().is_empty() {
            return Err(bad(msg::reason_bad_path(&path.display().to_string())));
        }
        // `entry_path` only keeps UTF-8 components.
        let parts: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
        if !seen.insert(&parts) {
            return Err(bad(msg::reason_duplicate_entry(
                &path.display().to_string(),
            )));
        }
        let size = entry.header().size().unwrap_or(u64::MAX);
        total = total.saturating_add(size);
        if total > MAX_UNPACKED {
            return Err(bad(msg::reason_too_large(MAX_UNPACKED)));
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(size)
            .read_to_end(&mut data)
            .map_err(|e| bad(msg::reason_corrupt(&e.to_string())))?;
        saw_manifest |= rel == Path::new(MANIFEST);
        files.push((rel, data));
    }
    if !saw_manifest {
        return Err(bad(msg::reason_missing(MANIFEST)));
    }
    for (rel, data) in files {
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, &e))?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| Error::io(&out, &e))?;
        f.write_all(&data).map_err(|e| Error::io(&out, &e))?;
    }
    Ok(())
}
