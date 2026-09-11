#!/usr/bin/env python3
"""Search-efficiency KPI: nodes-to-depth, ours vs native Stockfish.

The currency of search quality is *depth per node* (equivalently: nodes to reach a
nominal depth). This measures both engines on the same positions:

  * nodes-to-depth curve: nodes to complete depth 6..N (median over positions)
  * depth-at-budget: the average depth reached inside a fixed node budget
    (default 40k — the gate control), the headline "avg depth per node" number.

Nominal depth is not comparable across engines (each counts its own extensions and
reductions), so the nodes-to-depth curve is the honest structural KPI and the
depth-at-budget is the practical one. Game strength at equal nodes is the referee.

Usage:
  python bench_nodes_to_depth.py --exe target/release/uci.exe --net target-cvs/matrix-raw.json \
      --depths 6,8,10,12,14 --budget 40000 [--sf] [--positions 10]
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

SF = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"

DEFAULT_FENS = [
    "r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
    "r2qk2r/ppp2ppp/2nbbn2/3pp3/1P5P/P2P1NP1/2PQPPB1/RNB1K2R b KQkq - 0 1",
    "rnbqkb1r/pp3ppp/2p1pn2/3p4/2PP4/2N1PN2/PP3PPP/R1BQKB1R b KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "2r3k1/pp3pp1/2n1b2p/3p4/3P4/1BN1P2P/PP3PP1/2R3K1 w - - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "rnbqkbnr/pp1ppppp/8/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 1",
    "2kr3r/pp1q1ppp/2n1pn2/2b5/8/2N1PN2/PPQ2PPP/2KR3R w - - 0 1",
]


def run_go(exe: str, args: list[str], fen: str, *, depth: int | None = None,
           nodes: int | None = None) -> dict:
    p = subprocess.Popen([exe] + args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, bufsize=1)
    p.stdin.write("uci\n")
    p.stdin.flush()
    while p.stdout.readline().strip() != "uciok":
        pass
    p.stdin.write(f"position fen {fen}\n")
    p.stdin.write(f"go depth {depth}\n" if depth is not None else f"go nodes {nodes}\n")
    p.stdin.flush()
    out = {}
    while True:
        line = p.stdout.readline()
        if not line:
            break
        if line.startswith("info") and " pv " in line:
            parts = line.split()
            for key in ("depth", "nodes", "time"):
                if key in parts:
                    try:
                        out[key] = int(parts[parts.index(key) + 1])
                    except ValueError:
                        pass
                if key == "nodes":
                    out["last_nodes"] = out["nodes"]
        if line.startswith("bestmove"):
            break
    p.stdin.write("quit\n")
    p.stdin.flush()
    p.kill()
    return out


def nodes_to_depth(exe: str, args: list[str], fens: list[str], depths: list[int]) -> dict:
    """Median nodes to complete each depth (re-running the search per depth)."""
    curve: dict[int, list[int]] = {d: [] for d in depths}
    for fen in fens:
        for d in depths:
            r = run_go(exe, args, fen, depth=d)
            if r.get("depth") == d and r.get("nodes"):
                curve[d].append(r["nodes"])
    return {d: (statistics.median(v) if v else None) for d, v in curve.items()}


def depth_at_budget(exe: str, args: list[str], fens: list[str], budget: int) -> tuple[float, float]:
    """Average depth reached (and nodes used) inside a fixed node budget."""
    depths, used = [], []
    for fen in fens:
        r = run_go(exe, args, fen, nodes=budget)
        if r.get("depth"):
            depths.append(r["depth"])
            used.append(r.get("nodes", budget))
    return (statistics.mean(depths) if depths else 0.0,
            statistics.mean(used) if used else 0.0)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", required=True)
    ap.add_argument("--args", default="")
    ap.add_argument("--net", default=None)
    ap.add_argument("--sf", default=SF)
    ap.add_argument("--compare-sf", action="store_true")
    ap.add_argument("--depths", default="6,8,10,12,14")
    ap.add_argument("--budget", type=int, default=40000)
    ap.add_argument("--positions", type=int, default=8)
    ap.add_argument("--fens-file", default=None)
    ap.add_argument("--json", default=None)
    a = ap.parse_args(argv)

    fens = DEFAULT_FENS[: a.positions]
    if a.fens_file:
        fens = [ln.strip() for ln in Path(a.fens_file).read_text().splitlines() if ln.strip()][: a.positions]
    depths = [int(x) for x in a.depths.split(",")]
    ours_args = ([f"--nnue", a.net] if a.net else []) + a.args.split()
    sf_args: list[str] = []

    print(f"nodes-to-depth (median over {len(fens)} positions), budget {a.budget}")
    print(f"{'depth':>5s} {'ours':>12s} {'SF':>12s} {'ratio':>7s}")
    ours = nodes_to_depth(a.exe, ours_args, fens, depths)
    theirs = nodes_to_depth(a.sf, sf_args, fens, depths) if a.compare_sf else {}
    for d in depths:
        o, t = ours.get(d), theirs.get(d)
        ratio = f"{o/t:6.1f}x" if o and t else ""
        print(f"{d:5d} {o if o else '-':>12} {t if t else '-':>12} {ratio:>7s}")

    od, ou = depth_at_budget(a.exe, ours_args, fens, a.budget)
    print(f"\nat {a.budget} nodes: ours avg depth {od:.1f} (used {ou:.0f} nodes)")
    if a.compare_sf:
        sd, su = depth_at_budget(a.sf, sf_args, fens, a.budget)
        print(f"at {a.budget} nodes: SF   avg depth {sd:.1f} (used {su:.0f} nodes)  ratio {od/sd if sd else 0:.2f}")
    if a.json:
        Path(a.json).write_text(json.dumps({"ours": ours, "sf": theirs,
                                            "depthAtBudget": {"ours": od, "sf": (sd if a.compare_sf else None)}},
                                           indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
