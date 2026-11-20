// Runs every examples/qx program (plus error cases) through the built playground module and the
// release `qxide` binary, and fails unless stdout, stderr and the exit code are identical.
// Usage: npm run test:parity (builds the wasm module first).
import {spawnSync} from 'node:child_process';
import {mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {execute} from '../src/playground/engine.ts';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const root = resolve(site, '..');

const cargo = spawnSync('cargo', ['build', '--quiet', '--release', '-p', 'qxide', '--bin', 'qxide'], {
  cwd: root,
  stdio: 'inherit',
});
if (cargo.status !== 0) process.exit(1);
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'target');
const cli = join(targetDir, 'release', `qxide${process.platform === 'win32' ? '.exe' : ''}`);
const module = new WebAssembly.Module(readFileSync(join(site, 'src/playground/generated/qxide.wasm')));

function runCli(cmd, path, shots, seed) {
  const args = cmd === 'run' ? ['run', path, '--shots', String(shots), '--seed', String(seed)] : [cmd, path];
  const r = spawnSync(cli, args, {cwd: root, encoding: 'utf8'});
  return {exit: r.status, stdout: r.stdout, stderr: r.stderr};
}

function runWasm(cmd, path, shots, seed) {
  return execute(module, cmd, path, readFileSync(join(root, path), 'utf8'), BigInt(shots), BigInt(seed));
}

let checked = 0;
const failures = [];
function same(cmd, path, shots = 1, seed = 0) {
  const want = runCli(cmd, path, shots, seed);
  const got = runWasm(cmd, path, shots, seed);
  checked++;
  if (got.exit !== want.exit || got.stdout !== want.stdout || got.stderr !== want.stderr) {
    failures.push({cmd, path, shots, seed, want, got});
  }
  return got;
}

const examples = readdirSync(join(root, 'examples/qx'))
  .filter((f) => f.endsWith('.qx'))
  .sort()
  .map((f) => `examples/qx/${f}`);
const runs = [
  [1, 0],
  [1, 42],
  [1000, 0],
  [257, 7],
  [100, '18446744073709551615'],
];
for (const path of examples) {
  same('check', path);
  same('qasm', path);
  for (const [shots, seed] of runs) same('run', path, shots, seed);
}

const scratch = join(root, 'target/qxide-parity');
mkdirSync(scratch, {recursive: true});
const write = (name, src) => {
  writeFileSync(join(scratch, name), src);
  return `target/qxide-parity/${name}`;
};
const errorCases = {
  'unknown_var.qx': 'void main() {\n    int a = b;\n}\n',
  'parse.qx': 'void main() {\n    int a = ;\n}\n',
  'use_after_consume.qx': 'void main() {\n    Qubit q = qubit();\n    Bit m = measure(q);\n    h(q);\n}\n',
  'never_consumed.qx': 'void main() {\n    Qubit q = qubit();\n    h(q);\n}\n',
  'unicode.qx': 'void main() {\n    println("큐비트 {}", x);\n}\n',
  'bom_crlf.qx': '﻿void main() {\r\n    final int a = 1;\r\n    a = 2;\r\n}\r\n',
  'recursion.qx': 'int f(int n) {\n    return f(n + 1);\n}\n\nvoid main() {\n    println("{}", f(0));\n}\n',
  'div_zero.qx': 'void main() {\n    int z = 0;\n    println("{}", 1 / z);\n}\n',
  'deep.qx': `void main() {\n    int a = ${'('.repeat(300)}1${')'.repeat(300)};\n}\n`,
  'tab.qx': 'void main() {\n\tint a = c;\n}\n',
};
for (const [name, src] of Object.entries(errorCases)) {
  const path = write(name, src);
  for (const cmd of ['check', 'qasm', 'run']) same(cmd, path);
}
same('run', 'examples/qx/bell.qx', 0, 0);

// Playground-only limit: 21 non-Clifford qubits are refused in the browser but run in the CLI.
const rotated = (n) =>
  `void main() {\n    QReg r = qreg(${n});\n    for i in 0..${n} {\n        h(r[i]);\n        t(r[i]);\n    }\n    BitReg m = measure(r);\n    println("{}", m);\n}\n`;
const over = write('sv_21.qx', rotated(21));
const capped = runWasm('run', over, 1, 0);
const uncapped = runCli('run', over, 1, 0);
if (capped.exit !== 1 || !capped.stderr.startsWith('error[Q0406]: ') || uncapped.exit !== 0) {
  failures.push({cmd: 'run', path: over, note: '21-qubit cap', want: uncapped, got: capped});
}
const atLimit = runWasm('run', write('sv_20.qx', rotated(20)), 1, 0);
if (atLimit.exit !== 0) failures.push({cmd: 'run', path: 'sv_20.qx', note: '20 qubits must run', got: atLimit});

if (failures.length) {
  for (const f of failures) console.error(JSON.stringify(f, null, 2));
  console.error(`parity: ${failures.length} mismatch(es) in ${checked} comparisons`);
  process.exit(1);
}
console.log(`parity: ${checked} comparisons over ${examples.length} examples and ${Object.keys(errorCases).length} error cases match the CLI; 21-qubit cap verified`);
