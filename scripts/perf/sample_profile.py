#!/usr/bin/env python3
"""Minimal Windows x64 sampling profiler with no installs: suspends the target's threads at a
fixed interval, walks each stack with the system dbghelp.dll (StackWalk64), and symbolizes the
frames, including inlined ones, against the binary's PDB.

Usage (from the repository root):
    CARGO_PROFILE_RELEASE_DEBUG=true CARGO_TARGET_DIR=target/prof \
        cargo build --release --bin qxide
    python -I scripts/perf/sample_profile.py [--interval-ms 1] [--top 20] [--focus src/lang/] \
        target/prof/release/qxide.exe check FILE.qx

Reports, over the samples of the thread doing the work (a frame under --focus on the stack):
  * self:      the innermost frame (inlined code counts as its own frame);
  * inclusive: every distinct source line under --focus on the sample's stack, so a hot loop
               shows up at the line that calls into the hash map or the allocator.
"""

import argparse
import collections
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
dbg = ctypes.WinDLL("dbghelp", use_last_error=True)

# Handles and addresses are pointer-sized; the ctypes default (c_int) would truncate them.
k32.OpenProcess.restype = wt.HANDLE
k32.OpenThread.restype = wt.HANDLE
k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
k32.SuspendThread.argtypes = k32.ResumeThread.argtypes = k32.CloseHandle.argtypes = [wt.HANDLE]
k32.SuspendThread.restype = wt.DWORD
k32.GetThreadContext.argtypes = [wt.HANDLE, ctypes.c_void_p]
dbg.SymEnumerateModules64.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p]
k32.Thread32First.argtypes = k32.Thread32Next.argtypes = [wt.HANDLE, ctypes.c_void_p]

TH32CS_SNAPTHREAD = 0x4
THREAD_ACCESS = 0x0002 | 0x0008 | 0x0040  # SUSPEND_RESUME | GET_CONTEXT | QUERY_INFORMATION
PROCESS_ACCESS = 0x0400 | 0x0010  # QUERY_INFORMATION | VM_READ
CONTEXT_FULL = 0x10000B
CONTEXT_SIZE = 1232
OFF_FLAGS, OFF_RSP, OFF_RBP, OFF_RIP = 0x30, 0x98, 0xA0, 0xF8
MACHINE_AMD64 = 0x8664
ADDR_MODE_FLAT = 3
MAX_DEPTH = 96


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ThreadID", wt.DWORD),
                ("th32OwnerProcessID", wt.DWORD), ("tpBasePri", wt.LONG), ("tpDeltaPri", wt.LONG),
                ("dwFlags", wt.DWORD)]


class ADDRESS64(ctypes.Structure):
    _fields_ = [("Offset", ctypes.c_uint64), ("Segment", wt.WORD), ("Mode", wt.DWORD)]


class STACKFRAME64(ctypes.Structure):
    _fields_ = [("AddrPC", ADDRESS64), ("AddrReturn", ADDRESS64), ("AddrFrame", ADDRESS64),
                ("AddrStack", ADDRESS64), ("AddrBStore", ADDRESS64), ("FuncTableEntry", ctypes.c_void_p),
                ("Params", ctypes.c_uint64 * 4), ("Far", wt.BOOL), ("Virtual", wt.BOOL),
                ("Reserved", ctypes.c_uint64 * 3), ("KdHelp", ctypes.c_byte * 256)]


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.ULONG), ("TypeIndex", wt.ULONG), ("Reserved", ctypes.c_uint64 * 2),
                ("Index", wt.ULONG), ("Size", wt.ULONG), ("ModBase", ctypes.c_uint64), ("Flags", wt.ULONG),
                ("Value", ctypes.c_uint64), ("Address", ctypes.c_uint64), ("Register", wt.ULONG),
                ("Scope", wt.ULONG), ("Tag", wt.ULONG), ("NameLen", wt.ULONG), ("MaxNameLen", wt.ULONG),
                ("Name", ctypes.c_char * 1024)]


class IMAGEHLP_LINE64(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.DWORD), ("Key", ctypes.c_void_p), ("LineNumber", wt.DWORD),
                ("FileName", ctypes.c_char_p), ("Address", ctypes.c_uint64)]


