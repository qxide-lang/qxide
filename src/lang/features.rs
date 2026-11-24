// Experimental language features, off by default. `value-semantics` turns on the memory model
// of docs/design/memory-model.md (`inout` and classical `consume` parameters, moves, `.copy()`,
// `Shared<T>`); without it the language, its diagnostics and its step counts are unchanged.
// `--experimental value-semantics` on the command line turns it on for the whole process,
// `QXIDE_EXPERIMENTAL=value-semantics` for any thread that has not set it (`qxide lsp` uses
// that), and tests set it per thread like the engine (src/lang/engine.rs).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// The name of the value-semantics feature on the command line and in `QXIDE_EXPERIMENTAL`.
pub const VALUE_SEMANTICS: &str = "value-semantics";

/// Every experimental feature this build knows.
pub const ALL: &[&str] = &[VALUE_SEMANTICS];

thread_local! {
    static VALUES: Cell<Option<bool>> = const { Cell::new(None) };
}

// `--experimental value-semantics` on the command line: the default of every thread.
static CLI_VALUES: AtomicBool = AtomicBool::new(false);

/// Takes `--experimental NAME` out of command-line arguments (anywhere before `--`) and turns
/// those features on for the whole process. Returns the other arguments, or the unknown name.
pub fn take_cli_flags(args: Vec<String>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(args.len());
    let mut rest = args.into_iter();
    while let Some(a) = rest.next() {
        if a == "--" {
            out.push(a);
            out.extend(rest.by_ref());
            break;
        }
        if a != "--experimental" {
            out.push(a);
            continue;
        }
        match rest.next() {
            Some(name) if name == VALUE_SEMANTICS => CLI_VALUES.store(true, Ordering::Relaxed),
            Some(name) => return Err(name),
            None => return Err(String::new()),
        }
    }
    Ok(out)
}

/// Turns value semantics on or off for programs checked and run on this thread (tests, the
/// debug-build check of `values::lower`).
#[cfg_attr(not(any(test, debug_assertions)), allow(dead_code))]
pub fn set_value_semantics(on: bool) {
    VALUES.with(|v| v.set(Some(on)));
}

/// Whether value semantics is on for this thread: set explicitly, else from `QXIDE_EXPERIMENTAL`.
pub fn value_semantics() -> bool {
    VALUES.with(|v| match v.get() {
        Some(on) => on,
        None => {
            let on = CLI_VALUES.load(Ordering::Relaxed) || from_env(VALUE_SEMANTICS);
            v.set(Some(on));
            on
        }
    })
}

// `QXIDE_EXPERIMENTAL=a,b` names the features to turn on.
fn from_env(name: &str) -> bool {
    std::env::var("QXIDE_EXPERIMENTAL").is_ok_and(|s| s.split(',').any(|f| f.trim() == name))
}
