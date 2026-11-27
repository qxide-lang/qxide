// Checks of the share-link decoder (src/playground/share.ts). Node 22.18+ runs the TypeScript
// module directly (type stripping).
//   node --expose-gc scripts/share-test.mjs
import assert from 'node:assert/strict';
import {deflateRawSync} from 'node:zlib';
import {decodeShare, encodeShare} from '../src/playground/share.ts';

const b64url = (buf) => buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

// Round trip.
{
  const shared = {code: 'void main() {\n    println("hi");\n}\n', shots: '100', seed: '7'};
  assert.deepEqual(await decodeShare(await encodeShare(shared)), shared);
}

// B-04 (docs/security/BASELINE_FINDINGS.md): a link whose program inflates to 140 MiB fits in
// the hash limit. It must be rejected while it is inflated, not after the whole program sits in
// memory.
{
  let bomb = deflateRawSync(Buffer.alloc(140 * 1024 * 1024, 'a'), {level: 9});
  const hash = `#code=${b64url(bomb)}`;
  bomb = null;
  assert.ok(hash.length < 200_000, `hash of ${hash.length} characters`);
  globalThis.gc?.();
  const before = process.memoryUsage().arrayBuffers;
  await assert.rejects(decodeShare(hash), /shared program too large/);
  const grown = process.memoryUsage().arrayBuffers - before;
  assert.ok(grown < 16 * 1024 * 1024, `decoding grew array buffers by ${(grown / 2 ** 20).toFixed(1)} MiB`);
}

// A program of exactly the limit still decodes.
{
  const code = 'a'.repeat(256 * 1024);
  assert.equal((await decodeShare(await encodeShare({code, shots: null, seed: null}))).code, code);
}

console.log('share tests passed');
