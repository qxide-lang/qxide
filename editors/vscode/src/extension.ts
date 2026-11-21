// Starts `qxide lsp` (path from the `qxide.path` setting) for `.qx` files and restarts it when
// the setting changes or on the `qxide.restartServer` command.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

// The configured executable; `~` expands to the home directory, other values are used as-is
// (a bare name is looked up on PATH by the operating system).
export function serverCommand(configured: string | undefined, home: string): string {
  const value = (configured ?? "").trim();
  if (value === "") {
    return "qxide";
  }
  if (value === "~" || value.startsWith("~/") || value.startsWith("~\\")) {
    return home + value.slice(1);
  }
  return value;
}

function createClient(): LanguageClient {
  const config = vscode.workspace.getConfiguration("qxide");
  const command = serverCommand(config.get<string>("path"), process.env.HOME ?? process.env.USERPROFILE ?? "");
  const run = { command, args: ["lsp"] };
  const serverOptions: ServerOptions = { run, debug: run };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "qx" },
      { scheme: "untitled", language: "qx" },
    ],
  };
  return new LanguageClient("qxide", "Qxide", serverOptions, clientOptions);
}

async function start(): Promise<void> {
  client = createClient();
  try {
    await client.start();
  } catch (err) {
    const command = vscode.workspace.getConfiguration("qxide").get<string>("path") ?? "qxide";
    const detail = err instanceof Error ? err.message : String(err);
    const message = "Failed to start the Qxide language server ({command}). Install qxide or configure qxide.path in Settings. Details: {detail}";
    void vscode.window.showErrorMessage(
      message.replace("{command}", command).replace("{detail}", detail),
    );
  }
}

async function stop(): Promise<void> {
  const c = client;
  client = undefined;
  // stop() throws unless the client is running (for example after a failed start).
  if (c !== undefined && c.needsStop()) {
    try {
      await c.stop();
    } catch {
      // Already stopped or never started: nothing left to release.
    }
  }
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("qxide.restartServer", async () => {
      await stop();
      await start();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("qxide.path")) {
        await stop();
        await start();
      }
    }),
  );
  await start();
}

export function deactivate(): Promise<void> {
  return stop();
}
