// Deprecated snake_case aliases (src/lang/names.rs): every alias runs like its canonical name on
// every engine and warns exactly once with `Q0801`; the canonical name never warns.

use super::engine::{self, Engine};
use super::names::ALIASES;
use super::stdlib::Permissions;
use super::{check_input, run_with, take_warnings, Failure, Input, Limits};

// A program for each alias: `@` stands for the method or function name.
fn program(owner: &str, old: &str) -> Option<String> {
    let body = match (owner, old) {
        ("String", "char_at") => r#"println("{}", "hello".@(1));"#,
        ("String", "index_of") => r#"println("{}", "hello".@("l"));"#,
        ("String", "starts_with") => r#"println("{}", "hello".@("he"));"#,
        ("String", "ends_with") => r#"println("{}", "hello".@("lo"));"#,
        ("String", "to_upper") => r#"println("{}", "Hello".@());"#,
        ("String", "to_lower") => r#"println("{}", "Hello".@());"#,
        ("String", "parse_int") => r#"println("{}", " 42".@() + 1);"#,
        ("String", "parse_double") => r#"println("{}", "2.5".@() * 2.0);"#,
        ("String", "try_parse_int") => r#"println("{}", "x".@().isOk());"#,
        ("String", "try_parse_double") => r#"println("{}", "2.5".@().orElse(0.0));"#,
        ("String", "split_whitespace") => r#"String[] w = " a  b ".@(); println("{}", len(w));"#,
        ("BitReg", _) => r#"QReg r = qreg(2); BitReg m = measure(r); println("{}", m.@());"#,
        ("List", "remove_at") => {
            r#"List<int> xs = new List<int>(); xs.add(5); xs.add(7); println("{} {}", xs.@(0), xs.size());"#
        }
        ("List", "is_empty") => r#"List<int> xs = new List<int>(); println("{}", xs.@());"#,
        ("List", "to_array") => {
            r#"List<int> xs = new List<int>(); xs.add(5); int[] a = xs.@(); println("{}", a[0]);"#
        }
        ("Map", _) => {
            let call = match old {
                "get_or" => r#"m.@("b", 9)"#,
                "contains_key" => r#"m.@("a")"#,
                _ => "m.@()",
            };
            return Some(format!(
                "void main() {{ Map<String, int> m = new Map<String, int>(); m.put(\"a\", 1); println(\"{{}}\", {call}); }}\n"
            ));
        }
        ("Random", "next_int") => r#"Random g = new Random(7); println("{}", g.@(6));"#,
        ("Random", _) => r#"Random g = new Random(7); println("{}", g.@());"#,
        ("Result", "unwrap_or") => {
            r#"Result<int, String> r = "5".tryParseInt(); println("{}", r.@(0));"#
        }
        ("Result", _) => r#"Result<int, String> r = "5".tryParseInt(); println("{}", r.@());"#,
        ("Option", "unwrap_or") => {
            r#"List<int> xs = new List<int>(); xs.add(3); Option<int> o = xs.pop(); println("{}", o.@(0));"#
        }
        ("Option", _) => {
            r#"List<int> xs = new List<int>(); Option<int> o = xs.pop(); println("{}", o.@());"#
        }
        ("std.math", _) => {
            let args = match old {
                "is_nan" | "is_finite" => "0.5",
                "abs_int" => "-3",
                _ => "6, 4",
            };
            return Some(format!(
                "import std.math;\n\nvoid main() {{ println(\"{{}}\", math.@({args})); }}\n"
            ));
        }
        // Effects: only checked (`checked_aliases_of_effects`).
        _ => return None,
    };
    Some(format!("void main() {{ {body} }}\n"))
}

fn text(r: Result<String, Failure>) -> String {
    match r {
        Ok(s) => format!("ok\n{s}"),
        Err(Failure(d)) => format!("error\n{d}"),
    }
}

// Output on every engine (the same on each), and the warnings of the last run.
fn run_all(src: &str) -> (String, String) {
    let mut engines = vec![Engine::Ast, Engine::Ir];
    if engine::JIT {
        engines.push(Engine::Jit);
    }
    let mut seen: Option<(String, String)> = None;
    for e in engines {
        let got = engine::with(e, || {
            let out = text(run_with("t.qx", src, 1, 0, Limits::CLI));
            (out, take_warnings())
        });
        if let Some(first) = &seen {
            assert_eq!(first, &got, "{} differs on\n{src}", e.name());
        }
        seen = Some(got);
    }
    seen.unwrap()
}

fn q0801_count(warnings: &str) -> usize {
    warnings.matches("warning[Q0801]").count()
}

#[test]
fn every_alias_runs_like_its_canonical_name_and_warns_once() {
    let mut covered = 0;
    for &(owner, old, new) in ALIASES {
        let Some(template) = program(owner, old) else {
            continue;
        };
        covered += 1;
        let (out_new, warn_new) = run_all(&template.replace('@', new));
        assert!(out_new.starts_with("ok\n"), "{owner}.{new}: {out_new}");
        assert_eq!(warn_new, "", "{owner}.{new} warns");
        let (out_old, warn_old) = run_all(&template.replace('@', old));
        assert_eq!(out_old, out_new, "{owner}.{old}");
        assert_eq!(q0801_count(&warn_old), 1, "{owner}.{old}: {warn_old}");
        assert!(
            warn_old.contains(&format!("`{old}`")) && warn_old.contains(&format!("`{new}`")),
            "{warn_old}"
        );
    }
    assert_eq!(covered, 37);
}

