// `qxide add | remove | install | publish | search`. Kept apart from `main.rs` so other CLI
// work only meets this module at one match arm and the usage text.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use semver::VersionReq;

use super::cache::Home;
use super::index::{self, Entry, IndexDep, Registry, DEFAULT_DL};
use super::lock;
use super::manifest::{self, Manifest};
use super::project::{self, Installed};
use super::resolve;
use super::{archive, language_version, msg, sha256_hex, valid_name, Code, Error, Result};

pub const COMMANDS: &[&str] = &["add", "remove", "install", "publish", "search"];

/// Lines for the `Commands:` section of `qxide --help`.
pub fn usage() -> &'static str {
    msg::usage_commands()
}

/// The GitHub repository behind the default registry, where `publish` opens pull requests.
const REGISTRY_REPO: &str = "qxide-lang/registry";

#[derive(Default)]
struct Opts {
    registry: Option<String>,
    path: Option<String>,
    offline: bool,
    locked: bool,
    dry_run: bool,
    args: Vec<String>,
}

fn usage_error(text: String) -> ExitCode {
    eprintln!("qxide: {text}");
    ExitCode::from(2)
}

// Flags each command accepts; values follow their flag.
fn parse(cmd: &str, args: &[String]) -> std::result::Result<Opts, ExitCode> {
    let mut o = Opts {
        offline: std::env::var_os("QXIDE_OFFLINE").is_some_and(|v| v == "1"),
        ..Opts::default()
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let allowed = |flags: &[&str]| flags.contains(&cmd);
        match a.as_str() {
            "--registry" if allowed(&["add", "install", "publish", "search"]) => {
                o.registry = Some(
                    it.next()
                        .ok_or_else(|| usage_error(msg::flag_needs_value(a)))?
                        .clone(),
                )
            }
            "--path" if allowed(&["add"]) => {
                o.path = Some(
                    it.next()
                        .ok_or_else(|| usage_error(msg::flag_needs_value(a)))?
                        .clone(),
                )
            }
            "--offline" if allowed(&["add", "install", "search"]) => o.offline = true,
            "--locked" if allowed(&["install"]) => o.locked = true,
            "--dry-run" if allowed(&["publish"]) => o.dry_run = true,
            s if s.starts_with('-') => return Err(usage_error(msg::unexpected_arg(s, cmd))),
            s => o.args.push(s.to_string()),
        }
    }
    let want = match cmd {
        "add" | "remove" | "search" => 1,
        _ => 0,
    };
    if o.args.len() != want {
        return Err(usage_error(msg::wrong_arg_count(cmd)));
    }
    Ok(o)
}

/// Runs one package command; package errors exit with 1, usage errors with 2.
pub fn main(cmd: &str, args: &[String]) -> ExitCode {
    let o = match parse(cmd, args) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let result = match cmd {
        "add" => add(&o),
        "remove" => remove(&o),
        "install" => install(&o),
        "publish" => publish(&o),
        _ => search(&o),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprint!("{}", e.render());
            ExitCode::from(1)
        }
    }
}

fn status(verb: &str, text: &str) {
    eprintln!("{verb:>12} {text}");
}

fn current_manifest() -> Result<Manifest> {
    let cwd = std::env::current_dir().map_err(|e| Error::io(Path::new("."), &e))?;
    let dir = manifest::find(&cwd).ok_or_else(|| {
        Error::new(
            Code::ManifestNotFound,
            msg::manifest_not_found(&cwd.display().to_string()),
        )
    })?;
    manifest::load(&dir)
}

fn open_registry(o: &Opts) -> Result<Registry> {
    Registry::open(o.registry.as_deref(), o.offline, Home::from_env()?)
}

fn report(installed: &Installed) {
    if let Some(n) = installed.locked {
        status(msg::verb_locking(), &msg::n_packages(n));
    }
    for (name, version) in &installed.fetched {
        status(msg::verb_installed(), &format!("{name} v{version}"));
    }
}

// `name` or `name@req`.
fn parse_spec(spec: &str) -> Result<(String, Option<VersionReq>, Option<String>)> {
    let (name, req) = match spec.split_once('@') {
        Some((n, r)) => (n, Some(r)),
        None => (spec, None),
    };
    if !valid_name(name) {
        return Err(Error::new(
            Code::PackageNotFound,
            msg::bad_package_name(name),
        ));
    }
    let parsed = match req {
        None => None,
        Some(r) => Some(
            VersionReq::parse(r)
                .map_err(|_| Error::new(Code::NoMatchingVersion, msg::bad_requirement(r)))?,
        ),
    };
    Ok((name.to_string(), parsed, req.map(str::to_string)))
}

