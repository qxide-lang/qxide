//! WebAssembly binding of the `.qx` front end and interpreter for the qxide.dev playground.
//!
//! It compiles the CLI's `src/lang` module unchanged and mirrors `qxide check|run|qasm`: the same
//! stdout, stderr and exit code, except for the browser limits in [`LIMITS`] and [`MAX_SHOTS`].
//! The raw ABI (`qx_alloc`, `qx_exec`) avoids wasm-bindgen; see `site/src/playground/engine.ts`.

// The CLI-only tools in `lang` (fmt, explain, repl) are compiled here but unused; LTO drops them.
#[allow(dead_code)]
#[path = "../../../src/lang/mod.rs"]
mod lang;

/// Most qubits the state-vector backend simulates in the browser: 2^20 amplitudes = 16 MiB.
pub const MAX_STATE_VECTOR_QUBITS: usize = 20;

/// Largest stabilizer tableau the playground allocates (the CLI allows 1 GiB).
pub const STABILIZER_BYTES: u64 = 512 << 20;

/// Most shots per run in the playground.
pub const MAX_SHOTS: u64 = 100_000;

/// The limits `qx_exec` passes to the front end and the interpreter.
pub const LIMITS: lang::Limits = lang::Limits::PLAYGROUND;

// The constants above document the playground; the shared definition lives in `lang::Limits`.
const _: () = {
    assert!(matches!(
        LIMITS.max_state_vector_qubits,
        Some(MAX_STATE_VECTOR_QUBITS)
    ));
    assert!(LIMITS.stabilizer_bytes == STABILIZER_BYTES);
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Check,
    Run,
    Qasm,
}

impl Command {
    fn from_code(code: u32) -> Option<Command> {
        match code {
            0 => Some(Command::Check),
            1 => Some(Command::Run),
            2 => Some(Command::Qasm),
            _ => None,
        }
    }
}

/// What the CLI would print and return for the same command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub exit: u8,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn err(exit: u8, stderr: String) -> Self {
        Outcome {
            exit,
            stdout: String::new(),
            stderr,
        }
    }

    /// `{"exit":0,"stdout":"…","stderr":"…"}`.
    pub fn to_json(&self) -> String {
        let mut s = format!("{{\"exit\":{},\"stdout\":", self.exit);
        json_string(&mut s, &self.stdout);
        s.push_str(",\"stderr\":");
        json_string(&mut s, &self.stderr);
        s.push('}');
        s
    }
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn too_many_shots(shots: u64) -> String {
    let note = format!("the playground runs at most {MAX_SHOTS} shots per run; the installed `qxide` command has no cap");
    format!("qxide: invalid value `{shots}` for --shots\n{note}\n")
}

/// Runs one `.qx` command like `qxide <cmd> <name> [--shots N] [--seed S]` on the source text `src`.
/// `name` stands in for the file path in diagnostics (`--> name:line:col`).
/// `std.log` lines go to stderr, as in the CLI; other standard modules are not offered (Q0702).
pub fn execute(cmd: Command, name: &str, src: &str, shots: u64, seed: u64) -> Outcome {
    let mut input = lang::Input::file(name, src);
    input.perms = lang::stdlib::Permissions::playground();
    let mut host = lang::stdlib::LogOnly::default();
    let result = match cmd {
        Command::Check => lang::check_input_with(&input, &LIMITS).map(|()| format!("{name}: ok\n")),
        Command::Qasm => lang::qasm_input_with(&input, &LIMITS),
        Command::Run => {
            // Same wording as the CLI's own `--shots` validation.
            if shots == 0 {
                return Outcome::err(2, format!("qxide: invalid value `{shots}` for --shots\n"));
            }
            if shots > MAX_SHOTS {
                return Outcome::err(2, too_many_shots(shots));
            }
            lang::run_input_with(&input, shots, seed, LIMITS, &mut host)
        }
    };
    let logs: String = host.lines.iter().map(|l| format!("{l}\n")).collect();
    match result {
        Ok(stdout) => Outcome {
            exit: 0,
            stdout,
            stderr: logs,
        },
        Err(lang::Failure(diags)) => Outcome::err(1, logs + &diags),
    }
}

/// Allocates `len` bytes for the host to write UTF-8 input into.
#[no_mangle]
pub extern "C" fn qx_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Runs `cmd` (0 check, 1 run, 2 qasm) and returns a pointer to a little-endian `u32` byte length
/// followed by the outcome as UTF-8 JSON. Invalid UTF-8 or command codes give exit code 2.
///
/// # Safety
/// `name_ptr`/`src_ptr` must point to `name_len`/`src_len` initialised bytes from `qx_alloc`.
/// The host discards the instance after one call, so nothing is freed.
#[no_mangle]
pub unsafe extern "C" fn qx_exec(
    cmd: u32,
    name_ptr: *const u8,
    name_len: usize,
    src_ptr: *const u8,
    src_len: usize,
    shots: u64,
    seed: u64,
) -> *const u8 {
    // SAFETY: the caller guarantees both ranges are valid, initialised and unaliased.
    let (name, src) = unsafe {
        (
            std::slice::from_raw_parts(name_ptr, name_len),
            std::slice::from_raw_parts(src_ptr, src_len),
        )
    };
    let outcome = match (
        Command::from_code(cmd),
        std::str::from_utf8(name),
        std::str::from_utf8(src),
    ) {
        (Some(cmd), Ok(name), Ok(src)) => execute(cmd, name, src, shots, seed),
        _ => Outcome::err(
            2,
            "qxide: internal error: unknown command or invalid UTF-8 input\n".into(),
        ),
    };
    let json = outcome.to_json();
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(json.as_bytes());
    let ptr = buf.as_ptr();
    std::mem::forget(buf);
    ptr
}

#[cfg(test)]
mod tests;