dbg.SymInitialize.argtypes = [wt.HANDLE, ctypes.c_char_p, wt.BOOL]
dbg.SymGetModuleBase64.restype = ctypes.c_uint64
dbg.SymGetModuleBase64.argtypes = [wt.HANDLE, ctypes.c_uint64]
FTA = ctypes.WINFUNCTYPE(ctypes.c_void_p, wt.HANDLE, ctypes.c_uint64)(("SymFunctionTableAccess64", dbg))
GMB = ctypes.WINFUNCTYPE(ctypes.c_uint64, wt.HANDLE, ctypes.c_uint64)(("SymGetModuleBase64", dbg))
dbg.StackWalk64.argtypes = [wt.DWORD, wt.HANDLE, wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                            ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
dbg.SymRefreshModuleList.argtypes = [wt.HANDLE]
dbg.SymAddrIncludeInlineTrace.argtypes = [wt.HANDLE, ctypes.c_uint64]
dbg.SymAddrIncludeInlineTrace.restype = wt.DWORD
dbg.SymQueryInlineTrace.argtypes = [wt.HANDLE, ctypes.c_uint64, wt.DWORD, ctypes.c_uint64, ctypes.c_uint64,
                                    ctypes.POINTER(wt.DWORD), ctypes.POINTER(wt.DWORD)]
dbg.SymFromInlineContext.argtypes = [wt.HANDLE, ctypes.c_uint64, wt.ULONG, ctypes.POINTER(ctypes.c_uint64),
                                     ctypes.c_void_p]
dbg.SymGetLineFromInlineContext.argtypes = [wt.HANDLE, ctypes.c_uint64, wt.ULONG, ctypes.c_uint64,
                                            ctypes.POINTER(wt.DWORD), ctypes.c_void_p]
dbg.SymFromAddr.argtypes = [wt.HANDLE, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64), ctypes.c_void_p]
dbg.SymGetLineFromAddr64.argtypes = [wt.HANDLE, ctypes.c_uint64, ctypes.POINTER(wt.DWORD), ctypes.c_void_p]


def threads_of(pid):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    te = THREADENTRY32()
    te.dwSize = ctypes.sizeof(te)
    out = []
    ok = k32.Thread32First(snap, ctypes.byref(te))
    while ok:
        if te.th32OwnerProcessID == pid:
            out.append(te.th32ThreadID)
        ok = k32.Thread32Next(snap, ctypes.byref(te))
    k32.CloseHandle(snap)
    return out


def walk(hproc, hthread, ctx_addr):
    frame = STACKFRAME64()
    frame.AddrPC.Offset = ctypes.c_uint64.from_address(ctx_addr + OFF_RIP).value
    frame.AddrFrame.Offset = ctypes.c_uint64.from_address(ctx_addr + OFF_RBP).value
    frame.AddrStack.Offset = ctypes.c_uint64.from_address(ctx_addr + OFF_RSP).value
    frame.AddrPC.Mode = frame.AddrFrame.Mode = frame.AddrStack.Mode = ADDR_MODE_FLAT
    pcs = []
    while len(pcs) < MAX_DEPTH and dbg.StackWalk64(
        MACHINE_AMD64, hproc, hthread, ctypes.byref(frame), ctypes.c_void_p(ctx_addr), None,
        ctypes.cast(FTA, ctypes.c_void_p), ctypes.cast(GMB, ctypes.c_void_p), None
    ):
        if frame.AddrPC.Offset == 0:
            break
        pcs.append(frame.AddrPC.Offset)
    return pcs


def sample(cmd, interval):
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    hproc = k32.OpenProcess(PROCESS_ACCESS, False, proc.pid)
    dbg.SymSetOptions(0x2 | 0x10)  # UNDNAME | LOAD_LINES
    dbg.SymInitialize(hproc, os.path.dirname(os.path.abspath(cmd[0])).encode(), True)
    buf = (ctypes.c_byte * (CONTEXT_SIZE + 16))()
    ctx_addr = (ctypes.addressof(buf) + 15) & ~15
    handles, stacks, tids, tick = {}, [], [], 0
    while proc.poll() is None:
        if tick % 64 == 0:  # the thread snapshot is slow; refresh it now and then
            tids = threads_of(proc.pid)
            dbg.SymRefreshModuleList(hproc)
        tick += 1
        for tid in tids:
            h = handles.get(tid)
            if h is None:
                h = handles[tid] = k32.OpenThread(THREAD_ACCESS, False, tid)
            if not h or k32.SuspendThread(h) == 0xFFFFFFFF:
                continue
            ctypes.c_uint32.from_address(ctx_addr + OFF_FLAGS).value = CONTEXT_FULL
            if k32.GetThreadContext(h, ctypes.c_void_p(ctx_addr)):
                stacks.append(walk(hproc, h, ctx_addr))
            k32.ResumeThread(h)
        time.sleep(interval)
    for h in handles.values():
        k32.CloseHandle(h)
    return hproc, stacks


def short(path):
    path = path.replace("\\", "/")
    for key in ("/library/", "/src/"):
        if key in path:
            return path[path.find(key) + 1:]
    return os.path.basename(path)


