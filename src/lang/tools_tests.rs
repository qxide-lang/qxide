// Tests for `qxide fmt`, `qxide explain` and `qxide repl`.

use super::diag::Code;
use super::*;

const EXAMPLES: &[(&str, &str)] = &[
    ("bell.qx", include_str!("../../examples/qx/bell.qx")),
    ("ghz.qx", include_str!("../../examples/qx/ghz.qx")),
    ("teleport.qx", include_str!("../../examples/qx/teleport.qx")),
    (
        "functions.qx",
        include_str!("../../examples/qx/functions.qx"),
    ),
    ("rotation.qx", include_str!("../../examples/qx/rotation.qx")),
    (
        "teleport_fn.qx",
        include_str!("../../examples/qx/teleport_fn.qx"),
    ),
    (
        "conditional_fn.qx",
        include_str!("../../examples/qx/conditional_fn.qx"),
    ),
    ("template/main.qx", include_str!("../template/main.qx")),
];

const DESIGN_DOC: &str = include_str!("../../docs/design/qx-language.md");

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn fmt_ok(src: &str) -> String {
    match format("t.qx", src) {
        Ok(s) => s,
        Err(Failure(d)) => panic!("fmt failed:\n{d}"),
    }
}

// ---- fmt ----

#[test]
fn fmt_leaves_every_example_unchanged() {
    for (name, src) in EXAMPLES {
        assert_eq!(fmt_ok(src), *src, "{name} is not in canonical form");
    }
}

