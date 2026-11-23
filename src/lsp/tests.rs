// Protocol tests: a client drives the real server loop over an in-memory connection.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

use super::{serve, uri};
use crate::lang;

const TIMEOUT: Duration = Duration::from_secs(60);

struct Client {
    conn: Connection,
    server: Option<JoinHandle<Result<(), String>>>,
    next_id: i32,
    notifications: Vec<Notification>,
}

impl Client {
    fn start() -> Self {
        let (server, conn) = Connection::memory();
        let handle = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || serve(&server))
            .unwrap();
        let mut c = Client {
            conn,
            server: Some(handle),
            next_id: 0,
            notifications: Vec::new(),
        };
        let init = c.request("initialize", json!({ "capabilities": {} }));
        assert_eq!(init["serverInfo"]["name"], "qxide");
        c.notify("initialized", json!({}));
        c
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Message::Notification(Notification::new(
                method.into(),
                params,
            )))
            .unwrap();
    }

    fn raw_request(&mut self, method: &str, params: Value) -> lsp_server::Response {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn
            .sender
            .send(Message::Request(Request::new(
                id.clone(),
                method.into(),
                params,
            )))
            .unwrap();
        loop {
            match self.conn.receiver.recv_timeout(TIMEOUT).expect("no reply") {
                Message::Response(r) if r.id == id => return r,
                Message::Notification(n) => self.notifications.push(n),
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        match self.raw_request(method, params).response_result {
            Ok(v) => v,
            Err(e) => panic!("{method}: {e:?}"),
        }
    }

    /// The next `publishDiagnostics` for `uri`.
    fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        loop {
            if let Some(i) = self.notifications.iter().position(|n| {
                n.method == "textDocument/publishDiagnostics" && n.params["uri"] == uri
            }) {
                let n = self.notifications.remove(i);
                return n.params["diagnostics"].as_array().unwrap().clone();
            }
            match self
                .conn
                .receiver
                .recv_timeout(TIMEOUT)
                .expect("no diagnostics")
            {
                Message::Notification(n) => self.notifications.push(n),
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    fn open(&mut self, uri: &str, text: &str) -> Vec<Value> {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "qx", "version": 1, "text": text } }),
        );
        self.diagnostics(uri)
    }

    fn change(&mut self, uri: &str, version: i64, text: &str) -> Vec<Value> {
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }),
        );
        self.diagnostics(uri)
    }

    fn at(&mut self, method: &str, uri: &str, line: u32, character: u32) -> Value {
        self.request(
            method,
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        )
    }

    fn shutdown(mut self) {
        assert_eq!(self.request("shutdown", Value::Null), Value::Null);
        self.notify("exit", Value::Null);
        let server = self.server.take().unwrap();
        assert_eq!(server.join().unwrap(), Ok(()));
    }
}

const URI: &str = "untitled:t.qx";

// Zero-based (line, character) of the `n`th occurrence of `needle` (plus `delta` bytes).
fn pos_of(src: &str, needle: &str, n: usize, delta: usize) -> (u32, u32) {
    let at = src.match_indices(needle).nth(n).expect(needle).0 + delta;
    let p = lang::ide::position(src, at);
    (p.line, p.character)
}

fn start(d: &Value) -> (u64, u64) {
    let s = &d["range"]["start"];
    (
        s["line"].as_u64().unwrap(),
        s["character"].as_u64().unwrap(),
    )
}

/// (code, line, column, caret count) of every error `qxide check` prints, 1-based as printed.
fn check_locations(src: &str) -> Vec<(String, u64, u64, Option<u64>)> {
    // The errors, then the `Q0801` warnings: the order of the editor's diagnostics.
    let mut out = match lang::check("t.qx", src) {
        Err(lang::Failure(out)) => out,
        Ok(()) => String::new(),
    };
    out.push_str(&lang::take_warnings());
    let lines: Vec<&str> = out.lines().collect();
    let mut found = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let code = l
            .strip_prefix("error[")
            .or_else(|| l.strip_prefix("warning["))
            .and_then(|r| r.get(..5));
        let Some(code) = code else {
            continue;
        };
        // The closing Q0418 summary of a capped report has no location.
        if code == "Q0418" {
            continue;
        }
        let at = lines[i + 1]
            .trim_start()
            .strip_prefix("--> t.qx:")
            .expect("primary location");
        let (line, col) = at.split_once(':').unwrap();
        let carets = lines[i + 4].matches('^').count() as u64;
        found.push((
            code.to_string(),
            line.parse().unwrap(),
            col.parse().unwrap(),
            Some(carets),
        ));
    }
    found
}

