// Qxide Studio: a .qx editor whose checks and runs go through the app's .qx commands
// (src/app.qx), which call the compiler through `std.qxide`.
import { T } from "./strings.js";

const EXAMPLES = ["bell.qx", "ghz.qx", "teleport.qx", "functions.qx", "rotation.qx", "teleport_fn.qx", "conditional_fn.qx", "loops.qx", "text.qx", "arrays.qx", "structs.qx"];
// Same token rules as the playground highlighter (site/src/components/CodeBlock.tsx), plus
// `import` and `public`.
const KEYWORDS = new Set([
  "void", "int", "double", "boolean", "final", "var", "consume", "for", "in", "if", "else", "return",
  "true", "false", "import", "public",
]);
const INDENT = "    ";

const $ = (id) => document.getElementById(id);
const src = $("source");
let errorLines = new Set();
let current = null; // AbortController of the running call
let saveTimer = 0;

// ---- text ----
for (const el of document.querySelectorAll("[data-t]")) el.textContent = T[el.dataset.t];
for (const el of document.querySelectorAll("[data-t-label]")) el.setAttribute("aria-label", T[el.dataset.tLabel]);
const fill = (s, vars) => s.replace(/\{(\w+)\}/g, (_, k) => String(vars[k]));
const status = (text) => ($("status").textContent = text);

// ---- editor ----
function highlight(line) {
  const frag = document.createDocumentFragment();
  const re = /(\/\/.*)|("(?:[^"\\]|\\.)*")|\b([A-Za-z_][A-Za-z0-9_]*)\b/g;
  let last = 0;
  for (const m of line.matchAll(re)) {
    if (m.index > last) frag.append(line.slice(last, m.index));
    const cls = m[1] ? "tk-cm" : m[2] ? "tk-str" : KEYWORDS.has(m[3]) ? "tk-kw" : /^[A-Z]/.test(m[3]) ? "tk-ty" : "";
    if (cls) {
      const s = document.createElement("span");
      s.className = cls;
      s.textContent = m[0];
      frag.append(s);
    } else frag.append(m[0]);
    last = m.index + m[0].length;
  }
  if (last < line.length) frag.append(line.slice(last));
  return frag;
}

function render() {
  const lines = src.value.split("\n");
  const gutter = $("gutter");
  const hl = $("hl");
  gutter.replaceChildren();
  hl.replaceChildren();
  lines.forEach((line, i) => {
    const err = errorLines.has(i + 1);
    const n = document.createElement("span");
    n.className = err ? "ln ln-err" : "ln";
    n.textContent = String(i + 1);
    gutter.append(n);
    const l = document.createElement("span");
    l.className = err ? "line line-err" : "line";
    l.append(highlight(line));
    hl.append(l);
  });
}

function insert(text) {
  // execCommand keeps the browser's undo stack; fall back to a plain edit.
  if (document.execCommand?.("insertText", false, text)) return;
  const { selectionStart: s, selectionEnd: e, value } = src;
  src.value = value.slice(0, s) + text + value.slice(e);
  src.setSelectionRange(s + text.length, s + text.length);
  edited();
}

src.addEventListener("keydown", (e) => {
  if (e.isComposing || e.keyCode === 229) return;
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    run();
  } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    save(true);
  } else if (e.key === "Tab" && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    insert(INDENT);
  } else if (e.key === "Enter" && !e.shiftKey && !e.altKey) {
    e.preventDefault();
    const start = src.value.lastIndexOf("\n", src.selectionStart - 1) + 1;
    const before = src.value.slice(start, src.selectionStart);
    const lead = /^[ \t]*/.exec(before)[0];
    insert(`\n${lead}${before.trimEnd().endsWith("{") ? INDENT : ""}`);
  }
});

function edited() {
  if (errorLines.size) errorLines = new Set();
  render();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, 800);
}
src.addEventListener("input", edited);

function setSource(text) {
  src.value = text.replace(/\r\n?/g, "\n");
  errorLines = new Set();
  render();
}

// Autosave is quiet; Ctrl+S confirms in the status bar.
async function save(explicit = false) {
  clearTimeout(saveTimer);
  try {
    await qxide.invoke("save", { source: src.value });
    if (explicit === true) status(T.saved);
  } catch (e) {
    status(e.rendered ?? String(e));
  }
}

// ---- panels ----
function show(tab) {
  for (const t of document.querySelectorAll('[role="tab"]')) {
    const on = t.id === `tab-${tab}`;
    t.setAttribute("aria-selected", String(on));
    $(t.getAttribute("aria-controls")).hidden = !on;
  }
}
for (const t of document.querySelectorAll('[role="tab"]')) t.addEventListener("click", () => show(t.id.slice(4)));

