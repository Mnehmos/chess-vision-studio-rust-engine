#!/usr/bin/env python3
"""P-SEARCH probe (issue #8): the marginal value of compute under the current eval.

Runs the SAME engine / net / options / openings at several FIXED NODE budgets (via #6's
`go nodes N`) on a frozen position suite, COLD (a fresh engine process per probe), and reports
how the decision changes as the node budget grows:

  * moveChangeRate = fraction of positions whose best move takes >1 distinct value across the
    budgets = the marginal value of compute. High -> more search currently buys different
    decisions; low -> search is saturated under this eval. (Counts ANY change, so an A->B->A
    oscillation is correctly flagged unstable, not "settled".)
  * oscillationRate = positions that changed and returned to the first move.
  * settledAtBudget = the budget of the LAST move change (after which the move is stable).
  * per-budget score and depth trajectories, and the score/depth gain from 1x to Nx.

INV-2: single-thread, fixed nodes, cold isolation, recorded engine/net/options/positions hashes.
Interpretation discipline (issue #8): neither a large nor a small gain alone proves the eval is
good or bad — P-SEARCH only locates whether *compute* is the slack resource right now. A full
SPRT match + Elo gradient between budgets is a follow-up slice (it reuses #5's SPRT runner).

The engine call is injected (`engine_fn(fen, nodes) -> dict`) so the aggregation is unit-tested
without a binary; the CLI wires in a real cold uci.exe runner (`cold_uci_engine_fn`).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

# Mate scores map to a large signed sentinel so a position that becomes a forced mate at a
# higher budget shows the score gain (matches the engine's MATE_SCORE magnitude).
MATE_CP = 1_000_000


def _sha256_16(path) -> str | None:
    try:
        return hashlib.sha256(Path(path).read_bytes()).hexdigest()[:16]
    except Exception:
        return None


def parse_uci_output(text: str) -> dict:
    """Parse an engine's stdout for a single `go` into {bestMove, scoreCp, nodes, depth}.

    Robust to the review's findings: skips `info string ...` (F2), only reads score/depth/nodes
    from the principal variation `multipv 1` (or non-multipv) lines (F3), and maps `score mate N`
    to a signed sentinel (F4). Carries forward the last seen value per key.
    """
    best, score, nodes, depth = "0000", None, 0, 0
    for line in text.splitlines():
        toks = line.split()
        if not toks:
            continue
        if toks[0] == "bestmove":
            best = toks[1] if len(toks) > 1 else "0000"
            continue
        if toks[0] != "info" or (len(toks) > 1 and toks[1] == "string"):
            continue  # not a PV info line (F2: skip `info string`)
        if "multipv" in toks:
            try:
                if int(toks[toks.index("multipv") + 1]) != 1:
                    continue  # F3: only the best PV carries the authoritative score
            except (ValueError, IndexError):
                pass
        try:
            if "depth" in toks:
                depth = int(toks[toks.index("depth") + 1])
            if "nodes" in toks:
                nodes = int(toks[toks.index("nodes") + 1])
            if "score" in toks:
                si = toks.index("score")
                kind = toks[si + 1]
                if kind == "cp":
                    score = int(toks[si + 2])
                elif kind == "mate":
                    m = int(toks[si + 2])
                    score = (MATE_CP - abs(m)) * (1 if m >= 0 else -1)
        except (ValueError, IndexError):
            pass
    return {"bestMove": best, "scoreCp": score, "nodes": nodes, "depth": depth}


def run_p_search(positions, budgets, engine_fn) -> dict:
    """positions: list[FEN]. budgets: node budgets (asc). engine_fn(fen, nodes) ->
    {"bestMove": str, "scoreCp": int|None, "nodes": int, "depth": int}."""
    budgets = sorted(set(int(b) for b in budgets))
    per_position = []
    unstable_count = 0
    oscillated_count = 0
    for fen in positions:
        traj = [dict(budget=b, **engine_fn(fen, b)) for b in budgets]
        moves = [t["bestMove"] for t in traj]
        distinct = len(set(moves))
        unstable = distinct > 1  # ANY change across budgets (catches A->B->A oscillation)
        oscillated = unstable and moves[0] == moves[-1]
        settled_idx = 0
        for i in range(1, len(moves)):
            if moves[i] != moves[i - 1]:
                settled_idx = i
        unstable_count += 1 if unstable else 0
        oscillated_count += 1 if oscillated else 0
        s0 = traj[0]["scoreCp"] if traj else None
        s1 = traj[-1]["scoreCp"] if traj else None
        per_position.append(
            {
                "fen": fen,
                "trajectory": traj,
                "distinctMoves": distinct,
                "unstable": unstable,
                "oscillated": oscillated,
                "moveChangedSmallestToLargest": len(moves) > 1 and moves[0] != moves[-1],
                "settledAtBudget": budgets[settled_idx] if budgets else None,
                "scoreDeltaCp": (s1 - s0) if (s0 is not None and s1 is not None) else None,
                "depthDelta": (traj[-1]["depth"] - traj[0]["depth"]) if traj else 0,
            }
        )
    n = len(positions) or 1
    settled = [p["settledAtBudget"] for p in per_position if p["settledAtBudget"] is not None]
    return {
        "schemaVersion": 1,
        "probe": "P-SEARCH",
        "budgets": budgets,
        "positions": len(positions),
        "aggregate": {
            "moveChangeRate": round(unstable_count / n, 4),
            "oscillationRate": round(oscillated_count / n, 4),
            "meanSettledBudget": round(sum(settled) / len(settled), 1) if settled else None,
            "meanDepthGain": round(sum(p["depthDelta"] for p in per_position) / n, 2),
        },
        "perPosition": per_position,
        "interpretation": (
            "moveChangeRate is the marginal value of compute under the CURRENT eval; it does "
            "not by itself prove the eval is good or bad. See issue #8."
        ),
    }


def cold_uci_engine_fn(uci_path, net=None, flags=None):
    """An engine_fn that spawns a FRESH uci.exe per probe (empty TT = cold isolation) and runs
    `go nodes N`. Requires a uci.exe that supports `go nodes N` (issue #6 / PR #14)."""
    flags = list(flags or [])
    base = [uci_path] + (["--nnue", net] if net else []) + flags

    def fn(fen, nodes):
        try:
            proc = subprocess.run(
                base,
                input=f"uci\nisready\nposition fen {fen}\ngo nodes {nodes}\nquit\n",
                capture_output=True,
                text=True,
                timeout=120,
            )
        except subprocess.TimeoutExpired:
            # One pathological position must not lose the whole suite (review F5).
            return {"bestMove": "0000", "scoreCp": None, "nodes": 0, "depth": 0}
        return parse_uci_output(proc.stdout)

    return fn


def main(argv) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--uci", required=True, help="path to a uci.exe supporting `go nodes N` (#6)")
    ap.add_argument("--net")
    ap.add_argument("--positions", required=True, help="file with one FEN per line (# = comment)")
    ap.add_argument("--nodes", default="10000,80000", help="comma-separated node budgets")
    ap.add_argument("--flag", action="append", default=[], help="extra engine flag (repeatable)")
    ap.add_argument("--out")
    args = ap.parse_args(argv[1:])

    raw = Path(args.positions).read_text(encoding="utf-8")
    positions = [ln.strip() for ln in raw.splitlines() if ln.strip() and not ln.strip().startswith("#")]
    budgets = [int(x) for x in args.nodes.split(",")]
    report = run_p_search(positions, budgets, cold_uci_engine_fn(args.uci, args.net, args.flag))
    report["provenance"] = {
        "engineSha": _sha256_16(args.uci),
        "netSha": _sha256_16(args.net) if args.net else None,
        "positionsSha": hashlib.sha256("\n".join(positions).encode("utf-8")).hexdigest()[:16],
        "flags": args.flag,
        "threads": 1,
        "isolation": "cold",
    }
    out = json.dumps(report, indent=2)
    if args.out:
        Path(args.out).write_text(out + "\n", encoding="utf-8")
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
