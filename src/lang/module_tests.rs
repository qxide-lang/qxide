// Multi-file programs: `import`, `public`, cycles, and linearity across module boundaries.
// Files live in memory; assertions use error codes, file names and line:column.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;

// Package `app` (the entry package) depends on `bell_utils` and `other`.
const BELL_LIB: &str = "\
public void prepare(Qubit a, Qubit b) {
    h(a);
    cx(a, b);
}

public Bit read(consume Qubit q) {
    return measure(q);
}

public void flip(Qubit q) {
    helper(q);
}

void helper(Qubit q) {
    x(q);
}
";

struct Files(HashMap<PathBuf, String>);

impl Files {
    fn new(files: &[(&str, &str)]) -> Self {
        Self(
            files
                .iter()
                .map(|(p, s)| (PathBuf::from(p), s.to_string()))
                .collect(),
        )
    }

    fn read(&self, p: &Path) -> std::io::Result<String> {
        self.0
            .get(p)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }
}

fn project(entry_module: Option<Vec<String>>) -> Project {
    let root = |name: &str, deps: Vec<usize>| PackageRoot {
        name: name.into(),
        src_dir: PathBuf::from(name).join("src"),
        deps,
    };
    Project {
        packages: vec![
            root("app", vec![1, 2]),
            root("bell_utils", vec![]),
            root("other", vec![]),
        ],
        entry_package: 0,
        entry_module,
    }
}

fn path(p: &str) -> String {
    p.split('/').collect::<PathBuf>().display().to_string()
}

/// (code, file, line, column) of every error, or empty when the program checks.
fn errs(entry: &str, files: &[(&str, &str)]) -> Vec<(&'static str, String, usize, usize)> {
    errs_in(entry, files, None)
}

fn errs_in(
    entry: &str,
    files: &[(&str, &str)],
    entry_module: Option<Vec<String>>,
) -> Vec<(&'static str, String, usize, usize)> {
    let files = Files::new(files);
    let read = |p: &Path| files.read(p);
    let p = project(entry_module.clone());
    let input = Input {
        name: "main.qx",
        src: entry,
        project: Some(&p),
        read: &read,
        perms: crate::lang::stdlib::Permissions::cli(false),
    };
    let (map, result) = front_input(&input, entry_module.is_none());
    match result {
        Ok(_) => Vec::new(),
        Err(e) => e
            .iter()
            .map(|d| {
                let (l, c) = map.line_col(d.span.start);
                (
                    d.code.as_str(),
                    map.file_name(d.span.start).to_string(),
                    l,
                    c,
                )
            })
            .collect(),
    }
}

fn run_files(entry: &str, files: &[(&str, &str)], shots: u64) -> Result<String, String> {
    let files = Files::new(files);
    let read = |p: &Path| files.read(p);
    let p = project(None);
    let input = Input {
        name: "main.qx",
        src: entry,
        project: Some(&p),
        read: &read,
        perms: crate::lang::stdlib::Permissions::cli(false),
    };
    run_input(&input, shots, 0, &mut crate::lang::cli_host("data".into())).map_err(|Failure(d)| d)
}

fn bell_files() -> Vec<(&'static str, &'static str)> {
    vec![("bell_utils/src/lib.qx", BELL_LIB)]
}

#[test]
fn imported_functions_run_like_local_ones() {
    let entry = "\
import bell_utils.prepare;
import bell_utils.read;

void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    prepare(a, b);
    Bit ma = read(a);
    Bit mb = measure(b);
    println(\"{}{}\", ma, mb);
}
";
    let out = run_files(entry, &bell_files(), 200).unwrap();
    // Only correlated outcomes: the Bell pair was prepared by the imported function.
    for line in out.lines().filter(|l| l.starts_with('[')) {
        assert!(
            line.starts_with("[0, 0]") || line.starts_with("[1, 1]"),
            "{out}"
        );
    }
    // Same circuit as the single-file program.
    let inline = "\
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println(\"{}{}\", ma, mb);
}
";
    let files = Files::new(&bell_files());
    let read = |p: &Path| files.read(p);
    let p = project(None);
    let input = Input {
        name: "main.qx",
        src: entry,
        project: Some(&p),
        read: &read,
        perms: crate::lang::stdlib::Permissions::cli(false),
    };
    assert_eq!(
        qasm_input(&input).unwrap_or_else(|Failure(d)| panic!("{d}")),
        qasm("t.qx", inline).unwrap_or_else(|Failure(d)| panic!("{d}"))
    );
}

