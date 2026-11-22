// Which elaborator builds the circuit: the tree-walking interpreter (the default), the typed
// IR interpreter (docs/design/qx-ir.md), the IR interpreter with Cranelift native code for
// classical functions (docs/design/qx-jit.md, `jit` feature), or all of them with their results
// compared (`dual`, the default under `cargo test`). The choice is per thread, so parallel tests do not interfere;
// the CLI sets it from `--engine` or `QXIDE_ENGINE` on the thread that runs the program.

use std::cell::Cell;

use super::ast::Program;
use super::diag::Diagnostic;
use super::interp::{self, Elaborated, OutPiece, Value};
use super::stdlib::{Host, HostValue, StdResult};
use super::{ir, Limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    // The tree-walking interpreter over the checked AST (src/lang/interp.rs).
    Ast,
    // Lowering to the typed IR, then the IR interpreter (src/lang/ir/).
    Ir,
    // The IR interpreter, running build-time classical functions as native code (src/lang/jit/).
    // Without the `jit` feature it is the IR interpreter alone.
    Jit,
    // Every engine of this build; panics if their results differ, else returns the
    // tree-walking result.
    Dual,
}

impl Engine {
    pub fn parse(s: &str) -> Option<Engine> {
        match s {
            "ast" => Some(Engine::Ast),
            "ir" => Some(Engine::Ir),
            "jit" if JIT => Some(Engine::Jit),
            "dual" => Some(Engine::Dual),
            _ => None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            Engine::Ast => "ast",
            Engine::Ir => "ir",
            Engine::Jit => "jit",
            Engine::Dual => "dual",
        }
    }
}

/// Whether this build has the Cranelift JIT (`--engine jit`).
pub const JIT: bool = cfg!(feature = "jit");

// Tests compare both engines on every program they elaborate.
const DEFAULT: Engine = if cfg!(test) {
    Engine::Dual
} else {
    Engine::Ast
};

thread_local! {
    static CURRENT: Cell<Engine> = const { Cell::new(DEFAULT) };
}

/// The engine of this thread.
pub fn current() -> Engine {
    CURRENT.with(Cell::get)
}

/// Sets the engine of this thread (the CLI's worker thread, the playground).
#[allow(dead_code)]
pub fn set(e: Engine) {
    CURRENT.with(|c| c.set(e));
}

/// Runs `f` with `e` as this thread's engine.
#[cfg_attr(not(test), allow(dead_code))]
pub fn with<T>(e: Engine, f: impl FnOnce() -> T) -> T {
    let before = current();
    set(e);
    let out = f();
    set(before);
    out
}

/// Runs `main` once with `engine`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn elaborate(
    p: &Program,
    host: &mut dyn Host,
    limits: &Limits,
    engine: Engine,
) -> Result<Elaborated, Diagnostic> {
    elaborate_call_with(p, "main", Vec::new(), host, limits, engine).map(|(e, _)| e)
}

/// `interp::elaborate_call` with this thread's engine.
pub fn elaborate_call(
    p: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
) -> Result<(Elaborated, Value), Diagnostic> {
    elaborate_call_with(p, name, args, host, limits, current())
}

pub fn elaborate_call_with(
    p: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
    engine: Engine,
) -> Result<(Elaborated, Value), Diagnostic> {
    match engine {
        Engine::Ast => interp::elaborate_call(p, name, args, host, limits),
        Engine::Ir | Engine::Jit => {
            let module = ir::build(p);
            run_lowered(&module, engine, name, args, host, limits)
        }
        Engine::Dual => dual(p, name, args, host, limits),
    }
}

/// Runs `name` of a lowered module with the IR interpreter, plus native code for `Engine::Jit`.
pub fn run_lowered(
    module: &ir::Module,
    engine: Engine,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
) -> Result<(Elaborated, Value), Diagnostic> {
    #[cfg(feature = "jit")]
    if engine == Engine::Jit {
        // Machines Cranelift does not support run everything in the interpreter.
        if let Some(mut jit) = super::jit::Jit::new(module) {
            return ir::exec::run_with(module, name, args, host, limits, Some(&mut jit));
        }
    }
    let _ = engine;
    ir::exec::run(module, name, args, host, limits)
}

// Host calls of the first run, replayed for the second so effects happen once.
struct Recorder<'h> {
    inner: &'h mut dyn Host,
    calls: Vec<(String, String, Vec<HostValue>, StdResult)>,
}

impl Host for Recorder<'_> {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        let r = self.inner.call(module, function, args.clone());
        self.calls
            .push((module.into(), function.into(), args, r.clone()));
        r
    }

    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
}

