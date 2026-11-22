// `String`, the standard library and app commands (docs/design/qx-app.md).

use std::sync::{Arc, Mutex};

use super::command::{self, Call, JsArg, Ret};
use super::stdlib::{Host, HostValue, LogOnly, NativeStd, Permissions, StdResult};
use super::{run, Failure, Input, Limits};

fn out(src: &str) -> String {
    match run("t.qx", src, 1, 0) {
        Ok(s) => s,
        Err(Failure(d)) => panic!("{d}"),
    }
}

fn err_codes(src: &str, perms: Permissions) -> Vec<String> {
    let mut input = Input::file("t.qx", src);
    input.perms = perms;
    let mut host = LogOnly::default();
    match super::run_input(&input, 1, 0, &mut host) {
        Ok(o) => panic!("expected an error, got {o:?}"),
        Err(Failure(d)) => d
            .lines()
            .filter_map(|l| l.strip_prefix("error["))
            .map(|l| l[..5].to_string())
            .collect(),
    }
}

#[test]
fn strings_concatenate_compare_and_print() {
    let src = r#"
void main() {
    String a = "x = " + 1 + ", " + 2.5 + ", " + true;
    var b = "é" + "🙂";
    final String c = "done";
    println("{} | {} {} | {}", a, b, len(b), c == "done" && b != c);
}
"#;
    assert_eq!(out(src), "x = 1, 2.5, true | é🙂 2 | true\n");
}

#[test]
fn string_parameters_and_returns() {
    let src = r#"
String greet(String name, int times) {
    String s = "";
    for i in 0..times {
        s = s + "hi " + name + " ";
    }
    return s;
}

void main() {
    println("[{}]", greet("Ada", 2));
}
"#;
    assert_eq!(out(src), "[hi Ada hi Ada ]\n");
}