fn qx_blocks(entry: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = entry;
    while let Some(start) = rest.find("```qx\n") {
        let body = &rest[start + 6..];
        let end = body.find("```").unwrap();
        blocks.push(body[..end].to_string());
        rest = &body[end + 3..];
    }
    blocks
}

#[test]
fn initialize_advertises_the_mvp_features() {
    let (server, conn) = Connection::memory();
    let handle = std::thread::spawn(move || serve(&server));
    conn.sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "initialize".into(),
            json!({ "capabilities": {} }),
        )))
        .unwrap();
    let Message::Response(r) = conn.receiver.recv_timeout(TIMEOUT).unwrap() else {
        panic!("expected the initialize response");
    };
    let caps = &r.response_result.unwrap()["capabilities"];
    for key in [
        "hoverProvider",
        "definitionProvider",
        "documentSymbolProvider",
        "documentFormattingProvider",
    ] {
        assert_eq!(caps[key], true, "{key}");
    }
    assert_eq!(caps["textDocumentSync"]["change"], 1);
    assert_eq!(
        caps["completionProvider"]["triggerCharacters"],
        json!(["."])
    );
    conn.sender
        .send(Message::Notification(Notification::new(
            "initialized".into(),
            json!({}),
        )))
        .unwrap();
    drop(conn);
    // The client went away without `exit`: the loop ends when the channel closes.
    assert!(handle.join().unwrap().is_ok());
}

// The heart of "diagnostics match `qxide check`": every catalog example, both the failing and
// the corrected one, gives the same codes at the same line, column and width.
#[test]
fn diagnostics_match_check_for_every_catalog_example() {
    let mut c = Client::start();
    let mut compared = 0;
    let mut version = 1;
    c.open(URI, "");
    for (_, entry) in lang::explain_entries() {
        for src in qx_blocks(&entry) {
            version += 1;
            let lsp = c.change(URI, version, &src);
            let want = check_locations(&src);
            // `check` prints at most `max_diagnostics` errors; the editor gets all of them.
            if want.len() == lang::Limits::CLI.max_diagnostics {
                assert!(lsp.len() >= want.len(), "{src}\n{lsp:?}");
            } else {
                assert_eq!(lsp.len(), want.len(), "{src}\n{lsp:?}");
            }
            for (d, (code, line, col, carets)) in lsp.iter().zip(&want) {
                assert_eq!(d["code"], code.as_str(), "{src}");
                assert_eq!(d["source"], "qxide");
                // 2 (warning) for `Q08xx`, 1 (error) otherwise.
                let severity = if code.starts_with("Q08") { 2 } else { 1 };
                assert_eq!(d["severity"], severity, "{code}");
                assert_eq!(start(d), (line - 1, col - 1), "{code} in\n{src}");
                let end = &d["range"]["end"];
                if end["line"].as_u64() == Some(line - 1) {
                    let width = end["character"].as_u64().unwrap() - (col - 1);
                    assert_eq!(Some(width.max(1)), *carets, "{code} in\n{src}");
                }
                compared += 1;
            }
        }
    }
    assert!(compared > 40, "only {compared} diagnostics compared");
    c.shutdown();
}

#[test]
fn diagnostic_columns_are_utf16_on_crlf_lines() {
    let mut c = Client::start();
    let src = "void main() {\r\n    println(\"😀 {}\", nope);\r\n}\r\n";
    let d = c.open(URI, src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0]["code"], "Q0201");
    // `check` counts the emoji as one column; UTF-16 counts it as two.
    let want = check_locations(src);
    assert_eq!((want[0].1, want[0].2), (2, 21));
    assert_eq!(start(&d[0]), (1, 21));
    assert_eq!(d[0]["range"]["end"], json!({ "line": 1, "character": 25 }));
    c.shutdown();
}

