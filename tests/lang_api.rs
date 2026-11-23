// Roadmap 4b: `qxide::lang` (feature `unstable-lang`) checks and runs `.qx` programs from Rust,
// with the CLI's limits or an embedder's own host for the standard library.
#![cfg(feature = "unstable-lang")]

use qxide::lang::{self, Host, HostValue, Input, Limits, Permissions, StdError, StdErrorKind};

#[test]
fn check_and_run_from_rust() {
    let src = "void main() {\n    Qubit q = qubit();\n    h(q);\n    println(\"{} {}\", 6 * 7, measure(q));\n}\n";
    lang::check("bell.qx", src).unwrap();
    let out = lang::run("bell.qx", src, 1, 7).unwrap();
    assert!(out == "42 0\n" || out == "42 1\n", "{out}");
    // Deterministic for a fixed seed.
    assert_eq!(lang::run("bell.qx", src, 1, 7).unwrap(), out);
    let err = lang::check("bad.qx", "void main() {\n    Qubit q = qubit();\n}\n").unwrap_err();
    assert!(err.0.contains("error[Q0302]"), "{}", err.0);
    let tight = Limits {
        max_steps: 10,
        ..Limits::CLI
    };
    let spin = "void main() {\n    int s = 0;\n    for i in 0..1000 {\n        s += i;\n    }\n    println(\"{}\", s);\n}\n";
    let err = lang::run_with("spin.qx", spin, 1, 0, tight).unwrap_err();
    assert!(err.0.contains("Q0413"), "{}", err.0);
}

// An embedder answers std calls itself: here `std.log` lines are collected and every other
// module is refused.
#[derive(Default)]
struct Collect {
    lines: Vec<String>,
}

impl Host for Collect {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> lang::StdResult {
        match (module, args.first()) {
            ("log", Some(HostValue::Str(s))) => {
                self.lines.push(format!("{function}: {s}"));
                Ok(HostValue::Unit)
            }
            _ => Err(StdError::new(
                StdErrorKind::Unavailable,
                format!("std.{module} is not offered here"),
            )),
        }
    }
}

#[test]
fn run_with_an_embedder_host() {
    let src = "import std.log;\nimport std.json;\n\nvoid main() {\n    log.info(json.stringify(json.ofInt(3)));\n    println(\"done\");\n}\n";
    let input = Input {
        name: "embed.qx",
        src,
        project: None,
        read: &lang::read_file,
        perms: Permissions::repl(),
    };
    let mut host = Collect::default();
    let out = lang::run_input(&input, 1, 0, &mut host).unwrap();
    assert_eq!(out, "done\n");
    assert_eq!(host.lines, ["info: 3"]);
    // Modules the permissions do not grant are refused when the program is checked.
    let fs = "import std.fs;\n\nvoid main() {}\n";
    let input = Input { src: fs, ..input };
    assert!(lang::check_input(&input).unwrap_err().0.contains("Q0702"));
}
