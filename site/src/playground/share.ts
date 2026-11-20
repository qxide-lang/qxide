// Share links keep the program in the URL hash: #code=<base64url(deflate-raw(utf8))>&shots=…&seed=….
// The hash never reaches the server. Browsers without CompressionStream use #src= (uncompressed).

export type Shared = {code: string; shots: string | null; seed: string | null};

/** Longest hash we read back; anything larger is rejected rather than inflated. */
const MAX_HASH = 200_000;
const MAX_CODE = 256 * 1024;

function toBase64Url(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(text: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]*$/.test(text)) throw new Error('not base64url');
  const bin = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

async function pipe(bytes: Uint8Array, stream: CompressionStream | DecompressionStream): Promise<Uint8Array> {
  const out = new Response(new Blob([bytes as BlobPart]).stream().pipeThrough(stream));
  return new Uint8Array(await out.arrayBuffer());
}

const canCompress = typeof CompressionStream === 'function' && typeof DecompressionStream === 'function';

export async function encodeShare({code, shots, seed}: Shared): Promise<string> {
  const params = new URLSearchParams();
  const utf8 = new TextEncoder().encode(code);
  if (canCompress) params.set('code', toBase64Url(await pipe(utf8, new CompressionStream('deflate-raw'))));
  else params.set('src', toBase64Url(utf8));
  if (shots) params.set('shots', shots);
  if (seed) params.set('seed', seed);
  return `#${params.toString()}`;
}

/** null when the hash holds no program; throws when it holds a damaged one. */
export async function decodeShare(hash: string): Promise<Shared | null> {
  const raw = hash.startsWith('#') ? hash.slice(1) : hash;
  if (raw.length > MAX_HASH) throw new Error('share link too long');
  const params = new URLSearchParams(raw);
  const zipped = params.get('code');
  const plain = params.get('src');
  if (zipped === null && plain === null) return null;
  let bytes: Uint8Array;
  if (zipped !== null) {
    if (!canCompress) throw new Error('this browser cannot decompress share links');
    bytes = await pipe(fromBase64Url(zipped), new DecompressionStream('deflate-raw'));
  } else {
    bytes = fromBase64Url(plain!);
  }
  if (bytes.length > MAX_CODE) throw new Error('shared program too large');
  // Same LF normalisation a textarea applies, so caret offsets match the stored code.
  const code = new TextDecoder('utf-8', {fatal: true}).decode(bytes).replace(/\r\n?/g, '\n');
  const digits = (v: string | null) => (v !== null && /^\d{1,20}$/.test(v) ? v : null);
  return {code, shots: digits(params.get('shots')), seed: digits(params.get('seed'))};
}
