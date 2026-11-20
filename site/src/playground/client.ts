import type {Command, Outcome} from './engine';
import type {TrapReason, WorkerRequest, WorkerResponse} from './protocol';

/** Wall-clock budget for one command before the worker is terminated. */
export const TIME_LIMIT_MS = 30_000;

export type RunResult =
  | {kind: 'done'; outcome: Outcome; ms: number}
  | {kind: 'trap'; reason: TrapReason; detail: string}
  | {kind: 'load-error'; detail: string}
  | {kind: 'timeout'}
  | {kind: 'stopped'};

export type RunRequest = {cmd: Command; name: string; src: string; shots: string; seed: string};

/**
 * Owns the playground worker. Each run is synchronous inside the worker, so stopping it or
 * enforcing the time limit means terminating the worker and starting a new one.
 */
export class EngineClient {
  private worker: Worker | null = null;
  private nextId = 1;
  private pending: {id: number; resolve: (r: RunResult) => void; timer: number} | null = null;

  private spawn(): Worker {
    const w = new Worker(new URL('./worker.ts', import.meta.url), {type: 'module'});
    w.onmessage = (e: MessageEvent<WorkerResponse>) => {
      const p = this.pending;
      if (!p || e.data.id !== p.id) return;
      this.finish(e.data.kind === 'done' ? e.data : e.data.kind === 'trap' ? e.data : {kind: 'load-error', detail: e.data.detail});
    };
    w.onerror = (e) => {
      e.preventDefault();
      this.kill();
      this.finish({kind: 'load-error', detail: e.message || 'worker failed to start'});
    };
    return w;
  }

  get busy(): boolean {
    return this.pending !== null;
  }

  run(req: RunRequest, timeLimitMs = TIME_LIMIT_MS): Promise<RunResult> {
    this.stop();
    this.worker ??= this.spawn();
    const id = this.nextId++;
    return new Promise((resolve) => {
      const timer = window.setTimeout(() => {
        this.kill();
        this.finish({kind: 'timeout'});
      }, timeLimitMs);
      this.pending = {id, resolve, timer};
      this.worker!.postMessage({id, ...req} satisfies WorkerRequest);
    });
  }

  /** Ends the current run, if any; the next run gets a fresh worker. */
  stop(): void {
    if (!this.pending) return;
    this.kill();
    this.finish({kind: 'stopped'});
  }

  dispose(): void {
    this.stop();
    this.kill();
  }

  private kill(): void {
    this.worker?.terminate();
    this.worker = null;
  }

  private finish(result: RunResult): void {
    const p = this.pending;
    if (!p) return;
    this.pending = null;
    window.clearTimeout(p.timer);
    p.resolve(result);
  }
}
