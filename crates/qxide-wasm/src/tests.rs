// The binding must print exactly what the `qxide` binary prints. These tests build the CLI once
// and compare stdout, stderr and the exit code byte for byte.

use super::*;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use std::sync::OnceLock;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn cli() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let manifest = root().join("Cargo.toml");
        let status = Proc::new(cargo)
            .args([
                "build",
                "--quiet",
                "-p",
                "qxide",
                "--bin",
                "qxide",
                "--manifest-path",
            ])
            .arg(&manifest)
            .status()
            .expect("cargo runs");
        assert!(status.success(), "building the qxide CLI failed");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root().join("target"));
        target
            .join("debug")
            .join(format!("qxide{}", std::env::consts::EXE_SUFFIX))
    })
}

/// Runs the CLI from the repository root on `path` (relative to it).
fn run_cli(cmd: Command, path: &str, shots: u64, seed: u64) -> Outcome {
    let mut p = Proc::new(cli());
    p.current_dir(root());
    match cmd {
        Command::Check => p.args(["check", path]),
        Command::Qasm => p.args(["qasm", path]),
        Command::Run => p.args([
            "run",
            path,
            "--shots",
            &shots.to_string(),
            "--seed",
            &seed.to_string(),
        ]),
    };
    let out = p.output().expect("qxide runs");
    Outcome {
        exit: out.status.code().expect("exit code") as u8,
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn run_wasm(cmd: Command, path: &str, shots: u64, seed: u64) -> Outcome {
    let src = std::fs::read_to_string(root().join(path)).unwrap();
    execute(cmd, path, &src, shots, seed)
}

fn assert_same(cmd: Command, path: &str, shots: u64, seed: u64) -> Outcome {
    let want = run_cli(cmd, path, shots, seed);
    let got = run_wasm(cmd, path, shots, seed);
    assert_eq!(got, want, "{cmd:?} {path} shots={shots} seed={seed}");
    got
}

/// Writes `src` to a scratch file under `target/` and returns its path relative to the root.
fn scratch(name: &str, src: &str) -> String {
    let rel = format!("target/qxide-wasm-tests/{name}");
    let path = root().join(&rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, src).unwrap();
    rel
}

fn examples() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root().join("examples/qx"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".qx"))
        .map(|n| format!("examples/qx/{n}"))
        .collect();
    v.sort();
    assert!(v.len() >= 7, "examples/qx went missing");
    v
}

#[test]
fn examples_match_the_cli() {
    for path in examples() {
        assert_eq!(assert_same(Command::Check, &path, 1, 0).exit, 0);
        assert_eq!(assert_same(Command::Qasm, &path, 1, 0).exit, 0);
        for (shots, seed) in [(1, 0), (1, 42), (1000, 0), (257, 7)] {
            assert_eq!(assert_same(Command::Run, &path, shots, seed).exit, 0);
        }
    }
}

#[test]
fn diagnostics_match_the_cli() {
    let cases = [
        ("unknown_var.qx", "void main() {\n    int a = b;\n}\n"),
        ("parse.qx", "void main() {\n    int a = ;\n}\n"),
        (
            "use_after_consume.qx",
            "void main() {\n    Qubit q = qubit();\n    Bit m = measure(q);\n    h(q);\n}\n",
        ),
        (
            "never_consumed.qx",
            "void main() {\n    Qubit q = qubit();\n    h(q);\n}\n",
        ),
        // Columns count chars, so the Korean text before the error shifts the caret.
        (
            "unicode.qx",
            "void main() {\n    println(\"큐비트 {}\", x);\n}\n",
        ),
        (
            "bom_crlf.qx",
            "\u{feff}void main() {\r\n    final int a = 1;\r\n    a = 2;\r\n}\r\n",
        ),
        (
            "recursion.qx",
            "int f(int n) {\n    return f(n + 1);\n}\n\nvoid main() {\n    println(\"{}\", f(0));\n}\n",
        ),
        (
            "div_zero.qx",
            "void main() {\n    int z = 0;\n    println(\"{}\", 1 / z);\n}\n",
        ),
    ];
    for (name, src) in cases {
        let path = scratch(name, src);
        let check = assert_same(Command::Check, &path, 1, 0);
        let run = assert_same(Command::Run, &path, 1, 0);
        assert_same(Command::Qasm, &path, 1, 0);
        assert_eq!(run.exit, 1, "{name} should fail:\n{}", run.stdout);
        assert!(
            run.stderr.contains(&format!("--> {path}:")),
            "{}",
            run.stderr
        );
        if check.exit == 0 {
            // Runtime errors only appear once the program is elaborated.
            assert!(matches!(name, "recursion.qx" | "div_zero.qx"), "{name}");
        }
    }
}

