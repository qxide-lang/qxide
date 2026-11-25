// `--experimental value-semantics` (src/lang/values.rs, docs/design/memory-model.md): the rules
// (`Q0320`-`Q0326`), each with a program it rejects and one it accepts; the run-time meaning on
// every engine (`cargo test` runs `Engine::Dual`); and the language without the feature.

use super::diag::SourceMap;
use super::*;

fn on() {
    features::set_value_semantics(true);
}

fn codes_at(src: &str) -> Vec<(&'static str, usize, usize)> {
    match front(src) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let map = SourceMap::new("t.qx", src);
            e.iter()
                .map(|d| {
                    let (l, c) = map.line_col(d.span.start);
                    (d.code.as_str(), l, c)
                })
                .collect()
        }
    }
}

fn codes(src: &str) -> Vec<&'static str> {
    codes_at(src).into_iter().map(|e| e.0).collect()
}

fn ok(src: &str) {
    assert_eq!(codes_at(src), Vec::new(), "unexpected errors in:\n{src}");
}

fn run_ok(src: &str) -> String {
    match run("t.qx", src, 1, 0) {
        Ok(s) => s,
        Err(Failure(d)) => panic!("run failed:\n{d}"),
    }
}

// The fixes of the first diagnostic, applied one at a time.
fn fixed(src: &str) -> Vec<String> {
    let errs = match front(src) {
        Ok(_) => panic!("no error in:\n{src}"),
        Err(e) => e,
    };
    errs[0]
        .fixes
        .iter()
        .map(|f| {
            let mut out = src.to_string();
            let mut edits = f.edits.clone();
            edits.sort_by_key(|(s, _)| std::cmp::Reverse(s.start));
            for (s, t) in edits {
                out.replace_range(s.start..s.end, &t);
            }
            out
        })
        .collect()
}

const PARTICLE: &str = "struct P {\n    double x;\n    double v;\n}\n\n";

#[test]
fn without_the_feature_nothing_changes() {
    // `inout` is an ordinary name, and classical `consume` stays `Q0221`.
    let src = "int twice(int inout) {\n    return inout * 2;\n}\n\nvoid main() {\n    int inout = 2;\n    inout += 1;\n    println(\"{}\", twice(inout));\n}\n";
    assert_eq!(run("t.qx", src, 1, 0).map_err(|f| f.0).unwrap(), "6\n");
    let consume = "void f(consume int[] a) {\n}\n\nvoid main() {\n    f(new int[1]);\n}\n";
    assert_eq!(codes(consume), vec!["Q0221"]);
    let copy = "void main() {\n    int[] a = {1};\n    int[] b = a.copy();\n}\n";
    assert_eq!(codes(copy), vec!["Q0220"]);
    // Arrays are shared references.
    let shared = main_src("int[] a = {1};\nint[] b = a;\nb[0] = 9;\nprintln(\"{}\", a[0]);");
    assert_eq!(run_ok(&shared), "9\n");
}

fn main_src(body: &str) -> String {
    let body: String = body.lines().map(|l| format!("    {l}\n")).collect();
    format!("void main() {{\n{body}}}\n")
}

#[test]
fn inout_stays_a_name_where_it_is_not_the_word() {
    on();
    // A variable, a parameter and a function argument named `inout`.
    let src = "int twice(int inout) {\n    return inout * 2;\n}\n\nvoid main() {\n    int inout = 2;\n    println(\"{}\", twice(inout));\n    println(\"{}\", twice(inout + 1));\n}\n";
    assert_eq!(run_ok(src), "4\n6\n");
}