#[test]
fn diagnostics_carry_notes_help_and_clear_on_close() {
    let mut c = Client::start();
    let src = "void main() {\n    Qubit q = qubit();\n    Bit a = measure(q);\n    Bit b = measure(q);\n}\n";
    let d = c.open(URI, src);
    assert_eq!(d[0]["code"], "Q0301");
    let related = d[0]["relatedInformation"].as_array().unwrap();
    assert!(!related.is_empty(), "{d:?}");
    assert_eq!(related[0]["location"]["uri"], URI);
    assert_eq!(
        related[0]["location"]["range"]["start"]["line"], 2,
        "the note points at the first `measure(q)`"
    );
    // Q0302 has a `help:` line; the message carries it as `check` prints it.
    let unused = "void main() {\n    Qubit q = qubit();\n}\n";
    let d = c.change(URI, 2, unused);
    let Err(lang::Failure(rendered)) = lang::check("t.qx", unused) else {
        panic!("the example checks");
    };
    let message = d[0]["message"].as_str().unwrap();
    let help = rendered.lines().find(|l| l.starts_with("help: "));
    assert!(
        message.contains(&format!("\n{}", help.expect("a help line"))),
        "{message}"
    );
    c.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": URI } }),
    );
    assert!(c.diagnostics(URI).is_empty());
    c.shutdown();
}

const PROGRAM: &str = "\
Bit flip(consume Qubit q) {
    x(q);
    return measure(q);
}

