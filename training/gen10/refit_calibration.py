#!/usr/bin/env python3
"""Refit the shipped eval-calibration curve on a large, clean label set.

The shipped curve (target-cvs/eval-cal-20260911.json) was fitted on 1,500 positions.
Training/gen10's static-distillation corpus carries Stockfish's own static eval for
millions of positions, so the same curve can be fitted far more precisely -- a
cheap, architecture-free accuracy gain for the *shipped* evaluator.

Curve fit: monotone piecewise-linear map on |raw| (sign restored on read, so the eval
stays exactly antisymmetric), fitted on the odd half and scored on the even half.

Run:  python training/gen10/refit_calibration.py --corpus training/gen10/corpus-static \
          --positions 120000 --out target-cvs/eval-cal-refit.json
"""
from __future__ import annotations

import argparse
import glob
import json
import statistics
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EDGES = [0, 20, 40, 60, 80, 100, 125, 150, 180, 215, 255, 300, 355, 420, 500, 600,
         720, 860, 1030, 1240, 1500, 1820, 2200, 2660, 3200]


def fit(pairs, min_bin=25):
    pts = []
    for i in range(len(EDGES) - 1):
        b = [(abs(p), abs(l)) for l, p in pairs if EDGES[i] <= abs(p) < EDGES[i + 1]]
        if len(b) >= min_bin:
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
    (x0, y0), (x1, y1) = c[-2], c[-1]
    slope = (y1 - y0) / (x1 - x0) if x1 > x0 else 1.0
    return s * (y1 + slope * (ax - x1))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=str(REPO / "training/gen10/corpus-static"))
    ap.add_argument("--positions", type=int, default=120000)
    ap.add_argument("--stride", type=int, default=13)
    ap.add_argument("--exe", default=str(REPO / "target-sf3/release/analyze.exe"))
    ap.add_argument("--flags", default=None)
    ap.add_argument("--out", default=str(REPO / "target-cvs/eval-cal-refit.json"))
    ap.add_argument("--report", type=int, default=1)
    ap.add_argument("--source", choices=("corpus", "shards"), default="shards",
                    help="corpus: reuse the static-distillation labels (quiet-filtered, so "
                         "narrow); shards: sample the raw shard corpus and label a few "
                         "thousand positions live with SF's static eval (full spread, which "
                         "is what the curve's tails need)")
    a = ap.parse_args(argv)

    flags = a.flags.split() if a.flags else [
        "--nnue", str(REPO / "target-cvs/matrix-raw.json"),
        "--base", "f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json",
        "--rung2", "f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json",
    ]
    # sample positions and their Stockfish-static labels
    labels: dict[str, float] = {}
    want = a.positions
    if a.source == "shards":
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from build_corpus import Engine as _Eng
        eng = _Eng(3000)
        for f in sorted(glob.glob(str(REPO / "training/gen9/gen9-cvs/shard-*.jsonl"))):
            with open(f, encoding="utf-8") as fd:
                for i, line in enumerate(fd):
                    if i % a.stride:
                        continue
                    fen = json.loads(line).get("fen", "")
                    if not fen or fen in labels:
                        continue
                    st = eng.static_eval(fen)
                    if st is None:
                        continue
                    labels[fen] = st
                    if len(labels) >= want:
                        break
            if len(labels) >= want:
                break
        eng.stop()
    for f in ([] if a.source == "shards" else sorted(glob.glob(str(Path(a.corpus) / "part-*.jsonl")))):
        with open(f, encoding="utf-8") as fd:
            for i, line in enumerate(fd):
                if i % a.stride:
                    continue
                j = json.loads(line)
                labels[j["fen"]] = j["cp"]          # White POV static label
                if len(labels) >= want:
                    break
        if len(labels) >= want:
            break
    fens = list(labels)
    print(f"sampled {len(fens)} labelled positions", flush=True)

    p = subprocess.Popen([a.exe, "--serve", "--depth", "1"] + flags,
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, bufsize=1)
    pairs = []
    for fen in fens:
        p.stdin.write(json.dumps({"cmd": "eval", "fen": fen}) + "\n")
        p.stdin.flush()
        line = p.stdout.readline()
        if not line:
            break
        j = json.loads(line)
        v = j.get("nnueStmCp")
        if v is None:
            continue
        stm = fen.split()[1]
        label_stm = labels[fen] if stm == "w" else -labels[fen]
        pairs.append((float(v), float(label_stm)))
    p.stdin.write("quit\n"); p.stdin.flush(); p.kill()
    print(f"collected {len(pairs)} (raw eval, label) pairs")

    odd, even = pairs[0::2], pairs[1::2]
    def mae(data, f): return statistics.mean(abs(f(p) - l) for p, l in data)
    identity = mae(even, lambda v: v)
    shipped = None
    shipped_path = REPO / "target-cvs/eval-cal-20260911.json"
    if shipped_path.exists():
        sp = [tuple(pt) for pt in json.loads(shipped_path.read_text())["points"]]
        shipped = mae(even, lambda v: apply(sp, v))
    curve = fit(odd)
    refit = mae(even, lambda v: apply(curve, v))
    print(f"\nholdout MAE (n={len(even)}):")
    print(f"  raw (no curve)   : {identity:6.1f}")
    if shipped is not None:
        print(f"  shipped curve    : {shipped:6.1f}")
    print(f"  refitted curve   : {refit:6.1f}")
    if a.report:
        Path(a.out).write_text(json.dumps(
            {"name": "eval-cal-refit", "fitted_on": f"{len(odd)} positions",
             "holdout_mae": round(refit, 1), "shipped_curve_holdout_mae": None if shipped is None else round(shipped, 1),
             "raw_holdout_mae": round(identity, 1),
             "points": [[round(x, 1), round(y, 1)] for x, y in curve]}, indent=1) + "\n", encoding="utf-8")
        print(f"wrote {a.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