#[test]
fn borrowed_parameters_are_read_only() {
    on();
    let field = format!("{PARTICLE}void push(P p) {{\n    p.x = 1.0;\n}}\n\nvoid main() {{\n    P p = new P(0.0, 1.0);\n    push(inout p);\n}}\n");
    assert_eq!(codes(&field)[0], "Q0320");
    // The fixes: `inout`, then `consume`.
    let fixes = fixed(&field);
    assert!(fixes[0].contains("void push(inout P p)"), "{}", fixes[0]);
    assert!(fixes[1].contains("void push(consume P p)"), "{}", fixes[1]);
    ok(&fixes[0]);
    let array =
        "void zero(int[] a) {\n    a[0] = 0;\n}\n\nvoid main() {\n    zero(new int[1]);\n}\n";
    assert_eq!(codes(array), vec!["Q0320"]);
    let list = "void more(List<int> xs) {\n    xs.add(1);\n}\n\nvoid main() {\n    more(new List<int>());\n}\n";
    assert_eq!(codes(list), vec!["Q0320"]);
    let whole = "int down(int n) {\n    n -= 1;\n    return n;\n}\n\nvoid main() {\n    println(\"{}\", down(2));\n}\n";
    assert_eq!(codes(whole), vec!["Q0320"]);
    ok("int down(consume int n) {\n    n -= 1;\n    return n;\n}\n\nvoid main() {\n    println(\"{}\", down(2));\n}\n");
    // A method changes `this` only when declared `inout`.
    let method = "struct C {\n    int n;\n\n    void bump() {\n        this.n += 1;\n    }\n}\n\nvoid main() {\n    C c = new C(0);\n    c.bump();\n}\n";
    assert_eq!(codes(method), vec!["Q0320"]);
    let fixes = fixed(method);
    assert!(fixes[0].contains("    inout void bump() {"), "{}", fixes[0]);
    ok(&fixes[0]);
}

#[test]
fn loop_and_pattern_variables_are_views() {
    on();
    let each = format!("{PARTICLE}void main() {{\n    P[] ps = new P[2];\n    for p in ps {{\n        p.x = 1.0;\n    }}\n}}\n");
    assert_eq!(codes_at(&each), vec![("Q0320", 9, 9)]);
    let indexed = format!("{PARTICLE}void main() {{\n    P[] ps = new P[2];\n    for i in 0..len(ps) {{\n        ps[i].x = 1.0;\n    }}\n    for p in ps {{\n        println(\"{{}}\", p.x);\n    }}\n}}\n");
    assert_eq!(run_ok(&indexed), "1.0\n1.0\n");
    let bound = "void main() {\n    Option<int[]> o = Option.Some(new int[1]);\n    match o {\n        Some(a) => {\n            a[0] = 1;\n        }\n        None => {}\n    }\n}\n";
    assert_eq!(codes(bound), vec!["Q0320"]);
}

#[test]
fn large_values_move_and_moved_variables_are_gone() {
    on();
    let src = main_src("int[] a = {1, 2};\nint[] b = a;\nprintln(\"{}\", a[0]);");
    assert_eq!(codes_at(&src), vec![("Q0321", 4, 19)]);
    let fixes = fixed(&src);
    assert!(fixes[0].contains("int[] b = a.copy();"), "{}", fixes[0]);
    assert_eq!(run_ok(&fixes[0]), "1\n");
    // A new value brings a moved variable back.
    ok(&main_src(
        "int[] a = {1};\nint[] b = a;\na = new int[2];\nprintln(\"{} {}\", len(a), len(b));",
    ));
    // Strings are large too (provisional, memory-model.md §11).
    assert_eq!(
        codes(&main_src(
            "String s = \"a\";\nString t = s;\nprintln(\"{}\", s);"
        )),
        vec!["Q0321"]
    );
    // Moved on one branch is moved after the `if`.
    let branch =
        main_src("int[] a = {1};\nif len(a) > 0 {\n    int[] b = a;\n}\nprintln(\"{}\", len(a));");
    assert_eq!(codes(&branch), vec!["Q0321"]);
    // Small values are copied: both stay usable.
    let small = format!("{PARTICLE}void main() {{\n    P a = new P(1.0, 0.0);\n    P b = a;\n    b.x = 2.0;\n    println(\"{{}} {{}}\", a.x, b.x);\n}}\n");
    assert_eq!(run_ok(&small), "1.0 2.0\n");
}

