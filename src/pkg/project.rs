// Connects a project to `qxide run`/`check`/`qasm`: finds `qxide.toml`, checks the lock file,
// and maps every locked package to its source directory for `import` resolution. Never touches
// the network; missing packages are reported with a hint to run `qxide install`.

use std::path::{Path, PathBuf};

use crate::lang::{PackageRoot, Project};

use super::cache::{registry_id, Home};
use super::index::Registry;
use super::lock::{self, Lock, Source};
use super::manifest::{self, Manifest};
use super::resolve::{self, Resolver};
use super::{archive, import_name, language_version, msg, Code, Error, Result};

/// `src/main.qx` of the project around the current directory, for `qxide run` without a file.
pub fn default_entry() -> Result<PathBuf> {
    let cwd = std::env::current_dir().map_err(|e| Error::io(Path::new("."), &e))?;
    let dir = manifest::find(&cwd).ok_or_else(|| {
        Error::new(
            Code::ManifestNotFound,
            msg::manifest_not_found(&cwd.display().to_string()),
        )
    })?;
    let entry = dir.join("src").join("main.qx");
    // Shown in diagnostics, so keep it relative to where the user is.
    Ok(entry
        .strip_prefix(&cwd)
        .map(Path::to_path_buf)
        .unwrap_or(entry))
}

fn check_language(m: &Manifest) -> Result<()> {
    let lang = language_version();
    match &m.qxide {
        Some(req) if !req.matches(&lang) => Err(Error::new(
            Code::LanguageVersion,
            msg::needs_language(&m.name, &req.to_string(), &lang.to_string()),
        )),
        _ => Ok(()),
    }
}

/// Directory of a locked package: unpacked in the cache, or a path dependency.
fn package_dir(root: &Manifest, p: &lock::LockedPackage, home: &Home) -> Result<PathBuf> {
    let not_installed = || {
        Error::new(
            Code::NotInstalled,
            msg::not_installed(&p.name, &p.version.to_string()),
        )
    };
    match &p.source {
        Source::Root => Ok(root.dir.clone()),
        Source::Path(rel) => Ok(root.dir.join(rel)),
        Source::Registry(url) => {
            let cksum = p.checksum.as_deref().unwrap_or_default();
            home.installed(&registry_id(url), &p.name, &p.version, cksum)
                .ok_or_else(not_installed)
        }
    }
}

