// Talks to a real `qxide lsp` through vscode-jsonrpc, the transport vscode-languageclient uses.
// The binary comes from QXIDE_BIN or the repository's target/debug build; skipped if neither exists.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as rpc from "vscode-jsonrpc/node";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.platform === "win32" ? "qxide.exe" : "qxide";
const bin = process.env.QXIDE_BIN ?? join(here, "..", "..", "..", "target", "debug", exe);

test("initialize, diagnostics, hover and shutdown over vscode-jsonrpc", { skip: !existsSync(bin) && `no ${bin}` }, async () => {
  const proc = spawn(bin, ["lsp", "--stdio"], { stdio: ["pipe", "pipe", "inherit"] });
  const conn = rpc.createMessageConnection(
    new rpc.StreamMessageReader(proc.stdout),
    new rpc.StreamMessageWriter(proc.stdin),
  );
  const diagnostics = new Promise((resolve) => {
    conn.onNotification("textDocument/publishDiagnostics", (p) => resolve(p));
  });
  conn.listen();
  const init = await conn.sendRequest("initialize", { processId: process.pid, rootUri: null, capabilities: {} });
  assert.equal(init.serverInfo.name, "qxide");
  assert.equal(init.capabilities.hoverProvider, true);
  await conn.sendNotification("initialized", {});

  const uri = "untitled:Untitled-1";
  const text = "void main() {\n    Qubit q = qubit();\n}\n";
  await conn.sendNotification("textDocument/didOpen", {
    textDocument: { uri, languageId: "qx", version: 1, text },
  });
  const published = await diagnostics;
  assert.equal(published.uri, uri);
  assert.equal(published.diagnostics[0].code, "Q0302");
  assert.deepEqual(published.diagnostics[0].range.start, { line: 1, character: 10 });

  const hover = await conn.sendRequest("textDocument/hover", {
    textDocument: { uri },
    position: { line: 1, character: 10 },
  });
  assert.match(hover.contents.value, /## Q0302: /);

  await conn.sendRequest("shutdown");
  const exited = new Promise((resolve) => proc.on("exit", resolve));
  await conn.sendNotification("exit");
  assert.equal(await exited, 0);
  conn.dispose();
});
