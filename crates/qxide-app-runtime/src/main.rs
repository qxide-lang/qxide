//! `qxide-app`: the prebuilt desktop runtime of Qxide apps (docs/design/qx-app.md).
//!
//! A tao window with a wry webview (the OS webview: WebView2, WKWebView or WebKitGTK). The page
//! is served from the app bundle through the `qxapp` custom protocol, and calls the `.qx`
//! commands listed in `[app] commands` through `window.ipc`; each call runs the shared `.qx`
//! interpreter (`src/lang`, as in the CLI and the playground) on a worker thread.
//!
//! `qxide-app` runs the app attached to its own executable by `qxide app build`;
//! `qxide-app --dev <dev.json>` runs a project from disk for `qxide app dev`.

// Built apps are GUI programs: no console window. `qxide app dev` passes its own stderr.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(dead_code)]
#[path = "../../../src/lang/mod.rs"]
mod lang;

#[allow(dead_code)]
#[path = "../../../src/app/bundle.rs"]
mod bundle;

mod app;
mod host;
mod msg;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::http::{header, Request, Response, StatusCode};
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

use app::App;
use lang::command::{self, Call, JsArg};
use lang::stdlib::Permissions;

/// Events for the UI thread, sent by IPC handling and worker threads.
pub enum UserEvent {
    /// Script to run in the page (Promise settlement, `std.app.emit`, dev logs).
    Eval(String),
    SetTitle(String),
    SetSize(u32, u32),
    Close,
}

const SCHEME: &str = "qxapp";
const BRIDGE: &str = include_str!("bridge.js");
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";
// The interpreter recurses per nested call; same stack as `qxide run`.
const LANG_STACK: usize = 64 << 20;

/// The page origin wry gives the `qxapp` protocol on this platform.
fn origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://qxapp.localhost"
    } else {
        "qxapp://localhost"
    }
}

fn same_origin(url: &str) -> bool {
    let o = origin();
    url == o || url.starts_with(&format!("{o}/"))
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" | "md" | "qx" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn serve(app: &App, req: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let path = percent_decode(path);
    let builder = Response::builder()
        .header(header::CONTENT_SECURITY_POLICY, CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CACHE_CONTROL, "no-store");
    match app.ui_file(&path) {
        Some(data) => builder
            .header(header::CONTENT_TYPE, mime(&path))
            .body(Cow::Owned(data)),
        None => builder
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Cow::Borrowed(&b"404"[..])),
    }
    .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Limits concurrent calls to the number of CPU cores; extra calls wait.
struct Slots {
    free: Mutex<usize>,
    cv: Condvar,
}

impl Slots {
    fn acquire(&self) {
        let mut n = self.free.lock().unwrap();
        while *n == 0 {
            n = self.cv.wait(n).unwrap();
        }
        *n -= 1;
    }

    fn release(&self) {
        *self.free.lock().unwrap() += 1;
        self.cv.notify_one();
    }
}

struct Shared {
    app: App,
    proxy: EventLoopProxy<UserEvent>,
    slots: Slots,
    calls: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

fn settle(proxy: &EventLoopProxy<UserEvent>, id: u64, ok: bool, json: &str) {
    let _ = proxy.send_event(UserEvent::Eval(format!(
        "window.__qxide_settle && window.__qxide_settle({id},{ok},{json});"
    )));
}

fn reject_app(proxy: &EventLoopProxy<UserEvent>, id: u64, code: &str, message: String) {
    let mut s = String::from("{\"code\":");
    command::json_str(&mut s, code);
    s.push_str(",\"message\":");
    command::json_str(&mut s, &message);
    s.push_str(",\"diagnostics\":[],\"rendered\":");
    command::json_str(&mut s, &format!("error[{code}]: {message}\n"));
    s.push('}');
    settle(proxy, id, false, &s);
}

fn js_arg(v: &Value) -> JsArg {
    match v {
        Value::Null => JsArg::Null,
        Value::Bool(b) => JsArg::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map(JsArg::Int)
            .unwrap_or_else(|| JsArg::Float(n.as_f64().unwrap_or(f64::NAN))),
        Value::String(s) => JsArg::Str(s.clone()),
        Value::Array(_) => JsArg::Array,
        Value::Object(_) => JsArg::Object,
    }
}

fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    // SplitMix64 finaliser, so consecutive calls get unrelated seeds.
    let mut z = t ^ (std::process::id() as u64).rotate_left(32);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) >> 11 // within 2^53 so it round-trips through a JavaScript number
}

