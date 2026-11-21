// The app host: `std.log`, `std.time`, `std.fs` from `NativeStd`, plus `std.app` (window and page
// events, through the event loop) and `std.qxide` (Qxide Studio). The linker already refused
// modules the app was not granted; the host checks again (defense in depth).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tao::event_loop::EventLoopProxy;

use crate::lang::command::json_str;
use crate::lang::stdlib::{
    log_line, Host, HostValue, LogLevel, NativeStd, Permissions, StdError, StdErrorKind, StdResult,
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

impl AppHost {
    pub fn new(
        data: std::path::PathBuf,
        perms: Permissions,
        proxy: EventLoopProxy<UserEvent>,
        cancel: Arc<AtomicBool>,
        dev: bool,
    ) -> AppHost {
        let log_proxy = proxy.clone();
        let log = Box::new(move |level: LogLevel, text: &str| {
            let line = log_line(level, text);
            eprintln!("{line}");
            // In `qxide app dev`, `qxide.on("log", ...)` sees the lines too.
            if dev {
                let _ = log_proxy.send_event(UserEvent::Eval(emit_script("log", &line)));
            }
        });
        let mut native = NativeStd::new(data, log);
        native.cancel = Some(cancel);
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
            ("app", "set_title") => self.send(UserEvent::SetTitle(arg_str(&args, 0))),
            ("app", "set_size") => {
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
            ("app", "emit") => self.send(UserEvent::Eval(emit_script(
                &arg_str(&args, 0),
                &arg_str(&args, 1),
            ))),
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
