// `std.http` transport of the app runtime: the system `curl` (as the package manager uses), so the
// runtime needs no TLS stack of its own. The URL policy (https, `[app] http_hosts`, size and
// status checks) lives in `src/lang/stdlib.rs`; this file only moves bytes.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::lang::stdlib::{HttpReply, MAX_HTTP_BYTES};
use crate::msg;

/// Whole-request time limit, in seconds; connecting alone may take at most `CONNECT_SECS`.
pub const TIMEOUT_SECS: u64 = 30;
const CONNECT_SECS: u64 = 10;
/// Bytes kept from curl's error output.
const MAX_STDERR: u64 = 64 << 10;

/// Arguments for one GET: https only, no redirects (they could leave the allowed hosts), no URL
/// globbing, time and size limits, and the status code printed after the body.
pub fn curl_args(url: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        // Must come first: ignore the user's `.curlrc`, which could add output or TLS options.
        "--disable",
        "--silent",
        "--show-error",
        "--globoff",
        "--proto",
        "=https",
        "--max-redirs",
        "0",
        "--tlsv1.2",
        "--connect-timeout",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(CONNECT_SECS.to_string());
    a.push("--max-time".into());
    a.push(TIMEOUT_SECS.to_string());
    a.push("--max-filesize".into());
    a.push(MAX_HTTP_BYTES.to_string());
    a.push("--user-agent".into());
    a.push(format!("qxide-app/{}", env!("CARGO_PKG_VERSION")));
    a.push("--write-out".into());
    a.push("%{http_code}".into());
    a.push("--url".into());
    a.push(url.to_string());
    a
}

/// Splits curl's stdout into the body and the three-digit status code `--write-out` printed last.
pub fn parse_output(mut out: Vec<u8>) -> Result<HttpReply, String> {
    let n = out.len();
    let code = (n >= 3)
        .then(|| std::str::from_utf8(&out[n - 3..]).ok())
        .flatten()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|&c| (100..=599).contains(&c))
        .ok_or_else(msg::http_no_status)?;
    out.truncate(n - 3);
    Ok(HttpReply {
        status: code,
        body: out,
    })
}

/// `curl` from the Windows system folder when present, so a `curl.exe` placed next to the app
/// cannot stand in for it; elsewhere the `PATH` one.
fn curl() -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let system = std::env::var_os("SystemRoot")
            .map(|r| {
                std::path::PathBuf::from(r)
                    .join("System32")
                    .join("curl.exe")
            })
            .filter(|p| p.is_file());
        let mut c = Command::new(system.as_deref().unwrap_or("curl".as_ref()));
        // Built apps have no console; without this every request would flash a console window.
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    Command::new("curl")
}

/// Reads at most `cap` bytes; `full` turns true when the cap was reached.
fn read_capped(
    r: impl Read + Send + 'static,
    cap: u64,
    full: Arc<AtomicBool>,
) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut r = r.take(cap);
        let _ = r.read_to_end(&mut buf);
        if buf.len() as u64 >= cap {
            full.store(true, Ordering::Relaxed);
        }
        buf
    })
}

/// One GET of an already validated URL. Gives up when `cancelled` reports true, after the time
/// limit, or once the body exceeds `MAX_HTTP_BYTES` (returned truncated to one byte over).
pub fn get(url: &str, cancelled: &dyn Fn() -> bool) -> Result<HttpReply, String> {
    let mut child = curl()
        .args(curl_args(url))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| msg::curl_missing(&e.to_string()))?;
    // Body, one byte over the limit, and the 3-digit status code.
    let cap = (MAX_HTTP_BYTES + 1 + 3) as u64;
    let full = Arc::new(AtomicBool::new(false));
    let out = read_capped(
        child.stdout.take().expect("piped stdout"),
        cap,
        full.clone(),
    );
    // Stderr past its cap is dropped; curl stops on its own time limit if it keeps writing.
    let err = read_capped(
        child.stderr.take().expect("piped stderr"),
        MAX_STDERR,
        Arc::new(AtomicBool::new(false)),
    );
    let deadline = Instant::now() + Duration::from_secs(TIMEOUT_SECS + 5);
    let stop = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let status = loop {
        if cancelled() {
            stop(&mut child);
            return Err(msg::http_cancelled());
        }
        if Instant::now() > deadline {
            stop(&mut child);
            return Err(msg::http_timeout(TIMEOUT_SECS));
        }
        // The body hit the cap: curl would block on a full pipe, and the reply is too large anyway.
        if full.load(Ordering::Relaxed) {
            stop(&mut child);
            let mut body = out.join().unwrap_or_default();
            body.truncate(MAX_HTTP_BYTES + 1);
            return Ok(HttpReply { status: 200, body });
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                stop(&mut child);
                return Err(e.to_string());
            }
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if !status.success() {
        let text = String::from_utf8_lossy(&err).trim().to_string();
        return Err(if text.is_empty() {
            msg::curl_exit(status.code().unwrap_or(-1))
        } else {
            text
        });
    }
    parse_output(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_gets_https_only_without_redirects() {
        let a = curl_args("https://api.example.com/x?a=1");
        assert_eq!(a[0], "--disable");
        let has = |k: &str, v: &str| a.windows(2).any(|w| w[0] == k && w[1] == v);
        assert!(has("--proto", "=https"));
        assert!(has("--max-redirs", "0"));
        assert!(has("--max-filesize", &MAX_HTTP_BYTES.to_string()));
        assert!(has("--max-time", &TIMEOUT_SECS.to_string()));
        assert!(has("--write-out", "%{http_code}"));
        assert!(a.contains(&"--globoff".to_string()));
        assert!(!a.iter().any(|s| s == "--location" || s == "-L"));
        assert_eq!(a[a.len() - 2..], ["--url", "https://api.example.com/x?a=1"]);
    }

    #[test]
    fn status_code_follows_the_body() {
        assert_eq!(
            parse_output(b"hello200".to_vec()),
            Ok(HttpReply {
                status: 200,
                body: b"hello".to_vec()
            })
        );
        assert_eq!(parse_output(b"404".to_vec()).unwrap().status, 404);
        for bad in [&b""[..], b"20", b"body000", b"bodyabc"] {
            assert!(parse_output(bad.to_vec()).is_err(), "{bad:?}");
        }
    }

    // Runs the real `curl` against a closed local port: no network traffic leaves the machine.
    #[test]
    fn refused_connections_and_cancellation_fail_quickly() {
        if curl().arg("--version").output().is_err() {
            eprintln!("skipping: no curl on this machine");
            return;
        }
        let t = Instant::now();
        assert!(get("https://127.0.0.1:9/", &|| false).is_err());
        assert!(get("https://127.0.0.1:9/", &|| true).is_err());
        assert!(
            t.elapsed() < Duration::from_secs(TIMEOUT_SECS),
            "{:?}",
            t.elapsed()
        );
    }
}