/// One IPC message from the page. Unknown kinds or fields are rejected.
fn handle_ipc(shared: &Arc<Shared>, url: &str, body: &str) {
    if !same_origin(url) {
        eprintln!("{}", msg::ipc_foreign_origin(url));
        return;
    }
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(body) else {
        return;
    };
    let Some(id) = m.get("id").and_then(Value::as_u64) else {
        return;
    };
    let kind = m.get("kind").and_then(Value::as_str).unwrap_or("");
    let allowed: &[&str] = match kind {
        "invoke" => &["kind", "id", "command", "args", "shots", "seed"],
        "cancel" => &["kind", "id", "target"],
        "commands" => &["kind", "id"],
        _ => &[],
    };
    if allowed.is_empty() || m.keys().any(|k| !allowed.contains(&k.as_str())) {
        reject_app(&shared.proxy, id, "Q0712", msg::bad_message());
        return;
    }
    match kind {
        "cancel" => {
            if let Some(t) = m.get("target").and_then(Value::as_u64) {
                if let Some(flag) = shared.calls.lock().unwrap().get(&t) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        }
        "commands" => {
            let shared = shared.clone();
            spawn(move || {
                let app = &shared.app;
                let r = with_input(app, |input| {
                    command::signatures(input, &app.module, &app.commands)
                });
                match r {
                    Ok(Ok(sigs)) => {
                        settle(&shared.proxy, id, true, &command::signatures_json(&sigs))
                    }
                    Ok(Err(e)) => settle(&shared.proxy, id, false, &e.to_json()),
                    Err(e) => reject_app(&shared.proxy, id, "Q0412", e),
                }
            });
        }
        _ => {
            let command = m
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args: Vec<(String, JsArg)> = match m.get("args") {
                Some(Value::Object(a)) => a.iter().map(|(k, v)| (k.clone(), js_arg(v))).collect(),
                None | Some(Value::Null) => Vec::new(),
                Some(_) => {
                    reject_app(&shared.proxy, id, "Q0712", msg::bad_message());
                    return;
                }
            };
            let shots = m.get("shots").and_then(Value::as_u64).unwrap_or(0);
            let seed = match m.get("seed") {
                None | Some(Value::Null) => Some(random_seed()),
                Some(Value::String(s)) => s.trim().parse().ok(),
                Some(Value::Number(n)) => n.as_u64(),
                _ => None,
            };
            let Some(seed) = seed else {
                reject_app(&shared.proxy, id, "Q0712", msg::bad_message());
                return;
            };
            let flag = Arc::new(AtomicBool::new(false));
            shared.calls.lock().unwrap().insert(id, flag.clone());
            let shared = shared.clone();
            spawn(move || {
                shared.slots.acquire();
                let r = run_call(&shared, &command, &args, shots, seed, flag);
                shared.slots.release();
                shared.calls.lock().unwrap().remove(&id);
                match r {
                    Ok(Ok(json)) => settle(&shared.proxy, id, true, &json),
                    Ok(Err(json)) => settle(&shared.proxy, id, false, &json),
                    Err(e) => reject_app(&shared.proxy, id, "Q0412", e),
                }
            });
        }
    }
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().stack_size(LANG_STACK).spawn(f);
}

/// Builds the `Input` for the command module (re-read on every call, so dev edits apply).
fn with_input<T>(app: &App, f: impl FnOnce(&lang::Input) -> T) -> Result<T, String> {
    let entry = PathBuf::from(&app.entry);
    let src = app
        .read_qx(&entry)
        .map_err(|e| format!("{}: {e}", app.entry))?;
    let read = |p: &std::path::Path| app.read_qx(p);
    let input = lang::Input {
        name: &app.entry,
        src: &src,
        project: Some(&app.project),
        read: &read,
        perms: Permissions::app(&app.permissions),
    };
    Ok(f(&input))
}

fn run_call(
    shared: &Shared,
    name: &str,
    args: &[(String, JsArg)],
    shots: u64,
    seed: u64,
    flag: Arc<AtomicBool>,
) -> Result<Result<String, String>, String> {
    let app = &shared.app;
    if flag.load(Ordering::Relaxed) {
        let e = lang::command::cancelled_error();
        return Ok(Err(e.to_json()));
    }
    let mut host = host::AppHost::new(
        app.data.clone(),
        Permissions::app(&app.permissions),
        shared.proxy.clone(),
        flag,
        app.dev,
    );
    with_input(app, |input| {
        let call = Call {
            module: &app.module,
            commands: &app.commands,
            name,
            args,
            shots,
            seed,
            limits: lang::Limits::CLI,
        };
        command::call(input, &call, &mut host)
            .map(|r| r.to_json())
            .map_err(|e| e.to_json())
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let loaded = match args.as_slice() {
        [] => App::from_exe(),
        [flag, path] if flag == "--dev" => App::from_dev(std::path::Path::new(path)),
        [v] if v == "--version" => {
            println!("qxide-app {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => Err(msg::usage()),
    };
    let app = match loaded {
        Ok(a) => a,
        Err(e) => {
            eprintln!("qxide-app: {e}");
            return ExitCode::from(1);
        }
    };
    match run(app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("qxide-app: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(app: App) -> Result<(), String> {
    let _ = std::fs::create_dir_all(&app.data);
    let _ = std::fs::create_dir_all(&app.webview_data);
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title(&app.title)
        .with_inner_size(LogicalSize::new(app.width, app.height))
        .with_resizable(app.resizable)
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    let dev = app.dev;
    let shared = Arc::new(Shared {
        proxy: event_loop.create_proxy(),
        slots: Slots {
            free: Mutex::new(cores),
            cv: Condvar::new(),
        },
        calls: Mutex::new(HashMap::new()),
        app,
    });
    let mut context = WebContext::new(Some(shared.app.webview_data.clone()));
    let serve_shared = shared.clone();
    let ipc_shared = shared.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_initialization_script(BRIDGE)
        .with_custom_protocol(SCHEME.into(), move |_id, req| {
            serve(&serve_shared.app, &req)
        })
        .with_ipc_handler(move |req: Request<String>| {
            handle_ipc(&ipc_shared, &req.uri().to_string(), req.body())
        })
        .with_navigation_handler(|url| same_origin(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_devtools(dev)
        .with_url(format!("{SCHEME}://localhost/"));
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let webview = builder.build(&window).map_err(|e| e.to_string())?;
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder
            .build_gtk(window.default_vbox().ok_or_else(|| String::from("GTK"))?)
            .map_err(|e| e.to_string())?
    };
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let quit = |cf: &mut ControlFlow| {
            for flag in shared.calls.lock().unwrap().values() {
                flag.store(true, Ordering::Relaxed);
            }
            *cf = ControlFlow::Exit;
        };
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => quit(control_flow),
            Event::UserEvent(UserEvent::Eval(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::SetTitle(t)) => window.set_title(&t),
            Event::UserEvent(UserEvent::SetSize(w, h)) => {
                window.set_inner_size(LogicalSize::new(w, h))
            }
            Event::UserEvent(UserEvent::Close) => quit(control_flow),
            _ => {}
        }
    });
}