#[test]
fn wildcard_imports_bring_in_public_functions_only() {
    let entry = "\
import bell_utils.*;

void main() {
    Qubit q = qubit();
    flip(q);
    helper(q);
    discard(q);
}
";
    // `helper` is private, so the wildcard does not import it: unknown function.
    assert_eq!(
        errs(entry, &bell_files()),
        vec![("Q0202", "main.qx".into(), 6, 5)]
    );
    let ok = entry.replace("    helper(q);\n", "");
    assert_eq!(errs(&ok, &bell_files()), vec![]);
}

#[test]
fn private_functions_cannot_be_imported() {
    let entry = "import bell_utils.helper;\nvoid main() {}\n";
    assert_eq!(
        errs(entry, &bell_files()),
        vec![("Q0504", "main.qx".into(), 1, 19)]
    );
    let files = Files::new(&bell_files());
    let read = |p: &Path| files.read(p);
    let p = project(None);
    let input = Input {
        name: "main.qx",
        src: entry,
        project: Some(&p),
        read: &read,
        perms: crate::lang::stdlib::Permissions::cli(false),
    };
    let Err(Failure(d)) = check_input(&input) else {
        panic!("expected Q0504")
    };
    // The note points at the private declaration in the other file, and help suggests `public`.
    let lib = path("bell_utils/src/lib.qx");
    assert!(
        d.contains(&format!("--> {lib}:14:6")) && d.contains("help: "),
        "{d}"
    );
}

#[test]
fn private_helpers_with_the_same_name_do_not_collide() {
    let other =
        "public void g(Qubit q) {\n    helper(q);\n}\nvoid helper(Qubit q) {\n    z(q);\n}\n";
    let entry = "\
import bell_utils.flip;
import other.g;

void helper() {}

void main() {
    Qubit q = qubit();
    flip(q);
    g(q);
    helper();
    discard(q);
}
";
    let mut files = bell_files();
    files.push(("other/src/lib.qx", other));
    let out = run_files(entry, &files, 1).unwrap();
    assert_eq!(out, "");
    let files = Files::new(&files);
    let read = |p: &Path| files.read(p);
    let p = project(None);
    let input = Input {
        name: "main.qx",
        src: entry,
        project: Some(&p),
        read: &read,
        perms: crate::lang::stdlib::Permissions::cli(false),
    };
    let qasm = qasm_input(&input).unwrap_or_else(|Failure(d)| panic!("{d}"));
    // `flip` reaches bell_utils' `helper` (x) and `g` reaches other's `helper` (z).
    assert!(
        qasm.contains("x q[0];") && qasm.contains("z q[0];"),
        "{qasm}"
    );
}

#[test]
fn import_errors_have_codes() {
    let main = |imports: &str| format!("{imports}\nvoid main() {{}}\n");
    let files = bell_files();
    assert_eq!(
        errs(&main("import nope.f;"), &files),
        vec![("Q0501", "main.qx".into(), 1, 8)]
    );
    assert_eq!(
        errs(&main("import bell_utils.gates.f;"), &files),
        vec![("Q0502", "main.qx".into(), 1, 1)]
    );
    assert_eq!(
        errs(&main("import bell_utils.nothing;"), &files),
        vec![("Q0503", "main.qx".into(), 1, 19)]
    );
    assert_eq!(
        errs(&main("import bell_utils;"), &files),
        vec![("Q0509", "main.qx".into(), 1, 1)]
    );
    assert_eq!(
        errs("void main() {}\nimport bell_utils.flip;\n", &files),
        vec![("Q0510", "main.qx".into(), 2, 1)]
    );
    // Clashes with a local function and between two imports.
    assert_eq!(
        errs(
            "import bell_utils.flip;\nvoid flip(Qubit q) {}\nvoid main() {}\n",
            &files
        ),
        vec![("Q0506", "main.qx".into(), 1, 19)]
    );
    let mut two = bell_files();
    two.push((
        "other/src/lib.qx",
        "public void flip(Qubit q) {\n    z(q);\n}\n",
    ));
    assert_eq!(
        errs(&main("import bell_utils.flip;\nimport other.flip;"), &two),
        vec![("Q0506", "main.qx".into(), 2, 14)]
    );
    // Two wildcards providing `flip`: only the call is ambiguous; an explicit import settles it.
    let wild = "import bell_utils.*;\nimport other.*;\nvoid main() {\n    Qubit q = qubit();\n    flip(q);\n    discard(q);\n}\n";
    assert_eq!(errs(wild, &two), vec![("Q0507", "main.qx".into(), 5, 5)]);
    let settled = wild.replace("import other.*;", "import other.*;\nimport other.flip;");
    assert_eq!(errs(&settled, &two), vec![]);
    // Importing the same function twice is harmless.
    assert_eq!(
        errs(
            &main("import bell_utils.flip;\nimport bell_utils.flip;"),
            &files
        ),
        vec![]
    );
}

