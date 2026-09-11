#!/usr/bin/env python3
"""Where do the 40k nodes go? Node-split + search-shape diagnostic.

Runs our engine's diagnostic interface (nodeBudget=40000, cold isolation) over a
position set and prints the search-shape telemetry that decides nominal depth:
q-node share, TT hit rate, first-move cutoff rate, avg cutoff index, branching.
Optionally compares native Stockfish's depth at the same node budget.

Usage:
  python diag_nodes.py --exe target/release/analyze.exe --args "--futility --rfp ..." \
      [--positions 12] [--budget 40000] [--compare-sf]
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SF = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"

FENS = [
    # opening
    "r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
    "rnbqkb1r/pp3ppp/2p1pn2/3p4/2PP4/2N1PN2/PP3PPP/R1BQKB1R b KQkq - 0 1",
    # early middlegame
    "r2qk2r/ppp2ppp/2nbbn2/3pp3/1P5P/P2P1NP1/2PQPPB1/RNB1K2R b KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "2kr3r/pp1q1ppp/2n1pn2/2b5/8/2N1PN2/PPQ2PPP/2KR3R w - - 0 1",
    # middlegame, open
    "2r3k1/pp3pp1/2n1b2p/3p4/3P4/1BN1P2P/PP3PP1/2R3K1 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "1r1q1rk1/2p1bppp/p2pbn2/1p2p3/4P3/1BN1Q1P1/PPP2PBP/R1B2RK1 w - - 0 11",
    "3rr1k1/pp3pp1/2n1b2p/q2p4/3P4/1BN1P2P/PPQ2PP1/2RR2K1 w - - 0 1",
    # sharp tactical
    "r1bq1rk1/pp2bppp/2n1pn2/2pp4/3P1B2/2PBPN2/PP1N1PPP/R2Q1RK1 b - - 0 1",
    # endgames / tactical middlegames (no TB-mate rows: SF's depth column is not
    # comparable when it finds a tablebase or forced mate)
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r2q1rk1/1p2bppp/p1npbn2/4p3/4P3/1NN1B3/PPP2PPP/R2QR1K1 w - - 0 10",
]


def ours_diag(exe: str, args: list[str], fen: str, budget: int) -> dict:
    req = json.dumps({"cmd": "go", "fen": fen, "nodeBudget": budget,
                      "diagnosticIsolation": "cold"}) + "\n"
    p = subprocess.run([exe, "--serve", "--depth", "40"] + args, input=req,
                       capture_output=True, text=True, timeout=300)
    for line in p.stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            d = json.loads(line)
            t = d.get("telemetry", {})
            return {"depth": d.get("depth"), "nodes": d.get("nodes"),
                    "qPct": t.get("qNodePct"), "ttHitPct": t.get("ttHitPct"),
                    "fmcPct": t.get("firstMoveCutoffPct"), "cutIdx": t.get("avgCutoffMoveIndex"),
                    "branch": t.get("searchedEffectiveBranching"),
                    "legal": t.get("avgLegalMoves"), "lmrPct": t.get("lmrReductions"),
                    "aspRes": t.get("aspirationResearches")}
    raise RuntimeError(f"no json from engine: {p.stdout[-300:]} {p.stderr[-300:]}")


def sf_depth(exe: str, fen: str, budget: int) -> dict:
    p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=1)
    p.stdin.write("uci\n"); p.stdin.flush()
    while p.stdout.readline().strip() != "uciok":
        pass
    p.stdin.write(f"position fen {fen}\ngo nodes {budget}\n"); p.stdin.flush()
    depth = nodes = None
    while True:
        line = p.stdout.readline()
        if not line or line.startswith("bestmove"):
            break
        if line.startswith("info") and " pv " in line:
            parts = line.split()
            if "depth" in parts:
                depth = int(parts[parts.index("depth") + 1])
            if "nodes" in parts:
                nodes = int(parts[parts.index("nodes") + 1])
    p.stdin.write("quit\n"); p.stdin.flush(); p.kill()
    return {"depth": depth, "nodes": nodes}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=str(REPO / "target/release/analyze.exe"))
    ap.add_argument("--args", default="")
    ap.add_argument("--positions", type=int, default=12)
    ap.add_argument("--budget", type=int, default=40000)
    ap.add_argument("--compare-sf", action="store_true")
    ap.add_argument("--sf", default=SF)
    a = ap.parse_args(argv)

    fens = FENS[: a.positions]
    args = a.args.split() if a.args else []
    print(f"{'pos':>3} {'depth':>5} {'qPct':>5} {'ttHit%':>6} {'fmc%':>5} {'cutIdx':>6} "
          f"{'branch':>6} {'legal':>5} {'SFd':>4}")
    rows = {"depth": [], "qPct": [], "ttHitPct": [], "fmcPct": [], "cutIdx": [],
            "branch": [], "legal": [], "sf": []}
    for i, fen in enumerate(fens):
        r = ours_diag(a.exe, args, fen, a.budget)
        s = sf_depth(a.sf, fen, a.budget) if a.compare_sf else {}
        sf = s.get("depth")
        print(f"{i:3d} {r['depth']:5} {r['qPct'] or 0:5.1f} {r['ttHitPct'] or 0:6.1f} "
              f"{r['fmcPct'] or 0:5.1f} {r['cutIdx'] or 0:6.2f} {r['branch'] or 0:6.2f} "
              f"{r['legal'] or 0:5.1f} {sf if sf else '-':>4}")
        for k in ("depth", "qPct", "ttHitPct", "fmcPct", "cutIdx", "branch", "legal"):
            if r.get(k) is not None:
                rows[k].append(r[k])
        if sf:
            rows["sf"].append(sf)
    print("\nmedians:")
    for k in ("depth", "qPct", "ttHitPct", "fmcPct", "cutIdx", "branch", "legal", "sf"):
        v = rows[k]
        if v:
            print(f"  {k:>9}: {statistics.mean(v):.2f}"
                  + (f"  (SF depth mean {statistics.mean(rows['sf']):.1f}, "
                     f"ratio {statistics.mean(rows['depth'])/statistics.mean(rows['sf']):.2f})"
                     if k == "depth" and rows["sf"] else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
