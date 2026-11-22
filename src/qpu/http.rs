//! HTTP through the system `curl`, like the package registry (src/pkg/index.rs), so the CLI needs
//! no HTTP or TLS crate. Everything is passed as a curl config on stdin: API keys, bearer tokens
//! and request bodies never appear in the process arguments.

use std::io::Write as _;
use std::process::Stdio;

use super::msg;

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

// Results of a large job are a few MB; anything far bigger is not a sampler result.
const MAX_BODY: u64 = 256 << 20;

/// Sends one request. `url` must be `https://`, or `http://` on a loopback host (`Endpoint`
/// checks overrides before they get here). Redirects are not followed, so credentials only go to
/// the host they were meant for.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Result<Response, String> {
    assert!(
        url.starts_with("https://") || is_loopback(url),
        "plain HTTP only to loopback hosts"
    );
    let mut config = String::new();
    let mut line = |key: &str, value: &str| {
        config.push_str(key);
        config.push_str(" = \"");
        config.push_str(&quote(value));
        config.push_str("\"\n");
    };
    line("url", url);
    line("request", method);
    line(
        "proto",
        if url.starts_with("http://") {
            "=http"
        } else {
            "=https"
        },
    );
    line("max-time", "120");
    line("max-filesize", &MAX_BODY.to_string());
    line("write-out", "\n%{http_code}");
    // curl would otherwise wait for `100 Continue` before sending larger bodies.
    line("header", "Expect:");
    for (name, value) in headers {
        line("header", &format!("{name}: {value}"));
    }
    if let Some(body) = body {
        // A leading `@` would make curl read a file instead.
        assert!(!body.starts_with('@'), "request bodies are JSON or forms");
        line("data-binary", body);
    }
    let mut child = crate::pkg::command("curl")
        .and_then(|mut curl| {
            curl.args(["--silent", "--show-error", "--globoff", "--config", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        })
        .map_err(|e| msg::curl_missing(&e.to_string()))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let written = stdin.write_all(config.as_bytes());
    drop(stdin);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    written.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(msg::request_failed(
            url,
            String::from_utf8_lossy(&out.stderr).trim(),
        ));
    }
    let mut body = out.stdout;
    let split = body.iter().rposition(|&b| b == b'\n').unwrap_or(0);
    let status = std::str::from_utf8(&body[split..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| msg::request_failed(url, "no HTTP status"))?;
    body.truncate(split);
    Ok(Response { status, body })
}

// A curl config string: backslash escapes inside double quotes.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// `application/x-www-form-urlencoded` value.
pub fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Whether `url` is `http(s)://` on `localhost` or a loopback address: the only hosts an
/// endpoint override may point to, so a changed environment cannot send keys elsewhere.
pub fn is_loopback(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map_or(authority, |(h, _)| h)
    };
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_strings_are_escaped() {
        assert_eq!(quote(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(quote("x\ny\rz\t"), "x\\ny\\rz\\t");
    }

    #[test]
    fn form_values_are_percent_encoded() {
        assert_eq!(form_encode("a b+c/=&~"), "a%20b%2Bc%2F%3D%26~");
    }

    #[test]
    fn only_loopback_hosts_count() {
        for ok in [
            "http://127.0.0.1:8080/api",
            "http://localhost/x",
            "https://[::1]:9/x",
            "http://127.0.0.2",
        ] {
            assert!(is_loopback(ok), "{ok}");
        }
        for bad in [
            "https://quantum.cloud.ibm.com/api",
            "http://127.0.0.1.example.com/",
            "http://user@127.0.0.1/",
            "http://evil.com#@127.0.0.1",
            "ftp://127.0.0.1/",
            "127.0.0.1",
        ] {
            assert!(!is_loopback(bad), "{bad}");
        }
    }
}