fn check_warnings(src: &str, perms: Permissions) -> String {
    let mut input = Input::file("t.qx", src);
    input.perms = perms;
    if let Err(Failure(d)) = check_input(&input) {
        panic!("{src}\n{d}");
    }
    take_warnings()
}

#[test]
fn checked_aliases_of_effects() {
    let calls = [
        ("std.time", "nowIso()"),
        ("std.time", "currentTimeMillis()"),
        ("std.time", "sleep(1)"),
        ("std.time", "monotonicMillis()"),
        ("std.io", "writeLine(\"x\")"),
        ("std.io", "writeErr(\"x\")"),
        ("std.io", "writeErrLine(\"x\")"),
        ("std.io", "readLine()"),
        ("std.io", "readAll()"),
        ("std.fs", "readString(\"a.txt\")"),
        ("std.fs", "writeString(\"a.txt\", \"x\")"),
        ("std.fs", "appendString(\"a.txt\", \"x\")"),
        ("std.app", "setTitle(\"x\")"),
        ("std.app", "setSize(400, 300)"),
    ];
    let std_aliases = ALIASES
        .iter()
        .filter(|(o, _, _)| program(o, "").is_none() && o.starts_with("std."));
    assert_eq!(std_aliases.count(), calls.len());
    for (module, call) in calls {
        let (_, new, old) = ALIASES
            .iter()
            .map(|&(o, old, new)| (o, new, old))
            .find(|&(o, new, _)| o == module && call.starts_with(&format!("{new}(")))
            .unwrap_or_else(|| panic!("{module} {call}"));
        let short = module.trim_start_matches("std.");
        let perms = if short == "app" {
            Permissions::app(&["app".to_string()])
        } else {
            Permissions::cli(true)
        };
        let main = "void main() { f(); }\n";
        let src = |name: &str| {
            let c = call.replacen(new, name, 1);
            format!("import std.{short};\n\npublic void f() {{ {short}.{c}; }}\n{main}")
        };
        assert_eq!(
            check_warnings(&src(new), perms.clone()),
            "",
            "{module}.{new}"
        );
        let w = check_warnings(&src(old), perms);
        assert_eq!(q0801_count(&w), 1, "{module}.{old}: {w}");
    }
}

#[test]
fn imported_old_std_names_warn_and_run_under_the_new_name() {
    let src = "import std.math.abs_int;\nimport std.math.*;\n\nvoid main() {\n    println(\"{} {}\", abs_int(-2), pow_int(2, 3));\n}\n";
    let (out, w) = run_all(src);
    assert_eq!(out, "ok\n2 8\n");
    assert_eq!(q0801_count(&w), 2, "{w}");
    assert!(w.contains("--> t.qx:5:22"), "{w}");
}

#[test]
fn own_methods_and_types_with_old_names_are_left_alone() {
    // A struct method named like an alias is the program's own: no warning, no renaming.
    let src = "struct Bag {\n    int n;\n\n    boolean is_empty() {\n        return this.n == 0;\n    }\n}\n\nvoid main() {\n    Bag b = new Bag(0);\n    println(\"{}\", b.is_empty());\n}\n";
    assert_eq!(run_all(src), ("ok\ntrue\n".to_string(), String::new()));
    // A program's own `Result` replaces the prelude; its missing methods are not aliases.
    let own = "enum Result {\n    Ok,\n    Err;\n\n    boolean isOk() {\n        return true;\n    }\n}\n\nvoid main() {\n    Result r = Result.Ok;\n    println(\"{}\", r.is_ok());\n}\n";
    let (out, w) = run_all(own);
    assert!(out.contains("error[Q0220]"), "{out}");
    assert_eq!(w, "");
}

#[test]
fn generic_bodies_warn_once_and_mutator_aliases_stay_unconditional() {
    let src = "<T> T first(List<T> xs, T d) {\n    return xs.pop().unwrap_or(d);\n}\n\nvoid main() {\n    List<int> a = new List<int>();\n    List<String> b = new List<String>();\n    println(\"{} {}\", first(a, 1), first(b, \"s\"));\n}\n";
    let (out, w) = run_all(src);
    assert_eq!(out, "ok\n1 s\n");
    assert_eq!(q0801_count(&w), 1, "{w}");
    // `next_int` changes the generator, like `nextInt`: not callable under a measured bit.
    for name in ["nextInt", "next_int"] {
        let src = format!("int roll(Random g) {{\n    return g.{name}(6);\n}}\n\nvoid main() {{\n    Random g = new Random(1);\n    Qubit q = qubit();\n    if measure(q) {{\n        roll(g);\n    }}\n}}\n");
        let (out, _) = run_all(&src);
        assert!(out.contains("error[Q0314]"), "{name}: {out}");
    }
}

#[test]
fn warnings_do_not_change_output_or_failures() {
    // The warning comes with the error of the same program; the error is unchanged.
    let src = "void main() {\n    println(\"{}\", \"a\".to_upper());\n    int x = \"no\";\n}\n";
    let (out, w) = run_all(src);
    assert!(out.starts_with("error\nerror[Q0204]"), "{out}");
    assert!(!out.contains("Q0801"), "{out}");
    assert_eq!(q0801_count(&w), 1, "{w}");
}
