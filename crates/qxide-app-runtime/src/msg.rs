// User-facing text of the qxide-app runtime, kept in one place.

pub fn usage() -> String {
    "invalid arguments; usage: `qxide-app` (run the attached app) or `qxide-app --dev <dev.json>` (used by `qxide app dev`)".into()
}

pub fn dev_refused() -> String {
    "`--dev` is only accepted by the bare runtime that `qxide app dev` starts, not by a built app"
        .into()
}

pub fn no_workers() -> String {
    "could not start any worker thread for command calls".into()
}

pub fn call_panicked() -> String {
    "the call stopped because of an internal error in the runtime; please report it".into()
}

pub fn result_too_large(bytes: usize, limit: usize) -> String {
    format!(
        "the result is {} MiB, more than the {} MiB a call can return to the page; return less data or request fewer shots",
        bytes.div_ceil(1 << 20),
        limit >> 20
    )
}

pub fn emit_too_large(limit: usize) -> String {
    format!(
        "`app.emit` event and payload must be at most {} MiB",
        limit >> 20
    )
}

pub fn no_bundle() -> String {
    "no Qxide app attached to this executable; build one with `qxide app build` or run a project with `qxide app dev`".into()
}

pub fn bundle_format(n: u32) -> String {
    format!("unsupported bundle format version `{n}`; rebuild the app with the matching `qxide` version")
}

pub fn bundle_corrupt() -> String {
    "attached app bundle is damaged or incomplete; rebuild it with `qxide app build`".into()
}

pub fn bad_manifest(what: &str) -> String {
    format!("invalid app manifest `app.json` at `{what}`")
}

pub fn version_mismatch(built: &str, ours: &str) -> String {
    format!("app was built with `qxide` `{built}`, but this runtime is `{ours}`; rebuild the app with the same `qxide` version")
}

pub fn ipc_foreign_origin(url: &str) -> String {
    format!("ignored message from a page outside the app: `{url}`")
}

pub fn bad_message() -> String {
    "received a malformed message from the page (unknown fields or wrong types)".into()
}