#[test]
fn imports_need_a_project() {
    let src = "import bell_utils.flip;\nvoid main() {}\n";
    let (_, r) = front_input(&Input::file("t.qx", src), true);
    let codes: Vec<&str> = r.unwrap_err().iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["Q0508"]);
}

#[test]
fn import_cycles_are_rejected() {
    // main -> app.a -> app.b -> app.a
    let files = [
        ("app/src/a.qx", "import app.b.g;\npublic void f() {}\n"),
        ("app/src/b.qx", "import app.a.f;\npublic void g() {}\n"),
    ];
    let e = errs("import app.a.f;\nvoid main() {}\n", &files);
    assert_eq!(
        e,
        vec![("Q0505", path("app/src/b.qx"), 1, 1)],
        "the import closing the cycle is reported"
    );
    // A library that imports a module which imports the library back.
    let files = [
        ("app/src/lib.qx", "import app.a.f;\npublic void top() {}\n"),
        ("app/src/a.qx", "import app.top;\npublic void f() {}\n"),
    ];
    let e = errs_in(files[0].1, &files, Some(Vec::new()));
    assert_eq!(e, vec![("Q0505", path("app/src/a.qx"), 1, 1)]);
    // A module importing itself.
    let files = [("app/src/a.qx", "import app.a.f;\npublic void f() {}\n")];
    let e = errs("import app.a.f;\nvoid main() {}\n", &files);
    assert_eq!(e, vec![("Q0505", path("app/src/a.qx"), 1, 1)]);
}

#[test]
fn diamond_imports_are_not_cycles() {
    let files = [
        (
            "app/src/a.qx",
            "import app.c.h1;\npublic void f(Qubit q) {\n    h1(q);\n}\n",
        ),
        (
            "app/src/b.qx",
            "import app.c.h1;\npublic void g(Qubit q) {\n    h1(q);\n}\n",
        ),
        ("app/src/c.qx", "public void h1(Qubit q) {\n    h(q);\n}\n"),
    ];
    let entry = "import app.a.f;\nimport app.b.g;\nvoid main() {\n    Qubit q = qubit();\n    f(q);\n    g(q);\n    discard(q);\n}\n";
    assert_eq!(errs(entry, &files), vec![]);
}

#[test]
fn linearity_holds_across_modules() {
    let files = bell_files();
    // `read` takes ownership: using the qubit afterwards is a use after consume.
    let after = "import bell_utils.read;\nvoid main() {\n    Qubit q = qubit();\n    Bit m = read(q);\n    h(q);\n}\n";
    assert_eq!(errs(after, &files), vec![("Q0301", "main.qx".into(), 5, 7)]);
    // `prepare` only borrows: the caller still has to measure or discard.
    let leak = "import bell_utils.prepare;\nvoid main() {\n    Qubit a = qubit();\n    Qubit b = qubit();\n    prepare(a, b);\n    discard(a);\n}\n";
    assert_eq!(errs(leak, &files), vec![("Q0302", "main.qx".into(), 4, 11)]);
    // The same qubit twice into an imported two-qubit function.
    let dup = "import bell_utils.prepare;\nvoid main() {\n    Qubit a = qubit();\n    prepare(a, a);\n    discard(a);\n}\n";
    assert_eq!(errs(dup, &files), vec![("Q0303", "main.qx".into(), 4, 16)]);
    // A dependency that breaks the rules is reported in its own file.
    let bad = [(
        "bell_utils/src/lib.qx",
        "public void oops(Qubit q) {\n    discard(q);\n}\n",
    )];
    let entry = "import bell_utils.oops;\nvoid main() {\n    Qubit q = qubit();\n    oops(q);\n    discard(q);\n}\n";
    assert_eq!(
        errs(entry, &bad),
        vec![("Q0313", path("bell_utils/src/lib.qx"), 2, 13)]
    );
    // An imported function that measures cannot run inside an `if` on a measurement result.
    let cond = "import bell_utils.read;\nvoid main() {\n    Qubit a = qubit();\n    Bit m = measure(a);\n    Qubit t = qubit();\n    if m {\n        Bit r = read(t);\n    }\n    discard(t);\n}\n";
    let e = errs(cond, &files);
    assert!(e.iter().any(|x| x.0 == "Q0314"), "{e:?}");
}

