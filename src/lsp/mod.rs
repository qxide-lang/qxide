// `qxide lsp`: a Language Server Protocol server over stdio (docs/design/lsp.md).
// `lsp-server` does the JSON-RPC framing and the initialize/shutdown handshake; payloads are
// built with `serde_json` directly. All analysis is `lang::ide`, i.e. the `qxide check` front end.

mod uri;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use serde_json::{json, Value};

use crate::lang::ide::{self, Analysis, Diag, ItemKind, Pos, Range, Symbol, SymbolKind};
use crate::lang::{self, Input};
use crate::pkg;

struct Doc {
    text: String,
    version: Option<i64>,
    path: Option<PathBuf>,
    // Outline of the last version that parsed, shown while the current text does not.
    symbols: Value,
}

struct Analyzed {
    analysis: Analysis,
    // `qxide.toml` or lockfile problems: `check` reports only this error, and so does the server.
    project_error: Option<pkg::Error>,
}

#[derive(Default)]
pub struct Server {
    docs: HashMap<String, Doc>,
    // Cleared on every edit: an open module changes what its importers see.
    cache: HashMap<String, Analyzed>,
}

/// The `.qx` language this server checks, for editors that need more than the version: 1 is the
/// general-purpose language after 0.8.0 (`while`, arrays, structs, ...). Raise it whenever an
/// editor extension must tell a server without some syntax apart from one with it.
pub const LANGUAGE_LEVEL: u32 = 1;

/// Runs the server on stdin/stdout until the client sends `exit`.
pub fn main(stack: usize) -> ExitCode {
    // Editors show the server's stderr in their log: say which binary answered.
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    eprintln!(
        "qxide lsp {} (language level {LANGUAGE_LEVEL}) {exe}",
        env!("CARGO_PKG_VERSION")
    );
    let (conn, io) = Connection::stdio();
    let worker = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || serve(&conn));
    let served = match worker.map(|h| h.join()) {
        Ok(Ok(r)) => r,
        _ => return ExitCode::from(101),
    };
    if let Err(e) = served {
        eprintln!("qxide lsp: {e}");
        return ExitCode::from(1);
    }
    match io.join() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("qxide lsp: {e}");
            ExitCode::from(1)
        }
    }
}

pub fn capabilities() -> Value {
    let mut caps = json!({
        "positionEncoding": "utf-16",
        "textDocumentSync": { "openClose": true, "change": 1, "save": false },
        "hoverProvider": true,
        "definitionProvider": true,
        "documentSymbolProvider": true,
        "documentFormattingProvider": true,
        "completionProvider": { "triggerCharacters": ["."] },
        "experimental": { "qxideLanguageLevel": LANGUAGE_LEVEL },
    });
    // Quick fixes come with the diagnostics of `--experimental value-semantics` (and
    // `QXIDE_EXPERIMENTAL`); without it the server is unchanged.
    if lang::features::value_semantics() {
        caps["codeActionProvider"] = json!({ "codeActionKinds": ["quickfix"] });
    }
    caps
}