#[test]
fn keeping_a_borrowed_or_inner_value_needs_a_copy() {
    on();
    let ret = "int[] keep(int[] a) {\n    return a;\n}\n\nvoid main() {\n    println(\"{}\", len(keep(new int[2])));\n}\n";
    assert_eq!(codes(ret), vec!["Q0322"]);
    let fixes = fixed(ret);
    assert!(fixes[0].contains("return a.copy();"), "{}", fixes[0]);
    assert!(
        fixes[1].contains("int[] keep(consume int[] a)"),
        "{}",
        fixes[1]
    );
    ok(&fixes[0]);
    ok(&fixes[1]);
    let elem = main_src("int[][] m = new int[2][2];\nint[] row = m[0];");
    assert_eq!(codes(&elem), vec!["Q0322"]);
    let field = "struct Bag {\n    int[] items;\n}\n\nvoid main() {\n    Bag b = new Bag(new int[1]);\n    int[] xs = b.items;\n}\n";
    assert_eq!(codes(field), vec!["Q0322"]);
    // A collection read shares the element it returns.
    let get = main_src(
        "List<int[]> rows = new List<int[]>();\nrows.add(new int[1]);\nint[] r = rows[0];",
    );
    assert_eq!(codes(&get), vec!["Q0322"]);
    let payload = "void main() {\n    Map<String, int[]> m = new Map<String, int[]>();\n    m.put(\"k\", new int[1]);\n    Option<int[]> o = m.get(\"k\");\n}\n";
    assert_eq!(codes(payload), vec!["Q0322"]);
    // Calls and constructors make new values.
    ok(&main_src(
        "int[] a = new int[2];\nint[][] m = {a, new int[1]};\nprintln(\"{}\", len(m));",
    ));
}

#[test]
fn inout_arguments_do_not_overlap() {
    on();
    let swap =
        "void swap(inout int a, inout int b) {\n    int t = a;\n    a = b;\n    b = t;\n}\n\n";
    let same = format!("{swap}void main() {{\n    int x = 1;\n    swap(inout x, inout x);\n}}\n");
    assert_eq!(codes(&same), vec!["Q0323"]);
    let dynamic = format!("{swap}void main() {{\n    int[] xs = {{1, 2}};\n    int i = 0;\n    swap(inout xs[i], inout xs[1]);\n}}\n");
    assert_eq!(codes(&dynamic), vec!["Q0323"]);
    let read = "int sum(inout int[] a, int[] b) {\n    return a[0] + b[0];\n}\n\nvoid main() {\n    int[] xs = {1};\n    println(\"{}\", sum(inout xs, xs));\n}\n";
    assert_eq!(codes(read), vec!["Q0323"]);
    let apart = format!("{swap}void main() {{\n    int[] xs = {{1, 2}};\n    swap(inout xs[0], inout xs[1]);\n    println(\"{{}} {{}}\", xs[0], xs[1]);\n}}\n");
    assert_eq!(run_ok(&apart), "2 1\n");
}

#[test]
fn moving_an_outer_value_inside_a_loop_is_an_error() {
    on();
    let src = main_src("int[] a = {1};\nfor i in 0..2 {\n    int[] b = a;\n}");
    assert_eq!(codes_at(&src), vec![("Q0324", 4, 19)]);
    ok(&fixed(&src)[0]);
    // A new value before the next iteration is fine, and so is a move just before `break`.
    ok(&main_src(
        "int[] a = {1};\nfor i in 0..2 {\n    int[] b = a;\n    a = new int[1];\n}",
    ));
    ok(&main_src(
        "int[] a = {1};\nwhile true {\n    int[] b = a;\n    break;\n}",
    ));
    let cont = main_src("int[] a = {1};\nfor i in 0..2 {\n    int[] b = a;\n    continue;\n}");
    assert_eq!(codes(&cont), vec!["Q0324"]);
}

#[test]
fn inout_is_written_at_the_call_and_only_there() {
    on();
    let f = "void inc(inout int n) {\n    n += 1;\n}\n\n";
    let unmarked = format!("{f}void main() {{\n    int x = 1;\n    inc(x);\n}}\n");
    assert_eq!(codes(&unmarked), vec!["Q0325"]);
    let fixes = fixed(&unmarked);
    assert!(fixes[0].contains("inc(inout x);"), "{}", fixes[0]);
    ok(&fixes[0]);
    let extra = "int one(int n) {\n    return n;\n}\n\nvoid main() {\n    int x = 1;\n    println(\"{}\", one(inout x));\n}\n";
    assert_eq!(codes(extra), vec!["Q0325"]);
    assert!(fixed(extra)[0].contains("one(x)"), "{:?}", fixed(extra));
    let temp = format!("{f}void main() {{\n    inc(inout len(\"ab\"));\n}}\n");
    assert_eq!(codes(&temp), vec!["Q0325"]);
    let qubit = "void flip(inout Qubit q) {\n    x(q);\n}\n\nvoid main() {\n    Qubit q = qubit();\n    flip(q);\n    Bit b = measure(q);\n}\n";
    assert_eq!(codes(qubit), vec!["Q0325"]);
    let iface = "interface Counter {\n    void add(inout int n);\n}\n\nvoid main() {\n}\n";
    assert_eq!(codes(iface), vec!["Q0325"]);
    let mutating = "interface Tick {\n    void tick();\n}\n\nstruct C implements Tick {\n    int n;\n\n    inout void tick() {\n        this.n += 1;\n    }\n}\n\nvoid main() {\n}\n";
    assert_eq!(codes(mutating), vec!["Q0325"]);
}