void main() {
    int n = 2;
    Qubit a = qubit();
    for i in 0..n {
        h(a);
    }
    Bit m = flip(a);
    println(\"{} {}\", m, PI);
}
";

#[test]
fn hover_shows_explain_entries_and_declarations() {
    let mut c = Client::start();
    let bad = "void main() {\n    Qubit q = qubit();\n}\n";
    assert_eq!(c.open(URI, bad)[0]["code"], "Q0302");
    let (l, ch) = pos_of(bad, "q =", 0, 0);
    let md = c.at("textDocument/hover", URI, l, ch)["contents"]["value"]
        .as_str()
        .unwrap()
        .to_string();
    let entry = lang::explain("Q0302").unwrap();
    assert!(md.contains("## Q0302: "), "{md}");
    assert!(md.contains(entry.lines().nth(2).unwrap()), "{md}");
    assert!(md.starts_with("```qx\nQubit q\n```"), "{md}");

    c.change(URI, 2, PROGRAM);
    let hover = |c: &mut Client, needle: &str, n: usize| {
        let (l, ch) = pos_of(PROGRAM, needle, n, 0);
        c.at("textDocument/hover", URI, l, ch)["contents"]["value"]
            .as_str()
            .map(str::to_string)
    };
    assert_eq!(hover(&mut c, "a)", 0).unwrap(), "```qx\nQubit a\n```");
    assert_eq!(
        hover(&mut c, "flip(a)", 0).unwrap(),
        "```qx\nBit flip(consume Qubit q)\n```"
    );
    assert_eq!(
        hover(&mut c, "h(a)", 0).unwrap(),
        "```qx\nvoid h(Qubit q)\n```"
    );
    assert_eq!(
        hover(&mut c, "q);", 0).unwrap(),
        "```qx\nconsume Qubit q\n```"
    );
    assert_eq!(hover(&mut c, "i in", 0).unwrap(), "```qx\nint i\n```");
    assert!(hover(&mut c, "PI", 0)
        .unwrap()
        .contains("double PI = 3.14159"));
    assert_eq!(hover(&mut c, "\"{} {}\"", 0), None);

    // A `///` comment of a function follows its signature, separated like the other parts.
    let documented = "/// Flips it.\n/// Twice is a no-op.\nvoid flip(Qubit q) {\n    x(q);\n}\n\nvoid main() {\n    Qubit q = qubit();\n    flip(q);\n    discard(q);\n}\n";
    c.change(URI, 3, documented);
    let (l, ch) = pos_of(documented, "flip(q)", 0, 0);
    assert_eq!(
        c.at("textDocument/hover", URI, l, ch)["contents"]["value"],
        "```qx\nvoid flip(Qubit q)\n```\n\n---\n\nFlips it.\nTwice is a no-op."
    );
    c.shutdown();
}

#[test]
fn definition_finds_locals_parameters_and_functions() {
    let mut c = Client::start();
    assert!(c.open(URI, PROGRAM).is_empty());
    let def = |c: &mut Client, needle: &str, n: usize| {
        let (l, ch) = pos_of(PROGRAM, needle, n, 0);
        let v = c.at("textDocument/definition", URI, l, ch);
        if v.is_null() {
            return None;
        }
        assert_eq!(v["uri"], URI);
        Some(start(&v))
    };
    let (a_line, a_col) = pos_of(PROGRAM, "a = qubit", 0, 0);
    assert_eq!(def(&mut c, "a);", 0), Some((a_line.into(), a_col.into())));
    let (f_line, f_col) = pos_of(PROGRAM, "flip(consume", 0, 0);
    assert_eq!(
        def(&mut c, "flip(a)", 0),
        Some((f_line.into(), f_col.into()))
    );
    let (q_line, q_col) = pos_of(PROGRAM, "q) {", 0, 0);
    assert_eq!(def(&mut c, "q);", 1), Some((q_line.into(), q_col.into())));
    let (n_line, n_col) = pos_of(PROGRAM, "n = 2", 0, 0);
    assert_eq!(def(&mut c, "n {", 0), Some((n_line.into(), n_col.into())));
    assert_eq!(def(&mut c, "h(a)", 0), None, "built-ins have no source");
    c.shutdown();
}

#[test]
fn document_symbols_list_functions_with_their_locals() {
    let mut c = Client::start();
    c.open(URI, PROGRAM);
    let syms = c.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": URI } }),
    );
    let names: Vec<&str> = syms
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["flip", "main"]);
    let main = &syms[1];
    assert_eq!(main["kind"], 12);
    assert_eq!(main["detail"], "void main()");
    let children: Vec<&str> = main["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(children, ["n", "a", "i", "m"]);
    assert_eq!(syms[0]["children"][0]["detail"], "consume Qubit q");
    // While the text does not parse, the last good outline stays.
    c.change(URI, 2, "void main( {");
    let stale = c.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": URI } }),
    );
    assert_eq!(stale, syms);
    c.shutdown();
}