// Strips all indentation, moves each block's `{` to its own line and doubles blank lines.
fn mangle(src: &str) -> String {
    let mut out = String::new();
    for line in lf(src).lines() {
        let line = line.trim_start();
        match line.strip_suffix(" {") {
            Some(head) if !line.starts_with("//") => {
                out.push_str(head);
                out.push_str("\n{\n");
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
        if line.is_empty() {
            out.push('\n');
        }
    }
    out
}

#[test]
fn fmt_restores_mangled_examples_without_changing_meaning() {
    for (name, src) in EXAMPLES {
        let src = lf(src);
        let messy = mangle(&src);
        assert_ne!(messy, src, "{name}: mangling changed nothing");
        let out = fmt_ok(&messy);
        assert_eq!(out, src, "{name}");
        assert_eq!(fmt::fingerprint(&out), fmt::fingerprint(&messy), "{name}");
        assert_eq!(qasm("t.qx", &out).ok(), qasm("t.qx", &src).ok(), "{name}");
    }
}

const MESSY: &str = "// header comment

int   sign( int n ){
  if n<0{return -1;}   // negative
  else if(n==0)
  {
    return 0 ;
  }
  return(int) -2.5*-1+n%2;
}



void   noop(){}
void main( ) {
    double d=( double )sign(-3) ;
    // own line, indented wrong
        QReg r=qreg( 3 ) ;
    for i in 0 .. len(r) { h( r[ i ] ) ; }



    BitReg m=measure( r );
    boolean ok = !(1 < 2) || d >= -1.0 && true;
    println( \"{} {} {}\" , m.to_binary() , ok, // trailing in a call
        m[0] );
    noop();
}
";

const TIDY: &str = "// header comment

int sign(int n) {
    if n < 0 {
        return -1;
    }   // negative
    else if (n == 0) {
        return 0;
    }
    return (int) -2.5 * -1 + n % 2;
}

void noop() {}
void main() {
    double d = (double) sign(-3);
    // own line, indented wrong
    QReg r = qreg(3);
    for i in 0..len(r) {
        h(r[i]);
    }

    BitReg m = measure(r);
    boolean ok = !(1 < 2) || d >= -1.0 && true;
    println(\"{} {} {}\", m.to_binary(), ok, // trailing in a call
        m[0]);
    noop();
}
";

#[test]
fn fmt_canonical_layout() {
    let out = fmt_ok(MESSY);
    assert_eq!(out, TIDY);
    assert_eq!(fmt_ok(&out), out, "formatting is idempotent");
    assert_eq!(fmt::fingerprint(&out), fmt::fingerprint(MESSY));
    assert_eq!(run("t.qx", &out, 1, 0).ok(), run("t.qx", MESSY, 1, 0).ok());
}

#[test]
fn fmt_keeps_literal_spelling_bom_and_crlf() {
    let src = "\u{feff}void main() {\r\n  int big=1_000;double e=1e3;\r\n  println(\"tab\\t{{}} {} {}\",big,e);\r\n}\r\n";
    let out = fmt_ok(src);
    assert_eq!(
        out,
        "\u{feff}void main() {\r\n    int big = 1_000;\r\n    double e = 1e3;\r\n    println(\"tab\\t{{}} {} {}\", big, e);\r\n}\r\n"
    );
}

#[test]
fn fmt_refuses_sources_that_do_not_parse() {
    let Err(Failure(d)) = format("t.qx", "void main() {\n    int x = ;\n}\n") else {
        panic!("expected a parse error");
    };
    assert!(d.starts_with("error[Q0105]"), "{d}");
    // Type errors do not stop formatting: only the layout is checked.
    assert_eq!(
        fmt_ok("void main(){int x=true;}"),
        "void main() {\n    int x = true;\n}\n"
    );
}

// ---- explain ----

fn table_codes() -> Vec<String> {
    let mut codes: Vec<String> = DESIGN_DOC
        .lines()
        .filter_map(|l| l.strip_prefix("| `Q"))
        .filter_map(|rest| rest.split_once("` |").map(|(d, _)| format!("Q{d}")))
        .filter(|c| explain::normalize(c).as_deref() == Some(c.as_str()))
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

fn known_codes() -> Vec<String> {
    let mut all: Vec<String> = Code::ALL.iter().map(|c| c.as_str().to_string()).collect();
    all.extend(explain::RETIRED.iter().map(|c| c.to_string()));
    all.sort();
    all
}

#[test]
fn explain_catalog_covers_every_code_once_in_order() {
    let codes: Vec<String> = explain::entries().into_iter().map(|(c, _)| c).collect();
    assert_eq!(codes, known_codes());
}

#[test]
fn design_doc_tables_list_the_same_codes() {
    assert_eq!(table_codes(), known_codes());
}

/// The ```qx blocks of an entry: the example that triggers the code, then the corrected one.
fn qx_blocks(entry: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = entry;
    while let Some(start) = rest.find("```qx\n") {
        let body = &rest[start + 6..];
        let end = body.find("```").expect("unterminated code block");
        blocks.push(body[..end].to_string());
        rest = &body[end + 3..];
    }
    blocks
}

#[test]
fn explain_examples_trigger_their_code_and_fixes_run() {
    // Safety-net and retired codes cannot be triggered by any program.
    let untriggerable = ["Q0219", "Q0407", "Q0412"];
    // Import errors need a project with several files; their examples are `java` blocks and the
    // same cases are tested in module_tests.rs. Q0508 (import in a lone file) stays a `qx` example.
    let needs_project = [
        "Q0501", "Q0502", "Q0503", "Q0504", "Q0505", "Q0506", "Q0507", "Q0509", "Q0510",
    ];
    for (code, entry) in explain::entries() {
        let blocks = qx_blocks(&entry);
        if untriggerable.contains(&code.as_str()) {
            assert!(blocks.is_empty(), "{code}");
            continue;
        }
        if needs_project.contains(&code.as_str()) {
            assert!(blocks.is_empty(), "{code}");
            assert_eq!(
                entry.matches("```java\n").count(),
                2,
                "{code} needs an example and a fix"
            );
            continue;
        }
        assert_eq!(blocks.len(), 2, "{code} needs an example and a fix");
        match run("example.qx", &blocks[0], 1, 0) {
            Ok(_) => panic!("{code}: the example runs without errors"),
            Err(Failure(d)) => {
                let reported: Vec<&str> = d.lines().filter(|l| l.starts_with("error[")).collect();
                assert!(
                    reported
                        .iter()
                        .any(|l| l.starts_with(&format!("error[{code}]"))),
                    "{code}: example reports {reported:?}"
                );
            }
        }
        if let Err(Failure(d)) = run("fixed.qx", &blocks[1], 1, 0) {
            panic!("{code}: the corrected example fails:\n{d}");
        }
        assert_eq!(
            fmt_ok(&blocks[1]),
            blocks[1],
            "{code}: fixed example is not formatted"
        );
    }
}

#[test]
fn explain_accepts_any_case_and_rejects_unknown_codes() {
    let entry = explain("q0302").expect("lowercase code");
    assert!(entry.starts_with("Q0302: "), "{entry}");
    assert_eq!(explain("Q0302"), Some(entry));
    assert!(explain("Q0219").is_some(), "retired codes keep their entry");
    for bad in ["Q9999", "Q0208", "Q302", "0302", "E0302", ""] {
        assert_eq!(explain(bad), None, "{bad}");
    }
}

// ---- repl ----

fn repl(input: &str) -> (String, String, u8) {
    repl_seed(input, 0)
}

fn repl_seed(input: &str, seed: u64) -> (String, String, u8) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = repl::run(input.as_bytes(), &mut out, &mut err, seed, false);
    (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        code,
    )
}

fn error_codes(err: &str) -> Vec<&str> {
    err.lines()
        .filter_map(|l| l.strip_prefix("error[")?.split(']').next())
        .collect()
}

#[test]
fn repl_keeps_variables_and_echoes_expressions() {
    let (out, err, code) = repl("int n = 2\nn = n + 1;\nn * 10\ndouble half = PI / 2;\n(int) half\nprintln(\"done {}\", n);\n");
    assert_eq!(
        (out.as_str(), err.as_str(), code),
        ("30\n1\ndone 3\n", "", 0)
    );
}

#[test]
fn repl_applies_linearity_and_keeps_going() {
    let (out, err, code) = repl(
        "Qubit q = qubit();\nx(q);\nBit m = measure(q);\nh(q);\nm\nQubit q = qubit();\ndiscard(q);\n",
    );
    assert_eq!(out, "1\n");
    assert_eq!(error_codes(&err), ["Q0301"], "{err}");
    // The rejected line is not counted: `h(q)` is reported on line 4, the line it was typed on.
    assert!(err.contains("<repl>:4:3"), "{err}");
    assert_eq!(code, 0);
}

#[test]
fn repl_reports_live_qubits_when_the_session_ends() {
    let (_, err, code) = repl("Qubit q = qubit();\nh(q);\nQReg r = qreg(2);\ndiscard(r);\n");
    assert_eq!(error_codes(&err), ["Q0302"], "{err}");
    assert!(err.contains("<repl>:1:7"), "{err}");
    assert!(
        !err.contains("| }"),
        "the hidden closing brace is not shown:\n{err}"
    );
    assert_eq!(code, 1);
    // `:quit` ends the session the same way.
    let (_, err, code) = repl("Qubit q = qubit();\n:quit\nh(q);\n");
    assert_eq!((error_codes(&err), code), (vec!["Q0302"], 1));
}

#[test]
fn repl_shown_bits_never_change_later_in_the_session() {
    // A later non-Clifford gate must not change the outcome of a measurement already printed.
    for seed in 0..16 {
        let (out, err, _) = repl_seed(
            "Qubit a = qubit();\nh(a);\nBit m = measure(a);\nm\nQubit b = qubit();\nh(b);\nt(b);\nBit n = measure(b);\nprintln(\"{} {}\", m, n);\n",
            seed,
        );
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[1].starts_with(lines[0]), "seed {seed}: {out}");
    }
}

#[test]
fn repl_defines_functions_across_lines_and_calls_them() {
    let src = "void flip(Qubit q) {\n    x(q);\n}\nint twice(int n) {\n    return n * 2;\n}\nQubit q = qubit();\nflip(q);\nmeasure(q)\ntwice(21)\n";
    let (out, err, code) = repl(src);
    assert_eq!((out.as_str(), err.as_str(), code), ("1\n42\n", "", 0));
}

#[test]
fn repl_commands() {
    let (out, err, code) = repl(":help\nQubit q = qubit();\nh(q);\n:qasm\n:reset\nq\n:nope\n");
    assert!(out.contains("h q[0];"), "{out}");
    assert!(out.contains("OPENQASM 3"), "{out}");
    // After `:reset`, `q` is gone (and no longer leaks).
    assert_eq!(error_codes(&err), ["Q0201"], "{err}");
    assert!(err.contains(":nope") || err.contains("nope"), "{err}");
    assert_eq!(code, 0);
}

#[test]
fn repl_rejects_top_level_return() {
    let (out, err, code) = repl("return;\nprintln(\"after\");\n");
    assert_eq!(out, "after\n");
    assert!(!err.is_empty());
    assert_eq!(code, 0);
}

#[test]
fn repl_reports_runtime_errors_without_keeping_the_input() {
    let (out, err, code) = repl("int zero = 0;\n10 / zero\nzero + 1\n");
    assert_eq!(out, "1\n");
    assert_eq!(error_codes(&err), ["Q0402"], "{err}");
    assert_eq!(code, 0);
}

#[test]
fn repl_waits_for_closing_brackets() {
    assert_eq!(repl::open_brackets("if b {\n"), 1);
    assert_eq!(repl::open_brackets("println(\"{\"); // {\n"), 0);
    assert_eq!(repl::open_brackets("f(a,\n"), 1);
}

#[test]
fn repl_interactive_mode_shows_banner_and_prompts() {
    // The README transcript: a prompt goes to stdout before each line is read, diagnostics to stderr.
    let input = "Qubit q = qubit();\nh(q);\nBit m = measure(q);\nm\nh(q);\nvoid f() {\n}\n:quit\n";
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = repl::run(input.as_bytes(), &mut out, &mut err, 0, true);
    let out = String::from_utf8(out).unwrap();
    let banner = msg::repl_banner(env!("CARGO_PKG_VERSION"));
    assert_eq!(out, format!("{banner}qx> qx> qx> qx> 1\nqx> qx> ... qx> "));
    let err = String::from_utf8(err).unwrap();
    assert_eq!(error_codes(&err), ["Q0301"]);
    assert_eq!(code, 0);
}
