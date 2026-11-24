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
    "void main() { boolean b = !true; while (b) {} helper(); m.asInt(); }",
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
  assert.equal(scopeOf("<T> T id(T x) {}", "T"), "entity.name.type.parameter.qx");
  assert.equal(scopeOf("public <A, B> int f(A a, B b) {}", "public"), "storage.modifier.qx");
  assert.equal(scopeOf("public <A, B> int f(A a, B b) {}", "B"), "entity.name.type.parameter.qx");
  assert.equal(scopeOf("struct Pair<A, B> { A a; }", "struct"), "storage.type.struct.qx");
  assert.equal(scopeOf("enum Option<T> { None }", "Option"), "entity.name.type.qx");
  assert.equal(scopeOf("enum Option<T> { None }", "T"), "entity.name.type.parameter.qx");
  assert.equal(scopeOf("boolean c = a < b;", "<"), scopeOf("boolean c = a > b;", ">"));
  assert.equal(scopeOf("match e { A => {} }", "match"), "keyword.control.qx");
  assert.equal(scopeOf("match e { A => {} }", "=>"), "keyword.operator.arrow.qx");
  const tried = "Result<int, String> f(String s) { int n = s.tryParseInt()?; return Result.Ok(n); }";
  assert.equal(scopeOf(tried, "Result"), "support.type.prelude.qx");
  assert.equal(scopeOf(tried, "tryParseInt"), "support.function.method.qx");
  assert.equal(scopeOf(tried, "?"), "keyword.operator.try.qx");
  assert.equal(scopeOf(tried, "Ok"), "support.constant.variant.qx");
  assert.equal(scopeOf("Option<int> o = Option.None;", "None"), "support.constant.variant.qx");
  const shapes = "interface Shape { double area(); }\nstruct Sq implements Shape { double s; double area() { return this.s; } }\n<T extends Shape> double sum(T x) { return x.area(); }";
  assert.equal(scopeOf(shapes, "interface"), "storage.type.interface.qx");
  assert.equal(scopeOf(shapes, "implements"), "keyword.other.implements.qx");
  assert.equal(scopeOf(shapes, "extends"), "keyword.other.extends.qx");
  assert.equal(scopeOf(shapes, "this"), "variable.language.this.qx");
  assert.equal(scopeOf(shapes, "T"), "entity.name.type.parameter.qx");
  assert.equal(scopeOf("interface Ordered<T> { int compare(T o); }", "Ordered"), "entity.name.type.qx");
  assert.equal(scopeOf("int interface = 1;", " interface "), "source.qx");
  assert.equal(scopeOf('extern "m" double cos(double x);', "extern"), "storage.modifier.extern.qx");
  assert.equal(scopeOf("int extern = 1;", " extern "), "source.qx");
  const bits = "int f(int x) { x <<= 2; return x >>> 1 ^ ~x & 3 | x >> 2; }";
  assert.equal(scopeOf(bits, "<<="), "keyword.operator.assignment.compound.qx");
  assert.equal(scopeOf(bits, ">>>"), "keyword.operator.bitwise.shift.qx");
  assert.equal(scopeOf(bits, "^"), "keyword.operator.bitwise.qx");
  assert.equal(scopeOf(bits, "~"), "keyword.operator.bitwise.qx");
  assert.equal(scopeOf(bits, ">>"), "keyword.operator.bitwise.shift.qx");
  const coll = "Map<String, int> m = new Map(); List<int> xs = new List(); Random r = new Random(1); m.put(\"a\", r.nextInt(6)); for w in \"a b\".split(\" \") {}";
  assert.equal(scopeOf(coll, "Map"), "support.type.builtin.qx");
  assert.equal(scopeOf(coll, "List"), "support.type.builtin.qx");
  assert.equal(scopeOf(coll, "Random"), "support.type.builtin.qx");
  const js = "Json v = json.ofInt(1); String t = json.stringify(v);";
  assert.equal(scopeOf(js, "Json"), "support.type.builtin.qx");
  assert.equal(scopeOf("String s = v.asString();", "asString"), "support.function.method.qx");
  assert.equal(scopeOf(coll, "put"), "support.function.method.qx");
  assert.equal(scopeOf(coll, "nextInt"), "support.function.method.qx");
  assert.equal(scopeOf(coll, "split"), "support.function.method.qx");
  // Nested type arguments close with two `>`, not a shift.
  assert.equal(scopeOf("Option<Option<int>> o;", ">"), scopeOf("Option<int> o;", ">"));
  assert.equal(scopeOf(src, "helper"), "entity.name.function.call.qx");
  assert.equal(scopeOf(src, "asInt"), "support.function.method.qx");
});

test("strings with escapes and placeholders", () => {
  const src = 'println("a={} \\n \\q", x);';
  assert.equal(scopeOf(src, "{}"), "constant.other.placeholder.qx");
  assert.equal(scopeOf(src, "\\n"), "constant.character.escape.qx");
  assert.equal(scopeOf(src, "\\q"), "invalid.illegal.unknown-escape.qx");
  assert.equal(scopeOf(src, " x)"), "source.qx");
  const esc = 'io.write("\\r\\u001B[K");';
  assert.equal(scopeOf(esc, "\\r"), "constant.character.escape.qx");
  assert.equal(scopeOf(esc, "\\u001B"), "constant.character.escape.qx");
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
