#!/usr/bin/env python3
"""Peak working-set (RSS) of a baseline/candidate pair under an identical search load.

INV-2 requires a performance-only change to prove the speedup was not bought with memory.
This runs the same fixed-node searches through both engines and reads each child process's
PeakWorkingSetSize (Windows PROCESS_MEMORY_COUNTERS) while it is still alive.

Usage:
  python bench_peak_rss.py --baseline B.exe --candidate C.exe --net NET.json \
      [--nodes 320000] [--positions 6] [--out peak-rss.json]
"""
from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt
import json
from pathlib import Path

import benchlib as B

ROOT = Path(__file__).resolve().parents[2]
FLAGS = ["--futility", "--rfp", "--tt-prune-store", "--qtt", "--histmalus", "--histlmr",
         "--no-lmp", "--no-seeprune", "--no-delta", "--no-countermove", "--no-conthist",
         "--no-rule50", "--no-caphist", "--no-tt2", "--no-improving", "--no-king-activity",
         "--no-singular", "--no-syzygy", "--no-book", "--cvs-helpers", "0"]


class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD),
        ("PageFaultCount", wt.DWORD),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def peak_working_set(pid: int) -> int:
    PROCESS_QUERY_INFORMATION = 0x0400
    PROCESS_VM_READ = 0x0010
    handle = ctypes.windll.kernel32.OpenProcess(
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not handle:
        raise OSError(f"OpenProcess failed for pid {pid}")
    try:
        counters = PROCESS_MEMORY_COUNTERS()
        counters.cb = ctypes.sizeof(counters)
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(
            handle, ctypes.byref(counters), counters.cb)
        if not ok:
            raise OSError("GetProcessMemoryInfo failed")
        return int(counters.PeakWorkingSetSize)
    finally:
        ctypes.windll.kernel32.CloseHandle(handle)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--net", required=True)
    ap.add_argument("--nodes", type=int, default=320000)
    ap.add_argument("--positions", type=int, default=6)
    ap.add_argument("--out")
    args = ap.parse_args()

    canonical = json.loads((ROOT / "benchmarks/suites/canonical.json").read_text())["positions"]
    fens = [r["fen"] for r in canonical][: args.positions]
    peaks = {}
    for name, exe in (("baseline", args.baseline), ("candidate", args.candidate)):
        cfg = B.engine_cfg(name, exe=exe, net=args.net, depth=64, threads=1, extra=FLAGS)
        engine = B.Engine(cfg)
        try:
            for fen in fens:
                engine.search_nodes(fen, args.nodes)
            peaks[name] = peak_working_set(engine.p.pid)
        finally:
            engine.close()
    delta_pct = round((peaks["candidate"] / peaks["baseline"] - 1.0) * 100.0, 4)
    out = {
        "nodesPerPosition": args.nodes,
        "positions": len(fens),
        "peakWorkingSetBytes": peaks,
        "peakRssDeltaPct": delta_pct,
        "binaryBytes": {
            "baseline": Path(args.baseline).stat().st_size,
            "candidate": Path(args.candidate).stat().st_size,
        },
    }
    out["binaryBytesDeltaPct"] = round(
        (out["binaryBytes"]["candidate"] / out["binaryBytes"]["baseline"] - 1.0) * 100.0, 4)
    print(json.dumps(out, indent=2))
    if args.out:
        Path(args.out).write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