fn add(o: &Opts) -> Result<()> {
    let root = current_manifest()?;
    let (name, req, req_text) = parse_spec(&o.args[0])?;
    if name == root.name {
        return Err(Error::new(
            Code::VersionConflict,
            msg::depends_on_itself(&name),
        ));
    }
    let mut registry = open_registry(o)?;
    // Both files are restored byte for byte if anything below fails.
    let manifest_file = root.dir.join(manifest::FILE);
    let lock_file = root.dir.join(lock::FILE);
    let saved_manifest =
        std::fs::read(&manifest_file).map_err(|e| Error::io(&manifest_file, &e))?;
    let saved_lock = std::fs::read(&lock_file).ok();
    let result = add_and_install(o, &root, &name, req, req_text, &mut registry);
    if result.is_err() {
        let _ = std::fs::write(&manifest_file, &saved_manifest);
        match &saved_lock {
            Some(bytes) => {
                let _ = std::fs::write(&lock_file, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&lock_file);
            }
        }
    }
    result
}

fn add_and_install(
    o: &Opts,
    root: &Manifest,
    name: &str,
    req: Option<VersionReq>,
    req_text: Option<String>,
    registry: &mut Registry,
) -> Result<()> {
    if let Some(path) = &o.path {
        let dep = manifest::load(&root.dir.join(path))?;
        if dep.name != name {
            return Err(Error::new(
                Code::InvalidManifest,
                msg::path_dep_wrong_name(path, name, &dep.name),
            ));
        }
        manifest::add_dependency(&root.dir, name, req_text.as_deref(), Some(path))?;
        status(
            msg::verb_adding(),
            &format!("{name} v{} ({path})", dep.version),
        );
    } else {
        let entries = registry.entries(name)?;
        let lang = language_version();
        let req = req.unwrap_or(VersionReq::STAR);
        let best = entries
            .iter()
            .filter(|e| !e.yanked && req.matches(&e.vers))
            .filter(|e| e.qxide.as_ref().is_none_or(|q| q.matches(&lang)))
            .max_by(|a, b| a.vers.cmp(&b.vers))
            .ok_or_else(|| {
                Error::new(
                    Code::NoMatchingVersion,
                    msg::no_matching_version(name, &req.to_string()),
                )
            })?;
        // Like Cargo: a bare version means a caret requirement (`0.1.0` = `^0.1.0`).
        let written = req_text.unwrap_or_else(|| best.vers.to_string());
        manifest::add_dependency(&root.dir, name, Some(&written), None)?;
        status(msg::verb_adding(), &format!("{name} v{}", best.vers));
    }
    let root = manifest::load(&root.dir)?;
    let (_, installed) = project::install(&root, registry, false)?;
    report(&installed);
    Ok(())
}

fn remove(o: &Opts) -> Result<()> {
    let root = current_manifest()?;
    let name = &o.args[0];
    if !manifest::remove_dependency(&root.dir, name)? {
        return Err(Error::new(
            Code::NotADependency,
            msg::not_a_dependency(name),
        ));
    }
    let root = manifest::load(&root.dir)?;
    if let Some(lock) = lock::load(&root.dir)? {
        resolve::prune(&lock, &root).save(&root.dir)?;
    }
    status(msg::verb_removing(), name);
    Ok(())
}

fn install(o: &Opts) -> Result<()> {
    let root = current_manifest()?;
    let mut registry = open_registry(o)?;
    let (lock, installed) = project::install(&root, &mut registry, o.locked)?;
    report(&installed);
    let n = lock.packages.len() - 1;
    status(msg::verb_finished(), &msg::n_packages_ready(n));
    Ok(())
}

fn search(o: &Opts) -> Result<()> {
    let registry = open_registry(o)?;
    let word = o.args[0].to_lowercase();
    let rows: Vec<_> = registry
        .search_rows()?
        .into_iter()
        .filter(|r| r.name.contains(&word) || r.description.to_lowercase().contains(&word))
        .collect();
    if rows.is_empty() {
        eprintln!("{}", msg::no_search_results(&o.args[0]));
    }
    let width = rows
        .iter()
        .map(|r| r.name.len() + r.version.len())
        .max()
        .unwrap_or(0)
        + 5;
    for r in rows {
        let left = format!("{} = \"{}\"", r.name, r.version);
        if r.description.is_empty() {
            println!("{left}");
        } else {
            println!("{left:<width$} # {}", r.description);
        }
    }
    Ok(())
}

