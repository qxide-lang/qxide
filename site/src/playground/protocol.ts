import type {Command, Outcome} from './engine';

/** shots and seed travel as decimal strings: u64 values do not fit in a JS number. */
export type WorkerRequest = {id: number; cmd: Command; name: string; src: string; shots: string; seed: string};

export type TrapReason = 'stack' | 'memory' | 'crash';

export type WorkerResponse =
  | {id: number; kind: 'done'; outcome: Outcome; ms: number}
  | {id: number; kind: 'trap'; reason: TrapReason; detail: string}
  | {id: number; kind: 'load-error'; detail: string};
