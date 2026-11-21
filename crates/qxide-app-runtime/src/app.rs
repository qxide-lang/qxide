// What the runtime runs: the `app.json` manifest written by `qxide app build|dev`, and where the
// `.qx` modules and `ui/` files come from (the bundle in this executable, or disk in dev mode).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::bundle;
use crate::lang::{PackageRoot, Project};
use crate::msg;

pub enum Files {
    Bundle(BTreeMap<String, Vec<u8>>),
    Disk,
}

pub struct App {
    pub dev: bool,
    pub title: String,
    pub module: String,
    pub commands: Vec<String>,
    pub permissions: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub project: Project,
    /// The command module file: a bundle path, or a file on disk in dev mode.
    pub entry: String,
    /// `ui` in a bundle; the `ui/` folder in dev mode.
    pub ui: String,
    /// Root of `std.fs`.
    pub data: PathBuf,
    /// WebView2/WebKit profile folder.
    pub webview_data: PathBuf,
    pub files: Files,
}

fn s(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| msg::bad_manifest(key))
}

fn strings(v: &Value, key: &str) -> Result<Vec<String>, String> {
    v[key]
        .as_array()
        .and_then(|a| a.iter().map(|x| x.as_str().map(str::to_string)).collect())
        .ok_or_else(|| msg::bad_manifest(key))
}

fn num(v: &Value, key: &str) -> Result<u32, String> {
    v[key]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| msg::bad_manifest(key))
}

/// Per-user folders: `%APPDATA%` and `%LOCALAPPDATA%` on Windows, the XDG folders on Linux,
/// `~/Library/Application Support` on macOS.
fn user_dirs(identifier: &str) -> (PathBuf, PathBuf) {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("HOME")
        .or_else(|| env("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("."));
    let (roaming, local) = if cfg!(windows) {
        let r = env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let l = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
        (r, l)
    } else if cfg!(target_os = "macos") {
        let d = home.join("Library").join("Application Support");
        (d.clone(), d)
    } else {
        let d = env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
        (d.clone(), d)
    };
    (
        roaming.join(identifier),
        local.join(identifier).join("webview"),
    )
}

impl App {
    pub fn parse(json: &[u8], files: Files) -> Result<App, String> {
        let v: Value =
            serde_json::from_slice(json).map_err(|e| msg::bad_manifest(&e.to_string()))?;
        let built_by = s(&v, "qxide")?;
        let ours = env!("CARGO_PKG_VERSION");
        if built_by != ours {
            return Err(msg::version_mismatch(&built_by, ours));
        }
        let dev = v["dev"].as_bool().unwrap_or(false);
        let packages = v["packages"]
            .as_array()
            .ok_or_else(|| msg::bad_manifest("packages"))?
            .iter()
            .map(|p| {
                Ok(PackageRoot {
                    name: s(p, "name")?,
                    src_dir: PathBuf::from(s(p, "src")?),
                    deps: p["deps"]
                        .as_array()
                        .map(|d| {
                            d.iter()
                                .filter_map(|x| x.as_u64().map(|n| n as usize))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if packages.is_empty() {
            return Err(msg::bad_manifest("packages"));
        }
        let module = s(&v, "module")?;
        let identifier = s(&v, "identifier")?;
        let (data, webview_data) = match v["data"].as_str() {
            Some(d) => {
                let d = PathBuf::from(d);
                let w = d.parent().unwrap_or(&d).join("qxide-app").join("webview");
                (d, w)
            }
            None => user_dirs(&identifier),
        };
        Ok(App {
            dev,
            title: s(&v, "title")?,
            project: Project {
                packages,
                entry_package: 0,
                entry_module: Some(module.split('.').map(str::to_string).collect()),
            },
            module,
            commands: strings(&v, "commands")?,
            permissions: strings(&v, "permissions")?,
            width: num(&v, "width")?,
            height: num(&v, "height")?,
            resizable: v["resizable"].as_bool().unwrap_or(true),
            entry: s(&v, "entry")?,
            ui: s(&v, "ui")?,
            data,
            webview_data,
            files,
        })
    }

    /// The app attached to this executable.
    pub fn from_exe() -> Result<App, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&exe).map_err(|e| e.to_string())?;
        let files = match bundle::read(&bytes) {
            Ok(f) => f,
            Err(bundle::BundleError::Missing) => return Err(msg::no_bundle()),
            Err(bundle::BundleError::Format(n)) => return Err(msg::bundle_format(n)),
            Err(bundle::BundleError::Corrupt) => return Err(msg::bundle_corrupt()),
        };
        let manifest = files
            .get(bundle::MANIFEST)
            .cloned()
            .ok_or_else(msg::bundle_corrupt)?;
        App::parse(&manifest, Files::Bundle(files))
    }

    /// `qxide app dev`: the manifest names files on disk.
    pub fn from_dev(path: &Path) -> Result<App, String> {
        let json = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        App::parse(&json, Files::Disk)
    }

    /// Reads a `.qx` module by the path the linker asks for.
    pub fn read_qx(&self, path: &Path) -> std::io::Result<String> {
        match &self.files {
            Files::Disk => std::fs::read_to_string(path),
            Files::Bundle(map) => map
                .get(&bundle::key(path))
                .and_then(|b| String::from_utf8(b.clone()).ok())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
        }
    }

    /// A `ui/` file for the page; `rel` is the URL path without the leading `/`.
    pub fn ui_file(&self, rel: &str) -> Option<Vec<u8>> {
        // Only plain relative paths: no `..`, roots or drive prefixes.
        let ok = !rel.is_empty()
            && Path::new(rel)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            && !rel.contains('\\')
            && !rel.contains(':');
        if !ok {
            return None;
        }
        match &self.files {
            Files::Disk => std::fs::read(Path::new(&self.ui).join(rel)).ok(),
            Files::Bundle(map) => map.get(&format!("{}/{rel}", self.ui)).cloned(),
        }
    }
}