#[test]
fn string_type_errors() {
    let codes = |body: &str| {
        err_codes(
            &format!("void main() {{\n{body}\n}}\n"),
            Permissions::cli(false),
        )
    };
    assert_eq!(
        codes(r#"String s = "a" - "b";"#),
        ["Q0204", "Q0204", "Q0204"]
    );
    assert_eq!(codes(r#"boolean b = "a" == 1;"#), ["Q0204"]);
    assert_eq!(codes(r#"String s = "{}";"#), ["Q0214"]);
    assert_eq!(
        codes("Qubit q = qubit(); Bit b = measure(q); String s = \"\" + b;"),
        ["Q0218"]
    );
    // `String` is a type name now.
    assert_eq!(codes("int String = 1;"), ["Q0112"]);
}

#[test]
fn std_imports_by_module_function_and_wildcard() {
    let src = r#"
import std.log;
import std.time.now_iso;
import std.time.*;

void main() {
    log.info("hello");
    String t = now_iso();
    int ms = now_millis();
    println("{} {}", len(t), ms > 1700000000000);
}
"#;
    assert_eq!(out(src), "24 true\n");
}

#[test]
fn std_permissions_per_host() {
    let fs = "import std.fs;\nvoid main() {\n    fs.write_text(\"a.txt\", \"x\");\n}\n";
    assert_eq!(err_codes(fs, Permissions::cli(false)), ["Q0703"]);
    assert_eq!(err_codes(fs, Permissions::playground()), ["Q0702"]);
    assert_eq!(err_codes(fs, Permissions::app(&[])), ["Q0703"]);
    let app = "import std.app;\nvoid main() {\n    app.close();\n}\n";
    assert_eq!(err_codes(app, Permissions::cli(true)), ["Q0702"]);
    let unknown = "import std.net;\nvoid main() {}\n";
    assert_eq!(err_codes(unknown, Permissions::cli(true)), ["Q0701"]);
    let bad_fn = "import std.log;\nvoid main() {\n    log.shout(\"x\");\n}\n";
    assert!(err_codes(bad_fn, Permissions::cli(true)).contains(&"Q0701".to_string()));
    let deep = "import std.fs.x.y;\nvoid main() {}\n";
    assert_eq!(err_codes(deep, Permissions::cli(true)), ["Q0701"]);
}

#[test]
fn std_calls_are_rejected_inside_bit_ifs() {
    let src = r#"
import std.log;

void main() {
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    if (b) {
        log.info("one");
    }
}
"#;
    assert_eq!(err_codes(src, Permissions::cli(false)), ["Q0314"]);
}

// Counts host calls, to show that effects run once per run however many shots there are.
struct Counting {
    calls: Arc<Mutex<Vec<String>>>,
}

impl Host for Counting {
    fn call(&mut self, module: &str, function: &str, args: Vec<HostValue>) -> StdResult {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{module}.{function}{args:?}"));
        Ok(HostValue::Unit)
    }
}

#[test]
fn effects_run_once_before_the_shots() {
    let src = r#"
import std.log;

void main() {
    log.info("start");
    Qubit q = qubit();
    h(q);
    Bit b = measure(q);
    println("{}", b);
}
"#;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut host = Counting {
        calls: calls.clone(),
    };
    let input = Input::file("t.qx", src);
    let table = super::run_input(&input, input_shots(), 3, &mut host);
    assert!(table.is_ok());
    assert_eq!(*calls.lock().unwrap(), ["log.info[Str(\"start\")]"]);
}

fn input_shots() -> u64 {
    500
}

#[test]
fn qasm_refuses_effects() {
    let src = "import std.fs;\nvoid main() {\n    fs.write_text(\"a.txt\", \"x\");\n}\n";
    let mut input = Input::file("t.qx", src);
    input.perms = Permissions::cli(true);
    let Err(Failure(d)) = super::qasm_input(&input) else {
        panic!("qasm must refuse std.fs");
    };
    assert!(d.starts_with("error[Q0702]"), "{d}");
}

#[test]
fn std_fs_through_the_cli_host() {
    let root = std::env::temp_dir().join(format!("qxide-std-fs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let src = r#"
import std.fs;

void main() {
    fs.write_text("notes/a.txt", "one");
    fs.append_text("notes/a.txt", " two");
    println("{} {}", fs.read_text("notes/a.txt"), fs.exists("nope.txt"));
    fs.remove("notes/a.txt");
}
"#;
    let mut input = Input::file("t.qx", src);
    input.perms = Permissions::cli(true);
    let mut host = NativeStd::new(root.clone(), Box::new(|_, _| {}));
    assert_eq!(
        super::run_input(&input, 1, 0, &mut host).unwrap(),
        "one two false\n"
    );
    let escape = "import std.fs;\nvoid main() {\n    fs.write_text(\"../x.txt\", \"x\");\n}\n";
    let mut input = Input::file("t.qx", escape);
    input.perms = Permissions::cli(true);
    let Err(Failure(d)) = super::run_input(&input, 1, 0, &mut host) else {
        panic!("escaping path accepted");
    };
    assert!(
        d.starts_with("error[Q0705]") && d.contains("--> t.qx:3:5"),
        "{d}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn std_http_get_in_an_app_with_the_http_permission() {
    use super::stdlib::{HttpAccess, HttpReply};
    let src = r#"
import std.http;

void main() {
    String body = http.get("https://api.example.com/ping");
    println("{} {}", body, len(body));
}
"#;
    let host_with = |status: u16| {
        let mut host = NativeStd::new(std::env::temp_dir(), Box::new(|_, _| {}));
        host.http = Some(HttpAccess {
            hosts: vec!["api.example.com".into()],
            get: Box::new(move |_, _| {
                Ok(HttpReply {
                    status,
                    body: b"pong".to_vec(),
                })
            }),
        });
        host
    };
    let mut input = Input::file("t.qx", src);
    input.perms = Permissions::app(&["http".into()]);
    assert_eq!(
        super::run_input(&input, 1, 0, &mut host_with(200)).unwrap(),
        "pong 4\n"
    );
    let Err(Failure(d)) = super::run_input(&input, 1, 0, &mut host_with(500)) else {
        panic!("a 500 reply must fail");
    };
    assert!(
        d.starts_with("error[Q0707]") && d.contains("--> t.qx:5:19"),
        "{d}"
    );
    let other = src.replace("api.example.com", "evil.example.com");
    let mut input = Input::file("t.qx", &other);
    input.perms = Permissions::app(&["http".into()]);
    let Err(Failure(d)) = super::run_input(&input, 1, 0, &mut host_with(200)) else {
        panic!("an unlisted host must be refused");
    };
    assert!(d.starts_with("error[Q0708]"), "{d}");
    // Without the permission the import itself is refused; the CLI and playground never offer it.
    assert_eq!(err_codes(src, Permissions::app(&[])), ["Q0703"]);
    assert_eq!(err_codes(src, Permissions::cli(true)), ["Q0702"]);
    assert_eq!(err_codes(src, Permissions::playground()), ["Q0702"]);
}

const APP: &str = r#"
import std.log;

public BitReg measure_bell() {
    QReg q = qreg(2);
    h(q[0]);
    cx(q[0], q[1]);
    return measure(q);
}

public Bit coin() {
    Qubit q = qubit();
    h(q);
    return measure(q);
}

public String save_result(String text, int n) {
    log.info("saving " + text);
    return text + " #" + n;
}

public double half(double x) {
    return x / 2;
}

public int big() {
    return 9007199254740993;
}

public void flip(Qubit q) {
    h(q);
}

void hidden() {}

public int total(int[] xs) {
    return len(xs);
}

public int[] squares(int n) {
    int[] out = new int[n];
    return out;
}

struct Pair {
    int a;
    int b;
}

public Pair pair(int n) {
    return new Pair(n, n);
}
"#;

fn app_call(
    name: &str,
    args: &[(&str, JsArg)],
    shots: u64,
    seed: u64,
) -> Result<String, command::CallError> {
    let commands: Vec<String> = [
        "measure_bell",
        "coin",
        "save_result",
        "half",
        "big",
        "flip",
        "hidden",
        "total",
        "squares",
        "pair",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let args: Vec<(String, JsArg)> = args
        .iter()
        .map(|(n, a)| (n.to_string(), a.clone()))
        .collect();
    let mut input = Input::file("app.qx", APP);
    input.perms = Permissions::app(&[]);
    let mut host = LogOnly::default();
    command::call(
        &input,
        &Call {
            module: "app",
            commands: &commands,
            name,
            args: &args,
            shots,
            seed,
            limits: Limits::CLI,
        },
        &mut host,
    )
    .map(|r| r.to_json())
}

#[test]
fn commands_return_distributions_and_values() {
    let r = app_call("measure_bell", &[], 1000, 7).unwrap();
    assert!(r.starts_with("{\"command\":\"measure_bell\",\"returns\":\"BitReg\",\"seed\":7,\"shots\":1000,\"value\":null,\"output\":\"\",\"counts\":[{\"value\":["), "{r}");
    assert!(!r.contains("[0,1]") && !r.contains("[1,0]"), "{r}");
    let one = app_call("coin", &[], 1, 3).unwrap();
    assert!(
        one.contains("\"value\":0,") || one.contains("\"value\":1,"),
        "{one}"
    );
    assert!(one.contains("\"counts\":null"), "{one}");
    let s = app_call(
        "save_result",
        &[
            ("text", JsArg::Str("00=4 \"q\"".into())),
            ("n", JsArg::Int(3)),
        ],
        1,
        0,
    )
    .unwrap();
    assert!(s.contains("\"value\":\"00=4 \\\"q\\\" #3\""), "{s}");
    assert!(app_call("half", &[("x", JsArg::Int(3))], 1, 0)
        .unwrap()
        .contains("\"value\":1.5,"));
    assert!(
        app_call("half", &[("x", JsArg::Str("Infinity".into()))], 1, 0)
            .unwrap()
            .contains("\"value\":\"Infinity\",")
    );
    // Beyond 2^53 an int travels as a decimal string.
    assert!(app_call("big", &[], 1, 0)
        .unwrap()
        .contains("\"value\":\"9007199254740993\""));
}

#[test]
fn same_seed_same_counts_as_the_cli_sampling() {
    let a = app_call("measure_bell", &[], 2000, 42).unwrap();
    let b = app_call("measure_bell", &[], 2000, 42).unwrap();
    assert_eq!(a, b);
    let c = app_call("measure_bell", &[], 2000, 43).unwrap();
    assert_ne!(a, c);
}

#[test]
fn command_errors() {
    let code = |r: Result<String, command::CallError>| r.unwrap_err().code;
    assert_eq!(code(app_call("nope", &[], 1, 0)), "Q0711");
    assert_eq!(
        code(app_call(
            "save_result",
            &[("text", JsArg::Int(1)), ("n", JsArg::Int(1))],
            1,
            0
        )),
        "Q0712"
    );
    assert_eq!(
        code(app_call(
            "save_result",
            &[("text", JsArg::Str("a".into()))],
            1,
            0
        )),
        "Q0712"
    );
    assert_eq!(
        code(app_call(
            "save_result",
            &[
                ("text", JsArg::Str("a".into())),
                ("n", JsArg::Int(1)),
                ("x", JsArg::Null)
            ],
            1,
            0
        )),
        "Q0712"
    );
    assert_eq!(
        code(app_call("half", &[("x", JsArg::Int(1))], 0, 0)),
        "Q0712"
    );
    assert_eq!(
        code(app_call("half", &[("x", JsArg::Int(1))], 1_000_001, 0)),
        "Q0712"
    );
    assert_eq!(code(app_call("flip", &[], 1, 0)), "Q0714");
    assert_eq!(code(app_call("hidden", &[], 1, 0)), "Q0714");
    // Arrays cannot cross the JavaScript boundary yet, in either direction.
    assert_eq!(code(app_call("total", &[], 1, 0)), "Q0714");
    assert_eq!(
        code(app_call("squares", &[("n", JsArg::Int(2))], 1, 0)),
        "Q0714"
    );
    assert_eq!(
        code(app_call("pair", &[("n", JsArg::Int(2))], 1, 0)),
        "Q0714"
    );
    let e = app_call("flip", &[], 1, 0).unwrap_err();
    assert_eq!(e.diagnostics[0].line, 30, "{e:?}");
    assert!(e.to_json().contains("\"endColumn\":"), "{}", e.to_json());
}

#[test]
fn compile_errors_come_back_structured() {
    let mut input = Input::file("app.qx", "public int f() {\n    return x;\n}\n");
    input.perms = Permissions::app(&[]);
    let e = command::signatures(&input, "app", &["f".to_string()]).unwrap_err();
    assert_eq!(e.code, "Q0201");
    assert_eq!((e.diagnostics[0].line, e.diagnostics[0].column), (2, 12));
    assert!(e.rendered.starts_with("error[Q0201]"), "{}", e.rendered);
}

#[test]
fn signatures_list_parameters_and_returns() {
    let mut input = Input::file("app.qx", APP);
    input.perms = Permissions::app(&[]);
    let sigs = command::signatures(
        &input,
        "app",
        &["save_result".to_string(), "measure_bell".to_string()],
    )
    .unwrap();
    assert_eq!(
        command::signatures_json(&sigs),
        "[{\"name\":\"save_result\",\"params\":[{\"name\":\"text\",\"type\":\"String\"},{\"name\":\"n\",\"type\":\"int\"}],\"returns\":\"String\"},{\"name\":\"measure_bell\",\"params\":[],\"returns\":\"BitReg\"}]"
    );
}

struct CancelAfter {
    left: std::cell::Cell<u32>,
}

impl Host for CancelAfter {
    fn call(&mut self, _: &str, _: &str, _: Vec<HostValue>) -> StdResult {
        Ok(HostValue::Unit)
    }

    fn cancelled(&self) -> bool {
        let n = self.left.get();
        self.left.set(n.saturating_sub(1));
        n == 0
    }
}

#[test]
fn cancelling_stops_between_shots() {
    let commands = vec!["measure_bell".to_string()];
    let mut input = Input::file("app.qx", APP);
    input.perms = Permissions::app(&[]);
    let mut host = CancelAfter {
        left: std::cell::Cell::new(10),
    };
    let e = command::call(
        &input,
        &Call {
            module: "app",
            commands: &commands,
            name: "measure_bell",
            args: &[],
            shots: 100_000,
            seed: 0,
            limits: Limits::CLI,
        },
        &mut host,
    )
    .unwrap_err();
    assert_eq!(e.code, "Q0713");
    let _ = Ret::Null;
}

// Elaboration asks the host every few thousand budget steps, so even a loop without statements
// (which never reached the old per-statement poll) stops when the page cancels the call.
#[test]
fn cancelling_stops_a_loop_without_statements() {
    let src =
        "public int spin() {\n    for i in 0..9000000000000000000 {\n    }\n    return 0;\n}\n";
    let commands = vec!["spin".to_string()];
    let mut input = Input::file("app.qx", src);
    input.perms = Permissions::app(&[]);
    let mut host = CancelAfter {
        left: std::cell::Cell::new(100),
    };
    let e = command::call(
        &input,
        &Call {
            module: "app",
            commands: &commands,
            name: "spin",
            args: &[],
            shots: 1,
            seed: 0,
            limits: Limits::APP,
        },
        &mut host,
    )
    .unwrap_err();
    assert_eq!(e.code, "Q0713", "{}", e.rendered);
}

// B-03 for commands: each distinct record of a multi-shot call adds its row to the output budget.
#[test]
fn command_counts_stop_at_the_output_limit() {
    let src = "public BitReg spread() {\n    QReg q = qreg(20);\n    for i in 0..20 {\n        h(q[i]);\n    }\n    return measure(q);\n}\n";
    let commands = ["spread".to_string()];
    let mut input = Input::file("app.qx", src);
    input.perms = Permissions::app(&[]);
    let call = |limits: Limits| {
        command::call(
            &input,
            &Call {
                module: "app",
                commands: &commands,
                name: "spread",
                args: &[],
                shots: 100_000,
                seed: 1,
                limits,
            },
            &mut LogOnly::default(),
        )
    };
    let small = Limits {
        max_output_bytes: 64 << 10,
        ..Limits::APP
    };
    let e = call(small).map(|r| r.to_json().len()).unwrap_err();
    assert_eq!(e.code, "Q0415", "{}", e.rendered);
    // The same call fits the app limits: about 100,000 rows of 20 bits.
    let json = call(Limits::APP).unwrap().to_json();
    assert!(json.len() > 1 << 20, "{} bytes", json.len());
}

// Diagnostics sent to the page are capped like rendered ones (M-01).
#[test]
fn command_errors_list_at_most_max_diagnostics() {
    let src = format!(
        "public int many() {{\n{}    return 0;\n}}\n",
        "    a;\n".repeat(500)
    );
    let commands = ["many".to_string()];
    let mut input = Input::file("app.qx", &src);
    input.perms = Permissions::app(&[]);
    let e = command::call(
        &input,
        &Call {
            module: "app",
            commands: &commands,
            name: "many",
            args: &[],
            shots: 1,
            seed: 1,
            limits: Limits::APP,
        },
        &mut LogOnly::default(),
    )
    .map(|r| r.to_json())
    .unwrap_err();
    assert_eq!(e.diagnostics.len(), Limits::APP.max_diagnostics);
    assert!(e.rendered.contains("Q0418"), "{}", e.rendered);
}

// M-13: programs typed into Qxide Studio (`std.qxide.run`) run under the desktop app limits,
// not the CLI's 1 GiB state vector.
#[test]
fn studio_runs_use_app_limits() {
    let src = "void main() {\n    QReg q = qreg(25);\n    t(q[0]);\n    BitReg b = measure(q);\n    println(\"{}\", b);\n}\n";
    let r = super::studio::call(
        "run",
        &[
            HostValue::Str(src.into()),
            HostValue::Int(1),
            HostValue::Int(0),
        ],
        &|| false,
    )
    .unwrap();
    let HostValue::Str(json) = r else {
        panic!("std.qxide.run returns a String");
    };
    assert!(json.contains("\"Q0406\""), "{json}");
    assert!(json.contains("desktop app limit"), "{json}");
}

// M-10 in Studio: `std.qxide.qasm` checks the QASM size estimate against the app output limit
// before it builds the text.
#[test]
fn studio_qasm_uses_the_app_output_limit() {
    let src = "void main() {\n    QReg q = qreg(1000000);\n    h(q[0]);\n    for i in 1..1000000 {\n        cx(q[0], q[i]);\n        cx(q[i], q[0]);\n    }\n    discard(q);\n}\n";
    let r = std::thread::Builder::new()
        .stack_size(super::STACK_SIZE)
        .spawn(move || super::studio::call("qasm", &[HostValue::Str(src.into())], &|| false))
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
    let HostValue::Str(json) = r else {
        panic!("std.qxide.qasm returns a String");
    };
    assert!(json.len() < 1 << 20, "{} bytes", json.len());
    assert!(
        json.contains("\"Q0415\""),
        "{}",
        &json[..json.len().min(400)]
    );
}
