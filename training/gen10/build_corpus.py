#!/usr/bin/env python3
"""Gen10 corpus: quietly-filtered, deduped, DEEP-labelled, stability-filtered.

Three things the gen7-9 corpora did not do, each of which caps a net's accuracy:

  1. **Deep labels.** The stored gen9 `cp` labels disagree with a fresh Stockfish
     d24 by a median of 48cp and a p90 of 1156cp (measured on 120 sampled rows) —
     the same order as the documented gen7-era "d12 vs d20 = 65cp" teacher noise.
     Shallow/noisy labels are why the trained net's output is compressed: under
     heavy-tailed target noise the MSE-optimal predictor shrinks toward the mean.
  2. **Quiet positions.** Eval nets should learn quiet positions; in-check and
     capture-available positions are tactical noise a static eval cannot model.
  3. **Stable labels.** Keep only positions where a shallow and a deep label agree
     within `--stable-max`; the disagreement tail is exactly where labels are
     unreliable (search-version/hash drift, tactical volatility).

Output rows: {"fen", "cp" (d24, white POV), "cpShallow", "stable"}.
Run:  python training/gen10/build_corpus.py --positions 250000 --workers 12
"""
from __future__ import annotations

import argparse
import glob
import json
import random
import statistics
import subprocess
import sys
import time
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import chess

REPO = Path(__file__).resolve().parents[2]
SF = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"


def is_quiet(fen: str) -> bool:
    """Skip in-check positions and positions with any legal capture (classic
    eval-training filter: a static eval cannot model the tactics there)."""
    try:
        b = chess.Board(fen)
    except ValueError:
        return False
    if b.is_check() or b.is_game_over():
        return False
    for m in b.legal_moves:
        if b.is_capture(m):
            return False
    return True


def sample_fens(shards: list[str], want: int, seed: int) -> list[str]:
    rng = random.Random(seed)
    seen: set[str] = set()
    out: list[str] = []
    files = sorted(f for pat in shards for f in glob.glob(pat))
    for f in files:
        with open(f, encoding="utf-8") as fd:
            for line in fd:
                try:
                    j = json.loads(line)
                except json.JSONDecodeError:
                    continue
                fen = j.get("fen", "")
                if not fen or fen in seen:
                    continue
                seen.add(fen)
                if not is_quiet(fen):
                    continue
                out.append(fen)
        if len(out) >= want * 3:      # sample from the front, then shuffle down
            break
    rng.shuffle(out)
    return out[:want]


def label_worker(args: tuple[list[str], int, int]) -> list[dict]:
    fens, deep, shallow = args
    p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=1)
    p.stdin.write("uci\n"); p.stdin.flush()
    while p.stdout.readline().strip() != "uciok":
        pass

    def go(fen: str, depth: int) -> int | None:
        p.stdin.write(f"position fen {fen}\ngo depth {depth}\n"); p.stdin.flush()
        score = None
        while True:
            line = p.stdout.readline()
            if not line or line.startswith("bestmove"):
                break
            if line.startswith("info") and " score cp " in line and " pv " in line:
                parts = line.split()
                score = int(parts[parts.index("cp") + 1])
        return score

    rows = []
    for fen in fens:
        stm_white = fen.split()[1] == "w"
        d = go(fen, deep)
        if d is None:
            continue
        # shallow probe: cheap, only used as a stability check
        s = go(fen, shallow) or d
        w = (lambda v: v if stm_white else -v)
        rows.append({"fen": " ".join(fen.split()[:4]), "cp": w(d), "cpShallow": w(s)})
    p.stdin.write("quit\n"); p.stdin.flush(); p.kill()
    return rows


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--shards", default=str(REPO / "training/gen9/gen9-cvs/shard-*.jsonl"))
    ap.add_argument("--out", default=str(REPO / "training/gen10/corpus/d24.jsonl"))
    ap.add_argument("--positions", type=int, default=250000)
    ap.add_argument("--workers", type=int, default=12)
    ap.add_argument("--depth", type=int, default=24)
    ap.add_argument("--shallow-depth", type=int, default=16)
    ap.add_argument("--stable-max", type=int, default=80,
                    help="drop positions where |deep - shallow| exceeds this (cp)")
    ap.add_argument("--max-abs", type=int, default=1500, help="drop |label| above this")
    ap.add_argument("--seed", type=int, default=11)
    a = ap.parse_args(argv)

    t0 = time.time()
    fens = sample_fens(a.shards.split(","), a.positions, a.seed)
    print(f"sampled {len(fens)} quiet, deduped FENs in {time.time()-t0:.0f}s", flush=True)
    chunks = [fens[i::a.workers] for i in range(a.workers)]
    rows: list[dict] = []
    with ProcessPoolExecutor(max_workers=a.workers) as ex:
        for i, part in enumerate(ex.map(label_worker, [(c, a.depth, a.shallow_depth) for c in chunks])):
            rows.extend(part)
            print(f"  worker {i+1}/{a.workers}: {len(part)} labelled ({time.time()-t0:.0f}s)", flush=True)
    keep = []
    dropped_vol = dropped_big = 0
    for r in rows:
        if abs(r["cp"]) > a.max_abs:
            dropped_big += 1
            continue
        if abs(r["cp"] - r["cpShallow"]) > a.stable_max:
            dropped_vol += 1
            continue
        r["stable"] = True
        keep.append(r)
    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, "w", encoding="utf-8") as fd:
        for r in keep:
            fd.write(json.dumps(r) + "\n")
    d = [abs(r["cp"] - r["cpShallow"]) for r in rows]
    print(f"\nlabelled {len(rows)}; kept {len(keep)} "
          f"(dropped {dropped_vol} unstable > {a.stable_max}cp, {dropped_big} |label| > {a.max_abs})")
    if d:
        print(f"deep-vs-shallow |delta|: mean {statistics.mean(d):.0f}cp median {statistics.median(d):.0f} "
              f"p90 {sorted(d)[int(len(d)*0.9)]:.0f}")
    print(f"-> {out} in {time.time()-t0:.0f}s")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