function problems(r) {
  const list = $("panel-problems");
  list.replaceChildren();
  const diags = (r?.diagnostics ?? []).filter((d) => d.file === "main.qx");
  errorLines = new Set(diags.map((d) => d.line));
  render();
  if (!diags.length) {
    const li = document.createElement("li");
    li.textContent = T.noProblems;
    list.append(li);
    return 0;
  }
  for (const d of diags) {
    const li = document.createElement("li");
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = `${T.line} ${d.line}:${d.column}  ${d.code}  ${d.message}`;
    b.addEventListener("click", () => jump(d.line, d.column));
    li.append(b);
    list.append(li);
  }
  return diags.length;
}

function jump(line, column) {
  const lines = src.value.split("\n");
  let pos = 0;
  for (let i = 0; i < line - 1 && i < lines.length; i++) pos += lines[i].length + 1;
  pos += Math.max(0, column - 1);
  src.focus();
  src.setSelectionRange(pos, pos);
}

function failed(r) {
  const n = problems(r);
  $("panel-output").textContent = r.rendered ?? "";
  show(n ? "problems" : "output");
  status(fill(T.errors, { count: Math.max(n, 1) }));
}

function chart(counts, shots) {
  const panel = $("panel-chart");
  panel.replaceChildren();
  const note = (text) => {
    const p = document.createElement("p");
    p.className = "note";
    p.textContent = text;
    panel.append(p);
  };
  if (shots === 1) return note(T.oneShot);
  if (!counts.length) return note(T.noMeasure);
  const cap = document.createElement("p");
  cap.className = "note";
  cap.textContent = fill(T.chartCaption, { shots });
  panel.append(cap);
  const max = Math.max(...counts.map((c) => c.count));
  for (const c of counts) {
    const row = document.createElement("div");
    row.className = "row";
    const key = document.createElement("span");
    key.className = "key";
    key.textContent = c.bits.join("");
    const bar = document.createElement("span");
    bar.className = "bar";
    bar.style.width = `${(100 * c.count) / max}%`;
    const n = document.createElement("span");
    n.className = "count";
    n.textContent = `${c.count}  ${((100 * c.count) / shots).toFixed(1)}%`;
    row.append(key, bar, n);
    panel.append(row);
  }
}

// ---- commands ----
const shots = () => Math.max(1, Math.min(1_000_000, Math.floor(Number($("shots").value) || 1)));
const seed = () => Math.max(0, Math.floor(Number($("seed").value) || 0));

async function call(command, args, options) {
  const r = await qxide.invoke(command, args, options);
  return JSON.parse(r.value);
}

function busy(on) {
  for (const id of ["run", "check", "format", "qasm"]) $(id).disabled = on;
  $("stop").disabled = !on;
}

async function run() {
  if (current) return;
  current = new AbortController();
  busy(true);
  status(T.running);
  const n = shots();
  const s = seed();
  try {
    const r = await call("run", { source: src.value, shots: n, seed: s }, { signal: current.signal });
    if (!r.ok) return failed(r);
    problems(null);
    const logs = r.logs.length ? `${r.logs.join("\n")}\n` : "";
    $("panel-output").textContent = logs + r.output;
    chart(r.counts, n);
    show(n > 1 && r.counts.length ? "chart" : "output");
    status(fill(T.ran, { shots: n, seed: s }));
  } catch (e) {
    status(e.code === "Q0713" ? T.stopped : e.rendered ?? String(e));
  } finally {
    current = null;
    busy(false);
  }
}

async function check() {
  try {
    const r = await call("check", { source: src.value });
    if (!r.ok) return failed(r);
    problems(null);
    status(T.noErrors);
  } catch (e) {
    status(e.rendered ?? String(e));
  }
}

async function format() {
  try {
    const r = await call("format", { source: src.value });
    if (!r.ok) return failed(r);
    setSource(r.text);
    status(T.formatted);
    save();
  } catch (e) {
    status(e.rendered ?? String(e));
  }
}

async function qasm() {
  try {
    const r = await call("qasm", { source: src.value });
    if (!r.ok) return failed(r);
    problems(null);
    $("panel-circuit").textContent = r.qasm;
    show("circuit");
  } catch (e) {
    status(e.rendered ?? String(e));
  }
}

async function openExample(name) {
  const res = await fetch(`examples/${name}`);
  setSource(await res.text());
  await qxide.invoke("set_file", { name });
  save();
}

$("run").addEventListener("click", run);
$("stop").addEventListener("click", () => current?.abort());
$("check").addEventListener("click", check);
$("format").addEventListener("click", format);
$("qasm").addEventListener("click", qasm);
const select = $("example");
for (const name of EXAMPLES) {
  const o = document.createElement("option");
  o.value = o.textContent = name;
  select.append(o);
}
select.addEventListener("change", () => openExample(select.value));

(async () => {
  render();
  try {
    const saved = (await qxide.invoke("load")).value;
    if (saved) {
      setSource(saved);
      await qxide.invoke("set_file", { name: "main.qx" });
      status(T.restored);
    } else {
      await openExample(EXAMPLES[0]);
      status(T.hint);
    }
  } catch (e) {
    status(e.rendered ?? String(e));
  }
})();
