// The app host: `std.log`, `std.time`, `std.fs` from `NativeStd`, plus `std.app` (window and page
// events, through the event loop) and `std.qxide` (Qxide Studio). The linker already refused
// modules the app was not granted; the host checks again (defense in depth).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tao::event_loop::EventLoopProxy;

use crate::lang::command::json_str;
use crate::lang::stdlib::{
    log_line, Host, HostValue, HttpAccess, LogLevel, NativeStd, Permissions, StdError,
    StdErrorKind, StdResult,
};
use crate::UserEvent;

pub struct AppHost {
    native: NativeStd,
    perms: Permissions,
    proxy: EventLoopProxy<UserEvent>,
}

/// `__qxide_emit("event", "payload")` for the page.
pub fn emit_script(event: &str, payload: &str) -> String {
    let mut s = String::from("window.__qxide_emit && window.__qxide_emit(");
    json_str(&mut s, event);
    s.push(',');
    json_str(&mut s, payload);
    s.push_str(");");
    s
}

/// `std.http` through `curl` for an app granted `http` with at least one host, otherwise none.
pub fn http_access(perms: &Permissions, hosts: &[String]) -> Option<HttpAccess> {
    let granted = perms.access("http") == Some(crate::lang::stdlib::Access::Allowed);
    (granted && !hosts.is_empty()).then(|| HttpAccess {
        hosts: hosts.to_vec(),
        get: Box::new(crate::http::get),
    })
}

/// The page script for a `std.app.emit` event or a dev log line, refused (Q0706) when it would be
/// too large to hand to the webview.
pub fn emit_event(event: &str, payload: &str) -> Result<String, StdError> {
    // JSON escaping can grow text up to 6x (`\u0000`); check the cheap bound first.
    let raw = event.len() + payload.len();
    if raw <= crate::MAX_SCRIPT_BYTES / 6 {
        return Ok(emit_script(event, payload));
    }
    if raw <= crate::MAX_SCRIPT_BYTES {
        let script = emit_script(event, payload);
        if script.len() <= crate::MAX_SCRIPT_BYTES {
            return Ok(script);
        }
    }
    Err(StdError::new(
        StdErrorKind::Argument,
        crate::msg::emit_too_large(crate::MAX_SCRIPT_BYTES),
    ))
}

impl AppHost {
    pub fn new(
        data: std::path::PathBuf,
        perms: Permissions,
        proxy: EventLoopProxy<UserEvent>,
        cancel: Arc<AtomicBool>,
        dev: bool,
        http_hosts: &[String],
    ) -> AppHost {
        let log_proxy = proxy.clone();
        let log = Box::new(move |level: LogLevel, text: &str| {
            let line = log_line(level, text);
            eprintln!("{line}");
            // In `qxide app dev`, `qxide.on("log", ...)` sees the lines too.
            if dev {
                if let Ok(js) = emit_event("log", &line) {
                    let _ = log_proxy.send_event(UserEvent::Eval(js));
                }
            }
        });
        let mut native = NativeStd::new(data, log);
        native.cancel = Some(cancel);
        native.http = http_access(&perms, http_hosts);
        AppHost {
            native,
            perms,
            proxy,
        }
    }

    fn send(&self, e: UserEvent) -> StdResult {
        let _ = self.proxy.send_event(e);
        Ok(HostValue::Unit)
    }
}

fn arg_str(args: &[HostValue], i: usize) -> String {
    match args.get(i) {
        Some(HostValue::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn arg_int(args: &[HostValue], i: usize) -> i64 {
    match args.get(i) {
        Some(HostValue::Int(n)) => *n,
        _ => 0,
    }
}

impl Host for AppHost {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        use crate::lang::stdlib::Access;
        if self.perms.access(module) != Some(Access::Allowed) {
            return Err(StdError::new(
                StdErrorKind::Unavailable,
                crate::lang::std_unavailable(&format!("std.{module}"), "this app"),
            ));
        }
        match (module, function) {
            ("app", "setTitle") => self.send(UserEvent::SetTitle(arg_str(&args, 0))),
            ("app", "setSize") => {
                let (w, h) = (arg_int(&args, 0), arg_int(&args, 1));
                if !(200..=8192).contains(&w) || !(200..=8192).contains(&h) {
                    return Err(StdError::new(
                        StdErrorKind::Argument,
                        crate::lang::std_size_range(w, h),
                    ));
                }
                self.send(UserEvent::SetSize(w as u32, h as u32))
            }
            ("app", "close") => self.send(UserEvent::Close),
            ("app", "emit") => {
                let js = emit_event(&arg_str(&args, 0), &arg_str(&args, 1))?;
                self.send(UserEvent::Eval(js))
            }
            ("qxide", f) => {
                let native = &self.native;
                crate::lang::studio::call(f, &args, &|| native.is_cancelled())
            }
            _ => self.native.call(module, function, &args),
        }
    }

    fn cancelled(&self) -> bool {
        self.native.is_cancelled()
    }
}
