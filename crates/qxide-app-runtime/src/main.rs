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
use std::collections::{HashMap, VecDeque};
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

/// Resource limits of one command call or `std.qxide.run`.
const CALL_LIMITS: lang::Limits = lang::Limits::APP;
/// Most calls running at once; with `CALL_LIMITS` they share a 1 GiB simulation budget.
const MAX_WORKERS: usize = 4;
/// Largest script handed to the webview at once (a call result, an event, a dev log line).
pub const MAX_SCRIPT_BYTES: usize = 32 << 20;

/// Concurrent command calls: the CPU cores, at most `MAX_WORKERS`.
fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(MAX_WORKERS)
}

#[cfg(test)]
/// Most simulation memory one call may use under `l`.
fn call_bytes(l: lang::Limits) -> u64 {
    let sv = l
        .max_state_vector_qubits
        .map_or(qxide::DEFAULT_MEMORY_LIMIT, |n| 16u64 << n);
    sv.max(l.stabilizer_bytes)
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A fixed pool of worker threads (each with the interpreter's large stack) fed from a queue.
/// Waiting calls are queued closures, not threads.
struct Workers {
    queue: Mutex<VecDeque<Job>>,
    cv: Condvar,
}

impl Workers {
    fn new(n: usize) -> Result<Arc<Workers>, String> {
        let w = Arc::new(Workers {
            queue: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        });
        let mut started = 0;
        for _ in 0..n.max(1) {
            let me = w.clone();
            let spawned = std::thread::Builder::new()
                .stack_size(LANG_STACK)
                .spawn(move || me.work());
            started += usize::from(spawned.is_ok());
        }
        if started == 0 {
            return Err(msg::no_workers());
        }
        Ok(w)
    }

    fn work(&self) {
        loop {
            let job = {
                let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    match q.pop_front() {
                        Some(job) => break job,
                        None => q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner()),
                    }
                }
            };
            // A panicking call must not end the worker (its slot would be lost for good).
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        }
    }

    fn submit(&self, job: Job) {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(job);
        self.cv.notify_one();
    }
}

struct Shared {
    app: App,
    proxy: EventLoopProxy<UserEvent>,
    workers: Arc<Workers>,
    // In-flight calls by (page token, call id), for cancellation.
    calls: Mutex<HashMap<(String, u64), Arc<AtomicBool>>>,
}

/// Where an answer goes: the page token and the call id of the request.
#[derive(Clone)]
struct Reply {
    page: String,
    id: u64,
}

/// The page script that settles call `to` with `json`. A result too large to hand to the webview
/// rejects the call with Q0716 instead.
fn settle_script(to: &Reply, ok: bool, json: &str) -> String {
    let mut page = String::new();
    command::json_str(&mut page, &to.page);
    let script = |ok: bool, json: &str| {
        format!(
            "window.__qxide_settle && window.__qxide_settle({page},{},{ok},{json});",
            to.id
        )
    };
    if json.len() + page.len() + 128 <= MAX_SCRIPT_BYTES {
        return script(ok, json);
    }
    script(
        false,
        &app_error_json(
            "Q0716",
            &msg::result_too_large(json.len(), MAX_SCRIPT_BYTES),
        ),
    )
}

/// A `QxError` without source diagnostics.
fn app_error_json(code: &str, message: &str) -> String {
    let mut s = String::from("{\"code\":");
    command::json_str(&mut s, code);
    s.push_str(",\"message\":");
    command::json_str(&mut s, message);
    s.push_str(",\"diagnostics\":[],\"rendered\":");
    command::json_str(&mut s, &format!("error[{code}]: {message}\n"));
    s.push('}');
    s
}

fn settle(proxy: &EventLoopProxy<UserEvent>, to: &Reply, ok: bool, json: &str) {
    let _ = proxy.send_event(UserEvent::Eval(settle_script(to, ok, json)));
}