/// The protocol loop: handshake, then requests and notifications until `shutdown` and `exit`.
pub fn serve(conn: &Connection) -> Result<(), String> {
    let (id, _params) = conn.initialize_start().map_err(|e| e.to_string())?;
    let result = json!({
        "capabilities": capabilities(),
        "serverInfo": { "name": "qxide", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.initialize_finish(id, result)
        .map_err(|e| e.to_string())?;
    let mut server = Server::default();
    for msg in &conn.receiver {
        let out = match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req).map_err(|e| e.to_string())? {
                    return Ok(());
                }
                vec![Message::Response(server.request(req))]
            }
            Message::Notification(n) if n.method == "exit" => return Ok(()),
            // Edits re-analyse every open document; a front-end panic there must not end the server.
            Message::Notification(n) => {
                match catch_unwind(AssertUnwindSafe(|| server.notification(n))) {
                    Ok(out) => out.into_iter().map(Message::Notification).collect(),
                    Err(_) => {
                        server.cache.clear();
                        Vec::new()
                    }
                }
            }
            Message::Response(_) => Vec::new(),
        };
        for m in out {
            conn.sender.send(m).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn range(r: Range) -> Value {
    let pos = |p: Pos| json!({ "line": p.line, "character": p.character });
    json!({ "start": pos(r.start), "end": pos(r.end) })
}

fn range_of(v: &Value) -> Option<Range> {
    let pos = |p: &Value| {
        Some(Pos {
            line: u32::try_from(p.get("line")?.as_u64()?).ok()?,
            character: u32::try_from(p.get("character")?.as_u64()?).ok()?,
        })
    };
    Some(Range {
        start: pos(v.get("start")?)?,
        end: pos(v.get("end")?)?,
    })
}

fn param_str<'a>(params: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(params, |v, k| v.get(k))?.as_str()
}

fn position(params: &Value) -> Option<Pos> {
    let p = params.get("position")?;
    Some(Pos {
        line: u32::try_from(p.get("line")?.as_u64()?).ok()?,
        character: u32::try_from(p.get("character")?.as_u64()?).ok()?,
    })
}

fn symbol(s: &Symbol) -> Value {
    json!({
        "name": s.name,
        "detail": s.detail,
        "kind": match s.kind { SymbolKind::Function => 12, SymbolKind::Variable => 13 },
        "range": range(s.range),
        "selectionRange": range(s.selection),
        "children": s.children.iter().map(symbol).collect::<Vec<_>>(),
    })
}

fn item_kind(k: ItemKind) -> u32 {
    match k {
        ItemKind::Method => 2,
        ItemKind::Function => 3,
        ItemKind::Variable => 6,
        ItemKind::Type => 7,
        ItemKind::Keyword => 14,
        ItemKind::Constant => 21,
    }
}

impl Server {
    fn file_uri(&self, doc_uri: &str, analysis_name: &str, file: &str) -> String {
        if file == analysis_name {
            doc_uri.to_string()
        } else {
            uri::from_path(Path::new(file))
        }
    }

    // Imported modules are read from open documents first, so unsaved edits count.
    fn read(&self, path: &Path) -> std::io::Result<String> {
        let key = uri::path_key(path);
        let open = self
            .docs
            .values()
            .find(|d| d.path.as_deref().is_some_and(|p| uri::path_key(p) == key));
        match open {
            Some(d) => Ok(d.text.clone()),
            None => std::fs::read_to_string(path),
        }
    }

    fn analyzed(&mut self, uri: &str) -> Option<&Analyzed> {
        if !self.cache.contains_key(uri) {
            let doc = self.docs.get(uri)?;
            let name = doc
                .path
                .as_ref()
                .map_or_else(|| uri.to_string(), |p| p.display().to_string());
            let (project, project_error) = match doc.path.as_deref().map(pkg::project::for_entry) {
                Some(Ok(p)) => (p, None),
                Some(Err(e)) => (None, Some(e)),
                None => (None, None),
            };
            let read = |p: &Path| self.read(p);
            // The standard modules `qxide check` allows: an app's `[app] permissions`, else the
            // CLI's without `--allow`.
            let perms = doc
                .path
                .as_deref()
                .and_then(crate::app::permissions_for)
                .unwrap_or_else(|| lang::stdlib::Permissions::cli(false));
            let input = Input {
                name: &name,
                src: &doc.text,
                project: project.as_ref(),
                read: &read,
                perms,
            };
            let analysis = ide::analyze(&input);
            self.cache.insert(
                uri.to_string(),
                Analyzed {
                    analysis,
                    project_error,
                },
            );
        }
        self.cache.get(uri)
    }

    fn diagnostic(&self, uri: &str, name: &str, d: &Diag) -> Value {
        let related: Vec<Value> = d
            .related
            .iter()
            .map(|r| {
                json!({
                    "location": {
                        "uri": self.file_uri(uri, name, &r.location.file),
                        "range": range(r.location.range),
                    },
                    "message": r.message,
                })
            })
            .collect();
        json!({
            "range": range(d.range),
            // LSP severities: 1 error, 2 warning.
            "severity": if d.warning { 2 } else { 1 },
            "code": d.code,
            "source": "qxide",
            "message": d.message,
            "relatedInformation": related,
        })
    }

    /// `textDocument/publishDiagnostics` for an open document.
    fn publish(&mut self, uri: &str) -> Option<Notification> {
        let version = self.docs.get(uri)?.version;
        let name = self
            .docs
            .get(uri)?
            .path
            .as_ref()
            .map_or_else(|| uri.to_string(), |p| p.display().to_string());
        let analyzed = self.analyzed(uri)?;
        let diagnostics: Vec<Value> = match &analyzed.project_error {
            Some(e) => {
                let mut message = e.message.clone();
                for n in &e.notes {
                    message.push_str("\nnote: ");
                    message.push_str(n);
                }
                if let Some(h) = &e.help {
                    message.push_str("\nhelp: ");
                    message.push_str(h);
                }
                let zero = Pos {
                    line: 0,
                    character: 0,
                };
                vec![json!({
                    "range": range(Range { start: zero, end: zero }),
                    "severity": 1,
                    "code": e.code.as_str(),
                    "source": "qxide",
                    "message": message,
                })]
            }
            None => {
                let diags = analyzed.analysis.diagnostics();
                diags
                    .iter()
                    .map(|d| self.diagnostic(uri, &name, d))
                    .collect()
            }
        };
        let mut params = json!({ "uri": uri, "diagnostics": diagnostics });
        if let Some(v) = version {
            params["version"] = json!(v);
        }
        Some(Notification::new(
            "textDocument/publishDiagnostics".into(),
            params,
        ))
    }

    fn publish_all(&mut self) -> Vec<Notification> {
        self.cache.clear();
        let mut uris: Vec<String> = self.docs.keys().cloned().collect();
        uris.sort();
        uris.iter().filter_map(|u| self.publish(u)).collect()
    }

    pub fn notification(&mut self, n: Notification) -> Vec<Notification> {
        let p = &n.params;
        match n.method.as_str() {
            "textDocument/didOpen" => {
                let (Some(uri), Some(text)) = (
                    param_str(p, &["textDocument", "uri"]),
                    param_str(p, &["textDocument", "text"]),
                ) else {
                    return Vec::new();
                };
                let version = p["textDocument"]["version"].as_i64();
                self.docs.insert(
                    uri.to_string(),
                    Doc {
                        text: text.to_string(),
                        version,
                        path: uri::to_path(uri),
                        symbols: json!([]),
                    },
                );
                self.publish_all()
            }
            "textDocument/didChange" => {
                let Some(uri) = param_str(p, &["textDocument", "uri"]) else {
                    return Vec::new();
                };
                // Full sync: the last change holds the whole text.
                let text = p["contentChanges"]
                    .as_array()
                    .and_then(|c| c.last())
                    .and_then(|c| c["text"].as_str());
                let version = p["textDocument"]["version"].as_i64();
                match (self.docs.get_mut(uri), text) {
                    (Some(doc), Some(text)) => {
                        doc.text = text.to_string();
                        doc.version = version;
                    }
                    _ => return Vec::new(),
                }
                self.publish_all()
            }
            "textDocument/didClose" => {
                let Some(uri) = param_str(p, &["textDocument", "uri"]) else {
                    return Vec::new();
                };
                self.docs.remove(uri);
                let cleared = Notification::new(
                    "textDocument/publishDiagnostics".into(),
                    json!({ "uri": uri, "diagnostics": [] }),
                );
                let mut out = vec![cleared];
                out.extend(self.publish_all());
                out
            }
            _ => Vec::new(),
        }
    }

    pub fn request(&mut self, req: Request) -> Response {
        let id = req.id.clone();
        let method = req.method.clone();
        // A panic in the front end fails this request only; the server keeps running.
        match catch_unwind(AssertUnwindSafe(|| self.handle(&req.method, &req.params))) {
            Ok(Ok(v)) => Response::new_ok(id, v),
            Ok(Err((code, message))) => Response::new_err(id, code as i32, message),
            Err(_) => {
                self.cache.clear();
                Response::new_err(
                    id,
                    ErrorCode::InternalError as i32,
                    format!("internal error while handling {method}"),
                )
            }
        }
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, (ErrorCode, String)> {
        let invalid = || {
            (
                ErrorCode::InvalidParams,
                format!("invalid parameters for {method}"),
            )
        };
        let uri = param_str(params, &["textDocument", "uri"]).map(str::to_string);
        let known = matches!(
            method,
            "textDocument/hover"
                | "textDocument/definition"
                | "textDocument/documentSymbol"
                | "textDocument/completion"
                | "textDocument/formatting"
        ) || (method == "textDocument/codeAction" && lang::features::value_semantics());
        if !known {
            return Err((
                ErrorCode::MethodNotFound,
                format!("unsupported method: {method}"),
            ));
        }
        let uri = uri
            .filter(|u| self.docs.contains_key(u))
            .ok_or_else(invalid)?;
        let doc_name = self.docs[&uri]
            .path
            .as_ref()
            .map_or_else(|| uri.clone(), |p| p.display().to_string());
        if method == "textDocument/formatting" {
            let doc = &self.docs[&uri];
            return Ok(match lang::format(&doc_name, &doc.text) {
                Ok(out) if out == doc.text => json!([]),
                Ok(out) => {
                    let end = ide::position(&doc.text, doc.text.len());
                    let start = Pos {
                        line: 0,
                        character: 0,
                    };
                    json!([{ "range": range(Range { start, end }), "newText": out }])
                }
                // Sources that do not parse are not formatted; their diagnostics say why.
                Err(_) => Value::Null,
            });
        }
        if method == "textDocument/codeAction" {
            let want = params.get("range").and_then(range_of).ok_or_else(invalid)?;
            let a = &self.analyzed(&uri).ok_or_else(invalid)?.analysis;
            let mut actions = Vec::new();
            for d in a.diagnostics() {
                let at = (d.range.start.line, d.range.start.character);
                let end = (d.range.end.line, d.range.end.character);
                let (from, to) = (
                    (want.start.line, want.start.character),
                    (want.end.line, want.end.character),
                );
                if end < from || at > to {
                    continue;
                }
                let shown = self.diagnostic(&uri, &doc_name, &d);
                for (i, f) in d.fixes.iter().enumerate() {
                    let edits: Vec<Value> = f
                        .edits
                        .iter()
                        .map(|(r, t)| json!({ "range": range(*r), "newText": t }))
                        .collect();
                    actions.push(json!({
                        "title": f.title,
                        "kind": "quickfix",
                        "diagnostics": [shown.clone()],
                        "isPreferred": i == 0,
                        "edit": { "changes": { uri.clone(): edits } },
                    }));
                }
            }
            return Ok(Value::Array(actions));
        }
        if method == "textDocument/documentSymbol" {
            let symbols = self
                .analyzed(&uri)
                .ok_or_else(invalid)?
                .analysis
                .symbols()
                .map(|s| Value::Array(s.iter().map(symbol).collect()));
            let doc = self.docs.get_mut(&uri).ok_or_else(invalid)?;
            if let Some(s) = symbols {
                doc.symbols = s;
            }
            return Ok(doc.symbols.clone());
        }
        let pos = position(params).ok_or_else(invalid)?;
        let a = &self.analyzed(&uri).ok_or_else(invalid)?.analysis;
        let at = a.offset_of(pos);
        Ok(match method {
            "textDocument/hover" => match a.hover(at) {
                Some(md) => json!({ "contents": { "kind": "markdown", "value": md } }),
                None => Value::Null,
            },
            "textDocument/definition" => match a.definition(at) {
                Some(loc) => {
                    let target = if loc.file == doc_name {
                        uri.clone()
                    } else {
                        uri::from_path(Path::new(&loc.file))
                    };
                    json!({ "uri": target, "range": range(loc.range) })
                }
                None => Value::Null,
            },
            _ => {
                let items: Vec<Value> = a
                    .completions(at)
                    .into_iter()
                    .map(|i| {
                        let mut v = json!({ "label": i.label, "kind": item_kind(i.kind) });
                        if let Some(d) = i.detail {
                            v["detail"] = json!(d);
                        }
                        v
                    })
                    .collect();
                json!(items)
            }
        })
    }
}
