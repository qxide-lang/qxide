// The `.qx` standard library (docs/design/qx-app.md §6): module signatures as `.qx` stubs, the
// per-host permission sets, the `Host` trait every std call goes through, and `NativeStd`, the
// shared implementation of `std.log`, `std.time` and `std.fs`.
//
// Std functions are build-time effects: the interpreter calls the host while it elaborates, once
// per run, before any shot is simulated. The stub bodies below are never executed.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::msg;

/// Every standard module, with the `.qx` declarations the checker sees.
pub const MODULES: &[(&str, &str)] = &[
    (
        "log",
        "public void info(String message) {}
public void warn(String message) {}
public void error(String message) {}
",
    ),
    (
        "time",
        "public int now_millis() { return 0; }
public String now_iso() { return \"\"; }
public void sleep_millis(int ms) {}
",
    ),
    (
        "fs",
        "public String read_text(String path) { return \"\"; }
public void write_text(String path, String text) {}
public void append_text(String path, String text) {}
public boolean exists(String path) { return false; }
public void remove(String path) {}
",
    ),
    (
        "app",
        "public void set_title(String title) {}
public void set_size(int width, int height) {}
public void close() {}
public void emit(String event, String payload) {}
",
    ),
    (
        "http",
        "public String get(String url) { return \"\"; }
",
    ),
    (
        "qxide",
        "public String check(String source) { return \"\"; }
public String run(String source, int shots, int seed) { return \"\"; }
public String qasm(String source) { return \"\"; }
public String format(String source) { return \"\"; }
",
    ),
];

/// The reserved package name of the standard library.
pub const PACKAGE: &str = "std";

pub fn module_source(name: &str) -> Option<&'static str> {
    MODULES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Whether code may import a standard module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    /// Not offered by this host at all (`std.app` in `qxide run`): names the host.
    Unavailable(&'static str),
    /// Offered, but not granted: says how to grant it.
    Denied(&'static str),
}

/// Which standard modules a host offers and the program was granted.
#[derive(Debug, Clone)]
pub struct Permissions {
    access: Vec<(&'static str, Access)>,
}

const IN_CLI: &str = "`qxide run`";
const IN_PLAYGROUND: &str = "the playground";
const IN_REPL: &str = "the REPL";
const GRANT_CLI_FS: &str = "`--allow fs`";
const GRANT_APP: &str = "`[app] permissions` in `qxide.toml`";

impl Permissions {
    fn with(f: impl Fn(&str) -> Access) -> Self {
        Permissions {
            access: MODULES.iter().map(|(m, _)| (*m, f(m))).collect(),
        }
    }

    /// `qxide run`/`check`: logging and the clock always, files with `--allow fs`.
    pub fn cli(allow_fs: bool) -> Self {
        Self::with(|m| match m {
            "log" | "time" => Access::Allowed,
            "fs" if allow_fs => Access::Allowed,
            "fs" => Access::Denied(GRANT_CLI_FS),
            _ => Access::Unavailable(IN_CLI),
        })
    }

    /// The browser playground: logging only.
    #[allow(dead_code)]
    pub fn playground() -> Self {
        Self::with(|m| match m {
            "log" => Access::Allowed,
            _ => Access::Unavailable(IN_PLAYGROUND),
        })
    }

    /// The REPL replays the session on every input, so effects other than logging are refused.
    pub fn repl() -> Self {
        Self::with(|m| match m {
            "log" => Access::Allowed,
            _ => Access::Unavailable(IN_REPL),
        })
    }

    /// A desktop app: `std.log` always, every other module only if listed in `[app] permissions`.
    #[allow(dead_code)]
    pub fn app(granted: &[String]) -> Self {
        Self::with(|m| {
            if m == "log" || granted.iter().any(|g| g == m) {
                Access::Allowed
            } else {
                Access::Denied(GRANT_APP)
            }
        })
    }

    pub fn access(&self, module: &str) -> Option<Access> {
        self.access
            .iter()
            .find(|(m, _)| *m == module)
            .map(|(_, a)| *a)
    }

    /// Permission names accepted in `[app] permissions`.
    #[allow(dead_code)]
    pub fn known(name: &str) -> bool {
        module_source(name).is_some()
    }
}

/// A build-time value passed to or returned from a std function.
#[derive(Debug, Clone, PartialEq)]
pub enum HostValue {
    Unit,
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdErrorKind {
    /// The module is not offered by this host (Q0702).
    Unavailable,
    /// A file operation failed (Q0704).
    Io,
    /// A path outside the data folder or otherwise not allowed (Q0705).
    Path,
    /// An argument out of range (Q0706).
    Argument,
    /// An HTTP request failed: network error, status other than 2xx, body too large or not
    /// UTF-8 (Q0707).
    Http,
    /// A URL that is not https or whose host is not in `[app] http_hosts` (Q0708).
    Url,
    /// The call was cancelled (Q0713).
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdError {
    pub kind: StdErrorKind,
    pub message: String,
}

impl StdError {
    pub fn new(kind: StdErrorKind, message: String) -> Self {
        StdError { kind, message }
    }
}

pub type StdResult = Result<HostValue, StdError>;

/// Runs standard-library calls for the interpreter. `module` is `fs` for `std.fs.read_text`.
pub trait Host {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult;

    /// Polled between shots and every few thousand interpreter steps.
    fn cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// `info: message`, the line `std.log` writes in the CLI, the REPL and the playground.
pub fn log_line(level: LogLevel, message: &str) -> String {
    format!("{}: {message}", level.as_str())
}

pub type LogSink = Box<dyn FnMut(LogLevel, &str) + Send>;

/// `std.log`, `std.time`, `std.fs` and (when the embedder supplies a transport) `std.http` on
/// the real OS. Other modules are left to the embedder.
pub struct NativeStd {
    /// Root of every `std.fs` path.
    pub data_root: PathBuf,
    pub log: LogSink,
    pub cancel: Option<Arc<AtomicBool>>,
    /// `std.http`; `None` where the host has no network access.
    pub http: Option<HttpAccess>,
}

/// An HTTP response as the transport read it; the body is at most `MAX_HTTP_BYTES + 1` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Performs one GET of an already validated https URL. It must stop reading after
/// `MAX_HTTP_BYTES + 1` body bytes and give up when the second argument reports cancellation.
pub type HttpGet = Box<dyn Fn(&str, &dyn Fn() -> bool) -> Result<HttpReply, String> + Send>;

/// The hosts an app may reach and how requests are made (the app runtime supplies `curl`).
pub struct HttpAccess {
    /// Lowercase host names from `[app] http_hosts`, matched exactly.
    pub hosts: Vec<String>,
    pub get: HttpGet,
}

/// Largest file `std.fs.read_text` reads.
pub const MAX_READ_BYTES: u64 = 16 << 20;
/// Largest response body `std.http.get` returns.
pub const MAX_HTTP_BYTES: usize = 16 << 20;
/// Longest URL `std.http.get` accepts.
pub const MAX_URL_LEN: usize = 2048;
/// Longest single `std.time.sleep_millis`.
pub const MAX_SLEEP_MS: i64 = 60_000;

fn str_arg(args: &[HostValue], i: usize) -> &str {
    match args.get(i) {
        Some(HostValue::Str(s)) => s,
        // The checker guarantees argument types; an empty string fails path validation anyway.
        _ => "",
    }
}

fn int_arg(args: &[HostValue], i: usize) -> i64 {
    match args.get(i) {
        Some(HostValue::Int(n)) => *n,
        _ => 0,
    }
}

impl NativeStd {
    pub fn new(data_root: PathBuf, log: LogSink) -> Self {
        NativeStd {
            data_root,
            log,
            cancel: None,
            http: None,
        }
    }

    /// `std.http.get`: an https URL on an allowed host, a 2xx status and a UTF-8 body of at most
    /// `MAX_HTTP_BYTES`.
    fn http_get(&self, url: &str) -> StdResult {
        let Some(http) = &self.http else {
            return Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::std_unavailable("std.http", "this host"),
            ));
        };
        let url = allowed_url(url, &http.hosts)?;
        let fail = |m: String| StdError::new(StdErrorKind::Http, m);
        let reply = (http.get)(url, &|| self.is_cancelled()).map_err(|e| {
            if self.is_cancelled() {
                StdError::new(StdErrorKind::Cancelled, msg::cancelled())
            } else {
                fail(msg::http_failed(url, &e))
            }
        })?;
        if !(200..300).contains(&reply.status) {
            return Err(fail(msg::http_status(url, reply.status)));
        }
        if reply.body.len() > MAX_HTTP_BYTES {
            return Err(fail(msg::http_too_large(url, MAX_HTTP_BYTES >> 20)));
        }
        String::from_utf8(reply.body)
            .map(HostValue::Str)
            .map_err(|_| fail(msg::http_not_utf8(url)))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// Handles `log`, `time` and `fs`; anything else is `Unavailable`.
    pub fn call(&mut self, module: &str, function: &str, args: &[HostValue]) -> StdResult {
        match (module, function) {
            ("log", level) => {
                let level = match level {
                    "warn" => LogLevel::Warn,
                    "error" => LogLevel::Error,
                    _ => LogLevel::Info,
                };
                (self.log)(level, str_arg(args, 0));
                Ok(HostValue::Unit)
            }
            ("time", "now_millis") => Ok(HostValue::Int(now_millis())),
            ("time", "now_iso") => Ok(HostValue::Str(iso_utc(now_millis()))),
            ("time", "sleep_millis") => self.sleep(int_arg(args, 0)),
            ("fs", f) => self.fs(f, args),
            ("http", "get") => self.http_get(str_arg(args, 0)),
            _ => Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::std_unavailable(&format!("std.{module}"), "this host"),
            )),
        }
    }

    fn sleep(&self, ms: i64) -> StdResult {
        if !(0..=MAX_SLEEP_MS).contains(&ms) {
            return Err(StdError::new(
                StdErrorKind::Argument,
                msg::std_sleep_range(ms, MAX_SLEEP_MS),
            ));
        }
        let mut left = Duration::from_millis(ms as u64);
        let step = Duration::from_millis(20);
        while !left.is_zero() {
            if self.is_cancelled() {
                return Err(StdError::new(StdErrorKind::Cancelled, msg::cancelled()));
            }
            let d = left.min(step);
            std::thread::sleep(d);
            left -= d;
        }
        Ok(HostValue::Unit)
    }

    fn fs(&self, function: &str, args: &[HostValue]) -> StdResult {
        let rel = str_arg(args, 0);
        let path = confine(&self.data_root, rel)?;
        let io =
            |e: std::io::Error| StdError::new(StdErrorKind::Io, msg::std_io(rel, &e.to_string()));
        match function {
            "read_text" => {
                let meta = std::fs::metadata(&path).map_err(io)?;
                if meta.len() > MAX_READ_BYTES {
                    return Err(StdError::new(
                        StdErrorKind::Io,
                        msg::std_file_too_large(rel, MAX_READ_BYTES >> 20),
                    ));
                }
                let bytes = std::fs::read(&path).map_err(io)?;
                String::from_utf8(bytes)
                    .map(HostValue::Str)
                    .map_err(|_| StdError::new(StdErrorKind::Io, msg::std_not_utf8(rel)))
            }
            "write_text" | "append_text" => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(io)?;
                }
                // Re-check after creating folders: a symlinked folder may point elsewhere.
                confine(&self.data_root, rel)?;
                let text = str_arg(args, 1);
                if function == "write_text" {
                    std::fs::write(&path, text).map_err(io)?;
                } else {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .and_then(|mut f| f.write_all(text.as_bytes()))
                        .map_err(io)?;
                }
                Ok(HostValue::Unit)
            }
            "exists" => Ok(HostValue::Bool(path.is_file())),
            "remove" => match std::fs::remove_file(&path) {
                Ok(()) => Ok(HostValue::Unit),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HostValue::Unit),
                Err(e) => Err(io(e)),
            },
            _ => Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::std_unavailable(&format!("std.fs.{function}"), "this host"),
            )),
        }
    }
}

impl Host for NativeStd {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        NativeStd::call(self, module, function, &args)
    }

    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

/// A host that refuses every effect: for `qxide qasm` (only `std.time` is read) and as a
/// fallback. `std.log` is discarded.
pub struct NoEffects {
    pub what: &'static str,
}

impl Host for NoEffects {
    fn call(&mut self, module: &str, function: &str, _args: Vec<HostValue>) -> StdResult {
        match (module, function) {
            ("log", _) => Ok(HostValue::Unit),
            ("time", "now_millis") => Ok(HostValue::Int(now_millis())),
            ("time", "now_iso") => Ok(HostValue::Str(iso_utc(now_millis()))),
            ("time", "sleep_millis") => Ok(HostValue::Unit),
            _ => Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::std_unavailable(&format!("std.{module}.{function}"), self.what),
            )),
        }
    }
}

