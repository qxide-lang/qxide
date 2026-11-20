// Version resolution: one version per package name, the newest that satisfies every requirement
// on it, preferring what `qxide.lock` already pins. Repeats until the chosen set is stable
// because a newly chosen version can add or drop requirements on other packages.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use semver::{Version, VersionReq};

use super::index::Entry;
use super::lock::{Lock, LockedPackage, Source};
use super::manifest::{self, Manifest};
use super::{msg, Code, Error, Result};

/// Index lookups; `Registry` in the CLI, an in-memory map in tests.
pub trait Provider {
    fn entries(&mut self, name: &str) -> Result<Vec<Entry>>;
}

impl Provider for super::index::Registry {
    fn entries(&mut self, name: &str) -> Result<Vec<Entry>> {
        super::index::Registry::entries(self, name)
    }
}

#[derive(Debug, Clone)]
enum Choice {
    Registry(Entry),
    // Manifest and its path relative to the project root.
    Path(Manifest, String),
}

impl Choice {
    fn version(&self) -> &Version {
        match self {
            Choice::Registry(e) => &e.vers,
            Choice::Path(m, _) => &m.version,
        }
    }

    fn same(&self, other: &Choice) -> bool {
        match (self, other) {
            (Choice::Registry(a), Choice::Registry(b)) => a.vers == b.vers,
            (Choice::Path(_, a), Choice::Path(_, b)) => a == b,
            _ => false,
        }
    }
}

/// Joins `rel` onto `base` and removes `.` and `x/..` lexically, keeping `/` separators.
pub fn join_rel(base: &str, rel: &str) -> String {
    let joined = if base.is_empty() || Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        Path::new(base).join(rel)
    };
    let mut parts: Vec<String> = Vec::new();
    let mut prefix = String::new();
    for c in joined.components() {
        match c {
            Component::Prefix(p) => prefix = p.as_os_str().to_string_lossy().into_owned(),
            Component::RootDir => prefix.push('/'),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".into());
                }
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
        }
    }
    let body = parts.join("/");
    if body.is_empty() && prefix.is_empty() {
        ".".into()
    } else {
        format!("{prefix}{body}")
    }
}

// (dependency name, version requirement, path relative to the project root)
type DepEdge = (String, Option<VersionReq>, Option<String>);

struct Need {
    reqs: Vec<(VersionReq, String)>,
    // (path relative to the project root, requiring package)
    path: Option<(String, String)>,
}

pub struct Resolver<'a> {
    root: &'a Manifest,
    lock: Option<&'a Lock>,
    provider: &'a mut dyn Provider,
    registry_url: String,
    lang: Version,
    entries: HashMap<String, Vec<Entry>>,
    manifests: HashMap<String, Manifest>,
}

impl<'a> Resolver<'a> {
    pub fn new(
        root: &'a Manifest,
        lock: Option<&'a Lock>,
        provider: &'a mut dyn Provider,
        registry_url: &str,
        lang: Version,
    ) -> Self {
        Self {
            root,
            lock,
            provider,
            registry_url: registry_url.to_string(),
            lang,
            entries: HashMap::new(),
            manifests: HashMap::new(),
        }
    }

    fn entries(&mut self, name: &str) -> Result<Vec<Entry>> {
        if let Some(e) = self.entries.get(name) {
            return Ok(e.clone());
        }
        let e = self.provider.entries(name)?;
        self.entries.insert(name.to_string(), e.clone());
        Ok(e)
    }

    fn path_manifest(&mut self, rel: &str) -> Result<Manifest> {
        if let Some(m) = self.manifests.get(rel) {
            return Ok(m.clone());
        }
        let dir = self.root.dir.join(rel);
        let m = manifest::load(&dir).map_err(|e| {
            Error::new(Code::PackageNotFound, msg::path_dep_missing(rel)).note(e.message)
        })?;
        self.manifests.insert(rel.to_string(), m.clone());
        Ok(m)
    }

