// Runs the qxide wasm module off the main thread. One request at a time; the page terminates the
// worker to stop a run or when it exceeds the time limit.
import wasmUrl from './generated/qxide.wasm?url';
import {execute} from './engine';
import type {TrapReason, WorkerRequest, WorkerResponse} from './protocol';

// Typed by hand: the webworker lib would clash with the DOM lib the rest of the site uses.
const scope = self as unknown as {
  postMessage(message: WorkerResponse): void;
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
};
let compiled: Promise<WebAssembly.Module> | null = null;

function load(): Promise<WebAssembly.Module> {
  // compileStreaming needs the application/wasm MIME type; fall back to a plain fetch otherwise.
  compiled ??= WebAssembly.compileStreaming(fetch(wasmUrl)).catch(async () => {
    const res = await fetch(wasmUrl);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return WebAssembly.compile(await res.arrayBuffer());
  });
  compiled.catch(() => {
    compiled = null;
  });
  return compiled;
}

function classify(err: unknown): TrapReason {
  const text = String(err);
  if (err instanceof RangeError && /call stack/i.test(text)) return 'stack';
  if (/memory|allocation|out of bounds/i.test(text)) return 'memory';
  return 'crash';
}

scope.onmessage = async (event) => {
  const {id, cmd, name, src, shots, seed} = event.data;
  let module: WebAssembly.Module;
  try {
    module = await load();
  } catch (err) {
    scope.postMessage({id, kind: 'load-error', detail: String(err)});
    return;
  }
  const start = performance.now();
  try {
    const outcome = execute(module, cmd, name, src, BigInt(shots), BigInt(seed));
    scope.postMessage({id, kind: 'done', outcome, ms: performance.now() - start});
  } catch (err) {
    scope.postMessage({id, kind: 'trap', reason: classify(err), detail: String(err)});
  }
};