/// Collects `std.log` lines and refuses everything else: the REPL and the playground.
#[derive(Default)]
pub struct LogOnly {
    pub lines: Vec<String>,
}

impl Host for LogOnly {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        if module == "log" {
            let level = match function {
                "warn" => LogLevel::Warn,
                "error" => LogLevel::Error,
                _ => LogLevel::Info,
            };
            self.lines.push(log_line(level, str_arg(&args, 0)));
            return Ok(HostValue::Unit);
        }
        Err(StdError::new(
            StdErrorKind::Unavailable,
            msg::std_unavailable(&format!("std.{module}"), "this host"),
        ))
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `2026-11-21T02:03:04.005Z` for Unix milliseconds (proleptic Gregorian, UTC).
pub fn iso_utc(ms: i64) -> String {
    let (secs, milli) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether `h` may appear in `[app] http_hosts`: a lowercase DNS name (letters, digits, `-` and
/// `.`; labels of 1 to 63 characters that neither start nor end with `-`), at most 253 long.
pub fn valid_http_host(h: &str) -> bool {
    h.len() <= 253
        && h.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Checks a `std.http.get` URL: `https://` (scheme in any case), a host listed in `hosts`, port
/// 443 or none, no user info, no spaces, control characters or backslashes, and at most
/// `MAX_URL_LEN` bytes. Returns the URL unchanged.
pub fn allowed_url<'a>(url: &'a str, hosts: &[String]) -> Result<&'a str, StdError> {
    let bad = || StdError::new(StdErrorKind::Url, msg::http_bad_url(url));
    if url.len() > MAX_URL_LEN
        || url
            .bytes()
            .any(|b| b <= b' ' || b == 0x7f || b == b'\\' || b >= 0x80)
    {
        return Err(bad());
    }
    let rest = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])
        .ok_or_else(bad)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(bad());
    }
    let host = match authority.split_once(':') {
        Some((h, "443")) => h,
        Some(_) => return Err(bad()),
        None => authority,
    };
    let host = host.to_ascii_lowercase();
    if !valid_http_host(&host) {
        return Err(bad());
    }
    if !hosts.contains(&host) {
        return Err(StdError::new(
            StdErrorKind::Url,
            msg::http_host_denied(&host),
        ));
    }
    Ok(url)
}

