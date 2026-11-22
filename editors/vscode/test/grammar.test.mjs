// Tokenizes .qx sources with the extension's TextMate grammar (the engine VS Code uses).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vsctm from "vscode-textmate";
import oniguruma from "vscode-oniguruma";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const repo = join(here, "..", "..", "..");

const wasm = readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm"));
await oniguruma.loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));

const registry = new vsctm.Registry({
  onigLib: Promise.resolve({
    createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
    createOnigString: (s) => new oniguruma.OnigString(s),
  }),
  loadGrammar: async (scope) => {
    if (scope !== "source.qx") return null;
    const path = join(here, "..", "syntaxes", "qx.tmLanguage.json");
    return vsctm.parseRawGrammar(readFileSync(path, "utf8"), path);
  },
});
const grammar = await registry.loadGrammar("source.qx");

// [text, innermost scope] for every token of `src`.
function tokens(src) {
  let state = vsctm.INITIAL;
  const out = [];
  for (const line of src.split(/\r?\n/)) {
    const r = grammar.tokenizeLine(line, state);
    for (const t of r.tokens) {
      out.push([line.slice(t.startIndex, t.endIndex), t.scopes[t.scopes.length - 1]]);
    }
    state = r.ruleStack;
  }
  return out;
}

function scopeOf(src, text) {
  const found = tokens(src).find(([t]) => t === text);
  assert.ok(found, `no token ${JSON.stringify(text)} in ${JSON.stringify(tokens(src))}`);
  return found[1];
}

test("declarations, keywords, types and calls", () => {
  const src = [
    "import bell_utils.gates.prepare;",
    "public Bit flip(consume Qubit q) {",
    "    final var n = 1_000;",
    "    for i in 0..n { rx(q, PI / 2.5e1); }",
    "    return measure(q); // done",
    "}",
    "void main() { boolean b = !true; while (b) {} helper(); m.as_int(); }",
  ].join("\n");
  assert.equal(scopeOf(src, "import"), "keyword.control.import.qx");
  assert.equal(scopeOf(src, "bell_utils"), "entity.name.namespace.qx");
  assert.equal(scopeOf(src, "prepare"), "entity.name.function.qx");
  assert.equal(scopeOf(src, "public"), "storage.modifier.qx");
  assert.equal(scopeOf(src, "Bit"), "storage.type.qx");
  assert.equal(scopeOf(src, "flip"), "entity.name.function.qx");
  assert.equal(scopeOf(src, "consume"), "storage.modifier.qx");
  assert.equal(scopeOf(src, "Qubit"), "storage.type.quantum.qx");
  assert.equal(scopeOf(src, "final"), "storage.modifier.qx");
  assert.equal(scopeOf(src, "1_000"), "constant.numeric.integer.qx");
  assert.equal(scopeOf(src, "for"), "keyword.control.qx");
  assert.equal(scopeOf(src, ".."), "keyword.operator.range.qx");
  assert.equal(scopeOf(src, "rx"), "support.function.builtin.qx");
  assert.equal(scopeOf(src, "PI"), "constant.language.pi.qx");
  assert.equal(scopeOf(src, "2.5e1"), "constant.numeric.float.qx");
  assert.equal(scopeOf(src, "measure"), "support.function.builtin.qx");
  assert.equal(scopeOf(src, " done"), "comment.line.double-slash.qx");
  assert.equal(scopeOf(src, "boolean"), "storage.type.primitive.qx");
  assert.equal(scopeOf(src, "true"), "constant.language.boolean.qx");
  assert.equal(scopeOf(src, "while"), "keyword.control.qx");
  assert.equal(scopeOf("int[] a = new int[3];", "new"), "keyword.other.new.qx");
  assert.equal(scopeOf("int new = 1;", " new "), "source.qx");
  assert.equal(scopeOf("struct P { int x; }", "struct"), "storage.type.struct.qx");
  assert.equal(scopeOf("enum E { A, B }", "enum"), "storage.type.enum.qx");
  assert.equal(scopeOf("match e { A => {} }", "match"), "keyword.control.qx");
  assert.equal(scopeOf("match e { A => {} }", "=>"), "keyword.operator.arrow.qx");
  assert.equal(scopeOf(src, "helper"), "entity.name.function.call.qx");
  assert.equal(scopeOf(src, "as_int"), "support.function.method.qx");
});

test("strings with escapes and placeholders", () => {
  const src = 'println("a={} \\n \\q", x);';
  assert.equal(scopeOf(src, "{}"), "constant.other.placeholder.qx");
  assert.equal(scopeOf(src, "\\n"), "constant.character.escape.qx");
  assert.equal(scopeOf(src, "\\q"), "invalid.illegal.unknown-escape.qx");
  assert.equal(scopeOf(src, " x)"), "source.qx");
});

test("every example program tokenizes and its comments stay comments", () => {
  const dir = join(repo, "examples", "qx");
  const files = readdirSync(dir).filter((f) => f.endsWith(".qx"));
  assert.ok(files.length > 3);
  for (const f of files) {
    const src = readFileSync(join(dir, f), "utf8");
    const toks = tokens(src);
    for (const line of src.split(/\r?\n/)) {
      const at = line.indexOf("//");
      if (at >= 0 && !line.slice(0, at).includes('"')) {
        const comment = line.slice(at + 2);
        if (comment.trim() !== "") {
          assert.ok(
            toks.some(([t, s]) => t === comment && s === "comment.line.double-slash.qx"),
            `${f}: ${line}`,
          );
        }
      }
    }
  }
});
