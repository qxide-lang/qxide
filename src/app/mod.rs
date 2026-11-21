// `qxide app new|dev|build` (docs/design/qx-app.md §9). The CLI has no GUI dependencies: `dev`
// starts the prebuilt `qxide-app` runtime on the project folder, and `build` copies that runtime
// and attaches a bundle of the app's `.qx` sources, its locked packages and `ui/` files.

pub mod bundle;
pub mod config;
mod msg;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::json;

use crate::lang::{self, stdlib::Permissions, Project};
use crate::pkg;
use config::AppConfig;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Largest `ui/` file `qxide app build` embeds.
const MAX_UI_FILE: u64 = 64 << 20;

const TEMPLATE_APP: &str = include_str!("../template/app/app.qx");
const TEMPLATE_HTML: &str = include_str!("../template/app/index.html");
const TEMPLATE_JS: &str = include_str!("../template/app/app.js");
const TEMPLATE_CSS: &str = include_str!("../template/app/style.css");
const TEMPLATE_README: &str = include_str!("../template/app/README.md");

pub fn usage() -> String {
    format!(
        "  {:<33}{}\n  {:<33}{}\n  {:<33}{}\n",
        "app new <NAME>",
        msg::usage_new(),
        "app dev [DIR]",
        msg::usage_dev(),
        "app build [DIR]",
        msg::usage_build()
    )
}

pub fn command(args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("new") => new(&args[1..]),
        Some("dev") => dev(&args[1..]),
        Some("build") => build(&args[1..]),
        Some(other) => Err(Failure::usage(msg::unknown_subcommand(other))),
        None => Err(Failure::usage(msg::missing_subcommand())),
    };
    match result {
        Ok(code) => code,
        Err(f) => {
            eprint!("{}", f.text);
            if f.usage {
                eprint!("{}", usage());
            }
            ExitCode::from(f.code)
        }
    }
}

struct Failure {
    text: String,
    code: u8,
    usage: bool,
}

impl Failure {
    fn usage(message: String) -> Self {
        Failure {
            text: format!("qxide: {message}\n"),
            code: 2,
            usage: true,
        }
    }

    fn plain(message: String) -> Self {
        Failure {
            text: format!("qxide: {message}\n"),
            code: 1,
            usage: false,
        }
    }

    // Already rendered diagnostics (`error[Q0715]: ...`).
    fn rendered(text: String) -> Self {
        Failure {
            text,
            code: 1,
            usage: false,
        }
    }
}

type Result<T> = std::result::Result<T, Failure>;

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn new(args: &[String]) -> Result<ExitCode> {
    let [dir] = args else {
        return Err(Failure::usage(msg::new_needs_name()));
    };
    if dir.starts_with('-') {
        return Err(Failure::usage(msg::unexpected_argument(dir)));
    }
    let root = Path::new(dir);
    let folder = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| Failure::usage(msg::bad_name(dir)))?;
    if root.exists() {
        return Err(Failure::plain(msg::exists(dir)));
    }
    let name = crate::package_name(&folder);
    let (major, minor) = VERSION.split_once('.').map_or((VERSION, "0"), |(a, rest)| {
        (a, rest.split('.').next().unwrap_or("0"))
    });
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nqxide = \">={major}.{minor}\"\n\n[dependencies]\n\n[app]\ntitle = \"{folder}\"\nmodule = \"app\"\ncommands = [\"measure_bell\", \"save_result\"]\npermissions = [\"log\", \"time\", \"fs\"]\nwidth = 720\nheight = 560\n"
    );
    let files = [
        ("qxide.toml", manifest),
        ("src/app.qx", lf(TEMPLATE_APP)),
        (
            "ui/index.html",
            lf(TEMPLATE_HTML).replace("{title}", &folder),
        ),
        ("ui/app.js", lf(TEMPLATE_JS)),
        ("ui/style.css", lf(TEMPLATE_CSS)),
        ("README.md", lf(TEMPLATE_README).replace("{name}", &folder)),
        (".gitignore", "target/\n".to_string()),
    ];
    std::fs::create_dir(root).map_err(|e| Failure::plain(msg::io(dir, &e.to_string())))?;
    let written = std::fs::create_dir(root.join("src"))
        .and_then(|()| std::fs::create_dir(root.join("ui")))
        .and_then(|()| {
            files
                .iter()
                .try_for_each(|(file, text)| std::fs::write(root.join(file), text))
        });
    if let Err(e) = written {
        // Everything in `root` was created just now, so removing it loses nothing of the user's.
        let _ = std::fs::remove_dir_all(root);
        return Err(Failure::plain(msg::io(dir, &e.to_string())));
    }
    println!("{}", msg::created(&folder, dir));
    println!("{}", msg::next_steps(dir));
    Ok(ExitCode::SUCCESS)
}

/// A loaded app project: manifest, `[app]`, import roots and the command module file.
struct App {
    config: AppConfig,
    project: Project,
    entry: PathBuf,
}