#[test]
fn copy_needs_a_copyable_type() {
    on();
    assert_eq!(
        codes(&main_src("Random r = new Random(1);\nRandom s = r.copy();")),
        vec!["Q0326"]
    );
    let iface = "interface Shape {\n    double area();\n}\n\nstruct Sq implements Shape {\n    double s;\n\n    double area() {\n        return this.s * this.s;\n    }\n}\n\nvoid main() {\n    Shape[] all = {new Sq(1.0)};\n    Shape[] copy = all.copy();\n}\n";
    assert_eq!(codes(iface), vec!["Q0326"]);
    // A struct's own `copy` method comes first.
    ok("struct V {\n    int n;\n\n    V copy() {\n        return new V(this.n + 1);\n    }\n}\n\nvoid main() {\n    V v = new V(1);\n    println(\"{}\", v.copy().n);\n}\n");
}

#[test]
fn values_have_value_semantics_on_every_engine() {
    on();
    let src = "struct P {\n    double x;\n    double v;\n\n    inout void bump() {\n        this.x += 1.0;\n    }\n}\n\nstruct Bag {\n    int[] items;\n    P p;\n}\n\nvoid step(inout P p, double dt) {\n    p.x += p.v * dt;\n}\n\nvoid inc(inout int n) {\n    n += 1;\n}\n\nvoid swap(inout int a, inout int b) {\n    int t = a;\n    a = b;\n    b = t;\n}\n\nvoid grow(inout int[] a) {\n    a = new int[5];\n}\n\nvoid rename(inout String s) {\n    s = s + \"!\";\n}\n\nvoid main() {\n    P[] ps = new P[2];\n    ps[0].v = 2.0;\n    step(inout ps[0], 0.5);\n    P a = ps[0];\n    a.x = 9.0;\n    println(\"{} {}\", ps[0].x, a.x);\n    a.bump();\n    println(\"{}\", a.x);\n    int n = 1;\n    inc(inout n);\n    int[] xs = {1, 2, 3};\n    inc(inout xs[1]);\n    swap(inout xs[0], inout xs[2]);\n    println(\"{} {} {} {}\", n, xs[0], xs[1], xs[2]);\n    int[] ys = xs.copy();\n    ys[0] = 100;\n    println(\"{} {}\", xs[0], ys[0]);\n    grow(inout ys);\n    println(\"{}\", len(ys));\n    Bag b = new Bag(xs, a);\n    Bag c = b.copy();\n    c.items[0] = -1;\n    c.p.x = -2.0;\n    println(\"{} {} {} {}\", b.items[0], c.items[0], b.p.x, c.p.x);\n    List<String> words = new List<String>();\n    words.add(\"a\");\n    List<String> w2 = words.copy();\n    w2.add(\"b\");\n    println(\"{} {}\", words.size(), w2.size());\n    Map<String, int[]> m = new Map<String, int[]>();\n    m.put(\"k\", new int[2]);\n    Map<String, int[]> m2 = m.copy();\n    println(\"{}\", m2.size());\n    String s = \"hi\";\n    rename(inout s);\n    println(\"{}\", s);\n    int k = 0;\n    while next(inout k) {\n    }\n    println(\"{}\", k);\n}\n\nboolean next(inout int k) {\n    k += 1;\n    return k < 3;\n}\n";
    assert_eq!(
        run_ok(src),
        "1.0 9.0\n10.0\n2 3 3 1\n3 100\n5\n3 -1 10.0 -2.0\n1 2\n1\nhi!\n3\n"
    );
}