fn labels(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn completion_offers_names_in_scope_keywords_and_builtins() {
    let mut c = Client::start();
    c.open(URI, PROGRAM);
    let (l, ch) = pos_of(PROGRAM, "h(a)", 0, 0);
    let items = c.at("textDocument/completion", URI, l, ch);
    let got = labels(&items);
    for want in [
        "a", "n", "i", "flip", "main", "h", "measure", "PI", "Qubit", "for", "consume",
    ] {
        assert!(got.iter().any(|g| g == want), "{want} missing from {got:?}");
    }
    assert!(!got.iter().any(|g| g == "m"), "`m` is declared later");
    assert!(!got.iter().any(|g| g == "q"), "`q` belongs to `flip`");
    let a = items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "a")
        .unwrap();
    assert_eq!(a["kind"], 6);
    assert_eq!(a["detail"], "Qubit a");

    // Mid-edit: the unfinished line does not parse, the names around it still complete.
    let editing = PROGRAM.replace("Bit m = flip(a);", "Bit m = fl");
    c.change(URI, 2, &editing);
    let (l, ch) = pos_of(&editing, "fl\n", 0, 2);
    let got = labels(&c.at("textDocument/completion", URI, l, ch));
    for want in ["flip", "a", "n"] {
        assert!(got.iter().any(|g| g == want), "{want} missing from {got:?}");
    }

    // A call whose name merely starts with `import` is not an `import` line.
    let prefixed = "void import_all(Qubit q) {\n    h(q);\n}\n\nvoid main() {\n    Qubit a = qubit();\n    import_all(a);\n    discard(a);\n}\n";
    c.change(URI, 3, prefixed);
    let (l, ch) = pos_of(prefixed, "a);\n    discard", 0, 0);
    let got = labels(&c.at("textDocument/completion", URI, l, ch));
    assert!(got.iter().any(|g| g == "a"), "{got:?}");
    let (l, ch) = pos_of("import x", "x", 0, 0);
    c.change(URI, 4, "import x");
    assert!(labels(&c.at("textDocument/completion", URI, l, ch)).is_empty());

    // After `.`: the built-in methods (camelCase names only), `BitReg` first.
    let dotted = "void main() {\n    QReg r = qreg(2);\n    BitReg m = measure(r);\n    println(\"{}\", m.);\n}\n";
    c.change(URI, 3, dotted);
    let (l, ch) = pos_of(dotted, "m.)", 0, 2);
    let got = labels(&c.at("textDocument/completion", URI, l, ch));
    assert_eq!(got[..2], ["asInt", "toBinary"]);
    for m in [
        "indexOf",
        "toUpperCase",
        "tryParseInt",
        "nextBoolean",
        "getOrDefault",
        "orElse",
        "asString",
        "items",
        "isNull",
    ] {
        assert!(got.iter().any(|g| g == m), "{m}: {got:?}");
    }
    assert!(got.iter().all(|g| !g.contains('_')), "{got:?}");
    c.shutdown();
}

// Methods: hover on a call shows the method's signature, `this` is a local of the method, and
// after `.` the document's methods complete.
#[test]
fn methods_hover_and_complete() {
    let mut c = Client::start();
    let src = "struct Vec2 {\n    double x;\n\n    double scaled(double k) {\n        return this.x * k;\n    }\n}\n\nvoid main() {\n    Vec2 v = new Vec2(1);\n    double d = v.scaled(2);\n}\n";
    assert!(c.open(URI, src).is_empty());
    let hover = |c: &mut Client, needle: &str| {
        let (l, ch) = pos_of(src, needle, 0, 0);
        c.at("textDocument/hover", URI, l, ch)["contents"]["value"]
            .as_str()
            .map(str::to_string)
    };
    assert_eq!(
        hover(&mut c, "scaled(2)").unwrap(),
        "```qx\ndouble Vec2.scaled(double k)\n```"
    );
    assert_eq!(hover(&mut c, "this.x").unwrap(), "```qx\nVec2 this\n```");
    assert_eq!(
        hover(&mut c, "scaled(double").unwrap(),
        "```qx\ndouble Vec2.scaled(double k)\n```"
    );
    let dotted = src.replace("v.scaled(2)", "v.");
    c.change(URI, 2, &dotted);
    let (l, ch) = pos_of(&dotted, "v.;", 0, 2);
    let got = labels(&c.at("textDocument/completion", URI, l, ch));
    assert!(got.iter().any(|g| g == "scaled"), "{got:?}");
    c.shutdown();
}

#[test]
fn formatting_replaces_the_whole_document_with_fmt_output() {
    let mut c = Client::start();
    let messy = "void main(){\r\nint x=1;\r\n    println(\"{}\",x);}\r\n";
    c.open(URI, messy);
    let edits = c.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": { "tabSize": 4, "insertSpaces": true } }),
    );
    let want = lang::format("t.qx", messy).ok().unwrap();
    assert_eq!(edits.as_array().unwrap().len(), 1);
    assert_eq!(edits[0]["newText"], want.as_str());
    assert_eq!(
        edits[0]["range"],
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 3, "character": 0 } })
    );
    c.change(URI, 2, &want);
    let none = c.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": {} }),
    );
    assert_eq!(none, json!([]));
    c.change(URI, 3, "void main( {");
    let broken = c.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": {} }),
    );
    assert!(broken.is_null());
    c.shutdown();
}

