#!/usr/bin/env python3
"""Fit an output calibration curve on half a dump, score the other half.

Each candidate evaluator is judged on the same terms: raw MAE against Stockfish's
static eval, plus MAE after a monotone piecewise-linear calibration fitted on the
odd rows and measured on the even rows (never the same rows it was fitted on).
The incumbent reference is the shipped net + its shipped curve: 85.4 MAE.

Input dump rows come from benchmarks/scripts/bench_eval_quality.py --dump:
    {"fen", "label_stm", "nnue_stm", "classic_white"}

Run:  python training/gen10/fit_and_score.py dump.jsonl [--write-cal out.json]
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path

EDGES = [0, 25, 50, 75, 100, 140, 180, 230, 290, 360, 450, 560, 700, 900, 1200, 1600, 2200, 3000]


def fit(pairs):
    pts = []
    for i in range(len(EDGES) - 1):
        b = [(abs(p), abs(l)) for l, p in pairs if EDGES[i] <= abs(p) < EDGES[i + 1]]
        if len(b) >= 8:
            pts.append((statistics.median(x for x, _ in b), statistics.median(y for _, y in b)))
    pts = sorted([(0.0, 0.0)] + pts)
    out, last = [], 0.0
    for x, y in pts:
        y = max(y, last)
        last = y
        out.append((x, y))
    return out


def apply(c, x):
    ax, s = abs(x), (1 if x >= 0 else -1)
    for i in range(1, len(c)):
        x0, y0 = c[i - 1]
        x1, y1 = c[i]
        if ax <= x1:
            t = (ax - x0) / (x1 - x0) if x1 > x0 else 0.0
            return s * (y0 + t * (y1 - y0))
    return s * x


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--write-cal", default=None)
    ap.add_argument("--exclude-corpus", default=None,
                    help="glob of corpus part files; dump rows whose FEN appears there are "
                         "dropped (a net must not be scored on positions it trained on)")
    a = ap.parse_args(argv)
    rows = [json.loads(l) for l in Path(a.dump).read_text(encoding="utf-8").splitlines()]
    if a.exclude_corpus:
        import glob as _g
        seen = set()
        for f in _g.glob(a.exclude_corpus):
            with open(f, encoding="utf-8") as fd:
                for line in fd:
                    try:
                        seen.add(json.loads(line)["fen"])
                    except (json.JSONDecodeError, KeyError):
                        pass
        before = len(rows)
        rows = [r for r in rows if r["fen"] not in seen]
        print(f"excluded {before-len(rows)}/{before} dump rows present in the training corpus")
    data = [(r["nnue_stm"], r["label_stm"]) for r in rows if r.get("nnue_stm") is not None]
    odd, even = data[0::2], data[1::2]
    from statistics import mean
    raw = mean(abs(p - l) for l, p in [(l, p) for p, l in even])
    curve = fit(odd)
    cal = mean(abs(apply(curve, p) - l) for p, l in even)
    mx, my = mean([p for p, _ in even]), mean([l for _, l in even])
    cov = sum((p - mx) * (l - my) for p, l in even)
    vx = sum((p - mx) ** 2 for p, _ in even)
    slope = cov / vx if vx else 0.0
    print(f"n={len(even)}  raw MAE {raw:.1f}  calibrated MAE {cal:.1f}  slope {slope:.3f}")
    print(f"(incumbent reference: calibrated MAE 85.4)")
    if a.write_cal:
        Path(a.write_cal).write_text(json.dumps({"points": [[round(x, 1), round(y, 1)] for x, y in curve]}, indent=1) + "\n")
        print(f"wrote curve -> {a.write_cal}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
