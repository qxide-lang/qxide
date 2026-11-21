#!/usr/bin/env python3
"""Scaling measurement for `qxide check` on generated large inputs (M-05).

Generates one family of .qx files per checker code path, doubles the size parameter, times
`qxide check` (release build) and reports the empirical growth exponent between steps:
slope = log2(t2 / t1) / log2(n2 / n1)  (about 1 = linear, about 2 = quadratic).

Usage (from the repository root, after `cargo build --release`):
    python -I scripts/perf/checker_large_files.py [--bin target/release/qxide.exe]
        [--out target/perf-checker] [--repeat 3] [--budget 20] [--family NAME ...]
        [--json results.json]

Generated inputs go to --out (ignored by git). A family stops growing once one run exceeds
--budget seconds. Times are the median of --repeat runs (wall clock, process start included).
"""

import argparse
import json
import math
import os
import statistics
import subprocess
import sys
import time


def chain_callers_first(n):
    # f0 calls f1, ..., fN prints: the M-05 conditional_safety worst case.
    body = "".join(f"void f{i}(){{f{i + 1}();}}\n" for i in range(n))
    return body + f'void f{n}(){{println("x");}}\nvoid main(){{f0();}}\n'


def chain_callees_first(n):
    # Same call graph declared in the opposite order: one fixpoint pass suffices.
    body = f'void f{n}(){{println("x");}}\n'
    body += "".join(f"void f{i}(){{f{i + 1}();}}\n" for i in reversed(range(n)))
    return body + "void main(){f0();}\n"


def flat_functions(n):
    # Realistic baseline: n independent functions with a qubit, gates, measure and println.
    fns = "".join(
        f"void g{i}(){{\n    Qubit q = qubit();\n    h(q);\n    Bit b = measure(q);\n"
        f'    println("{{}}", b);\n}}\n'
        for i in range(n)
    )
    calls = "".join(f"    g{i}();\n" for i in range(n))
    return fns + "void main(){\n" + calls + "}\n"


def decls_then_ifs(n):
    # M-05 states() case: n declarations, then n empty `if`s in the same function.
    return (
        "void main(){\n"
        + "".join(f"int a{i}=0;\n" for i in range(n))
        + "if(true){}\n" * n
        + "}\n"
    )


def ifs_only(n):
    # Control for decls_then_ifs: the same number of `if`s with one variable in scope.
    return "void main(){\nint a=0;\n" + "if(true){a=1;}\n" * n + "}\n"


def decls_then_fors(n):
    # `for` takes the same states() snapshot as `if`.
    return (
        "void main(){\n"
        + "".join(f"int a{i}=0;\n" for i in range(n))
        + "for i in 0..1 {}\n" * n
        + "}\n"
    )


def decls_then_andand(n):
    # `&&`/`||` right operands take a states() snapshot too.
    return (
        "void main(){\n"
        + "".join(f"int a{i}=0;\n" for i in range(n))
        + "".join(f"boolean c{i} = true && true;\n" for i in range(n))
        + "}\n"
    )


def lookup_far(n):
    # Each declaration reads the first variable, so the scope scan in lookup() walks all of them.
    return (
        "void main(){\nint a0=0;\n"
        + "".join(f"int a{i}=a0+1;\n" for i in range(1, n))
        + "}\n"
    )


def lookup_near(n):
    # Control for lookup_far: each declaration reads the previous variable (found at once).
    return (
        "void main(){\nint a0=0;\n"
        + "".join(f"int a{i}=a{i - 1}+1;\n" for i in range(1, n))
        + "}\n"
    )


def qubits_then_ifs(n):
    # decls_then_ifs with qubits: only quantum variables carry a linearity state.
    return (
        "void main(){\n"
        + "".join(f"Qubit q{i}=qubit();\n" for i in range(n))
        + "if(true){}\n" * n
        + "".join(f"discard(q{i});\n" for i in range(n))
        + "}\n"
    )


