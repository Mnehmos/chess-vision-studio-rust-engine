#!/usr/bin/env python3
"""Static-eval quality: our evaluator vs the corpus' Stockfish oracle labels.

The search-efficiency campaign showed the engine is eval-limited (deep pruning is
strength-negative because the eval cannot tell which moves deserve the nodes), so
this is the instrument for the eval campaign: measure the static eval directly
against oracle labels, with a phase and magnitude breakdown that says *where* it
is wrong. No games needed, so a model change can be judged in minutes.

Labels: training/gen9/gen9-cvs/shard-*.jsonl rows carry {"fen", "cp"} where `cp`
is the Stockfish oracle score from WHITE's point of view (mate-scale rows are
excluded). Our serve `eval` returns `nnueStmCp` (side-to-move POV, NNUE) and
`evalWhiteCp` (classical/rung2). Errors are computed in the side-to-move POV.

Usage:
  python bench_eval_quality.py --exe target-sf3/release/analyze.exe \
      --flags "--nnue target-cvs/matrix-raw.json" --positions 20000
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
SHARDS = str(REPO / "training/gen9/gen9-cvs/shard-*.jsonl")


def sample_rows(patterns: list[str], want: int, stride: int) -> list[dict]:
    rows: list[dict] = []
    for pat in patterns:
        for f in sorted(glob.glob(pat)):
            with open(f, encoding="utf-8") as fd:
                for i, line in enumerate(fd):
                    if i % stride:
                        continue
                    try:
                        j = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if abs(j.get("cp", 0)) >= 3000:
                        continue  # mate-scale: not comparable to a static eval
                    rows.append(j)
                    if len(rows) >= want:
                        return rows
    return rows


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", default=str(REPO / "target-sf3/release/analyze.exe"))
    ap.add_argument("--flags", default="")
    ap.add_argument("--shards", default=SHARDS)
    ap.add_argument("--positions", type=int, default=20000)
    ap.add_argument("--stride", type=int, default=7)
    ap.add_argument("--json", default=None)
    ap.add_argument("--dump", default=None,
                    help="write per-position rows {fen,label_stm,nnue_stm,classic_white} for offline analysis")
    ap.add_argument("--reference", choices=("corpus", "sf-static"), default="corpus",
                    help="corpus = the shard's search labels; sf-static = SF's own static eval")
    ap.add_argument("--sf", default="f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe")
    a = ap.parse_args(argv)

    rows = sample_rows(a.shards.split(","), a.positions, a.stride)
    if not rows:
        raise SystemExit("no rows sampled")
    print(f"sampled {len(rows)} positions (labels: Stockfish oracle, mate rows excluded)")

    p = subprocess.Popen([a.exe, "--serve", "--depth", "1"] + (a.flags.split() if a.flags else []),
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, bufsize=1)
    errs_nnue, errs_classic = [], []
    signed_nnue, signed_classic = [], []
    # Interleave request/response: writing all requests first deadlocks once the
    # engine's replies fill the pipe buffer and it stops reading stdin.
    labels = []
    for r in rows:
        p.stdin.write(json.dumps({"cmd": "eval", "fen": r["fen"]}) + "\n")
        p.stdin.flush()
        line = p.stdout.readline()
        if not line:
            break
        j = json.loads(line)
        stm = r["fen"].split()[1]
        cp_white = j.get("evalWhiteCp")
        nnue_stm = j.get("nnueStmCp")
        label_stm = r["cp"] if stm == "w" else -r["cp"]
        labels.append((r, label_stm, nnue_stm, cp_white))
    p.stdin.write("quit\n")
    p.stdin.flush()
    p.kill()

    def sf_static(fen: str) -> float | None:
        """SF's own STATIC eval (UCI `eval` -> "Final evaluation"), white POV cp.
        Static-vs-static is the apples-to-apples comparison; corpus labels are
        SEARCH scores and include tactics a static eval cannot see. One SF process
        per position: SF's eval table is not line-flushed on a pipe, so a long-lived
        process would deadlock the reader."""
        import re
        out = subprocess.run([a.sf], input="position fen " + fen + chr(10) + "eval" + chr(10) + "quit" + chr(10),
                             capture_output=True, text=True, timeout=60).stdout
        m2 = re.search(r"Final evaluation" + r"\s+" + r"([+-]?[\d.]+)" + r"\s+\((white|black) side", out)
        if not m2:
            return None
        v = float(m2.group(1)) * 100.0
        return v if m2.group(2) == "white" else -v

    if a.reference == "sf-static":
        relabelled = []
        for (r, label_stm, nnue_stm, cp_white) in labels:
            ref = sf_static(r["fen"])
            if ref is None:
                continue
            stm = r["fen"].split()[1]
            relabelled.append((r, ref if stm == "w" else -ref, nnue_stm, cp_white))
        labels = relabelled

    def bucket(fen: str) -> str:
        pieces = sum(1 for c in fen.split()[0] if c.isalpha())
        if pieces <= 10:
            return "endgame"
        if pieces <= 20:
            return "middlegame"
        return "opening"

    per_bucket: dict[str, list[float]] = {}
    for r, label_stm, nnue_stm, cp_white in labels:
        if nnue_stm is not None:
            e = nnue_stm - label_stm
            errs_nnue.append(abs(e))
            signed_nnue.append(e)
            per_bucket.setdefault(bucket(r["fen"]), []).append(abs(e))
        if cp_white is not None:
            classic_stm = cp_white if r["fen"].split()[1] == "w" else -cp_white
            e = classic_stm - label_stm
            errs_classic.append(abs(e))
            signed_classic.append(e)

    def stats(v: list[float]) -> str:
        if not v:
            return "n/a"
        return f"MAE {statistics.mean(v):6.1f}  median {statistics.median(v):6.1f}  p90 {sorted(v)[int(len(v)*0.9)]:6.1f}"

    print(f"nnue   : {stats(errs_nnue)}  bias {statistics.mean(signed_nnue):+6.1f}")
    print(f"classic: {stats(errs_classic)}  bias {statistics.mean(signed_classic):+6.1f}")
    print("per phase (nnue MAE):")
    for k in ("opening", "middlegame", "endgame"):
        v = per_bucket.get(k, [])
        print(f"  {k:>11}: {stats(v)}  (n={len(v)})")
    # Correlation + scale fit on the NNUE head.
    xs = [l for _, l, n, _ in labels if n is not None]
    ys = [n for _, l, n, _ in labels if n is not None]
    if len(xs) > 2:
        mx, my = statistics.mean(xs), statistics.mean(ys)
        cov = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
        vx = sum((x - mx) ** 2 for x in xs)
        vy = sum((y - my) ** 2 for y in ys)
        slope = cov / vx if vx else 0.0
        r = cov / ((vx * vy) ** 0.5) if vx and vy else 0.0
        print(f"nnue vs label: slope {slope:.3f}  r {r:.3f}  (perfect scale slope=1.0)")
    if a.dump:
        with open(a.dump, "w", encoding="utf-8") as fd:
            for r, label_stm, nnue_stm, cp_white in labels:
                fd.write(json.dumps({"fen": r["fen"], "label_stm": label_stm,
                                     "nnue_stm": nnue_stm, "classic_white": cp_white}) + chr(10))
        print(f"dumped {len(labels)} rows -> {a.dump}")
    if a.json:
        Path(a.json).write_text(json.dumps({
            "positions": len(labels),
            "nnue_mae": statistics.mean(errs_nnue) if errs_nnue else None,
            "nnue_bias": statistics.mean(signed_nnue) if signed_nnue else None,
            "classic_mae": statistics.mean(errs_classic) if errs_classic else None,
            "per_phase": {k: statistics.mean(v) for k, v in per_bucket.items() if v},
        }, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
