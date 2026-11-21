// The `[app]` section of `qxide.toml` (docs/design/qx-app.md §4). `qxide run` and the package
// manager ignore it; `qxide app dev|build` read it here.

use toml_edit::{DocumentMut, TableLike};

use super::msg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub title: String,
    pub identifier: String,
    /// Module path of the command module: `app` for `src/app.qx`, `ui.cmds` for `src/ui/cmds.qx`.
    pub module: String,
    pub commands: Vec<String>,
    pub permissions: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
}

const KEYS: &[&str] = &[
    "title",
    "identifier",
    "module",
    "commands",
    "permissions",
    "width",
    "height",
    "resizable",
];

pub const PERMISSIONS: &[&str] = &["log", "time", "fs", "app", "qxide"];

fn ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn strings(t: &dyn TableLike, key: &str) -> Result<Option<Vec<String>>, String> {
    match t.get(key) {
        None => Ok(None),
        Some(i) => i
            .as_array()
            .and_then(|a| {
                a.iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            })
            .map(Some)
            .ok_or_else(|| msg::reason_string_list(key)),
    }
}

fn string(t: &dyn TableLike, key: &str) -> Result<Option<String>, String> {
    match t.get(key) {
        None => Ok(None),
        Some(i) => i
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| msg::reason_string(key)),
    }
}

fn size(t: &dyn TableLike, key: &str, default: u32) -> Result<u32, String> {
    match t.get(key) {
        None => Ok(default),
        Some(i) => i
            .as_integer()
            .filter(|n| (200..=8192).contains(n))
            .map(|n| n as u32)
            .ok_or_else(|| msg::reason_size(key)),
    }
}

/// `[app]` of a manifest, or `None` when the project is not an app.
pub fn parse(text: &str, package: &str) -> Result<Option<AppConfig>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.message().to_string())?;
    let Some(item) = doc.get("app") else {
        return Ok(None);
    };
    let t = item.as_table_like().ok_or_else(msg::reason_not_table)?;
    if let Some((k, _)) = t.iter().find(|(k, _)| !KEYS.contains(k)) {
        return Err(msg::reason_unknown_key(k));
    }
    let commands = strings(t, "commands")?.unwrap_or_default();
    if commands.is_empty() {
        return Err(msg::reason_no_commands());
    }
    if let Some(c) = commands.iter().find(|c| !ident(c)) {
        return Err(msg::reason_bad_command(c));
    }
    let permissions = strings(t, "permissions")?.unwrap_or_default();
    if let Some(p) = permissions
        .iter()
        .find(|p| !PERMISSIONS.contains(&p.as_str()))
    {
        return Err(msg::reason_bad_permission(p, &PERMISSIONS.join(", ")));
    }
    let module = string(t, "module")?.unwrap_or_else(|| "app".to_string());
    if !module.split('.').all(ident) || module == "main" || module == "lib" {
        return Err(msg::reason_bad_module(&module));
    }
    let identifier = string(t, "identifier")?.unwrap_or_else(|| format!("dev.qxide.app.{package}"));
    let id_ok = !identifier.is_empty()
        && identifier.len() <= 128
        && identifier
            .split('.')
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !id_ok {
        return Err(msg::reason_bad_identifier(&identifier));
    }
    let resizable = match t.get("resizable") {
        None => true,
        Some(i) => i.as_bool().ok_or_else(|| msg::reason_bool("resizable"))?,
    };
    Ok(Some(AppConfig {
        title: string(t, "title")?.unwrap_or_else(|| package.to_string()),
        identifier,
        module,
        commands,
        permissions,
        width: size(t, "width", 800)?,
        height: size(t, "height", 600)?,
        resizable,
    }))
}

/// The command module file inside `src/`: `app` -> `app.qx`, `ui.cmds` -> `ui/cmds.qx`.
pub fn module_file(src: &std::path::Path, module: &str) -> std::path::PathBuf {
    let mut p = src.to_path_buf();
    let parts: Vec<&str> = module.split('.').collect();
    for d in &parts[..parts.len() - 1] {
        p.push(d);
    }
    p.push(format!("{}.qx", parts[parts.len() - 1]));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let base = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
        assert_eq!(parse(base, "demo"), Ok(None));
        let c = parse(&format!("{base}[app]\ncommands = [\"go\"]\n"), "demo")
            .unwrap()
            .unwrap();
        assert_eq!(c.title, "demo");
        assert_eq!(c.identifier, "dev.qxide.app.demo");
        assert_eq!(
            (c.module.as_str(), c.width, c.height, c.resizable),
            ("app", 800, 600, true)
        );
        for bad in [
            "commands = []",
            "commands = [\"go\"]\nfoo = 1",
            "commands = [\"a b\"]",
            "commands = [\"go\"]\npermissions = [\"net\"]",
            "commands = [\"go\"]\nmodule = \"lib\"",
            "commands = [\"go\"]\nidentifier = \"a..b\"",
            "commands = [\"go\"]\nwidth = 10",
            "commands = [\"go\"]\nresizable = \"yes\"",
            "commands = \"go\"",
        ] {
            assert!(
                parse(&format!("{base}[app]\n{bad}\n"), "demo").is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn module_files() {
        let src = std::path::Path::new("src");
        assert_eq!(module_file(src, "app"), src.join("app.qx"));
        assert_eq!(module_file(src, "ui.cmds"), src.join("ui").join("cmds.qx"));
    }
}
