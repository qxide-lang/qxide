// Host side of the raw ABI in crates/qxide-wasm/src/lib.rs. No imports, so Node can load this file
// directly (scripts/parity.mjs) as well as the playground worker.

export type Command = 'check' | 'run' | 'qasm';

/** What `qxide <command>` prints and returns. */
export type Outcome = {exit: number; stdout: string; stderr: string};

const CODES: Record<Command, number> = {check: 0, run: 1, qasm: 2};
const U64_MAX = (1n << 64n) - 1n;

type Exports = {
  memory: WebAssembly.Memory;
  qx_alloc(len: number): number;
  qx_exec(cmd: number, namePtr: number, nameLen: number, srcPtr: number, srcLen: number, shots: bigint, seed: bigint): number;
};

/**
 * Runs one command in a fresh instance, so every run starts with clean memory and a trap cannot
 * leave state behind. Throws if the module traps (stack exhaustion, out of memory, a bug).
 */
export function execute(module: WebAssembly.Module, cmd: Command, name: string, src: string, shots: bigint, seed: bigint): Outcome {
  if (shots < 0n || shots > U64_MAX || seed < 0n || seed > U64_MAX) throw new RangeError('shots and seed must fit in u64');
  const instance = new WebAssembly.Instance(module, {});
  const ex = instance.exports as unknown as Exports;
  const enc = new TextEncoder();
  const nameBytes = enc.encode(name);
  const srcBytes = enc.encode(src);
  const namePtr = ex.qx_alloc(nameBytes.length);
  const srcPtr = ex.qx_alloc(srcBytes.length);
  // Re-read memory.buffer after each call: allocations may grow (and detach) it.
  new Uint8Array(ex.memory.buffer, namePtr, nameBytes.length).set(nameBytes);
  new Uint8Array(ex.memory.buffer, srcPtr, srcBytes.length).set(srcBytes);
  const out = ex.qx_exec(CODES[cmd], namePtr, nameBytes.length, srcPtr, srcBytes.length, shots, seed);
  const buf = ex.memory.buffer;
  const len = new DataView(buf).getUint32(out, true);
  const json = new TextDecoder().decode(new Uint8Array(buf, out + 4, len));
  return JSON.parse(json) as Outcome;
}