#[test]
fn shared_handles_share_one_value() {
    on();
    let src = "struct Node {\n    int n;\n}\n\nvoid bump(Shared<Node> s) {\n    s.value.n += 1;\n}\n\nvoid replace(Shared<Node> s) {\n    s.value = new Node(10);\n}\n\nvoid main() {\n    Shared<Node> a = new Shared<Node>(new Node(1));\n    Shared<Node> b = a;\n    bump(b);\n    println(\"{} {}\", a.value.n, b.value.n);\n    replace(a);\n    println(\"{}\", b.value.n);\n}\n";
    assert_eq!(run_ok(src), "2 2\n10\n");
    // A program's own `Shared` is an ordinary struct.
    let own = "struct Shared {\n    int n;\n}\n\nvoid main() {\n    Shared a = new Shared(1);\n    Shared b = a;\n    b.n = 2;\n    println(\"{}\", a.n);\n}\n";
    assert_eq!(run_ok(own), "1\n");
    // Without the feature `Shared` is not a type.
    features::set_value_semantics(false);
    assert_eq!(
        codes("void main() {\n    Shared<int> s = new Shared<int>(1);\n}\n")[0],
        "Q0105"
    );
}

#[test]
fn moves_on_break_paths_count_after_the_loop() {
    on();
    let holder = "struct H {\n    int[] items;\n}\n\n";
    let broke = format!("{holder}void main() {{\n    int[] a = {{1}};\n    H h = new H(new int[1]);\n    while true {{\n        h.items = a;\n        break;\n    }}\n    a[0] = 9;\n}}\n");
    assert_eq!(codes(&broke), vec!["Q0321"]);
    let for_break = main_src("int[] a = {1};\nint[] b = new int[1];\nfor i in 0..3 {\n    if i == 1 {\n        b = a;\n        break;\n    }\n}\nprintln(\"{}\", a[0]);");
    assert_eq!(codes(&for_break), vec!["Q0321"]);
    // A `while true` that breaks completes, so moves before it still count.
    let after = main_src(
        "int[] a = {1};\nint[] b = a;\nwhile true {\n    break;\n}\nprintln(\"{}\", a[0]);",
    );
    assert_eq!(codes(&after), vec!["Q0321"]);
    ok(&main_src("int[] a = {1};\nwhile true {\n    int[] b = a;\n    a = new int[1];\n    break;\n}\nprintln(\"{}\", a[0]);"));
}

#[test]
fn changes_through_a_collection_read_change_the_collection() {
    on();
    let p = PARTICLE;
    let get = format!("{p}void f(List<P> xs) {{\n    xs[0].x = 1.0;\n}}\n\nvoid main() {{\n    List<P> xs = new List<P>();\n    f(xs);\n}}\n");
    assert_eq!(codes(&get), vec!["Q0320"]);
    let nested = "void f(List<List<int>> rows) {\n    rows[0].add(1);\n}\n\nvoid main() {\n    f(new List<List<int>>());\n}\n";
    assert_eq!(codes(nested), vec!["Q0320"]);
    let method = "struct C {\n    int n;\n\n    inout void bump() {\n        this.n += 1;\n    }\n}\n\nvoid f(List<C> xs) {\n    xs[0].bump();\n}\n\nvoid main() {\n    f(new List<C>());\n}\n";
    assert_eq!(codes(method), vec!["Q0320"]);
    let payload = "struct P {\n    double x;\n}\n\nOption<int> f(Map<String, P> m) {\n    m.get(\"k\")?.x = 1.0;\n    return Option.None;\n}\n\nvoid main() {\n    Map<String, P> m = new Map<String, P>();\n    println(\"{}\", f(m).isPresent());\n}\n";
    assert_eq!(codes(payload), vec!["Q0320"]);
}

#[test]
fn a_value_cannot_be_lent_and_given_away_in_one_call() {
    on();
    let g = "int g(int[] a, consume int[] b) {\n    return a[0] + b[0];\n}\n\nvoid main() {\n    int[] a = {1};\n    println(\"{}\", g(a, a));\n}\n";
    assert!(codes(g).contains(&"Q0323"), "{:?}", codes(g));
    let inout = "void f(inout int[] a, consume int[] b) {\n}\n\nvoid main() {\n    int[] a = {1};\n    f(inout a, a);\n}\n";
    assert!(codes(inout).contains(&"Q0323"), "{:?}", codes(inout));
    // Small values are copied, so they may be both.
    ok("int g(int a, consume int b) {\n    return a + b;\n}\n\nvoid main() {\n    int a = 1;\n    println(\"{}\", g(a, a));\n}\n");
}

