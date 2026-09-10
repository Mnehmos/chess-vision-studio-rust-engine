#!/usr/bin/env python3
"""Flagship anchor: CVS vs native Stockfish at pinned UCI_Elo, run in cutechess-cli.

Answers "what Elo is the engine playing at?" against LOCAL native Stockfish (never
WASM), at a fixed time control, over a diverse EPD book. Each book position is played
twice with colours swapped; the book slice is consumed sequentially so no position
repeats inside one run.

For each anchor the script reports the score, a 95% CI, the implied Elo difference, and
the anchor+delta point estimate. The 50% crossing is interpolated across anchors.

Usage:
  python bench_anchor.py --exe target/release/uci.exe \
      --net target-cvs/matrix-raw.json --helper target-cvs/matrix-residual.json \
      --book benchmarks/suites/openings-inv1-20260910.epd --positions 100 \
      --anchors 2000,2200,2400,2600 --games-per-anchor 200 --tc 10+0.1 --conc 10
"""
from __future__ import annotations

import argparse
import math
import statistics
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import match_fixed_nodes as M  # noqa: E402

STOCKFISH = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"
CUTECHESS = M.CUTECHESS


def score_to_elo(score: float) -> float:
    """Logistic Elo difference from a score in (0,1)."""
    if score <= 0.0 or score >= 1.0:
        return float("inf") if score >= 1.0 else float("-inf")
    return -400.0 * math.log10(1.0 / score - 1.0)


def wilson(score: float, n: int, z: float = 1.96) -> tuple[float, float]:
    denom = 1 + z * z / n
    c = (score + z * z / (2 * n)) / denom
    half = z * math.sqrt(score * (1 - score) / n + z * z / (4 * n * n)) / denom
    return max(0.0, c - half), min(1.0, c + half)


def run_anchor(args, anchor: int, positions: list[str], out_dir: Path) -> dict:
    pgn = out_dir / f"anchor-{anchor}.pgn"
    book = out_dir / f"book-{anchor}.epd"
    book.write_text("\n".join(positions) + "\n", encoding="utf-8")
    cmd = (
        [CUTECHESS,
         "-engine", "name=CVS", f"cmd={args.exe}",
         *[f"arg={a}" for a in args.flags],
         "arg=--nnue", f"arg={args.net}",
         *(["arg=--helper-nnue", f"arg={args.helper}"] if args.helper else []),
         "-engine", "name=SF", f"cmd={STOCKFISH}",
         "option.UCI_LimitStrength=true", f"option.UCI_Elo={anchor}",
         "-each", "proto=uci", f"tc={args.tc}",
         "-games", str(args.games_per_anchor), "-repeat",
         "-concurrency", str(args.conc),
         "-openings", f"file={book}", "format=epd", "order=sequential",
         "-draw", "movenumber=40", "movecount=8", "score=10",
         "-resign", "movecount=4", "score=900",
         "-maxmoves", "200",
         "-pgnout", str(pgn)]
    )
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(f"[anchor {anchor}] cutechess exit {proc.returncode}\n{proc.stderr[-800:]}\n")
    text = pgn.read_text(encoding="utf-8", errors="replace") if pgn.exists() else ""
    rows = M.pgn_to_results(text, "CVS")
    w = sum(1 for r in rows if (r["result"] == "1-0") == (r["candidateColor"] == "white"))
    l = sum(1 for r in rows if r["result"] != "1/2-1/2" and (r["result"] == "1-0") != (r["candidateColor"] == "white"))
    d = len(rows) - w - l
    n = len(rows)
    score = (w + 0.5 * d) / n if n else 0.0
    lo, hi = wilson(score, n) if n else (0.0, 0.0)
    return {"anchor": anchor, "games": n, "wins": w, "losses": l, "draws": d,
            "score": score, "elo": score_to_elo(score) if n else None,
            "elo_lo": score_to_elo(lo) if n else None, "elo_hi": score_to_elo(hi) if n else None}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", required=True)
    ap.add_argument("--net", required=True)
    ap.add_argument("--helper", default=None)
    ap.add_argument("--flags", default="--futility --rfp --tt-prune-store --qtt --histmalus --histlmr --lmp --smarttime")
    ap.add_argument("--book", required=True)
    ap.add_argument("--positions", type=int, default=100)
    ap.add_argument("--anchors", default="2000,2200,2400,2600")
    ap.add_argument("--games-per-anchor", type=int, default=200)
    ap.add_argument("--tc", default="10+0.1")
    ap.add_argument("--conc", type=int, default=10)
    ap.add_argument("--out-dir", default="benchmarks/results/anchor-20260910")
    args = ap.parse_args(argv)
    args.flags = args.flags.split()
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    book = [ln.strip() for ln in Path(args.book).read_text(encoding="utf-8").splitlines() if ln.strip()]
    if len(book) < args.positions:
        raise SystemExit(f"book has {len(book)} positions, need {args.positions}")
    positions = book[: args.positions]

    print(f"CVS flagship anchor vs native Stockfish (UCI_LimitStrength) — tc={args.tc}, "
          f"{args.games_per_anchor} games/anchor over {len(positions)} distinct positions")
    results = []
    for anchor in [int(a) for a in args.anchors.split(",")]:
        r = run_anchor(args, anchor, positions, out)
        results.append(r)
        print(f"  SF{anchor}: +{r['wins']} -{r['losses']} ={r['draws']} ({r['games']}g) "
              f"score {100*r['score']:.1f}%  Elo {r['elo']:+.0f} "
              f"[{r['elo_lo']:+.0f},{r['elo_hi']:+.0f}]  => strength ~{anchor + r['elo']:.0f}",
              flush=True)

    # Interpolate the 50% crossing across anchors (linear in Elo space on the score curve).
    pts = sorted((r["anchor"] + r["elo"], r["score"]) for r in results if r["games"])
    est = None
    for (e1, s1), (e2, s2) in zip(pts, pts[1:]):
        if (s1 - 0.5) * (s2 - 0.5) <= 0 and s1 != s2:
            est = e1 + (0.5 - s1) * (e2 - e1) / (s2 - s1)
            break
    if est is None and pts:
        est = pts[0][0] if pts[0][1] < 0.5 else pts[-1][0]
    print(f"\nEstimated flagship strength vs native Stockfish @ {args.tc}: ~{est:.0f} Elo"
          if est else "\nNo crossing found (all scores on one side of 50%)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
