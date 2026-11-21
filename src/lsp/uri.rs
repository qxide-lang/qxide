// `file:` URIs <-> paths. Editors send percent-encoded URIs such as `file:///d%3A/dev/a.qx`;
// documents stay keyed by the URI string the client sent, so this is only used to find a
// document's project and to name files that are not open.

use std::path::{Path, PathBuf};

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = hex(*bytes.get(i + 1)?)?;
            let lo = hex(*bytes.get(i + 2)?)?;
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// `/c:/x` (a Windows drive path inside a URI).
fn drive_path(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
}

/// The local path of a `file:` URI; `None` for other schemes and remote hosts.
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("FILE://"))?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let path = percent_decode(path.split(['?', '#']).next().unwrap_or(path))?;
    match host {
        "" | "localhost" => {}
        // `file://server/share/a.qx` is a UNC path on Windows only.
        _ if cfg!(windows) => {
            return Some(PathBuf::from(format!(
                r"\\{host}{}",
                path.replace('/', "\\")
            )))
        }
        _ => return None,
    }
    if cfg!(windows) {
        let p = if drive_path(&path) { &path[1..] } else { &path };
        Some(PathBuf::from(p.replace('/', "\\")))
    } else {
        Some(PathBuf::from(path))
    }
}

/// A `file:` URI for `path`, made absolute against the current directory, encoded the way
/// VS Code writes them (`file:///c%3A/dev/a.qx` on Windows).
pub fn from_path(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut p = abs.to_string_lossy().into_owned();
    if cfg!(windows) {
        p = p.replace('\\', "/");
        if let Some(unc) = p.strip_prefix("//") {
            return format!("file://{}", encode(unc, true));
        }
        if p.as_bytes().get(1) == Some(&b':') {
            p = format!("/{}{}", p[..1].to_ascii_lowercase(), &p[1..]);
        }
    }
    format!("file://{}", encode(&p, false))
}

fn encode(p: &str, keep_host: bool) -> String {
    let mut out = String::with_capacity(p.len());
    for (i, b) in p.bytes().enumerate() {
        let host_part = keep_host && !p[..i].contains('/');
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ if host_part => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Compares paths the way the file system does (case-insensitive on Windows).
pub fn path_key(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes_and_drive_letters() {
        if cfg!(windows) {
            assert_eq!(
                to_path("file:///d%3A/dev/my%20app/src/main.qx"),
                Some(PathBuf::from(r"d:\dev\my app\src\main.qx"))
            );
            assert_eq!(to_path("file:///C:/x.qx"), Some(PathBuf::from(r"C:\x.qx")));
        } else {
            assert_eq!(
                to_path("file:///home/u/my%20app/main.qx"),
                Some(PathBuf::from("/home/u/my app/main.qx"))
            );
        }
        assert_eq!(to_path("untitled:Untitled-1"), None);
        assert_eq!(to_path("file:///bad%zz"), None);
    }

    #[test]
    fn round_trips_through_from_path() {
        let p = std::env::temp_dir().join("큐 x#1.qx");
        let uri = from_path(&p);
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(!uri.contains(' ') && !uri.contains('#'), "{uri}");
        assert_eq!(path_key(&to_path(&uri).unwrap()), path_key(&p));
    }
}