    // Requirements on every package reachable from the root through the current choices.
    fn needs(&self, chosen: &BTreeMap<String, Choice>) -> Result<BTreeMap<String, Need>> {
        let mut needs: BTreeMap<String, Need> = BTreeMap::new();
        let mut queue: Vec<(String, Vec<DepEdge>)> = Vec::new();
        let root_deps = self
            .root
            .dependencies
            .iter()
            .map(|(n, d)| {
                let path = d.path.as_ref().map(|p| join_rel("", p));
                (n.clone(), d.req.clone(), path)
            })
            .collect();
        queue.push((self.root.name.clone(), root_deps));
        let mut seen = vec![self.root.name.clone()];
        while let Some((from, deps)) = queue.pop() {
            for (name, req, path) in deps {
                let need = needs.entry(name.clone()).or_insert(Need {
                    reqs: Vec::new(),
                    path: None,
                });
                if let Some(r) = req {
                    need.reqs.push((r, from.clone()));
                }
                if let Some(p) = path {
                    match &need.path {
                        Some((q, _)) if *q != p => {
                            return Err(Error::new(
                                Code::VersionConflict,
                                msg::path_conflict(&name, q, &p),
                            ))
                        }
                        _ => need.path = Some((p, from.clone())),
                    }
                }
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name.clone());
                let Some(choice) = chosen.get(&name) else {
                    continue;
                };
                let deps = match choice {
                    Choice::Registry(e) => e
                        .deps
                        .iter()
                        .map(|d| (d.name.clone(), Some(d.req.clone()), None))
                        .collect(),
                    Choice::Path(m, rel) => m
                        .dependencies
                        .iter()
                        .map(|(n, d)| {
                            (
                                n.clone(),
                                d.req.clone(),
                                d.path.as_ref().map(|p| join_rel(rel, p)),
                            )
                        })
                        .collect(),
                };
                queue.push((name, deps));
            }
        }
        Ok(needs)
    }

    fn choose(&mut self, name: &str, need: &Need, current: Option<&Choice>) -> Result<Choice> {
        let satisfies = |v: &Version| need.reqs.iter().all(|(r, _)| r.matches(v));
        if let Some((rel, _)) = &need.path {
            let m = self.path_manifest(rel)?;
            if m.name != name {
                return Err(Error::new(
                    Code::InvalidManifest,
                    msg::path_dep_wrong_name(rel, name, &m.name),
                ));
            }
            if !satisfies(&m.version) {
                return Err(self.no_match(name, need, std::slice::from_ref(&m.version)));
            }
            return Ok(Choice::Path(m, rel.clone()));
        }
        let entries = self.entries(name)?;
        let locked = self
            .lock
            .and_then(|l| l.get(name))
            .and_then(|p| match &p.source {
                Source::Registry(url) if *url == self.registry_url => Some(p.version.clone()),
                _ => None,
            });
        let matching: Vec<&Entry> = entries.iter().filter(|e| satisfies(&e.vers)).collect();
        // Yanked versions stay usable only where the lock file already pins them.
        let usable: Vec<&Entry> = matching
            .iter()
            .copied()
            .filter(|e| !e.yanked || locked.as_ref() == Some(&e.vers))
            .collect();
        let lang_ok: Vec<&Entry> = usable
            .iter()
            .copied()
            .filter(|e| e.qxide.as_ref().is_none_or(|q| q.matches(&self.lang)))
            .collect();
        if lang_ok.is_empty() {
            if let Some(e) = usable.iter().max_by(|a, b| a.vers.cmp(&b.vers)) {
                let req = e
                    .qxide
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                return Err(Error::new(
                    Code::LanguageVersion,
                    msg::dep_needs_language(
                        name,
                        &e.vers.to_string(),
                        &req,
                        &self.lang.to_string(),
                    ),
                ));
            }
            let versions: Vec<Version> = entries
                .iter()
                .filter(|e| !e.yanked)
                .map(|e| e.vers.clone())
                .collect();
            return Err(self.no_match(name, need, &versions));
        }
        let pick = |v: &Option<Version>| {
            v.as_ref()
                .and_then(|v| lang_ok.iter().find(|e| e.vers == *v).copied())
        };
        let current_version = current.map(|c| c.version().clone());
        let e = pick(&locked)
            .or_else(|| pick(&current_version))
            .or_else(|| lang_ok.iter().copied().max_by(|a, b| a.vers.cmp(&b.vers)))
            .expect("non-empty");
        Ok(Choice::Registry(e.clone()))
    }

    fn no_match(&self, name: &str, need: &Need, available: &[Version]) -> Error {
        let reqs: Vec<String> = need
            .reqs
            .iter()
            .map(|(r, from)| msg::note_required_by(&r.to_string(), from))
            .collect();
        let conflict = need.reqs.len() > 1
            && need
                .reqs
                .iter()
                .all(|(r, _)| available.iter().any(|v| r.matches(v)));
        let mut err = if conflict {
            Error::new(Code::VersionConflict, msg::version_conflict(name))
        } else {
            let req = need
                .reqs
                .iter()
                .map(|(r, _)| r.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Error::new(
                Code::NoMatchingVersion,
                msg::no_matching_version(name, &req),
            )
        };
        for r in reqs {
            err = err.note(r);
        }
        let mut sorted = available.to_vec();
        sorted.sort();
        let list: Vec<String> = sorted.iter().map(ToString::to_string).collect();
        if !list.is_empty() {
            err = err.note(msg::note_available(&list.join(", ")));
        }
        err
    }

    /// Resolves the whole graph into a lock file.
    pub fn resolve(mut self) -> Result<Lock> {
        let mut chosen: BTreeMap<String, Choice> = BTreeMap::new();
        for _ in 0..256 {
            let needs = self.needs(&chosen)?;
            let mut next = BTreeMap::new();
            let mut changed = false;
            for (name, need) in &needs {
                if *name == self.root.name {
                    return Err(Error::new(
                        Code::VersionConflict,
                        msg::depends_on_itself(name),
                    ));
                }
                let c = self.choose(name, need, chosen.get(name))?;
                changed |= chosen.get(name).is_none_or(|old| !old.same(&c));
                next.insert(name.clone(), c);
            }
            changed |= next.len() != chosen.len();
            chosen = next;
            if !changed {
                return Ok(self.lock_from(&chosen));
            }
        }
        Err(Error::new(
            Code::VersionConflict,
            msg::resolution_diverged(),
        ))
    }

    fn lock_from(&self, chosen: &BTreeMap<String, Choice>) -> Lock {
        let mut packages = vec![LockedPackage {
            name: self.root.name.clone(),
            version: self.root.version.clone(),
            source: Source::Root,
            checksum: None,
            dependencies: self.root.dependencies.keys().cloned().collect(),
        }];
        for (name, c) in chosen {
            packages.push(match c {
                Choice::Registry(e) => LockedPackage {
                    name: name.clone(),
                    version: e.vers.clone(),
                    source: Source::Registry(self.registry_url.clone()),
                    checksum: Some(e.cksum.clone()),
                    dependencies: e.deps.iter().map(|d| d.name.clone()).collect(),
                },
                Choice::Path(m, rel) => LockedPackage {
                    name: name.clone(),
                    version: m.version.clone(),
                    source: Source::Path(rel.clone()),
                    checksum: None,
                    dependencies: m.dependencies.keys().cloned().collect(),
                },
            });
        }
        let mut lock = Lock { packages };
        lock.sort();
        lock
    }
}