/// Resolves a `std.fs` path inside `root`. Only plain relative paths are accepted: no absolute
/// paths, drive letters, UNC prefixes, `..`, reserved device names or empty segments, and no
/// existing part of the path may lead (through a symlink) outside `root`.
pub fn confine(root: &Path, rel: &str) -> Result<PathBuf, StdError> {
    let bad = || StdError::new(StdErrorKind::Path, msg::std_bad_path(rel));
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\0') || rel.contains(':') {
        return Err(bad());
    }
    let norm = rel.replace('\\', "/");
    if norm.starts_with('/') {
        return Err(bad());
    }
    let mut out = root.to_path_buf();
    for seg in norm.split('/') {
        if seg.is_empty() || seg == "." {
            if seg.is_empty() {
                return Err(bad());
            }
            continue;
        }
        let stem = seg.split('.').next().unwrap_or(seg).to_ascii_uppercase();
        if seg == ".." || RESERVED.contains(&stem.as_str()) || seg.ends_with(['.', ' ']) {
            return Err(bad());
        }
        out.push(seg);
    }
    if !Path::new(&norm)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(bad());
    }
    // Security: the root itself must not be a symlink or junction (B-A2); an absent root has
    // nothing below it yet.
    let root_c = match root.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => return Err(bad()),
        Ok(_) => root.canonicalize().map_err(|_| bad())?,
        Err(_) => return Ok(out),
    };
    // Symlinks: the deepest existing ancestor must still be inside the canonical root, and an
    // entry that exists but does not resolve (a dangling symlink) is refused (B-A1).
    let mut probe = out.clone();
    loop {
        match probe.canonicalize() {
            Ok(c) if c.starts_with(&root_c) => break,
            Ok(_) => return Err(bad()),
            Err(_) if probe.symlink_metadata().is_ok() => return Err(bad()),
            Err(_) => {}
        }
        if !probe.pop() || !probe.starts_with(root) {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_utc(1_795_226_584_005), "2026-11-21T02:03:04.005Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso_utc(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn confine_rejects_escapes() {
        let root = std::env::temp_dir().join("qxide-confine-test");
        std::fs::create_dir_all(&root).unwrap();
        for bad in [
            "",
            "/etc/passwd",
            "\\x",
            "C:\\x",
            "c:x",
            "\\\\server\\share\\x",
            "../x",
            "a/../../x",
            "a//b",
            "CON",
            "nul.txt",
            "a/aux",
            "x.",
            "x ",
            "a\0b",
        ] {
            assert!(confine(&root, bad).is_err(), "{bad:?} should be rejected");
        }
        for good in ["a.txt", "dir/b.txt", "./c.txt", "dir\\d.txt", ".hidden"] {
            let p = confine(&root, good).unwrap();
            assert!(p.starts_with(&root), "{good:?}");
        }
    }

    #[test]
    fn fs_round_trip_and_errors() {
        let root = std::env::temp_dir().join(format!("qxide-fs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = NativeStd::new(root.clone(), Box::new(|_, _| {}));
        let p = |v: &str| HostValue::Str(v.into());
        s.call("fs", "write_text", &[p("d/a.txt"), p("one")])
            .unwrap();
        s.call("fs", "append_text", &[p("d/a.txt"), p(" two")])
            .unwrap();
        assert_eq!(
            s.call("fs", "read_text", &[p("d/a.txt")]).unwrap(),
            p("one two")
        );
        assert_eq!(
            s.call("fs", "exists", &[p("d/a.txt")]).unwrap(),
            HostValue::Bool(true)
        );
        s.call("fs", "remove", &[p("d/a.txt")]).unwrap();
        s.call("fs", "remove", &[p("d/a.txt")]).unwrap();
        let e = s.call("fs", "read_text", &[p("d/a.txt")]).unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Io);
        let e = s.call("fs", "read_text", &[p("../a.txt")]).unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Path);
        let e = s
            .call("time", "sleep_millis", &[HostValue::Int(60_001)])
            .unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Argument);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Makes a symlink for the escape tests. Windows needs Developer Mode or admin rights: without
    /// them the test is skipped locally, but fails on CI so the check never silently disappears.
    fn symlink(target: &Path, link: &Path, dir: bool) -> bool {
        #[cfg(unix)]
        let r = {
            let _ = dir;
            std::os::unix::fs::symlink(target, link)
        };
        #[cfg(windows)]
        let r = if dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };
        match r {
            Ok(()) => true,
            Err(e) if std::env::var_os("CI").is_none() => {
                eprintln!("skipping symlink test: cannot create symlinks here ({e})");
                false
            }
            Err(e) => panic!("cannot create a symlink on CI: {e}"),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qxide-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // B-A1: a symlink inside the data root whose target does not exist yet must not let
    // `write_text`/`append_text` create that target outside the root.
    #[test]
    fn fs_refuses_dangling_symlink_escape() {
        let base = scratch("ba1");
        let (root, outside) = (base.join("data"), base.join("outside"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let target = outside.join("victim.txt");
        if !symlink(&target, &root.join("notes.txt"), false)
            || !symlink(&outside.join("newdir"), &root.join("sub"), true)
        {
            return;
        }
        let mut s = NativeStd::new(root.clone(), Box::new(|_, _| {}));
        let p = |v: &str| HostValue::Str(v.into());
        for f in ["write_text", "append_text"] {
            let e = s.call("fs", f, &[p("notes.txt"), p("x")]).unwrap_err();
            assert_eq!(e.kind, StdErrorKind::Path, "{f}");
            let e = s.call("fs", f, &[p("sub/x.txt"), p("x")]).unwrap_err();
            assert_eq!(
                e.kind,
                StdErrorKind::Path,
                "{f} through a dangling folder link"
            );
        }
        assert!(!target.exists(), "write escaped the data root");
        assert!(
            !outside.join("newdir").exists(),
            "folder created outside the data root"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // B-A2: the data root itself may be a symlink (or a Windows junction) to another folder.
    #[test]
    fn fs_refuses_symlinked_data_root() {
        let base = scratch("ba2");
        let outside = base.join("home");
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(windows)]
        {
            // A junction needs no privilege on Windows, so this part runs even without symlinks.
            let jroot = base.join("jdata");
            let ok = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&jroot)
                .arg(&outside)
                .output()
                .is_ok_and(|o| o.status.success());
            assert!(ok, "mklink /J failed");
            let p = |v: &str| HostValue::Str(v.into());
            let mut s = NativeStd::new(jroot.clone(), Box::new(|_, _| {}));
            let e = s
                .call("fs", "write_text", &[p("b.txt"), p("x")])
                .unwrap_err();
            assert_eq!(e.kind, StdErrorKind::Path);
            assert!(
                !outside.join("b.txt").exists(),
                "write went through the junction"
            );
        }
        let root = base.join("data");
        if !symlink(&outside, &root, true) {
            return;
        }
        let mut s = NativeStd::new(root.clone(), Box::new(|_, _| {}));
        let p = |v: &str| HostValue::Str(v.into());
        let e = s
            .call("fs", "write_text", &[p("a.txt"), p("x")])
            .unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Path);
        assert!(confine(&root, "a.txt").is_err());
        assert!(
            !outside.join("a.txt").exists(),
            "write went through the root link"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn fs_allows_symlinks_that_stay_inside() {
        let base = scratch("fsin");
        let root = base.join("data");
        std::fs::create_dir_all(root.join("real")).unwrap();
        if !symlink(&root.join("real"), &root.join("alias"), true) {
            return;
        }
        let mut s = NativeStd::new(root.clone(), Box::new(|_, _| {}));
        let p = |v: &str| HostValue::Str(v.into());
        s.call("fs", "write_text", &[p("alias/a.txt"), p("ok")])
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("real").join("a.txt")).unwrap(),
            "ok"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn http_urls_must_be_https_on_listed_hosts() {
        let hosts = vec!["api.example.com".to_string(), "10.0.0.1".to_string()];
        for ok in [
            "https://api.example.com",
            "https://api.example.com/v1/items?q=a&b=%20#top",
            "HTTPS://API.Example.COM/x",
            "https://api.example.com:443/x",
            "https://10.0.0.1/",
        ] {
            assert_eq!(allowed_url(ok, &hosts), Ok(ok), "{ok}");
        }
        for (bad, kind) in [
            ("http://api.example.com/", StdErrorKind::Url),
            ("ftp://api.example.com/", StdErrorKind::Url),
            ("https://", StdErrorKind::Url),
            ("https:/api.example.com", StdErrorKind::Url),
            ("https://evil.com/", StdErrorKind::Url),
            ("https://api.example.com.evil.com/", StdErrorKind::Url),
            ("https://evil.com#@api.example.com", StdErrorKind::Url),
            ("https://user@api.example.com/", StdErrorKind::Url),
            ("https://api.example.com:8443/", StdErrorKind::Url),
            ("https://api.example.com:/", StdErrorKind::Url),
            ("https://[::1]/", StdErrorKind::Url),
            ("https://api.example.com/a b", StdErrorKind::Url),
            ("https://api.example.com\\@evil.com/", StdErrorKind::Url),
            ("https://api.example.com/\n", StdErrorKind::Url),
            ("https://api%2eexample.com/", StdErrorKind::Url),
            ("https://äpi.example.com/", StdErrorKind::Url),
        ] {
            let e = allowed_url(bad, &hosts).unwrap_err();
            assert_eq!(e.kind, kind, "{bad}");
        }
        let long = format!("https://api.example.com/{}", "a".repeat(MAX_URL_LEN));
        assert!(allowed_url(&long, &hosts).is_err());
        assert!(valid_http_host("a-1.b.example"));
        for h in [
            "", "A.com", "-a.com", "a-.com", "a..com", "a.com.", "*.a.com", "a_b.com",
        ] {
            assert!(!valid_http_host(h), "{h:?}");
        }
    }

    fn with_http(
        reply: Result<HttpReply, String>,
    ) -> (NativeStd, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let mut s = NativeStd::new(std::env::temp_dir(), Box::new(|_, _| {}));
        s.http = Some(HttpAccess {
            hosts: vec!["api.example.com".into()],
            get: Box::new(move |url, _cancelled| {
                log.lock().unwrap().push(url.to_string());
                reply.clone()
            }),
        });
        (s, seen)
    }

    #[test]
    fn http_get_returns_text_bodies_of_2xx_replies() {
        let url = HostValue::Str("https://api.example.com/v1".into());
        let ok = |status, body: &[u8]| {
            Ok(HttpReply {
                status,
                body: body.to_vec(),
            })
        };
        let (mut s, seen) = with_http(ok(200, "héllo".as_bytes()));
        assert_eq!(
            s.call("http", "get", std::slice::from_ref(&url)),
            Ok(HostValue::Str("héllo".into()))
        );
        assert_eq!(*seen.lock().unwrap(), ["https://api.example.com/v1"]);
        let (mut s, _) = with_http(ok(204, b""));
        assert_eq!(
            s.call("http", "get", std::slice::from_ref(&url)),
            Ok(HostValue::Str(String::new()))
        );
        for reply in [
            ok(404, b"not found"),
            ok(301, b""),
            ok(200, &[0xff, 0xfe]),
            ok(200, &vec![b'a'; MAX_HTTP_BYTES + 1]),
            Err("could not resolve host".into()),
        ] {
            let (mut s, _) = with_http(reply);
            let e = s
                .call("http", "get", std::slice::from_ref(&url))
                .unwrap_err();
            assert_eq!(e.kind, StdErrorKind::Http, "{}", e.message);
        }
        // Refused URLs never reach the transport.
        let (mut s, seen) = with_http(ok(200, b"x"));
        let e = s
            .call("http", "get", &[HostValue::Str("https://evil.com/".into())])
            .unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Url);
        assert!(seen.lock().unwrap().is_empty());
        // A cancelled request is reported as a cancellation, not as a network error.
        let (mut s, _) = with_http(Err("killed".into()));
        s.cancel = Some(Arc::new(AtomicBool::new(true)));
        let e = s.call("http", "get", &[url]).unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Cancelled);
        // Hosts without a transport (the CLI) do not offer `std.http`.
        let mut cli = NativeStd::new(std::env::temp_dir(), Box::new(|_, _| {}));
        let e = cli
            .call(
                "http",
                "get",
                &[HostValue::Str("https://api.example.com/".into())],
            )
            .unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Unavailable);
    }

    #[test]
    fn permissions_by_host() {
        let cli = Permissions::cli(false);
        assert_eq!(cli.access("time"), Some(Access::Allowed));
        assert!(matches!(cli.access("fs"), Some(Access::Denied(_))));
        assert!(matches!(cli.access("app"), Some(Access::Unavailable(_))));
        assert_eq!(Permissions::cli(true).access("fs"), Some(Access::Allowed));
        let app = Permissions::app(&["fs".into()]);
        assert_eq!(app.access("log"), Some(Access::Allowed));
        assert_eq!(app.access("fs"), Some(Access::Allowed));
        assert!(matches!(app.access("time"), Some(Access::Denied(_))));
        // `std.http` is only for apps that list it; no other host offers it.
        assert!(matches!(app.access("http"), Some(Access::Denied(_))));
        let net = Permissions::app(&["http".into()]);
        assert_eq!(net.access("http"), Some(Access::Allowed));
        for host in [
            Permissions::cli(true),
            Permissions::playground(),
            Permissions::repl(),
        ] {
            assert!(matches!(host.access("http"), Some(Access::Unavailable(_))));
        }
        assert_eq!(app.access("shell"), None);
        assert!(matches!(
            Permissions::playground().access("time"),
            Some(Access::Unavailable(_))
        ));
    }
}
