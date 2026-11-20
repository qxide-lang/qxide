// Builds crates/qxide-wasm for the playground and copies the module to src/playground/generated/.
// Needs cargo and the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`).
import {spawnSync} from 'node:child_process';
import {copyFileSync, mkdirSync, readFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {brotliCompressSync, gzipSync} from 'node:zlib';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const root = resolve(site, '..');
const target = 'wasm32-unknown-unknown';
// Cloudflare Pages rejects any single file over 25 MiB.
const PAGES_FILE_LIMIT = 25 * 1024 * 1024;

const build = spawnSync(
  'cargo',
  ['build', '--quiet', '-p', 'qxide-wasm', '--target', target, '--profile', 'wasm'],
  {cwd: root, stdio: 'inherit'},
);
if (build.error || build.status !== 0) {
  console.error(build.error ?? `cargo exited with ${build.status}`);
  process.exit(1);
}

const targetDir = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'target');
const built = join(targetDir, target, 'wasm', 'qxide_wasm.wasm');
const outDir = join(site, 'src', 'playground', 'generated');
mkdirSync(outDir, {recursive: true});
const out = join(outDir, 'qxide.wasm');
copyFileSync(built, out);

const bytes = readFileSync(out);
const kib = (n) => `${(n / 1024).toFixed(1)} KiB`;
console.log(
  `qxide.wasm: ${kib(bytes.length)} (gzip ${kib(gzipSync(bytes, {level: 9}).length)}, brotli ${kib(brotliCompressSync(bytes).length)})`,
);
if (bytes.length > PAGES_FILE_LIMIT) {
  console.error(`qxide.wasm is over the 25 MiB Cloudflare Pages file limit`);
  process.exit(1);
}