#[test]
fn inout_cells_need_a_plain_statement_and_a_changeable_variable() {
    on();
    let f = "void inc(inout int n) {\n    n += 1;\n}\n\nboolean bump(inout int n) {\n    n += 1;\n    return true;\n}\n\n";
    let short = format!("{f}void main() {{\n    int[] xs = {{1}};\n    int i = 3;\n    boolean b = i < len(xs) && bump(inout xs[i]);\n}}\n");
    assert_eq!(codes(&short), vec!["Q0325"]);
    let fin = format!("{f}void main() {{\n    final int k = 1;\n    inc(inout k);\n}}\n");
    assert_eq!(codes(&fin), vec!["Q0325"]);
    // A local that hides a cell parameter is an ordinary variable.
    let hide = "void inc(inout int n) {\n    if n > 0 {\n        int m = n;\n        for n in 0..2 {\n            m += n;\n        }\n        n = m;\n    }\n}\n\nvoid main() {\n    int x = 1;\n    inc(inout x);\n    println(\"{}\", x);\n}\n";
    assert_eq!(run_ok(hide), "2\n");
}

#[test]
fn an_owned_payload_moves_its_enum_once() {
    on();
    let src =
        "void main() {\n    Option<int[]> o = Option.Some(new int[2]);\n    int[] a = o?;\n}\n";
    // `?` needs a function that returns an `Option`; the move itself is not an error.
    let c = codes(src);
    assert!(!c.contains(&"Q0322"), "{c:?}");
}

#[test]
fn every_offered_fix_removes_its_error() {
    on();
    let programs = [
        "void push(int[] a) {\n    a[0] = 1;\n}\n\nvoid main() {\n    push(new int[1]);\n}\n",
        "int[] keep(int[] a) {\n    return a;\n}\n\nvoid main() {\n    println(\"{}\", len(keep(new int[2])));\n}\n",
        "void main() {\n    int[] a = {1, 2};\n    int[] b = a;\n    println(\"{}\", a[0]);\n}\n",
        "void main() {\n    int[][] m = new int[2][2];\n    int[] row = m[0];\n}\n",
        "void main() {\n    int[] a = {1};\n    for i in 0..2 {\n        int[] b = a;\n    }\n}\n",
        "void inc(inout int n) {\n    n += 1;\n}\n\nvoid main() {\n    int x = 1;\n    inc(x);\n}\n",
        "int one(int n) {\n    return n;\n}\n\nvoid main() {\n    int x = 1;\n    println(\"{}\", one(inout x));\n}\n",
        "struct C {\n    int n;\n\n    void bump() {\n        this.n += 1;\n    }\n}\n\nvoid main() {\n    C c = new C(0);\n    c.bump();\n}\n",
    ];
    for src in programs {
        let first = codes_at(src)[0];
        for f in fixed(src) {
            let after = codes_at(&f);
            assert!(
                !after.iter().any(|e| e.0 == first.0 && e.1 == first.1),
                "{src}\nfix:\n{f}\nstill {after:?}"
            );
        }
    }
    // No `.copy()` fix where `.copy()` cannot copy (`Q0326`).
    let opt = "void main() {\n    Map<String, int[]> m = new Map<String, int[]>();\n    Option<int[]> o = m.get(\"k\");\n}\n";
    assert_eq!(codes(opt), vec!["Q0322"]);
    assert!(fixed(opt).is_empty());
}

