// `qxide lsp` as an editor starts it: a child process speaking Content-Length framed JSON-RPC.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

fn read_message(r: &mut impl BufRead) -> Option<String> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            len = n.parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len?];
    r.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

#[test]
fn stdio_session_publishes_check_diagnostics_and_exits_cleanly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start qxide lsp");
    let text = r#"void main() {\n    Qubit q = qubit();\n}\n"#;
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#
            .to_string(),
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"untitled:a.qx","languageId":"qx","version":1,"text":"{text}"}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for msg in &input {
        stdin.write_all(frame(msg).as_bytes()).unwrap();
    }
    stdin.flush().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut messages = Vec::new();
    while let Some(m) = read_message(&mut out) {
        messages.push(m);
    }
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "exit status {status:?}");

    let messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| serde_json::from_str(m).unwrap())
        .collect();
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[0]["result"]["serverInfo"]["name"], "qxide");
    let diag = messages
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics were published");
    let d = &diag["params"]["diagnostics"][0];
    assert_eq!(d["code"], "Q0302", "{diag}");
    // `qxide check` puts this error at 2:11.
    assert_eq!(
        d["range"]["start"],
        serde_json::json!({ "line": 1, "character": 10 })
    );
    let last = messages.last().unwrap();
    assert_eq!(
        (&last["id"], &last["result"]),
        (&2.into(), &serde_json::Value::Null)
    );
}

#[test]
fn lsp_rejects_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["lsp", "--port"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
