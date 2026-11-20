// Reads the CLI-format text the engine prints: diagnostics on stderr, the multi-shot table on stdout.

export type Diagnostic = {code: string; message: string; line: number; col: number};

const ERROR_RE = /^error\[(Q\d{4})\]: (.*)$/;
const LOC_RE = /^\s*--> (.*):(\d+):(\d+)$/;

/** Primary location of each `error[Q….]` block (notes carry their own `-->` lines and are skipped). */
export function parseDiagnostics(stderr: string, name: string): Diagnostic[] {
  const out: Diagnostic[] = [];
  let current: {code: string; message: string} | null = null;
  for (const line of stderr.split('\n')) {
    const err = ERROR_RE.exec(line);
    if (err) {
      current = {code: err[1], message: err[2]};
      continue;
    }
    const loc = LOC_RE.exec(line);
    if (loc && current && loc[1] === name) {
      out.push({...current, line: Number(loc[2]), col: Number(loc[3])});
      current = null;
    }
  }
  return out;
}

export type CountRow = {bits: string; binary: string; value: string | null; count: number; lines: string[]};

// `[0, 1, 1]  0b110 = 6  ×503`; the `= value` part is absent above 64 bits.
const ROW_RE = /^(\[[01, ]*\])  (0b[01]+)(?: = (\d+))?  ×(\d+)$/;

/** Rows of a multi-shot run, or null when stdout is plain program output. */
export function parseCounts(stdout: string): CountRow[] | null {
  const rows: CountRow[] = [];
  const lines = stdout.split('\n');
  if (lines.at(-1) === '') lines.pop();
  for (const line of lines) {
    const m = ROW_RE.exec(line);
    if (m) rows.push({bits: m[1], binary: m[2], value: m[3] ?? null, count: Number(m[4]), lines: []});
    else if (line.startsWith('  ') && rows.length) rows.at(-1)!.lines.push(line.slice(2));
    else return null;
  }
  return rows.length ? rows : null;
}

/** UTF-16 offset of a 1-based line and char column (the diagnostics count chars, not bytes). */
export function offsetOf(src: string, line: number, col: number): number {
  let offset = 0;
  for (let l = 1; l < line; l++) {
    const nl = src.indexOf('\n', offset);
    if (nl < 0) return src.length;
    offset = nl + 1;
  }
  // Skip a BOM like the compiler does.
  if (line === 1 && src.startsWith('﻿')) offset = 1;
  for (let c = 1; c < col && offset < src.length && src[offset] !== '\n'; c++) {
    offset += src.codePointAt(offset)! > 0xffff ? 2 : 1;
  }
  return offset;
}