def nested_ifs(d):
    # Nesting depth d (each `if` level costs two of the parser's MAX_DEPTH = 128, so d <= 63),
    # one declaration per level, and enough repetitions that the file is not trivially small.
    one = ""
    for i in range(d):
        one += f"int v{i}=0;\nif(true){{\n"
    one += "}\n" * d
    return "void main(){\n" + one * 200 + "}\n"


FAMILIES = {
    # name: (generator, sizes, parameter meaning)
    "chain_callers_first": (chain_callers_first, [1000, 2000, 4000, 8000, 16000, 32000, 64000], "functions"),
    "chain_callees_first": (chain_callees_first, [1000, 2000, 4000, 8000, 16000, 32000, 64000], "functions"),
    "flat_functions": (flat_functions, [1000, 2000, 4000, 8000, 16000, 32000, 64000], "functions"),
    "decls_then_ifs": (decls_then_ifs, [2500, 5000, 10000, 20000, 40000, 80000], "decls = ifs"),
    "ifs_only": (ifs_only, [2500, 5000, 10000, 20000, 40000, 80000], "ifs"),
    "decls_then_fors": (decls_then_fors, [2500, 5000, 10000, 20000, 40000, 80000], "decls = fors"),
    "decls_then_andand": (decls_then_andand, [2500, 5000, 10000, 20000, 40000, 80000], "decls = &&"),
    "qubits_then_ifs": (qubits_then_ifs, [2500, 5000, 10000, 20000, 40000, 80000], "qubits = ifs"),
    "lookup_far": (lookup_far, [2500, 5000, 10000, 20000, 40000, 80000], "decls"),
    "lookup_near": (lookup_near, [2500, 5000, 10000, 20000, 40000, 80000], "decls"),
    "nested_ifs": (nested_ifs, [4, 8, 16, 32, 60], "depth (x200 blocks)"),
}


def run_once(binary, path, timeout):
    start = time.perf_counter()
    try:
        proc = subprocess.run(
            [binary, "check", path], capture_output=True, timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return None, "timeout"
    elapsed = time.perf_counter() - start
    status = "ok" if proc.returncode == 0 else f"exit {proc.returncode}"
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode(errors="replace")[:2000])
    return elapsed, status


def measure(binary, out_dir, name, repeat, budget, timeout):
    gen, sizes, unit = FAMILIES[name]
    rows = []
    for n in sizes:
        path = os.path.join(out_dir, f"{name}_{n}.qx")
        with open(path, "wb") as f:
            f.write(gen(n).encode())
        times, status = [], "ok"
        for _ in range(repeat):
            t, status = run_once(binary, path, timeout)
            if t is None or status != "ok":
                break
            times.append(t)
            if t > budget:
                break  # one sample is enough for slow sizes
        row = {
            "family": name,
            "n": n,
            "unit": unit,
            "bytes": os.path.getsize(path),
            "status": status,
            "seconds": statistics.median(times) if times else None,
            "samples": len(times),
        }
        if rows and rows[-1]["seconds"] and row["seconds"]:
            prev = rows[-1]
            row["slope"] = math.log2(row["seconds"] / prev["seconds"]) / math.log2(n / prev["n"])
        rows.append(row)
        secs = f"{row['seconds']:.3f}s" if row["seconds"] is not None else status
        slope = f"  slope {row['slope']:.2f}" if "slope" in row else ""
        print(f"{name:22} n={n:<7} {row['bytes']:>9} B  {secs}{slope}", flush=True)
        if status != "ok" or (row["seconds"] or 0) > budget:
            break
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    exe = "qxide.exe" if os.name == "nt" else "qxide"
    ap.add_argument("--bin", default=os.path.join("target", "release", exe))
    ap.add_argument("--out", default=os.path.join("target", "perf-checker"))
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--budget", type=float, default=20.0)
    ap.add_argument("--timeout", type=float, default=180.0)
    ap.add_argument("--family", nargs="*", default=list(FAMILIES))
    ap.add_argument("--json")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    rows = []
    for name in args.family:
        rows += measure(args.bin, args.out, name, args.repeat, args.budget, args.timeout)
    if args.json:
        with open(args.json, "w") as f:
            json.dump(rows, f, indent=1)


if __name__ == "__main__":
    main()