fn project_dir(args: &[String], cmd: &str) -> Result<PathBuf> {
    let start = match args {
        [] => std::env::current_dir().map_err(|e| Failure::plain(e.to_string()))?,
        [d, ..] if d.starts_with('-') => {
            return Err(Failure::usage(msg::unexpected_argument_for(d, cmd)))
        }
        [d] => PathBuf::from(d),
        [_, extra, ..] => return Err(Failure::usage(msg::unexpected_argument_for(extra, cmd))),
    };
    pkg::manifest::find(&start)
        .ok_or_else(|| Failure::plain(msg::not_a_project(&start.display().to_string())))
}

fn load(dir: &Path) -> Result<App> {
    let manifest = pkg::manifest::load(dir).map_err(|e| Failure::rendered(e.render()))?;
    let path = dir.join(pkg::manifest::FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Failure::plain(msg::io(&path.display().to_string(), &e.to_string())))?;
    let bad = |reason: String| {
        Failure::rendered(format!("error[Q0715]: {}\n", lang::bad_app_config(&reason)))
    };
    let config = config::parse(&text, &manifest.name)
        .map_err(bad)?
        .ok_or_else(|| bad(msg::reason_no_app_section()))?;
    let entry = config::module_file(&dir.join("src"), &config.module);
    if !entry.is_file() {
        return Err(bad(msg::reason_missing_module(
            &entry.display().to_string(),
        )));
    }
    let project = pkg::project::for_entry(&entry)
        .map_err(|e| Failure::rendered(e.render()))?
        .ok_or_else(|| bad(msg::reason_no_app_section()))?;
    Ok(App {
        config,
        project,
        entry,
    })
}

/// Checks the command module like the runtime will: it links and type-checks with the app's
/// permissions, and every listed command is `public` with build-time parameters.
fn validate(app: &App) -> Result<()> {
    let src = std::fs::read_to_string(&app.entry)
        .map_err(|e| Failure::plain(msg::io(&app.entry.display().to_string(), &e.to_string())))?;
    let name = app.entry.display().to_string();
    let project = app.project.clone();
    let config = app.config.clone();
    // Same large stack as `qxide check`, so deep nesting ends in a diagnostic, not a crash.
    let worker = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let input = lang::Input {
                name: &name,
                src: &src,
                project: Some(&project),
                read: &lang::read_file,
                perms: Permissions::app(&config.permissions),
            };
            lang::command::signatures(&input, &config.module, &config.commands)
                .map(|_| ())
                .map_err(|e| e.rendered)
        });
    match worker.map(|h| h.join()) {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(rendered))) => Err(Failure::rendered(rendered)),
        _ => Err(Failure::plain(msg::internal())),
    }
}

/// The `[app] permissions` of the app project around `file`, if it is one.
pub fn permissions_for(file: &Path) -> Option<Permissions> {
    let dir = pkg::manifest::find(file.parent().unwrap_or(Path::new(".")))?;
    let text = std::fs::read_to_string(dir.join(pkg::manifest::FILE)).ok()?;
    let name = pkg::manifest::load(&dir).ok()?.name;
    let config = config::parse(&text, &name).ok()??;
    Some(Permissions::app(&config.permissions))
}

fn data_dir_json(dir: Option<&Path>) -> serde_json::Value {
    dir.map_or(serde_json::Value::Null, |d| json!(d.display().to_string()))
}

/// `app.json`: what the runtime needs to know besides the files.
fn manifest_json(
    app: &App,
    packages: Vec<serde_json::Value>,
    entry: &str,
    ui: &str,
    dev: bool,
    data: Option<&Path>,
) -> serde_json::Value {
    let c = &app.config;
    json!({
        "format": bundle::FORMAT,
        "qxide": VERSION,
        "dev": dev,
        "title": c.title,
        "identifier": c.identifier,
        "module": c.module,
        "commands": c.commands,
        "permissions": c.permissions,
        "width": c.width,
        "height": c.height,
        "resizable": c.resizable,
        "packages": packages,
        "entry": entry,
        "ui": ui,
        "data": data_dir_json(data),
    })
}

fn package_json(p: &lang::PackageRoot, src: String) -> serde_json::Value {
    json!({ "name": p.name, "src": src, "deps": p.deps })
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `qxide-app[.exe]`: `QXIDE_APP_RUNTIME`, or next to this `qxide` executable.
fn find_runtime() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("QXIDE_APP_RUNTIME") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(Failure::plain(msg::runtime_env_missing(
                &p.display().to_string(),
            )))
        };
    }
    let exe = format!("qxide-app{}", std::env::consts::EXE_SUFFIX);
    let here = std::env::current_exe().ok();
    let beside = here.as_deref().and_then(Path::parent).map(|d| d.join(&exe));
    match beside {
        Some(p) if p.is_file() => Ok(p),
        _ => Err(Failure::plain(msg::runtime_missing(&exe))),
    }
}

