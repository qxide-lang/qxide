"""Qxide versus Rust on the same algorithms (bench/compare): prints a Markdown table.

Build first (one heavy job at a time):
    cargo build --release --features jit
    cargo build --release --manifest-path bench/compare/rust/Cargo.toml
Then:
    python scripts/perf/rust_compare.py [--reps 5] [--ir-reps 3] [--only nbody,matmul] [--values] [--no-budget]

`--values` adds a column for the programs of bench/compare/qx-values run with
`--experimental value-semantics` on the jit engine (docs/design/memory-model.md).

Each program times its own kernel (Qxide: `std.time.monotonicMillis`, 1 ms resolution; Rust:
`Instant`) and prints `result=R ms=T`. Runs are interleaved (Rust, Qxide jit, Qxide ir per
round) so a change in machine load hits every column alike. "Rust %" is Rust time / Qxide time
x 100 on the medians: 100 means as fast as Rust.
"""

import argparse
import os
import platform
import statistics
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EXE = '.exe' if os.name == 'nt' else ''
QXIDE = os.path.join(ROOT, 'target', 'release', 'qxide' + EXE)
RUST = os.path.join(ROOT, 'bench', 'compare', 'rust', 'target', 'release')

# (name, input) chosen so the Rust kernel takes tens of milliseconds.
CASES = [
    ('nbody', '1000000'),
    ('binarytrees', '14'),
    ('matmul', '400'),
    ('strings', '200000'),
]


def run(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f'{cmd} failed ({out.returncode}): {out.stderr.strip()[:400]}')
    line = out.stdout.strip().splitlines()[-1]
    fields = dict(f.split('=', 1) for f in line.split())
    return fields['result'], float(fields['ms'])


def same(a, b):
    if a == b:
        return True
    try:
        x, y = float(a), float(b)
    except ValueError:
        return False
    return abs(x - y) <= 1e-9 * max(abs(x), abs(y), 1.0)


def summary(ts):
    med = statistics.median(ts)
    return med, min(ts), max(ts)


def fmt(ts):
    if not ts:
        return '-'
    med, lo, hi = summary(ts)
    spread = (hi - lo) / med * 100 if med else 0.0
    return f'{med:.1f} ({lo:.1f}-{hi:.1f}, ±{spread / 2:.0f}%)'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--reps', type=int, default=5)
    ap.add_argument('--ir-reps', type=int, default=3)
    ap.add_argument('--only', default='')
    ap.add_argument('--values', action='store_true')
    # Also time `--engine jit --no-budget` (no step budget, no step checks in native code).
    ap.add_argument('--no-budget', action='store_true')
    args = ap.parse_args()
    only = set(filter(None, args.only.split(',')))
    cases = [c for c in CASES if not only or c[0] in only]
    rows = []
    for name, n in cases:
        qx = os.path.join(ROOT, 'bench', 'compare', 'qx', name + '.qx')
        rust_cmd = [os.path.join(RUST, name + EXE), n]
        qx_cmd = lambda e: [QXIDE, 'run', qx, '--engine', e, '--trusted', '--', n]
        qxv = os.path.join(ROOT, 'bench', 'compare', 'qx-values', name + '.qx')
        values_cmd = [QXIDE, 'run', qxv, '--experimental', 'value-semantics', '--engine', 'jit',
                      '--trusted', '--', n]
        free_cmd = [QXIDE, 'run', qx, '--engine', 'jit', '--no-budget', '--', n]
        times = {'rust': [], 'jit': [], 'ir': [], 'values': [], 'free': []}
        results = set()
        for r in range(args.reps):
            for col, cmd in [('rust', rust_cmd), ('jit', qx_cmd('jit'))] + (
                [('values', values_cmd)] if args.values else []
            ) + (
                [('free', free_cmd)] if args.no_budget else []
            ) + (
                [('ir', qx_cmd('ir'))] if r < args.ir_reps else []
            ):
                res, ms = run(cmd)
                results.add(res)
                times[col].append(ms)
        first = next(iter(results))
        match = all(same(first, x) for x in results)
        rust_med = statistics.median(times['rust'])
        jit_med = statistics.median(times['jit'])
        ir_med = statistics.median(times['ir']) if times['ir'] else None
        pct = rust_med / jit_med * 100 if jit_med else float('inf')
        values_med = statistics.median(times['values']) if times['values'] else None
        values_pct = rust_med / values_med * 100 if values_med else None
        free_med = statistics.median(times['free']) if times['free'] else None
        free_pct = rust_med / free_med * 100 if free_med else None
        rows.append((name, n, fmt(times['rust']), fmt(times['jit']), fmt(times['ir']), pct,
                     ir_med / jit_med if ir_med and jit_med else None, match,
                     fmt(times['values']), values_pct, fmt(times['free']), free_pct))
        print(f'{name}: done', file=sys.stderr)
    rustc = subprocess.run(['rustc', '--version'], capture_output=True, text=True).stdout.strip()
    commit = subprocess.run(['git', '-C', ROOT, 'rev-parse', '--short', 'HEAD'],
                            capture_output=True, text=True).stdout.strip()
    print(f'- qxide `{commit}` (`cargo build --release --features jit`), `--engine jit|ir --trusted`; '
          f'{rustc}, `cargo build --release` (opt-level 3)')
    print(f'- {platform.system()} {platform.release()} {platform.machine()}; '
          f'{args.reps} interleaved rounds (ir: {args.ir_reps}); times are the kernel only, ms: '
          f'median (min-max, ± half the range as % of the median)')
    print()
    extra = ' Qxide jit, values ms | Rust % (values) |' if args.values else ''
    extra += ' Qxide jit, no budget ms | Rust % (no budget) |' if args.no_budget else ''
    print('| Benchmark | Input | Rust ms | Qxide jit ms | Qxide ir ms | Rust % (jit) | jit / Rust | ir / jit |'
          + extra + ' Same result |')
    print('| --- ' * (9 + (2 if args.values else 0) + (2 if args.no_budget else 0)) + '|')
    for name, n, rust, jit, ir, pct, ratio, match, values, values_pct, free, free_pct in rows:
        r = f'{ratio:.2f}' if ratio else '-'
        slow = f'{100 / pct:.1f}x' if pct else '-'
        v = f' {values} | {values_pct:.2f} % |' if args.values else ''
        v += f' {free} | {free_pct:.2f} % |' if args.no_budget else ''
        print(f'| `{name}` | {n} | {rust} | {jit} | {ir} | {pct:.2f} % | {slow} | {r} |{v} {"yes" if match else "NO"} |')


if __name__ == '__main__':
    main()