// `qxide fix` (M2): a program that ran with reference semantics prints the same once its fixes
// are applied and it runs with value semantics. The first fix of a changed parameter keeps the
// old meaning: `consume` for a number (its own copy), `inout` for an array (the caller's).
#[test]
fn fixed_programs_print_what_they_printed_before() {
    let programs = [
        "struct C {\n    int n;\n\n    void bump() {\n        this.n += 1;\n    }\n}\n\nvoid push(int[] a) {\n    a[0] = 7;\n}\n\nvoid main() {\n    int[] a = {1, 2};\n    push(a);\n    C c = new C(0);\n    c.bump();\n    c.bump();\n    println(\"{} {}\", a[0], c.n);\n}\n",
        "String tag(String s, int n) {\n    s += \"#\" + n;\n    return s;\n}\n\nvoid main() {\n    String s = \"a\";\n    println(\"{} {}\", tag(s, 1), s);\n}\n",
        "int halve(int n) {\n    int k = 0;\n    while n > 1 {\n        n /= 2;\n        k += 1;\n    }\n    return k;\n}\n\nvoid main() {\n    int x = 40;\n    println(\"{} {} {}\", halve(27), halve(x), x);\n}\n",
        "void fill(double[] d, double v) {\n    for i in 0..len(d) {\n        d[i] = v;\n    }\n}\n\nvoid main() {\n    double[] d = new double[3];\n    fill(d, 1.5);\n    println(\"{}\", d[2]);\n}\n",
    ];
    for src in programs {
        features::set_value_semantics(false);
        let before = run_ok(src);
        on();
        assert!(!codes(src).is_empty(), "nothing to fix in\n{src}");
        let fixed = fix(&Input::file("t.qx", src));
        assert!(fixed.remaining.is_none(), "{src}\n{:?}", fixed.remaining);
        assert!(!fixed.applied.is_empty());
        assert_eq!(run_ok(&fixed.src), before, "{}", fixed.src);
    }
    // An alias: `b` saw the change made through `a`. The first fix makes both `Shared<int[]>`
    // handles, so the output stays; without a change afterwards `.copy()` is enough, and `fix`
    // lists its line (`copies`) for the CLI to point out.
    let alias = "void show(int[] xs) {\n    println(\"{}\", xs[0]);\n}\n\nvoid main() {\n    int[] a = {1, 2};\n    int[] b = a;\n    a[0] = 7;\n    show(b);\n    println(\"{} {} {}\", a[0], b[1], len(a));\n    for x in b {\n        println(\"{}\", x);\n    }\n}\n";
    features::set_value_semantics(false);
    let before = run_ok(alias);
    assert_eq!(before, "7\n7 2 2\n7\n2\n");
    on();
    let fixed = fix(&Input::file("t.qx", alias));
    assert!(
        fixed.remaining.is_none(),
        "{:?}\n{}",
        fixed.remaining,
        fixed.src
    );
    assert!(fixed.copies.is_empty(), "{:?}", fixed.copies);
    assert!(fixed.src.contains("Shared<int[]> b = a;"), "{}", fixed.src);
    assert_eq!(run_ok(&fixed.src), before);
    let unchanged = "void main() {\n    int[] a = {1, 2};\n    int[] b = a;\n    println(\"{} {}\", a[0], b[0]);\n}\n";
    let fixed = fix(&Input::file("t.qx", unchanged));
    assert!(fixed.remaining.is_none(), "{:?}", fixed.remaining);
    assert_eq!(fixed.copies, vec![3]);
    assert!(!fixed.src.contains("Shared"), "{}", fixed.src);
    assert_eq!(run_ok(&fixed.src), "1 1\n");
    // `27` cannot be an `inout` argument, so no fix inserts `inout` before it.
    let lit = "void inc(inout int n) {\n    n += 1;\n}\n\nvoid main() {\n    inc(27);\n}\n";
    assert_eq!(codes(lit), vec!["Q0325"]);
    let fixed = fix(&Input::file("t.qx", lit));
    assert!(fixed.applied.is_empty() && fixed.remaining.is_some());
}

// A value of a type parameter is decided per instance after monomorphization: copied for a
// small type, `.copy()` (or `consume`) needed for a large one, and `.copy()` on `T` is then
// accepted for every instance that can be copied.
#[test]
fn type_parameter_values_are_decided_per_instance() {
    on();
    let first = "<T> T first(T[] items) {\n    return items[0];\n}\n\n";
    let small = format!(
        "{first}void main() {{\n    int[] a = {{4, 5}};\n    println(\"{{}}\", first(a));\n}}\n"
    );
    ok(&small);
    assert_eq!(run_ok(&small), "4\n");
    let large = format!("{first}void main() {{\n    String[] a = {{\"x\", \"y\"}};\n    println(\"{{}}\", first(a));\n}}\n");
    assert_eq!(codes(&large), vec!["Q0322"]);
    // The editor shows it too, with its quick fix.
    let diags = ide::analyze(&Input::file("t.qx", &large)).diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].code, diags[0].fixes.len()), ("Q0322", 1));
    let fixes = fixed(&large);
    assert!(fixes[0].contains("return items[0].copy();"), "{}", fixes[0]);
    // Both instances after the fix: `.copy()` of an `int` and of a `String`.
    let both = "<T> T first(T[] items) {\n    return items[0].copy();\n}\n\nvoid main() {\n    int[] a = {4, 5};\n    String[] b = {\"x\", \"y\"};\n    println(\"{} {}\", first(a), first(b));\n}\n";
    ok(both);
    assert_eq!(run_ok(both), "4 x\n");
    // An instance that cannot be copied at all reports `Q0326` at the `.copy()`.
    let qubits = "<T> T first(T[] items) {\n    return items[0].copy();\n}\n\ninterface Named {\n    String name();\n}\n\nstruct A implements Named {\n    int n;\n\n    String name() {\n        return \"a\";\n    }\n}\n\nvoid main() {\n    Named[] xs = {new A(1)};\n    println(\"{}\", first(xs).name());\n}\n";
    assert_eq!(codes(qubits), vec!["Q0326"]);
}

