// Random-input check of the playground share-link decoder (site/src/playground/share.ts).
// No dependencies; Node 22.18+ / 24 runs the TypeScript module directly (type stripping).
//   node fuzz/js/share_decode.mjs [iterations=20000] [seed=1]
// Invariant: decodeShare returns null, returns a well-formed Shared, or throws an Error; the
// code it returns is at most 256 KiB and shots/seed are 1-20 digits or null.
import {decodeShare, encodeShare} from '../../site/src/playground/share.ts';

const iterations = Number(process.argv[2] ?? 20000);
// xorshift needs a non-zero state; every other seed gives its own sequence.
let state = BigInt(process.argv[3] ?? 1) || 0x5eedn;
const next = () => {
  // xorshift64*
  state ^= state >> 12n;
  state ^= (state << 25n) & 0xffffffffffffffffn;
  state ^= state >> 27n;
  return Number(((state * 0x2545f4914f6cdd1dn) & 0xffffffffffffffffn) >> 11n);
};
const below = (n) => next() % Math.max(1, n);
const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=+/%&#?;\u0000é\u{1F600}';
const KEYS = ['code', 'src', 'shots', 'seed', 'x'];
const VALUES = ['', '0', '1', '99999999999999999999', '999999999999999999999', '-1', '1e3', '%00', '%FF', 'AAAA'];

const seeds = await Promise.all([
  encodeShare({code: 'void main() {\n    println("hi");\n}\n', shots: '100', seed: '7'}),
  encodeShare({code: '', shots: null, seed: null}),
  encodeShare({code: 'é\r\n\u{1F600}'.repeat(50), shots: '1', seed: null}),
]);

function mutate(s) {
  let out = s;
  for (let k = 0; k < 1 + below(4); k++) {
    const i = below(out.length + 1);
    switch (below(5)) {
      case 0: out = out.slice(0, i) + out.slice(i + 1 + below(8)); break;
      case 1: out = out.slice(0, i) + ALPHABET[below(ALPHABET.length)] + out.slice(i); break;
      case 2: out = out.slice(0, i); break;
      case 3: out += `&${KEYS[below(KEYS.length)]}=${VALUES[below(VALUES.length)]}`; break;
      default: out = out.slice(0, i) + out.slice(i, i + below(64)).repeat(1 + below(4)) + out.slice(i);
    }
  }
  return out;
}

let accepted = 0, rejected = 0, empty = 0;
for (let n = 0; n < iterations; n++) {
  const hash = mutate(seeds[below(seeds.length)]);
  try {
    const r = await decodeShare(hash);
    if (r === null) { empty++; continue; }
    accepted++;
    const okDigits = (v) => v === null || /^\d{1,20}$/.test(v);
    if (typeof r.code !== 'string' || new TextEncoder().encode(r.code).length > 256 * 1024 || !okDigits(r.shots) || !okDigits(r.seed)) {
      console.error('invariant violated for hash:', JSON.stringify(hash), r);
      process.exit(1);
    }
  } catch (e) {
    if (!(e instanceof Error)) {
      console.error('non-Error thrown for hash:', JSON.stringify(hash), e);
      process.exit(1);
    }
    rejected++;
  }
}
console.log(`${iterations} hashes: ${accepted} decoded, ${empty} without a program, ${rejected} rejected`);
