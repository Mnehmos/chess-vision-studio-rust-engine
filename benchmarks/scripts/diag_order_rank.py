#!/usr/bin/env python3
"""Ordering-quality diagnostic: where does the true best move sit in our ordering?

Movecount pruning is only safe when the good moves are ordered to the front. For each
position we take native Stockfish's top-K moves (deep MultiPV) as the reference and
report the rank of each inside our engine's root move order (`rootOrder` telemetry at a
fixed node budget, cold isolation). A fat tail (ranks > 15) means the "quiet tail" our
pruning would skip still contains the best move — which is exactly when LMP-style
pruning loses strength.

Usage:
  python diag_order_rank.py --positions 40 [--budget 40000] [--sf-depth 18]
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SF = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"


def ours_root_order(exe: str, args: list[str], fen: str, budget: int) -> tuple[list[str], int]:
    req = json.dumps({"cmd": "go", "fen": fen, "nodeBudget": budget,
                      "diagnosticIsolation": "cold"}) + "\n"
    p = subprocess.run([exe, "--serve", "--depth", "40"] + args, input=req,
                       capture_output=True, text=True, timeout=300)
    for line in p.stdout.splitlines():
        if line.startswith("{"):
            d = json.loads(line)
            return d.get("rootOrder") or [], d.get("depth") or 0
    raise RuntimeError(f"no json: {p.stdout[-200:]} {p.stderr[-200:]}")


def sf_topk(exe: str, fen: str, depth: int, k: int) -> list[str]:
    p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=1)
    p.stdin.write("uci\n"); p.stdin.flush()
    while p.stdout.readline().strip() != "uciok":
        pass
    p.stdin.write("setoption name MultiPV value %d\n" % k)
    p.stdin.write(f"position fen {fen}\ngo depth {depth}\n"); p.stdin.flush()
    best: dict[int, str] = {}
    while True:
        line = p.stdout.readline()
        if not line or line.startswith("bestmove"):
            break
        if line.startswith("info") and " pv " in line and " multipv " in line:
            parts = line.split()
            mpv = int(parts[parts.index("multipv") + 1])
            pv = parts[parts.index("pv") + 1]
            if "depth" in parts and int(parts[parts.index("depth") + 1]) == depth:
                best[mpv] = pv
    p.stdin.write("quit\n"); p.stdin.flush(); p.kill()
    return [best[i] for i in sorted(best)]


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=str(REPO / "target/release/analyze.exe"))
    ap.add_argument("--args", default="")
    ap.add_argument("--fens-file", default=str(REPO / "benchmarks/suites/openings-inv1-20260910.epd"))
    ap.add_argument("--positions", type=int, default=40)
    ap.add_argument("--stride", type=int, default=97, help="take every Nth book line")
    ap.add_argument("--budget", type=int, default=40000)
    ap.add_argument("--sf-depth", type=int, default=18)
    ap.add_argument("--sf", default=SF)
    a = ap.parse_args(argv)

    lines = [ln.strip() for ln in Path(a.fens_file).read_text().splitlines() if ln.strip()]
    fens = [" ".join(ln.split()[:4]) for ln in lines][:: a.stride][: a.positions]
    args = a.args.split() if a.args else []

    ranks: list[int] = []
    top1_in_10 = 0
    for i, fen in enumerate(fens):
        order, depth = ours_root_order(a.exe, args, fen, a.budget)
        ref = sf_topk(a.sf, fen, a.sf_depth, 3)
        if not order or not ref:
            continue
        r = [order.index(m) + 1 if m in order else -1 for m in ref]
        ranks.append(r[0])
        if r[0] != -1 and r[0] <= 10:
            top1_in_10 += 1
        print(f"{i:3d} d{depth} SF#1 rank {r[0]:3d} | top3 ranks {r}  (order len {len(order)})")
    if ranks:
        valid = [x for x in ranks if x > 0]
        print(f"\nSF#1 rank in our order: mean {statistics.mean(valid):.1f}, "
              f"median {statistics.median(valid):.0f}, max {max(valid)}; "
              f"<=10 in {top1_in_10}/{len(ranks)} positions")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