fn reject(reason: String) -> Error {
    Error::new(Code::PublishRejected, msg::publish_rejected(&reason))
}

// Everything the registry needs from a manifest before it accepts a package.
fn validate_for_publish(m: &Manifest) -> Result<()> {
    for (key, value) in [("description", &m.description), ("license", &m.license)] {
        if value.as_deref().is_none_or(|s| s.trim().is_empty()) {
            return Err(reject(msg::reason_missing(key)));
        }
    }
    if m.qxide.is_none() {
        return Err(reject(msg::reason_missing("qxide")));
    }
    // Registries refuse build metadata (`1.0.0+x`): it would tie with `1.0.0` in precedence.
    if !m.version.build.is_empty() {
        return Err(reject(msg::reason_bad_version(&m.version.to_string())));
    }
    for (name, dep) in &m.dependencies {
        if dep.req.is_none() {
            return Err(reject(msg::reason_path_only(name)));
        }
    }
    if !m.dir.join("src").join("lib.qx").is_file() {
        return Err(reject(msg::reason_missing("src/lib.qx")));
    }
    Ok(())
}

fn check_library(m: &Manifest) -> Result<()> {
    let lib = m.dir.join("src").join("lib.qx");
    let src = std::fs::read_to_string(&lib).map_err(|e| Error::io(&lib, &e))?;
    let project = project::for_entry(&lib)?;
    let name = lib.display().to_string();
    // Same large stack as `qxide check`, so deep nesting ends in a diagnostic, not a crash.
    let worker = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let input = crate::lang::Input {
                name: &name,
                src: &src,
                project: project.as_ref(),
                read: &crate::lang::read_file,
            };
            crate::lang::check_input(&input)
        });
    match worker.map(|h| h.join()) {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(crate::lang::Failure(d)))) => {
            eprint!("{d}");
            Err(reject(msg::reason_check_failed()))
        }
        _ => Err(reject(msg::reason_check_failed())),
    }
}

fn publish(o: &Opts) -> Result<()> {
    let m = current_manifest()?;
    validate_for_publish(&m)?;
    check_library(&m)?;
    let (bytes, files) = archive::pack(&m)?;
    archive::check_paths(files.iter().map(String::as_str)).map_err(reject)?;
    if bytes.len() as u64 > archive::MAX_ARCHIVE {
        return Err(reject(msg::reason_too_large(archive::MAX_ARCHIVE)));
    }
    let cksum = sha256_hex(&bytes);
    let entry = Entry {
        name: m.name.clone(),
        vers: m.version.clone(),
        deps: m
            .dependencies
            .iter()
            .map(|(n, d)| IndexDep {
                name: n.clone(),
                req: d.req_or_any(),
            })
            .collect(),
        cksum: cksum.clone(),
        qxide: m.qxide.clone(),
        description: m.description.clone(),
        dl: None,
        yanked: false,
    };
    let out = m
        .dir
        .join("target")
        .join("package")
        .join(format!("{}.tar.gz", archive::prefix(&m.name, &m.version)));
    super::cache::write_atomic(&out, &bytes)?;
    status(
        msg::verb_packaged(),
        &msg::packaged(files.len(), bytes.len(), &out.display().to_string()),
    );
    for f in &files {
        eprintln!("{:>12} {f}", "");
    }
    println!("{}", msg::checksum_line(&cksum));
    println!("{}", msg::index_line(&index::index_path(&m.name)));
    println!("{}", entry.to_json());
    if o.dry_run {
        status(msg::verb_dry_run(), msg::dry_run_done());
        return Ok(());
    }
    let registry = open_registry(o)?;
    if let Some(dir) = registry.local_dir() {
        let written = write_into_registry(dir, &entry, &bytes)?;
        status(
            msg::verb_published(),
            &msg::published_local(&m.name, &m.version.to_string(), &dir.display().to_string()),
        );
        for w in written {
            eprintln!("{:>12} {}", "", w.display());
        }
        return Ok(());
    }
    if registry.is_default() && gh_available() {
        return open_pull_request(&entry, &bytes);
    }
    // No `gh`, or a registry that is not on GitHub: say what to add by hand.
    eprintln!("{}", msg::manual_publish(REGISTRY_REPO));
    eprintln!(
        "  {}  <- {}",
        index::index_path(&m.name),
        msg::manual_append_line()
    );
    eprintln!(
        "  {}  <- {}",
        DEFAULT_DL
            .replace("{prefix}", &index::prefix(&m.name))
            .replace("{name}", &m.name)
            .replace("{version}", &m.version.to_string()),
        out.display()
    );
    Ok(())
}