#[test]
fn unknown_requests_and_documents_are_errors_not_crashes() {
    let mut c = Client::start();
    let r = c.raw_request("textDocument/rename", json!({}));
    assert_eq!(
        r.response_result.unwrap_err().code,
        lsp_server::ErrorCode::MethodNotFound as i32
    );
    let r = c.raw_request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": "untitled:nope.qx" }, "position": { "line": 0, "character": 0 } }),
    );
    assert_eq!(
        r.response_result.unwrap_err().code,
        lsp_server::ErrorCode::InvalidParams as i32
    );
    c.shutdown();
}

// A temporary project folder, removed on drop.
struct TempProject(PathBuf);

impl TempProject {
    fn new(tag: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("qxide-lsp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Self(dir)
    }

    fn uri(&self, path: &str) -> String {
        uri::from_path(&self.0.join(path))
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n";
const GATES: &str = "public void prepare(Qubit a, Qubit b) {\n    h(a);\n    cx(a, b);\n}\n";
const MAIN: &str = "\
import app.gates.prepare;

void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    prepare(a, b);
    println(\"{} {}\", measure(a), measure(b));
}
";

#[test]
fn projects_resolve_imports_across_files_and_unsaved_buffers() {
    let p = TempProject::new(
        "imports",
        &[
            ("qxide.toml", MANIFEST),
            ("src/main.qx", MAIN),
            ("src/gates.qx", GATES),
        ],
    );
    let (main, gates) = (p.uri("src/main.qx"), p.uri("src/gates.qx"));
    let mut c = Client::start();
    assert_eq!(c.open(&main, MAIN), Vec::<Value>::new());

    // Go to definition jumps into the imported module.
    let (l, ch) = pos_of(MAIN, "prepare(a", 0, 0);
    let def = c.at("textDocument/definition", &main, l, ch);
    let target = uri::to_path(def["uri"].as_str().unwrap()).unwrap();
    assert_eq!(
        uri::path_key(&target),
        uri::path_key(&p.0.join("src/gates.qx"))
    );
    assert_eq!(start(&def), (0, 12));
    let (l, ch) = pos_of(MAIN, "gates", 0, 1);
    let def = c.at("textDocument/definition", &main, l, ch);
    assert_eq!(start(&def), (0, 0));
    let (l, ch) = pos_of(MAIN, "prepare(a", 0, 0);
    let md = c.at("textDocument/hover", &main, l, ch)["contents"]["value"].clone();
    assert_eq!(
        md,
        "```qx\npublic void app.gates.prepare(Qubit a, Qubit b)\n```"
    );

    // An unsaved edit of the module reaches its importer: `prepare` is no longer public.
    let private = GATES.replace("public ", "");
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": gates, "languageId": "qx", "version": 1, "text": private } }),
    );
    let d = c.diagnostics(&main);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0]["code"], "Q0504");
    let related = &d[0]["relatedInformation"][0]["location"]["uri"];
    let note_file = uri::to_path(related.as_str().unwrap()).unwrap();
    assert_eq!(
        uri::path_key(&note_file),
        uri::path_key(&p.0.join("src/gates.qx"))
    );

    // An error inside the module is shown on the importer's `import` line, with its location.
    let broken = GATES.replace("cx(a, b);", "cx(a, a);");
    c.change(&gates, 2, &broken);
    let d = c.diagnostics(&main);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0]["code"], "Q0303");
    assert_eq!(start(&d[0]), (0, 0));
    let msg = d[0]["message"].as_str().unwrap();
    assert!(msg.contains("gates.qx:3:11: "), "{msg}");
    c.shutdown();
}

#[test]
fn project_errors_are_reported_like_check() {
    let p = TempProject::new(
        "badmanifest",
        &[
            ("qxide.toml", "[package\n"),
            ("src/main.qx", "void main() {}\n"),
        ],
    );
    let mut c = Client::start();
    let d = c.open(&p.uri("src/main.qx"), "void main() {}\n");
    assert_eq!(d.len(), 1, "{d:?}");
    assert!(d[0]["code"].as_str().unwrap().starts_with("Q06"), "{d:?}");
    c.shutdown();
}