fn dev(args: &[String]) -> Result<ExitCode> {
    let dir = project_dir(args, "app dev")?;
    let app = load(&dir)?;
    validate(&app)?;
    let runtime = find_runtime()?;
    let target = dir.join("target");
    let data = target.join("app-data");
    let packages = app
        .project
        .packages
        .iter()
        .map(|p| package_json(p, absolute(&p.src_dir).display().to_string()))
        .collect();
    let m = manifest_json(
        &app,
        packages,
        &absolute(&app.entry).display().to_string(),
        &absolute(&dir.join("ui")).display().to_string(),
        true,
        Some(&absolute(&data)),
    );
    let state = target.join("qxide-app");
    let file = state.join("dev.json");
    std::fs::create_dir_all(&state)
        .and_then(|()| std::fs::write(&file, serde_json::to_string_pretty(&m).unwrap_or_default()))
        .map_err(|e| Failure::plain(msg::io(&file.display().to_string(), &e.to_string())))?;
    println!("{}", msg::dev_started(&app.config.title));
    let status = std::process::Command::new(&runtime)
        .arg("--dev")
        .arg(&file)
        .status()
        .map_err(|e| {
            Failure::plain(msg::runtime_failed(
                &runtime.display().to_string(),
                &e.to_string(),
            ))
        })?;
    Ok(ExitCode::from(
        status.code().unwrap_or(1).clamp(0, 255) as u8
    ))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let t = e.file_type()?;
        if t.is_dir() {
            walk(&e.path(), out)?;
        } else if t.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}

fn rel_key(base: &Path, p: &Path) -> String {
    bundle::key(p.strip_prefix(base).unwrap_or(p))
}

fn build(args: &[String]) -> Result<ExitCode> {
    let dir = project_dir(args, "app build")?;
    let app = load(&dir)?;
    validate(&app)?;
    let runtime = find_runtime()?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let io = |p: &Path, e: std::io::Error| {
        Failure::plain(msg::io(&p.display().to_string(), &e.to_string()))
    };
    let mut packages = Vec::new();
    for (i, p) in app.project.packages.iter().enumerate() {
        // Bundle paths such as `pkg/0/bell_utils/src/lib.qx`, shown in diagnostics.
        let base = format!("pkg/{i}/{}/src", p.name);
        let mut found = Vec::new();
        walk(&p.src_dir, &mut found).map_err(|e| io(&p.src_dir, e))?;
        for f in found
            .iter()
            .filter(|f| f.extension().is_some_and(|x| x == "qx"))
        {
            let data = std::fs::read(f).map_err(|e| io(f, e))?;
            files.insert(format!("{base}/{}", rel_key(&p.src_dir, f)), data);
        }
        packages.push(package_json(p, base));
    }
    let ui = dir.join("ui");
    let mut found = Vec::new();
    walk(&ui, &mut found).map_err(|e| io(&ui, e))?;
    for f in &found {
        let len = std::fs::metadata(f).map_err(|e| io(f, e))?.len();
        if len > MAX_UI_FILE {
            return Err(Failure::plain(msg::ui_file_too_large(
                &f.display().to_string(),
                MAX_UI_FILE >> 20,
            )));
        }
        let data = std::fs::read(f).map_err(|e| io(f, e))?;
        files.insert(format!("ui/{}", rel_key(&ui, f)), data);
    }
    // `src_dir` may be relative to the current directory while `entry` is not; derive the
    // module's place in `src/` from the module path instead.
    let entry = format!(
        "pkg/{}/{}/src/{}",
        app.project.entry_package,
        app.project.packages[app.project.entry_package].name,
        bundle::key(&config::module_file(Path::new(""), &app.config.module))
    );
    let m = manifest_json(&app, packages, &entry, "ui", false, None);
    files.insert(
        bundle::MANIFEST.to_string(),
        serde_json::to_vec_pretty(&m).unwrap_or_default(),
    );
    let mut out = std::fs::read(&runtime).map_err(|e| io(&runtime, e))?;
    if bundle::read(&out).is_ok() {
        return Err(Failure::plain(msg::runtime_has_bundle(
            &runtime.display().to_string(),
        )));
    }
    let runtime_len = out.len();
    bundle::write(&mut out, &files);
    let name = pkg::manifest::load(&dir)
        .map(|m| m.name)
        .unwrap_or_else(|_| "app".to_string());
    let out_dir = dir.join("target").join("app");
    let out_file = out_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(&out_dir)
        .and_then(|()| std::fs::write(&out_file, &out))
        .map_err(|e| io(&out_file, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out_file, std::fs::Permissions::from_mode(0o755));
    }
    let shown = out_file
        .strip_prefix(std::env::current_dir().unwrap_or_default())
        .unwrap_or(&out_file)
        .display()
        .to_string();
    println!(
        "{}",
        msg::built(
            &shown,
            files.len(),
            (out.len() - runtime_len) as u64 >> 10,
            out.len() as u64 >> 10
        )
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests;