fn reject_app(proxy: &EventLoopProxy<UserEvent>, id: &Reply, code: &str, message: String) {
    settle(proxy, id, false, &app_error_json(code, &message));
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
    let (Some(n), Some(page)) = (
        m.get("id").and_then(Value::as_u64),
        m.get("page").and_then(Value::as_str),
    ) else {
        return;
    };
    let id = Reply {
        page: page.to_string(),
        id: n,
    };
    let kind = m.get("kind").and_then(Value::as_str).unwrap_or("");
    let allowed: &[&str] = match kind {
        "invoke" => &["kind", "id", "page", "command", "args", "shots", "seed"],
        "cancel" => &["kind", "id", "page", "target"],
        "commands" => &["kind", "id", "page"],
        _ => &[],
    };
    if allowed.is_empty() || m.keys().any(|k| !allowed.contains(&k.as_str())) {
        reject_app(&shared.proxy, &id, "Q0712", msg::bad_message());
        return;
    }
    match kind {
        "cancel" => {
            if let Some(t) = m.get("target").and_then(Value::as_u64) {
                if let Some(flag) = lock(&shared.calls).get(&(id.page.clone(), t)) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        }
        "commands" => {
            let s = shared.clone();
            shared.workers.submit(Box::new(move || {
                let shared = s;
                let r = guarded(|| {
                    let app = &shared.app;
                    with_input(app, |input| {
                        command::signatures(input, &app.module, &app.commands)
                            .map(|sigs| command::signatures_json(&sigs))
                            .map_err(|e| e.to_json())
                    })
                });
                answer(&shared.proxy, &id, r);
            }));
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
                    reject_app(&shared.proxy, &id, "Q0712", msg::bad_message());
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
                reject_app(&shared.proxy, &id, "Q0712", msg::bad_message());
                return;
            };
            let flag = Arc::new(AtomicBool::new(false));
            let key = (id.page.clone(), id.id);
            {
                let mut calls = lock(&shared.calls);
                // A reused call id would replace the cancel flag of the call still running.
                if calls.contains_key(&key) {
                    drop(calls);
                    reject_app(&shared.proxy, &id, "Q0712", msg::bad_message());
                    return;
                }
                calls.insert(key.clone(), flag.clone());
            }
            let s = shared.clone();
            shared.workers.submit(Box::new(move || {
                let shared = s;
                let r = guarded(|| run_call(&shared, &command, &args, shots, seed, flag));
                lock(&shared.calls).remove(&key);
                answer(&shared.proxy, &id, r);
            }));
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs a call; a panic becomes an internal error (Q0412) so the page's Promise still settles.
fn guarded(
    f: impl FnOnce() -> Result<Result<String, String>, String>,
) -> Result<Result<String, String>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(msg::call_panicked()))
}

fn answer(
    proxy: &EventLoopProxy<UserEvent>,
    id: &Reply,
    r: Result<Result<String, String>, String>,
) {
    match r {
        Ok(Ok(json)) => settle(proxy, id, true, &json),
        Ok(Err(json)) => settle(proxy, id, false, &json),
        Err(e) => reject_app(proxy, id, "Q0412", e),
    }
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
    call_app(app, name, args, shots, seed, &mut host)
}

/// Runs command `name` of `app` with `host`: the JSON result, or the JSON error.
fn call_app(
    app: &App,
    name: &str,
    args: &[(String, JsArg)],
    shots: u64,
    seed: u64,
    host: &mut dyn lang::stdlib::Host,
) -> Result<Result<String, String>, String> {
    with_input(app, |input| {
        let call = Call {
            module: &app.module,
            commands: &app.commands,
            name,
            args,
            shots,
            seed,
            limits: CALL_LIMITS,
        };
        command::call(input, &call, host)
            .map(|r| r.to_json())
            .map_err(|e| e.to_json())
    })
}

/// What the command line asks for.
enum Start {
    Version,
    App(Box<App>),
}

/// Decides what to run from the arguments and the bytes of this executable.
fn start(args: &[String], exe: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<Start, String> {
    match args {
        [] => App::from_exe(&exe()?).map(|a| Start::App(Box::new(a))),
        [flag, path] if flag == "--dev" => {
            // Security: only the bare runtime started by `qxide app dev` takes `--dev` (B-A3).
            if bundle::read(&exe()?) != Err(bundle::BundleError::Missing) {
                return Err(msg::dev_refused());
            }
            App::from_dev(std::path::Path::new(path)).map(|a| Start::App(Box::new(a)))
        }
        [v] if v == "--version" => Ok(Start::Version),
        _ => Err(msg::usage()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = match start(&args, App::exe_bytes) {
        Ok(Start::Version) => {
            println!("qxide-app {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Start::App(a)) => *a,
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
    let dev = app.dev;
    let shared = Arc::new(Shared {
        proxy: event_loop.create_proxy(),
        workers: Workers::new(worker_count())?,
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
            for flag in lock(&shared.calls).values() {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use lang::stdlib::LogOnly;

    // A bundle as `qxide app build` writes it, with a dependency package next to the app.
    fn bundled(lib: &str) -> App {
        let app = "import bell_utils.prepare;\n\npublic BitReg pair() {\n    QReg q = qreg(2);\n    prepare(q[0], q[1]);\n    return measure(q);\n}\n";
        bundled_app(app, "pair", lib)
    }

    fn bundled_app(app: &str, command: &str, lib: &str) -> App {
        let manifest = serde_json::json!({
            "format": 1, "qxide": env!("CARGO_PKG_VERSION"), "dev": false,
            "title": "t", "identifier": "dev.qxide.app.test", "module": "app",
            "commands": [command], "permissions": ["log"],
            "width": 400, "height": 300, "resizable": true,
            "packages": [
                {"name": "demo", "src": "pkg/0/demo/src", "deps": [1]},
                {"name": "bell_utils", "src": "pkg/1/bell_utils/src", "deps": []}
            ],
            "entry": "pkg/0/demo/src/app.qx", "ui": "ui", "data": null
        });
        let mut files = BTreeMap::new();
        files.insert(
            bundle::MANIFEST.to_string(),
            manifest.to_string().into_bytes(),
        );
        files.insert("pkg/0/demo/src/app.qx".into(), app.as_bytes().to_vec());
        files.insert(
            "pkg/1/bell_utils/src/lib.qx".into(),
            lib.as_bytes().to_vec(),
        );
        files.insert("ui/index.html".into(), b"<p>hi</p>".to_vec());
        App::parse(manifest.to_string().as_bytes(), app::Files::Bundle(files)).unwrap()
    }

    fn call(app: &App) -> Result<String, String> {
        call_app(app, "pair", &[], 200, 1, &mut LogOnly::default()).unwrap()
    }

    // M-13: command calls run under the desktop app limits, not the CLI's 1 GiB state vector.
    #[test]
    fn app_calls_use_app_limits() {
        let src = "public BitReg big() {\n    QReg q = qreg(25);\n    t(q[0]);\n    return measure(q);\n}\n";
        let app = bundled_app(src, "big", "public void unused() {\n}\n");
        let e = call_app(&app, "big", &[], 1, 0, &mut LogOnly::default())
            .unwrap()
            .unwrap_err();
        assert!(e.contains("\"code\":\"Q0406\""), "{e}");
        assert!(e.contains("desktop app limit"), "{e}");
    }

    // M-13: concurrent calls together stay within one memory budget.
    #[test]
    fn concurrent_calls_share_one_memory_budget() {
        let total = worker_count() as u64 * call_bytes(CALL_LIMITS);
        assert!(
            total <= 1 << 30,
            "{total} bytes across {} workers",
            worker_count()
        );
    }

    // M-13 (B-A7): calls waiting for a worker must not each hold a thread (64 MiB stack).
    #[test]
    fn queued_calls_do_not_hold_threads() {
        use std::collections::HashSet;
        use std::time::Duration;
        let w = Workers::new(2).unwrap();
        let ids = Arc::new(Mutex::new(HashSet::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..16 {
            let (ids, tx) = (ids.clone(), tx.clone());
            w.submit(Box::new(move || {
                ids.lock().unwrap().insert(std::thread::current().id());
                std::thread::sleep(Duration::from_millis(5));
                tx.send(()).unwrap();
            }));
        }
        for _ in 0..16 {
            rx.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        let n = ids.lock().unwrap().len();
        assert!(n <= 2, "16 queued calls used {n} threads");
    }

    // M-13 (B-A7): a call that panics must not take its worker slot with it.
    #[test]
    fn a_panicking_call_keeps_its_worker() {
        use std::time::Duration;
        let w = Workers::new(1).unwrap();
        let (started, ran) = std::sync::mpsc::channel();
        w.submit(Box::new(move || {
            started.send(()).unwrap();
            panic!("call panicked (expected by this test)");
        }));
        ran.recv_timeout(Duration::from_secs(20)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let (tx, rx) = std::sync::mpsc::channel();
        w.submit(Box::new(move || tx.send(()).unwrap()));
        rx.recv_timeout(Duration::from_secs(5))
            .expect("worker lost after a panic");
    }

    // M-13: an oversized result is turned into a rejection instead of a huge page script.
    #[test]
    fn oversized_results_are_rejected_not_sent() {
        let to = Reply {
            page: "p".into(),
            id: 7,
        };
        let big = format!("\"{}\"", "x".repeat(MAX_SCRIPT_BYTES));
        let script = settle_script(&to, true, &big);
        assert!(script.len() <= MAX_SCRIPT_BYTES, "{} bytes", script.len());
        assert!(script.contains("\"Q0716\"") && script.contains(",7,false,"));
        let small = settle_script(&to, true, "1");
        assert!(small.ends_with("(\"p\",7,true,1);"), "{small}");
        let e = host::emit_event("e", &"x".repeat(MAX_SCRIPT_BYTES)).unwrap_err();
        assert_eq!(e.kind, lang::stdlib::StdErrorKind::Argument);
        assert!(host::emit_event("e", "ok").is_ok());
    }

    #[test]
    fn bundled_commands_import_dependency_packages() {
        let lib = "import std.log;\n\npublic void prepare(Qubit a, Qubit b) {\n    log.info(\"prepare\");\n    h(a);\n    cx(a, b);\n}\n";
        let app = bundled(lib);
        let r = call(&app).unwrap();
        assert!(r.contains("\"counts\":[{\"value\":["), "{r}");
        assert!(!r.contains("[0,1]") && !r.contains("[1,0]"), "{r}");
        assert_eq!(
            app.ui_file("index.html").as_deref(),
            Some(&b"<p>hi</p>"[..])
        );
        assert_eq!(app.ui_file("../app.json"), None);
        assert_eq!(app.ui_file("C:/x"), None);
    }

    #[test]
    fn bundled_dependencies_cannot_use_files() {
        let lib = "import std.fs;\n\npublic void prepare(Qubit a, Qubit b) {\n    h(a);\n    cx(a, b);\n}\n";
        let e = call(&bundled(lib)).unwrap_err();
        assert!(e.contains("\"code\":\"Q0703\""), "{e}");
        assert!(e.contains("bell_utils") && e.contains("lib.qx"), "{e}");
    }

    // B-A3: a built app (an executable with a bundle attached) must not accept `--dev <json>`,
    // which would run any `.qx`/UI with any permissions, data folder and devtools.
    #[test]
    fn built_apps_refuse_dev_mode() {
        let dir = std::env::temp_dir().join(format!("qxide-ba3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dev_json = dir.join("dev.json");
        let manifest = serde_json::json!({
            "format": 1, "qxide": env!("CARGO_PKG_VERSION"), "dev": true,
            "title": "evil", "identifier": "x", "module": "app",
            "commands": ["f"], "permissions": ["fs", "app", "qxide"],
            "width": 400, "height": 300, "resizable": true,
            "packages": [{"name": "x", "src": dir.display().to_string(), "deps": []}],
            "entry": dir.join("app.qx").display().to_string(), "ui": dir.display().to_string(),
            "data": "C:\\"
        });
        std::fs::write(&dev_json, manifest.to_string()).unwrap();
        let args = ["--dev".to_string(), dev_json.display().to_string()];

        // The bare runtime (no bundle) is what `qxide app dev` starts: `--dev` works there.
        let bare = || Ok(b"MZ runtime".to_vec());
        assert!(matches!(start(&args, bare), Ok(Start::App(a)) if a.dev));

        // With any bundle attached, including a damaged or foreign-format one, it is refused.
        let mut built = b"MZ runtime".to_vec();
        let mut files = BTreeMap::new();
        files.insert(bundle::MANIFEST.to_string(), b"{}".to_vec());
        bundle::write(&mut built, &files);
        let mut corrupt = built.clone();
        let n = corrupt.len();
        corrupt[n - 24..n - 16].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut other = built.clone();
        other[n - 12..n - 8].copy_from_slice(&9u32.to_le_bytes());
        for exe in [built, corrupt, other] {
            let r = start(&args, move || Ok(exe));
            assert!(r.is_err(), "--dev accepted by a built app");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn origins_and_paths() {
        assert!(same_origin(origin()));
        assert!(same_origin(&format!("{}/index.html", origin())));
        assert!(!same_origin(&format!("{}.evil.com/", origin())));
        assert!(!same_origin("https://example.com/"));
        assert_eq!(percent_decode("a%20b%2F"), "a b/");
        assert_eq!(percent_decode("bad%2"), "bad%2");
    }
}