/// Builds the import roots for `entry`, or `None` when it is not inside a project.
pub fn for_entry(entry: &Path) -> Result<Option<Project>> {
    let entry = std::path::absolute(entry).map_err(|e| Error::io(entry, &e))?;
    let Some(dir) = entry.parent().and_then(manifest::find) else {
        return Ok(None);
    };
    let root = manifest::load(&dir)?;
    check_language(&root)?;
    let lock = if root.dependencies.is_empty() {
        resolve::prune(&Lock::default(), &root)
    } else {
        let l = lock::load(&root.dir)?
            .ok_or_else(|| Error::new(Code::NotInstalled, msg::no_lock(&root.name)))?;
        // Any registry URL in the lock is fine here; only the manifest must still match.
        let url = l
            .packages
            .iter()
            .find_map(|p| match &p.source {
                Source::Registry(u) => Some(u.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if !resolve::is_fresh(&root, &l, &url) {
            return Err(Error::new(Code::LockOutdated, msg::lock_outdated()));
        }
        l
    };
    let home = Home::from_env()?;
    let mut packages: Vec<(String, PathBuf, Vec<String>)> = Vec::new();
    let order: Vec<&lock::LockedPackage> = std::iter::once(lock.root().expect("lock has a root"))
        .chain(lock.packages.iter().filter(|p| p.source != Source::Root))
        .collect();
    for p in &order {
        let pdir = package_dir(&root, p, &home)?;
        if p.source != Source::Root {
            check_language(&manifest::load(&pdir)?)?;
        }
        packages.push((p.name.clone(), pdir, p.dependencies.clone()));
    }
    let index_of = |n: &str| packages.iter().position(|(name, _, _)| name == n);
    let roots = packages
        .iter()
        .map(|(name, pdir, deps)| PackageRoot {
            name: import_name(name),
            src_dir: relative_to_cwd(pdir.join("src")),
            deps: deps.iter().filter_map(|d| index_of(d)).collect(),
        })
        .collect();
    Ok(Some(Project {
        packages: roots,
        entry_package: 0,
        entry_module: entry_module(&root.dir.join("src"), &entry),
    }))
}

// Module file names appear in diagnostics; project files read better relative to the cwd.
fn relative_to_cwd(p: PathBuf) -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| p.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or(p)
}

// `src/lib.qx` -> `[]`, `src/a/b.qx` -> `[a, b]`; `src/main.qx` and files elsewhere are not modules.
fn entry_module(src: &Path, entry: &Path) -> Option<Vec<String>> {
    let rel = entry.strip_prefix(src).ok()?;
    if rel.extension()? != "qx" {
        return None;
    }
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parts.as_slice() {
        [one] if one == "main" => None,
        [one] if one == "lib" => Some(Vec::new()),
        _ => {
            if parts.is_empty() {
                return None;
            }
            parts.shrink_to_fit();
            Some(parts)
        }
    }
}

/// What `install` did, for the status lines.
pub struct Installed {
    pub locked: Option<usize>,
    pub fetched: Vec<(String, semver::Version)>,
}

/// Brings `qxide.lock` up to date (unless `locked`) and makes sure every registry package is
/// downloaded, verified against its locked checksum and unpacked.
pub fn install(
    root: &Manifest,
    registry: &mut Registry,
    locked: bool,
) -> Result<(Lock, Installed)> {
    check_language(root)?;
    let previous = lock::load(&root.dir)?;
    let mut report = Installed {
        locked: None,
        fetched: Vec::new(),
    };
    let lock = match previous {
        Some(l) if resolve::is_fresh(root, &l, &registry.url) => l,
        _ if locked => return Err(Error::new(Code::LockOutdated, msg::lock_outdated_locked())),
        previous => {
            let url = registry.url.clone();
            let lock = Resolver::new(root, previous.as_ref(), registry, &url, language_version())
                .resolve()?;
            if previous.as_ref() != Some(&lock) {
                lock.save(&root.dir)?;
                report.locked = Some(lock.packages.len() - 1);
            }
            lock
        }
    };
    fetch_locked(&lock, registry, &mut report)?;
    Ok((lock, report))
}

fn fetch_locked(lock: &Lock, registry: &Registry, report: &mut Installed) -> Result<()> {
    let home = &registry.home;
    for p in &lock.packages {
        let Source::Registry(url) = &p.source else {
            continue;
        };
        let cksum = p.checksum.as_deref().unwrap_or_default();
        let id = registry_id(url);
        if home.installed(&id, &p.name, &p.version, cksum).is_some() {
            continue;
        }
        let bytes = match home.cached_archive(&id, &p.name, &p.version, cksum) {
            Some(b) => b,
            None => {
                let entries = registry.entries(&p.name)?;
                let e = entries
                    .iter()
                    .find(|e| e.vers == p.version)
                    .ok_or_else(|| {
                        Error::new(
                            Code::NoMatchingVersion,
                            msg::locked_version_missing(&p.name, &p.version.to_string()),
                        )
                    })?;
                // The index and the lock must agree before anything is downloaded.
                if e.cksum != cksum {
                    return Err(Error::new(
                        Code::ChecksumMismatch,
                        msg::index_checksum_changed(&p.name, &p.version.to_string()),
                    )
                    .note(msg::note_expected_checksum(cksum))
                    .note(msg::note_index_checksum(&e.cksum)));
                }
                let bytes = registry.download(e)?;
                if bytes.len() as u64 > archive::MAX_ARCHIVE {
                    return Err(Error::new(
                        Code::BadArchive,
                        msg::bad_archive(&msg::reason_too_large(archive::MAX_ARCHIVE)),
                    ));
                }
                bytes
            }
        };
        home.install(&id, &p.name, &p.version, cksum, &bytes)?;
        report.fetched.push((p.name.clone(), p.version.clone()));
    }
    Ok(())
}