struct Replay {
    calls: std::vec::IntoIter<(String, String, Vec<HostValue>, StdResult)>,
    mismatch: Option<String>,
}

impl Host for Replay {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        match self.calls.next() {
            Some((m, f, a, r)) if m == module && f == function && a == args => r,
            other => {
                let msg = format!(
                    "replayed run called std.{module}.{function}{args:?}, tree-walking run {:?}",
                    other.map(|(m, f, a, _)| format!("std.{m}.{f}{a:?}"))
                );
                self.mismatch.get_or_insert(msg);
                Err(super::stdlib::StdError::new(
                    super::stdlib::StdErrorKind::Unavailable,
                    "replay mismatch".into(),
                ))
            }
        }
    }
}

// Process-wide ids (circuit numbers) differ between the runs; everything else must match.
fn scrub(s: &str) -> String {
    const KEYS: [&str; 2] = ["circuit: ", " id: "];
    let find = |from: usize, k: &str| s[from..].find(k).map(|i| from + i);
    // Next match of each key at or after `pos`, so every byte is searched once per key.
    let mut next = KEYS.map(|k| find(0, k));
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    while let Some((k, i)) = next
        .iter()
        .enumerate()
        .filter_map(|(k, i)| i.map(|i| (k, i)))
        .min_by_key(|&(_, i)| i)
    {
        let start = i + KEYS[k].len();
        let digits = s[start..]
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(s.len() - start);
        out.push_str(&s[pos..start]);
        out.push('#');
        pos = start + digits;
        for (j, key) in KEYS.iter().enumerate() {
            if next[j].is_some_and(|n| n < pos) {
                next[j] = find(pos, key);
            }
        }
    }
    out.push_str(&s[pos..]);
    out
}

// Circuits up to this many recorded operations are compared in full (`Debug`), larger ones by
// their counts, QASM size estimate and measurement record.
const FULL_COMPARE_OPS: u64 = 200_000;

fn fingerprint(r: &Result<(Elaborated, Value), Diagnostic>) -> String {
    match r {
        Err(d) => format!("error {d:?}"),
        Ok((e, v)) => {
            let circuit = if e.usage.ops <= FULL_COMPARE_OPS {
                format!("{:?}", e.circuit)
            } else {
                format!("{} measured", e.measured.len())
            };
            let output: Vec<String> = e
                .output
                .iter()
                .map(|p| match p {
                    OutPiece::Text(t) => format!("{t:?}"),
                    other => format!("{other:?}"),
                })
                .collect();
            scrub(&format!(
                "ret {v:?}\noutput {output:?}\nmeasured {:?}\nslots {}\ncounts {:?}\nqasm_bytes {}\nusage {:?}\ncircuit {circuit}",
                if e.measured.len() as u64 <= FULL_COMPARE_OPS { format!("{:?}", e.measured) } else { e.measured.len().to_string() },
                e.slots,
                e.counts,
                e.qasm_bytes,
                e.usage,
            ))
        }
    }
}

fn dual(
    p: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    limits: &Limits,
) -> Result<(Elaborated, Value), Diagnostic> {
    let mut rec = Recorder {
        inner: host,
        calls: Vec::new(),
    };
    let tree = interp::elaborate_call(p, name, args.clone(), &mut rec, limits);
    // A cancelled run depends on when the host was asked; there is nothing to compare.
    if matches!(&tree, Err(d) if d.code == super::diag::Code::Cancelled) {
        return tree;
    }
    let want = fingerprint(&tree);
    let module = ir::build(p);
    let others: &[Engine] = if JIT {
        &[Engine::Ir, Engine::Jit]
    } else {
        &[Engine::Ir]
    };
    for &engine in others {
        let mut replay = Replay {
            calls: rec.calls.clone().into_iter(),
            mismatch: None,
        };
        let got = run_lowered(&module, engine, name, args.clone(), &mut replay, limits);
        let e = engine.name();
        if let Some((m, f, a, _)) = replay.calls.next() {
            replay
                .mismatch
                .get_or_insert(format!("{e} run skipped std.{m}.{f}{a:?}"));
        }
        if let Some(m) = replay.mismatch {
            panic!("{e} engine differs from the tree-walking interpreter: {m}");
        }
        let got = fingerprint(&got);
        if want != got {
            let line = want
                .lines()
                .zip(got.lines())
                .find(|(x, y)| x != y)
                .map_or_else(String::new, |(x, y)| format!("\n  ast: {x}\n  {e}: {y}"));
            panic!("{e} engine differs from the tree-walking interpreter in `{name}`:{line}");
        }
    }
    tree
}