// The `Shared<T>` fix for an alias changed afterwards: through `List.add`, a field, a changing
// method, a function that changes its argument, `+=`, with `var` declarations and with a local
// already named `aInit`. Each program checks after `fix` and prints what it printed before.
#[test]
fn shared_fixes_keep_aliases_working() {
    let programs = [
        "void main() {\n    List<int> a = new List<int>();\n    List<int> b = a;\n    a.add(3);\n    println(\"{} {}\", len(a), b.size());\n}\n",
        "struct P {\n    String name;\n    int n;\n}\n\nvoid main() {\n    P a = new P(\"x\", 1);\n    P b = a;\n    a.n = 5;\n    println(\"{} {}\", a.n, b.n);\n}\n",
        "struct P {\n    String name;\n    int n;\n\n    void bump() {\n        this.n += 1;\n    }\n}\n\nvoid main() {\n    P a = new P(\"x\", 1);\n    P b = a;\n    a.bump();\n    println(\"{} {}\", a.n, b.n);\n}\n",
        "void set(int[] xs) {\n    xs[0] = 9;\n}\n\nvoid main() {\n    int[] a = {1, 2};\n    int[] b = a;\n    set(a);\n    println(\"{} {}\", a[0], b[0]);\n}\n",
        "void main() {\n    int[] a = {1, 2};\n    int[] b = a;\n    a[0] += 1;\n    println(\"{} {}\", a[0], b[0]);\n}\n",
        "void main() {\n    var a = new int[]{1, 2};\n    var b = a;\n    b[1] = 4;\n    println(\"{} {}\", a[1], b[1]);\n}\n",
        "void main() {\n    int aInit = 3;\n    int[] a = {1, 2};\n    int[] b = a;\n    a[0] = aInit;\n    println(\"{} {}\", a[0], b[0]);\n}\n",
    ];
    for src in programs {
        features::set_value_semantics(false);
        let before = run_ok(src);
        on();
        let fixed = fix(&Input::file("t.qx", src));
        assert!(
            fixed.remaining.is_none(),
            "{src}\n{:?}\n{}",
            fixed.remaining,
            fixed.src
        );
        assert!(fixed.src.contains("Shared<"), "{}", fixed.src);
        assert_eq!(run_ok(&fixed.src), before, "{}", fixed.src);
    }
}

#[test]
fn qubit_rules_are_the_same_with_the_feature() {
    // The catalog's examples of the qubit codes report the same code, and their fixes run.
    for (code, entry) in explain::entries() {
        if !("Q0301".."Q0316").contains(&code.as_str()) {
            continue;
        }
        let blocks: Vec<&str> = entry
            .split("```qx\n")
            .skip(1)
            .map(|b| b.split("```").next().unwrap_or(""))
            .collect();
        if blocks.len() != 2 {
            continue;
        }
        let mut seen = Vec::new();
        for value_semantics in [false, true] {
            features::set_value_semantics(value_semantics);
            seen.push((
                codes(blocks[0]),
                run("t.qx", blocks[1], 1, 0).map_err(|f| f.0),
            ));
        }
        assert_eq!(seen[0], seen[1], "{code}");
        assert!(
            seen[1].0.contains(&code.as_str()),
            "{code}: {:?}",
            seen[1].0
        );
    }
    // Quantum examples run as before.
    for src in [
        include_str!("../../examples/qx/bell.qx"),
        include_str!("../../examples/qx/teleport.qx"),
    ] {
        features::set_value_semantics(false);
        let off = run("t.qx", src, 4, 3).map_err(|f| f.0);
        features::set_value_semantics(true);
        assert_eq!(run("t.qx", src, 4, 3).map_err(|f| f.0), off);
    }
}