/// Requirements a locked registry package puts on its dependencies (from its unpacked manifest
/// or its index entry), or `None` when they are not known locally.
pub type DepsOf<'a> = dyn FnMut(&LockedPackage) -> Option<Vec<(String, VersionReq)>> + 'a;

/// Whether `lock` still matches `root` without resolving again: same direct dependencies, every
/// locked version satisfies every requirement on it (from the root, path packages as they are on
/// disk now, and registry packages through `deps_of`), sources are the ones a resolution would
/// pick, and every locked package is reachable from the root.
pub fn is_fresh(root: &Manifest, lock: &Lock, registry_url: &str, deps_of: &mut DepsOf) -> bool {
    let Some(r) = lock.root() else { return false };
    let names: Vec<String> = root.dependencies.keys().cloned().collect();
    if r.name != root.name || r.dependencies != names {
        return false;
    }
    for (name, dep) in &root.dependencies {
        let Some(p) = lock.get(name) else {
            return false;
        };
        match (&dep.path, &p.source) {
            (Some(path), Source::Path(q)) if join_rel("", path) == *q => {}
            (None, Source::Registry(url)) if url == registry_url => {}
            _ => return false,
        }
        if !dep.req_or_any().matches(&p.version) {
            return false;
        }
    }
    // Every package's dependencies must be locked too, and nothing may be locked that the root
    // does not reach: `install` would fetch it.
    let mut reachable = vec![r.name.clone()];
    let mut i = 0;
    while i < reachable.len() {
        let Some(p) = lock.get(&reachable[i]) else {
            return false;
        };
        for d in &p.dependencies {
            if !reachable.contains(d) {
                reachable.push(d.clone());
            }
        }
        i += 1;
    }
    if lock.packages.iter().any(|p| !reachable.contains(&p.name)) {
        return false;
    }
    // Every requirement edge, as (dependency, requirement, path relative to the project root).
    let mut edges: Vec<DepEdge> = root
        .dependencies
        .iter()
        .map(|(n, d)| {
            (
                n.clone(),
                d.req.clone(),
                d.path.as_ref().map(|p| join_rel("", p)),
            )
        })
        .collect();
    for p in &lock.packages {
        match &p.source {
            Source::Root => {}
            // Path packages may have been edited since the lock was written.
            Source::Path(rel) => {
                let Ok(m) = manifest::load(&root.dir.join(rel)) else {
                    return false;
                };
                let deps: Vec<String> = m.dependencies.keys().cloned().collect();
                if m.name != p.name || m.version != p.version || deps != p.dependencies {
                    return false;
                }
                edges.extend(m.dependencies.iter().map(|(n, d)| {
                    (
                        n.clone(),
                        d.req.clone(),
                        d.path.as_ref().map(|q| join_rel(rel, q)),
                    )
                }));
            }
            Source::Registry(url) => {
                if url != registry_url {
                    return false;
                }
                let Some(reqs) = deps_of(p) else { continue };
                let mut deps: Vec<String> = reqs.iter().map(|(n, _)| n.clone()).collect();
                deps.sort();
                deps.dedup();
                if deps != p.dependencies {
                    return false;
                }
                edges.extend(reqs.into_iter().map(|(n, r)| (n, Some(r), None)));
            }
        }
    }
    let mut paths: HashMap<&str, &str> = HashMap::new();
    for (name, req, path) in &edges {
        let Some(d) = lock.get(name) else {
            return false;
        };
        if req.as_ref().is_some_and(|r| !r.matches(&d.version)) {
            return false;
        }
        if let Some(path) = path {
            // Two different paths for one package do not resolve at all (`path_conflict`).
            if paths
                .insert(name.as_str(), path.as_str())
                .is_some_and(|q| q != path)
            {
                return false;
            }
        }
    }
    // A package is a path package exactly when something asks for it by path.
    lock.packages
        .iter()
        .all(|p| match (&p.source, paths.get(p.name.as_str())) {
            (Source::Root, _) => true,
            (Source::Path(q), Some(path)) => q == path,
            (Source::Registry(_), None) => true,
            _ => false,
        })
}

/// Drops packages no longer reachable from the root (after `qxide remove`), without the
/// registry: removing a requirement never invalidates the other pinned versions.
pub fn prune(lock: &Lock, root: &Manifest) -> Lock {
    let mut keep: Vec<String> = Vec::new();
    let mut stack: Vec<String> = root.dependencies.keys().cloned().collect();
    while let Some(n) = stack.pop() {
        if keep.contains(&n) {
            continue;
        }
        if let Some(p) = lock.get(&n) {
            stack.extend(p.dependencies.iter().cloned());
        }
        keep.push(n);
    }
    let mut packages: Vec<LockedPackage> = lock
        .packages
        .iter()
        .filter(|p| p.source != Source::Root && keep.contains(&p.name))
        .cloned()
        .collect();
    packages.push(LockedPackage {
        name: root.name.clone(),
        version: root.version.clone(),
        source: Source::Root,
        checksum: None,
        dependencies: root.dependencies.keys().cloned().collect(),
    });
    let mut lock = Lock { packages };
    lock.sort();
    lock
}