#[test]
fn library_modules_need_no_main() {
    assert_eq!(
        errs_in(BELL_LIB, &[], Some(Vec::new())),
        vec![],
        "lib.qx checks without `main`"
    );
    // Syntax errors in imported files point into those files.
    let files = [("bell_utils/src/lib.qx", "public void f( {}\n")];
    let e = errs("import bell_utils.f;\nvoid main() {}\n", &files);
    assert_eq!(e, vec![("Q0105", path("bell_utils/src/lib.qx"), 1, 16)]);
}

// Struct names are unique across the program; a struct type is named only in its own file, but
// its values may cross files through `var`.
// A public generic function is instantiated by the module that calls it.
#[test]
fn generic_functions_across_modules() {
    let lib = "public <T> T pick(boolean c, T a, T b) {\n    if c {\n        return a;\n    }\n    return b;\n}\n";
    let files = [("bell_utils/src/lib.qx", lib)];
    let entry = "import bell_utils.pick;\n\nvoid main() {\n    println(\"{} {}\", pick(true, 1, 2), pick(false, \"a\", \"b\"));\n}\n";
    assert_eq!(run_files(entry, &files, 1), Ok("1 b\n".to_string()));
}

#[test]
fn structs_across_modules() {
    let lib = "struct Pt {\n    int x;\n}\n\npublic int make(int n) {\n    Pt p = new Pt(n);\n    return p.x;\n}\n";
    let files = [("bell_utils/src/lib.qx", lib)];
    let entry = "import bell_utils.make;\n\nvoid main() {\n    println(\"{}\", make(4));\n}\n";
    assert_eq!(run_files(entry, &files, 1), Ok("4\n".to_string()));
    // The same struct name in the entry module is a duplicate.
    let dup = "import bell_utils.make;\n\nstruct Pt {\n    int y;\n}\n\nvoid main() {\n    println(\"{}\", make(1));\n}\n";
    let e = errs(dup, &files);
    // (The dependency's own uses of its struct may then also be reported.)
    assert_eq!(e[0].0, "Q0205", "{e:?}");
    // Another file cannot name the type...
    let named = "import bell_utils.make;\n\nvoid main() {\n    Pt p = new Pt(1);\n}\n";
    assert_eq!(errs(named, &files)[0].0, "Q0108");
    // ...but receives its values through `var`.
    let lib2 = "struct Pt {\n    int x;\n}\n\npublic Pt make(int n) {\n    return new Pt(n);\n}\n";
    let via_var = "import bell_utils.make;\n\nvoid main() {\n    var p = make(5);\n    p.x += 1;\n    println(\"{}\", p.x);\n}\n";
    assert_eq!(
        run_files(via_var, &[("bell_utils/src/lib.qx", lib2)], 1),
        Ok("6\n".to_string())
    );
}

// The prelude's types work across modules: a dependency returns a `Result` the entry matches.
#[test]
fn prelude_types_across_modules() {
    let lib = "public Result<int, String> parse(String s) {\n    int v = s.try_parse_int()?;\n    return Result.Ok(v * 2);\n}\n";
    let files = [("bell_utils/src/lib.qx", lib)];
    let entry = "import bell_utils.parse;\n\nvoid main() {\n    match parse(\"21\") {\n        Ok(v) => {\n            println(\"{}\", v);\n        }\n        Err(e) => {\n            println(\"{}\", e);\n        }\n    }\n}\n";
    assert_eq!(run_files(entry, &files, 1), Ok("42\n".to_string()));
}

#[test]
fn dependencies_may_only_import_std_log() {
    let lib =
        "import std.log;\nimport std.time;\n\npublic void note() {\n    log.info(\"x\");\n}\n";
    let entry = "import bell_utils.note;\nimport std.time;\n\nvoid main() {\n    note();\n}\n";
    let e = errs(entry, &[("bell_utils/src/lib.qx", lib)]);
    assert_eq!(e, vec![("Q0703", path("bell_utils/src/lib.qx"), 2, 1)]);
    let ok = "import std.log;\n\npublic void note() {\n    log.info(\"x\");\n}\n";
    assert_eq!(errs(entry, &[("bell_utils/src/lib.qx", ok)]), vec![]);
    // Pure modules have no effects, so dependencies may use them too.
    let math = "import std.math;\n\npublic void note() {\n    double d = math.sqrt(2.0);\n}\n";
    assert_eq!(errs(entry, &[("bell_utils/src/lib.qx", math)]), vec![]);
}