#[test]
fn deep_expressions_match_the_cli() {
    let deep = format!(
        "void main() {{\n    int a = {}1{};\n}}\n",
        "(".repeat(300),
        ")".repeat(300)
    );
    let path = scratch("deep.qx", &deep);
    let out = assert_same(Command::Check, &path, 1, 0);
    assert!(out.stderr.contains("error[Q0106]"), "{}", out.stderr);
}

#[test]
fn shot_count_is_validated_like_the_cli() {
    let path = "examples/qx/bell.qx";
    let out = assert_same(Command::Run, path, 0, 0);
    assert_eq!(out.exit, 2);
    let over = run_wasm(Command::Run, path, MAX_SHOTS + 1, 0);
    assert_eq!(over.exit, 2);
    assert!(over.stderr.starts_with(&format!(
        "qxide: invalid value `{}` for --shots\n",
        MAX_SHOTS + 1
    )));
    assert_eq!(run_wasm(Command::Run, path, MAX_SHOTS, 0).exit, 0);
}

fn rotated_register(n: usize) -> String {
    format!(
        "void main() {{\n    QReg r = qreg({n});\n    for i in 0..{n} {{\n        h(r[i]);\n        t(r[i]);\n    }}\n    BitReg m = measure(r);\n    println(\"{{}}\", m);\n}}\n"
    )
}

#[test]
fn state_vector_qubits_are_capped_in_the_playground_only() {
    let at_limit = scratch("sv_20.qx", &rotated_register(MAX_STATE_VECTOR_QUBITS));
    assert_eq!(run_wasm(Command::Run, &at_limit, 1, 0).exit, 0);

    let over = scratch("sv_21.qx", &rotated_register(MAX_STATE_VECTOR_QUBITS + 1));
    let wasm = run_wasm(Command::Run, &over, 1, 0);
    assert_eq!(wasm.exit, 1);
    assert!(
        wasm.stderr.starts_with("error[Q0406]: simulation failed: "),
        "{}",
        wasm.stderr
    );
    assert!(wasm.stderr.contains("21 qubits"), "{}", wasm.stderr);
    assert!(
        wasm.stderr.contains(&format!("--> {over}:1:6")),
        "{}",
        wasm.stderr
    );
    assert!(wasm.stderr.contains("\nhelp: "), "{}", wasm.stderr);
    // The CLI has no such cap, and check/qasm never simulate.
    assert_eq!(run_cli(Command::Run, &over, 1, 0).exit, 0);
    assert_same(Command::Check, &over, 1, 0);
    assert_same(Command::Qasm, &over, 1, 0);
}

#[test]
fn clifford_circuits_use_the_stabilizer_beyond_the_cap() {
    let src = "void main() {\n    int n = 200;\n    QReg r = qreg(n);\n    h(r[0]);\n    for i in 1..n {\n        cx(r[0], r[i]);\n    }\n    BitReg m = measure(r);\n    println(\"{}\", m.to_binary());\n}\n";
    let path = scratch("ghz_200.qx", src);
    assert_eq!(assert_same(Command::Run, &path, 50, 3).exit, 0);
}

#[test]
fn outcome_json_escapes_text() {
    let o = Outcome {
        exit: 1,
        stdout: "a\"b\\c\n\t\u{1}큐".into(),
        stderr: String::new(),
    };
    assert_eq!(
        o.to_json(),
        "{\"exit\":1,\"stdout\":\"a\\\"b\\\\c\\n\\t\\u0001큐\",\"stderr\":\"\"}"
    );
}

#[test]
fn abi_round_trip() {
    let (name, src) = ("t.qx", "void main() {\n    println(\"hi\");\n}\n");
    let (np, sp) = (qx_alloc(name.len()), qx_alloc(src.len()));
    // SAFETY: both buffers were just allocated with the right lengths.
    let json = unsafe {
        std::ptr::copy_nonoverlapping(name.as_ptr(), np, name.len());
        std::ptr::copy_nonoverlapping(src.as_ptr(), sp, src.len());
        let out = qx_exec(1, np, name.len(), sp, src.len(), 1, 0);
        let len = u32::from_le_bytes(std::slice::from_raw_parts(out, 4).try_into().unwrap());
        String::from_utf8(std::slice::from_raw_parts(out.add(4), len as usize).to_vec()).unwrap()
    };
    assert_eq!(json, "{\"exit\":0,\"stdout\":\"hi\\n\",\"stderr\":\"\"}");
    // SAFETY: a zero-length name is valid; command 9 does not exist.
    let bad = unsafe { qx_exec(9, np, 0, sp, 0, 1, 0) };
    assert!(!bad.is_null());
}
