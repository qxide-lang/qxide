// The page side of the qxide-app IPC (docs/design/qx-app.md §5): `qxide.invoke`, `qxide.commands`,
// `qxide.on`. Injected before any page script; talks to the runtime through `window.ipc`.
(() => {
  if (window.qxide) return;
  const ipc = window.ipc;
  const pending = new Map();
  const listeners = new Map();
  let next = 1;

  class QxError extends Error {
    constructor(e) {
      super(e.message);
      this.name = "QxError";
      this.code = e.code;
      this.diagnostics = e.diagnostics ?? [];
      this.rendered = e.rendered ?? e.message;
    }
  }

  // JSON cannot carry NaN, infinities or integers beyond 2^53; they travel as strings.
  const toWire = (v) => {
    if (typeof v === "bigint") return v.toString();
    if (typeof v === "number" && !Number.isFinite(v)) {
      return Number.isNaN(v) ? "NaN" : v > 0 ? "Infinity" : "-Infinity";
    }
    return v;
  };
  const fromWire = (type, v) => {
    if (typeof v !== "string") return v;
    if (type === "double") return Number(v);
    if (type === "int") return BigInt(v);
    return v;
  };

  const send = (msg) =>
    new Promise((resolve, reject) => {
      const id = next++;
      pending.set(id, { resolve, reject });
      ipc.postMessage(JSON.stringify({ ...msg, id }));
      return id;
    });

  window.__qxide_settle = (id, ok, payload) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (!ok) {
      p.reject(new QxError(payload));
      return;
    }
    if (payload && typeof payload === "object" && "returns" in payload) {
      payload.value = fromWire(payload.returns, payload.value);
    }
    p.resolve(payload);
  };

  window.__qxide_emit = (event, payload) => {
    for (const cb of listeners.get(event) ?? []) {
      try {
        cb(payload);
      } catch (e) {
        console.error(e);
      }
    }
  };

  const invoke = (command, args = {}, options = {}) => {
    const wireArgs = {};
    for (const [k, v] of Object.entries(args ?? {})) wireArgs[k] = toWire(v);
    const id = next;
    const msg = {
      kind: "invoke",
      command: String(command),
      args: wireArgs,
      shots: options.shots ?? 1,
      seed: options.seed === undefined || options.seed === null ? null : String(options.seed),
    };
    const signal = options.signal;
    const promise = send(msg);
    if (signal) {
      // The runtime answers a cancelled call with error Q0713.
      const abort = () => ipc.postMessage(JSON.stringify({ kind: "cancel", id: 0, target: id }));
      if (signal.aborted) abort();
      else {
        signal.addEventListener("abort", abort, { once: true });
        promise.finally(() => signal.removeEventListener("abort", abort)).catch(() => {});
      }
    }
    return promise;
  };

  window.qxide = Object.freeze({
    invoke,
    commands: () => send({ kind: "commands" }),
    on(event, callback) {
      const set = listeners.get(event) ?? new Set();
      set.add(callback);
      listeners.set(event, set);
      return () => set.delete(callback);
    },
    QxError,
  });
})();