/// Adds the index line and the archive to a registry checkout; returns the files written.
pub fn write_into_registry(dir: &Path, entry: &Entry, bytes: &[u8]) -> Result<Vec<PathBuf>> {
    let index_file = dir.join(index::index_path(&entry.name));
    let existing = match std::fs::read_to_string(&index_file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::io(&index_file, &e)),
    };
    for line in existing.lines().filter(|l| !l.trim().is_empty()) {
        if Entry::parse(line).is_ok_and(|e| e.vers == entry.vers) {
            return Err(reject(msg::reason_version_exists(
                &entry.name,
                &entry.vers.to_string(),
            )));
        }
    }
    let template = std::fs::read(dir.join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("dl").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_DL.to_string());
    let rel = template
        .replace("{name}", &entry.name)
        .replace("{version}", &entry.vers.to_string())
        .replace("{prefix}", &index::prefix(&entry.name))
        .replace("{sha256-checksum}", &entry.cksum);
    let mut written = Vec::new();
    // Archives hosted elsewhere (an absolute `dl`) are uploaded by the registry operator.
    if !rel.contains("://") {
        let archive_file = dir.join(&rel);
        super::cache::write_atomic(&archive_file, bytes)?;
        written.push(archive_file);
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&entry.to_json());
    text.push('\n');
    super::cache::write_atomic(&index_file, text.as_bytes())?;
    written.push(index_file);
    Ok(written)
}

fn gh_available() -> bool {
    super::command("gh").is_ok_and(|mut gh| {
        gh.arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

/// `gh` or `git` from `PATH` (see `super::command`).
fn tool(name: &str) -> Result<Command> {
    super::command(name).map_err(|e| {
        Error::new(
            Code::RegistryUnavailable,
            msg::command_failed(name, &e.to_string()),
        )
    })
}

fn run(cmd: &mut Command) -> Result<String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| {
        Error::new(
            Code::RegistryUnavailable,
            msg::command_failed(&shown, &e.to_string()),
        )
    })?;
    if !out.status.success() {
        return Err(Error::new(
            Code::RegistryUnavailable,
            msg::command_failed(&shown, String::from_utf8_lossy(&out.stderr).trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// Fork the registry, commit the archive and index line on a branch, and open a pull request.
fn open_pull_request(entry: &Entry, bytes: &[u8]) -> Result<()> {
    let version = entry.vers.to_string();
    let branch = format!("publish/{}-{version}", entry.name);
    let user = run(tool("gh")?.args(["api", "user", "--jq", ".login"]))?;
    status(msg::verb_forking(), REGISTRY_REPO);
    run(tool("gh")?.args(["repo", "fork", REGISTRY_REPO, "--clone=false"]))?;
    let work = std::env::temp_dir().join(format!(
        "qxide-publish-{}-{version}-{}",
        entry.name,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&work);
    run(tool("gh")?
        // `--no-upstream`: the `upstream` remote is added explicitly below.
        .args(["repo", "clone", &format!("{user}/registry")])
        .arg("--no-upstream")
        .arg(&work)
        .args(["--", "--depth", "1"]))?;
    let git = |args: &[&str]| {
        let mut c = tool("git")?;
        c.arg("-C").arg(&work).args(args);
        run(&mut c)
    };
    git(&[
        "remote",
        "add",
        "upstream",
        &format!("https://github.com/{REGISTRY_REPO}.git"),
    ])?;
    git(&["fetch", "--depth", "1", "upstream", "main"])?;
    git(&["checkout", "-b", &branch, "upstream/main"])?;
    write_into_registry(&work, entry, bytes)?;
    git(&["add", "-A"])?;
    git(&["commit", "-m", &msg::pr_title(&entry.name, &version)])?;
    git(&["push", "-u", "origin", &branch])?;
    let url = run(tool("gh")?.args([
        "pr",
        "create",
        "--repo",
        REGISTRY_REPO,
        "--head",
        &format!("{user}:{branch}"),
        "--title",
        &msg::pr_title(&entry.name, &version),
        "--body",
        &msg::pr_body(&entry.name, &version, &entry.cksum),
    ]))?;
    let _ = std::fs::remove_dir_all(&work);
    status(msg::verb_published(), &msg::pr_opened(&url));
    Ok(())
}
