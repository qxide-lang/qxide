// Stage 2c: `std.json` (conformance, round trips, limits), `std.path` and the `Json` type. Under
// `cfg(test)` every run goes through every engine and compares them (src/lang/engine.rs).

use super::diag::SourceMap;
use super::stdlib::{Access, Permissions};
use super::*;

fn run_ok(src: &str) -> String {
    match run("t.qx", src, 1, 0) {
        Ok(s) => s,
        Err(Failure(d)) => panic!("run failed:\n{d}\n{src}"),
    }
}

// A `.qx` string literal holding `text`.
fn lit(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const SHOW: &str = "import std.json;

// The compact text of a parsed document, checking that the compact and the indented text both
// read back as the same value.
void show(String t) {
    match json.parse(t) {
        Ok(v) => {
            String s = json.stringify(v);
            String again = \"?\";
            match json.parse(s) {
                Ok(w) => {
                    again = json.stringify(w);
                }
                Err(e) => {
                    again = e;
                }
            }
            String pretty = \"?\";
            match json.parse(json.pretty(v)) {
                Ok(w) => {
                    pretty = json.stringify(w);
                }
                Err(e) => {
                    pretty = e;
                }
            }
            if again == s && pretty == s {
                println(\"ok {}\", s);
            } else {
                println(\"round trip changed {} to {} / {}\", s, again, pretty);
            }
        }
        Err(e) => {
            println(\"err {}\", e);
        }
    }
}
";

// Runs `show` on every input and returns one line per input.
fn shown(inputs: &[&str]) -> Vec<String> {
    let mut src = format!("{SHOW}\nvoid main() {{\n");
    for t in inputs {
        src.push_str(&format!("    show({});\n", lit(t)));
    }
    src.push_str("}\n");
    run_ok(&src).lines().map(String::from).collect()
}

#[test]
fn json_accepts_valid_documents_and_round_trips_them() {
    let cases: &[(&str, &str)] = &[
        ("null", "null"),
        ("true", "true"),
        ("false", "false"),
        ("0", "0"),
        ("-0", "0"),
        ("123", "123"),
        ("-17", "-17"),
        ("1.5", "1.5"),
        ("1.0", "1.0"),
        ("0.1", "0.1"),
        ("1e2", "100.0"),
        ("1E-2", "0.01"),
        ("-1.25e+3", "-1250.0"),
        ("1e300", "1e300"),
        ("123.456e-789", "0.0"),
        ("9223372036854775807", "9223372036854775807"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("9223372036854775808", "9.223372036854776e18"),
        ("\"\"", "\"\""),
        (r#""\"\\\/\b\f\n\r\t""#, r#""\"\\/\b\f\n\r\t""#),
        (r#""\u0041\u00e9""#, "\"Aé\""),
        (r#""\ud83d\ude00""#, "\"😀\""),
        (r#""\u0001\u001F""#, r#""\u0001\u001f""#),
        ("\"κόσμε\"", "\"κόσμε\""),
        ("[]", "[]"),
        ("{}", "{}"),
        (" [ 1 , [ ] , { } ] ", "[1,[],{}]"),
        ("\t\n\r 7 \n", "7"),
        (r#"{"b":1,"a":[true,null]}"#, r#"{"b":1,"a":[true,null]}"#),
        (r#"{"a":1,"b":2,"a":3}"#, r#"{"a":3,"b":2}"#),
        (r#"{"":{"":[]}}"#, r#"{"":{"":[]}}"#),
    ];
    let inputs: Vec<&str> = cases.iter().map(|c| c.0).collect();
    let want: Vec<String> = cases.iter().map(|c| format!("ok {}", c.1)).collect();
    assert_eq!(shown(&inputs), want);
}

#[test]
fn json_rejects_invalid_documents_with_a_position() {
    let cases: &[(&str, &str)] = &[
        ("", "1, column 1"),
        ("nul", "1, column 1"),
        ("tru e", "1, column 1"),
        ("[1,]", "1, column 4"),
        ("[,1]", "1, column 2"),
        (r#"{"a"}"#, "1, column 5"),
        (r#"{"a":1,}"#, "1, column 8"),
        ("{a:1}", "1, column 2"),
        ("01", "1, column 1"),
        ("-", "1, column 1"),
        ("1.", "1, column 1"),
        (".5", "1, column 1"),
        ("1e", "1, column 1"),
        ("+1", "1, column 1"),
        ("1e400", "1, column 1"),
        ("NaN", "1, column 1"),
        ("'a'", "1, column 1"),
        ("\"abc", "1, column 5"),
        (r#""\x""#, "1, column 2"),
        (r#""\ud800""#, "1, column 2"),
        (r#""\udc00""#, "1, column 2"),
        (r#""\ud800\u0041""#, "1, column 2"),
        (r#""\u12""#, "1, column 4"),
        ("\"a\tb\"", "1, column 3"),
        ("[1] 2", "1, column 5"),
        ("[\n  1,\n  x\n]", "3, column 3"),
        ("\u{feff}1", "1, column 1"),
    ];
    let inputs: Vec<&str> = cases.iter().map(|c| c.0).collect();
    let got = shown(&inputs);
    for ((input, at), line) in cases.iter().zip(&got) {
        assert!(
            line.starts_with("err invalid JSON at line ") && line.contains(at),
            "{input:?}: {line}"
        );
    }
    assert_eq!(got.len(), cases.len());
}

#[test]
fn json_nesting_is_limited() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    let ok = deep(json::MAX_DEPTH);
    let too_deep = deep(json::MAX_DEPTH + 1);
    let got = shown(&[&ok, &too_deep]);
    assert_eq!(got[0], format!("ok {ok}"));
    assert!(got[1].starts_with("err invalid JSON"), "{}", got[1]);
    assert!(got[1].contains(&json::MAX_DEPTH.to_string()), "{}", got[1]);
    // Building values nests one level per `ofArray`, up to the same limit.
    let build = |n: usize| {
        format!("import std.json;\n\nvoid main() {{\n    Json v = json.ofNull();\n    for i in 0..{n} {{\n        Json[] one = {{v}};\n        v = json.ofArray(one);\n    }}\n    println(\"{{}}\", json.stringify(v).length());\n}}\n")
    };
    assert_eq!(
        run_ok(&build(json::MAX_DEPTH)),
        format!("{}\n", 2 * json::MAX_DEPTH + 4)
    );
    assert_eq!(runtime_code(&build(json::MAX_DEPTH + 1)), "Q0706");
}

// The deepest value, parsed, printed and dropped near the call-depth limit, fits the
// interpreter's stack on every engine (the parser, printer and drop recurse per level).
#[test]
fn deepest_json_near_the_call_depth_limit() {
    let text = format!(
        "{}{}",
        "[".repeat(json::MAX_DEPTH),
        "]".repeat(json::MAX_DEPTH)
    );
    let src = format!(
        "import std.json;\n\nint deep(int n, String t) {{\n    if n == 0 {{\n        Json v = json.parse(t).orElse(json.ofNull());\n        return json.pretty(v).length() + json.stringify(v).length();\n    }}\n    return deep(n - 1, t);\n}}\n\nvoid main() {{\n    println(\"{{}}\", deep({}, {}));\n}}\n",
        super::machine::MAX_CALL_DEPTH - 3,
        lit(&text)
    );
    let n = json::MAX_DEPTH;
    // Compact: 2 per level; pretty: also a newline and indentation around each inner level.
    let pretty = 2 * n + (0..n - 1).map(|k| 2 + 2 * k + 2 * (k + 1)).sum::<usize>();
    let out = std::thread::Builder::new()
        .stack_size(super::STACK_SIZE)
        .spawn(move || run_ok(&src))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(out, format!("{}\n", 2 * n + pretty));
}

// The first run-time error of a program that passes the checker, as (code, line, column).
fn runtime_at(src: &str) -> (&'static str, usize, usize) {
    if let Err(e) = front(src) {
        panic!("static errors {:?} in:\n{src}", e);
    }
    let Err(e) = build(src) else {
        panic!("expected a run-time error in:\n{src}")
    };
    let map = SourceMap::new("t.qx", src);
    let (l, c) = map.line_col(e[0].span.start);
    (e[0].code.as_str(), l, c)
}

fn runtime_code(src: &str) -> &'static str {
    runtime_at(src).0
}

fn body(lines: &str) -> String {
    format!("import std.json;\n\nvoid main() {{\n{lines}\n}}\n")
}

#[test]
fn json_values_and_their_methods() {
    let src = body(
        r#"    Json v = json.parse("{{\"a\": [1, 2.5, \"x\", false, null], \"b\": {{\"c\": -3}}}}").orElse(json.ofNull());
    println("{} {} {} {}", v.kind(), v.size(), v.has("a"), v.has("z"));
    Json a = v.get("a").orElse(json.ofNull());
    println("{} {} {} {} {}", a.at(0).asInt(), a.at(0).asDouble(), a.at(1).asDouble(), a.at(2).asString(), a.at(3).asBoolean());
    println("{} {} {}", a.at(4).isNull(), a.at(4).kind(), a.size());
    println("{}", v.get("b").orElse(json.ofNull()).get("c").orElse(json.ofInt(0)).asInt());
    println("{}", v.get("z").isPresent());
    String[] keys = v.keys();
    Json[] items = a.items();
    println("{} {} {}", len(keys), keys[1], len(items));
    Json n = new Json();
    Json[] blanks = new Json[2];
    println("{} {} {}", n.isNull(), blanks[1].kind(), json.stringify(json.ofDouble(3)));
    List<Json> xs = new List();
    xs.add(json.ofBoolean(true));
    xs.add(json.ofString("q\"\n"));
    Map<String, Json> m = new Map();
    m.put("list", json.ofList(xs));
    m.put("arr", json.ofArray(items));
    xs.add(json.ofInt(9));
    println("{}", json.stringify(json.ofMap(m)));
    println("{}", json.pretty(json.ofArray(new Json[0])));"#,
    );
    assert_eq!(
        run_ok(&src),
        "object 2 true false\n1 1.0 2.5 x false\ntrue null 5\n-3\nfalse\n2 b 5\ntrue null 3.0\n{\"list\":[true,\"q\\\"\\n\"],\"arr\":[1,2.5,\"x\",false,null]}\n[]\n"
    );
}

#[test]
fn json_kind_and_range_errors() {
    let cases = [
        ("    json.ofString(\"x\").asInt();", ("Q0421", 4, 5)),
        ("    json.ofDouble(1.0).asInt();", ("Q0421", 4, 5)),
        ("    json.ofInt(1).asString();", ("Q0421", 4, 5)),
        ("    json.ofInt(1).size();", ("Q0421", 4, 5)),
        ("    json.ofNull().get(\"a\");", ("Q0421", 4, 5)),
        ("    json.ofNull().at(0);", ("Q0421", 4, 5)),
        ("    json.ofArray(new Json[1]).at(1);", ("Q0403", 4, 5)),
        ("    json.ofDouble(0.0 / 0.0);", ("Q0706", 4, 5)),
        ("    json.ofDouble(1.0 / 0.0);", ("Q0706", 4, 5)),
    ];
    for (line, want) in cases {
        assert_eq!(runtime_at(&body(line)), want, "{line}");
    }
}

// A value that shares its members has a text far larger than its memory: the budget refuses it
// before any text is built.
#[test]
fn json_text_of_shared_values_is_charged_first() {
    let src = body(
        "    Json v = json.ofString(\"0123456789\");\n    for i in 0..200 {\n        Json[] two = {v, v};\n        v = json.ofArray(two);\n    }\n    println(\"{}\", json.stringify(v).length());",
    );
    assert_eq!(runtime_code(&src), "Q0413");
}

#[test]
fn json_static_errors() {
    let codes = |src: &str| match front(src) {
        Ok(_) => Vec::new(),
        Err(e) => e.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
    };
    assert_eq!(codes(&body("    json.parse(1);")), ["Q0204"]);
    assert_eq!(
        codes(&body("    Json v = json.ofInt(1);\n    v.put(\"a\", v);")),
        ["Q0220"]
    );
    assert_eq!(
        codes(&body("    Json v = json.ofInt(1);\n    int n = v.at(0);")),
        ["Q0204"]
    );
    assert_eq!(
        codes("void main() {\n    Json v = new Json();\n}\n"),
        Vec::<&str>::new()
    );
}

#[test]
fn path_functions_work_on_text_alone() {
    let cases: &[(&str, &str, &str)] = &[
        ("join", "\"a/b\", \"c.txt\"", "a/b/c.txt"),
        ("join", "\"a/b/\", \"c\"", "a/b/c"),
        ("join", "\"a\", \"/etc\"", "/etc"),
        ("join", "\"a\", \"C:\\\\x\"", "C:\\x"),
        ("join", "\"\", \"c\"", "c"),
        ("join", "\"a\", \"\"", "a"),
        ("join", "\"/\", \"c\"", "/c"),
        ("join", "\"C:/\", \"c\"", "C:/c"),
        ("fileName", "\"a/b/c.tar.gz\"", "c.tar.gz"),
        ("fileName", "\"a\\\\b\\\\\"", "b"),
        ("fileName", "\"/\"", ""),
        ("parent", "\"a/b/c\"", "a/b"),
        ("parent", "\"a//b\"", "a"),
        ("parent", "\"a\"", ""),
        ("parent", "\"/a\"", "/"),
        ("parent", "\"/\"", ""),
        ("parent", "\"C:\\\\x\"", "C:\\"),
        ("extension", "\"a/b.tar.gz\"", "gz"),
        ("extension", "\".bashrc\"", ""),
        ("extension", "\"a.\"", ""),
        ("stem", "\"a/b.tar.gz\"", "b.tar"),
        ("stem", "\".bashrc\"", ".bashrc"),
        ("normalize", "\"a/./b/../c//d/\"", "a/c/d"),
        ("normalize", "\"../a/../../b\"", "../../b"),
        ("normalize", "\"/../a\"", "/a"),
        ("normalize", "\"C:\\\\a\\\\..\\\\b\"", "C:/b"),
        ("normalize", "\"a/..\"", "."),
        ("normalize", "\"\"", "."),
    ];
    let mut src = String::from("import std.path;\n\nvoid main() {\n");
    for (f, args, _) in cases {
        src.push_str(&format!("    println(\"[{{}}]\", path.{f}({args}));\n"));
    }
    src.push_str("    println(\"{} {} {} {}\", path.isAbsolute(\"/a\"), path.isAbsolute(\"C:\\\\a\"), path.isAbsolute(\"C:a\"), path.isAbsolute(\"a/b\"));\n}\n");
    let mut want: String = cases.iter().map(|(_, _, w)| format!("[{w}]\n")).collect();
    want.push_str("true true false false\n");
    assert_eq!(run_ok(&src), want);
}

// `std.test` assertions are pure and fail with `Q0422`; `assertEquals` shows both texts.
#[test]
fn test_assertions() {
    let ok = "import std.test;\n\nvoid main() {\n    test.assertTrue(1 < 2, \"a\");\n    test.assertFalse(1 > 2, \"b\");\n    test.assertEquals(\"3\", \"\" + (1 + 2));\n    println(\"ok\");\n}\n";
    assert_eq!(run_ok(ok), "ok\n");
    for (line, col) in [
        ("    test.assertTrue(1 > 2, \"a\");", 5),
        ("    test.assertFalse(true, \"b\");", 5),
        ("    test.assertEquals(\"x\", \"y\");", 5),
        ("    test.fail(\"no\");", 5),
    ] {
        let src = format!("import std.test;\n\nvoid main() {{\n{line}\n}}\n");
        assert_eq!(runtime_at(&src), ("Q0422", 4, col), "{line}");
    }
    assert_eq!(
        Permissions::playground().access("test"),
        Some(Access::Allowed)
    );
    // Inside an `if` on a measured bit an assertion would fail whatever the bit is: refused like
    // effects (`Q0314`); pure `std.json`, `std.path` and `std.math` calls stay allowed there.
    let in_bit_if = |call: &str| {
        format!("import std.test;\nimport std.json;\nimport std.path;\n\nvoid main() {{\n    Qubit q = qubit();\n    h(q);\n    Bit m = measure(q);\n    if m {{\n        {call}\n    }}\n}}\n")
    };
    let codes = |src: &str| match front(src) {
        Ok(_) => Vec::new(),
        Err(e) => e.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
    };
    assert_eq!(codes(&in_bit_if("test.fail(\"x\");")), ["Q0314"]);
    assert_eq!(
        codes(&in_bit_if("test.assertTrue(true, \"x\");")),
        ["Q0314"]
    );
    for ok in ["json.stringify(json.ofInt(1));", "path.join(\"a\", \"b\");"] {
        assert_eq!(codes(&in_bit_if(ok)), Vec::<&str>::new(), "{ok}");
    }
}

// `std.json` and `std.path` are pure: every host offers them, including to dependencies.
#[test]
fn json_and_path_are_allowed_everywhere() {
    for p in [
        Permissions::cli(false),
        Permissions::playground(),
        Permissions::repl(),
        Permissions::app(&[]),
    ] {
        assert_eq!(p.access("json"), Some(Access::Allowed));
        assert_eq!(p.access("path"), Some(Access::Allowed));
    }
    assert!(Permissions::known("json") && Permissions::known("path"));
}