def symbolize(hproc, pc):
    """Frames at `pc`, innermost (inlined) first, as (function, file:line)."""
    frames = []
    count = dbg.SymAddrIncludeInlineTrace(hproc, pc)
    ctx, idx = wt.DWORD(0), wt.DWORD(0)
    inline = count and dbg.SymQueryInlineTrace(hproc, pc, 0, pc, pc, ctypes.byref(ctx), ctypes.byref(idx))
    for k in range(count + 1 if inline else 1):
        sym = SYMBOL_INFO()
        sym.SizeOfStruct = ctypes.sizeof(SYMBOL_INFO) - 1024
        sym.MaxNameLen = 1024
        line = IMAGEHLP_LINE64()
        line.SizeOfStruct = ctypes.sizeof(line)
        disp, d32 = ctypes.c_uint64(), wt.DWORD()
        if inline:
            c = ctx.value + k
            ok_s = dbg.SymFromInlineContext(hproc, pc, c, ctypes.byref(disp), ctypes.byref(sym))
            ok_l = dbg.SymGetLineFromInlineContext(hproc, pc, c, 0, ctypes.byref(d32), ctypes.byref(line))
        else:
            ok_s = dbg.SymFromAddr(hproc, pc, ctypes.byref(disp), ctypes.byref(sym))
            ok_l = dbg.SymGetLineFromAddr64(hproc, pc, ctypes.byref(d32), ctypes.byref(line))
        name = sym.Name.decode(errors="replace") if ok_s else "?"
        where = f"{short(line.FileName.decode(errors='replace'))}:{line.LineNumber}" if ok_l else "?"
        frames.append((name, where))
    return frames


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--interval-ms", type=float, default=1.0)
    ap.add_argument("--top", type=int, default=20)
    ap.add_argument("--focus", default="src/lang/")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    exe = os.path.basename(args.cmd[0]).lower()
    start = time.perf_counter()
    hproc, stacks = sample(args.cmd, args.interval_ms / 1000)
    wall = time.perf_counter() - start
    mod = dbg.SymGetModuleBase64
    cache = {}

    def frames_of(pc, is_return):
        key = (pc, is_return)
        if key not in cache:
            # A return address points after the call; look up the call instruction itself.
            cache[key] = symbolize(hproc, pc - 1 if is_return else pc)
        return cache[key]

    # Worker samples: stacks with a frame under --focus. This drops idle threads (the main thread
    # waits in join) but keeps time spent in system DLLs (allocator, page faults) on behalf of
    # the checker; such leaves are named with dbghelp's export symbols.
    modules = {}
    enum_cb = ctypes.WINFUNCTYPE(wt.BOOL, ctypes.c_char_p, ctypes.c_uint64, ctypes.c_void_p)(
        lambda name, base, _ctx: modules.__setitem__(base, name.decode(errors="replace").lower()) or True
    )
    dbg.SymEnumerateModules64(hproc, enum_cb, None)
    own, leaf_mod = [], collections.Counter()
    self_fn, self_line, incl = collections.Counter(), collections.Counter(), collections.Counter()
    for st in stacks:
        expanded = []
        for i, pc in enumerate(st):
            expanded += frames_of(pc, i > 0)
        if not any(args.focus in where for _, where in expanded):
            continue
        own.append(st)
        leaf_mod[modules.get(mod(hproc, st[0]), "?")] += 1
        name, where = expanded[0]
        self_fn[name] += 1
        self_line[(where, name)] += 1
        seen = set()
        for name, where in expanded:
            if args.focus in where and where not in seen:
                seen.add(where)
                incl[(where, name)] += 1
    if not own:
        # Seen in about one run in four: the stacks lack the binary's frames. Running again helps.
        print("warning: no sample reached a frame under --focus; run again")
    n = max(len(own), 1)
    print(f"wall {wall:.2f}s, {len(stacks)} thread samples, {len(own)} on the checking thread "
          f"(a frame under {args.focus})")
    print("leaf module: " + ", ".join(f"{m} {100 * c / n:.1f}%" for m, c in leaf_mod.most_common()))
    print("\nself, by function:")
    for name, c in self_fn.most_common(args.top):
        print(f"{c:7d} {100 * c / n:5.1f}%  {name[:110]}")
    print("\nself, by source line:")
    for (where, name), c in self_line.most_common(args.top):
        print(f"{c:7d} {100 * c / n:5.1f}%  {where:34} {name[:70]}")
    print(f"\ninclusive, by source line under {args.focus}:")
    for (where, name), c in incl.most_common(args.top):
        print(f"{c:7d} {100 * c / n:5.1f}%  {where:34} {name[:70]}")


if __name__ == "__main__":
    main()
