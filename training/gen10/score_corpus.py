#!/usr/bin/env python3
"""Eval instrument on the CURRENT corpus: SF-static labels, net MAE.

The gen9-based instrument samples positions from gen9 play, which is weaker
than the shipped gen10-v10 incumbent, so its distribution misrepresents what
the engine faces today. This scorer samples positions from the live d20
self-play corpus (played by the current engine) and labels them once with
Stockfish's STATIC eval (apples-to-apples: static vs static), then writes one
fit_and_score.py-compatible dump per candidate net:

    {"fen", "label_stm", "nnue_stm", "classic_white"}

Labeling runs SF in file-batch mode (one process, commands from a file), so
there is no per-position startup cost and no interactive-pipe deadlock.

    python score_corpus.py --shards 'training/gen10/corpus-d20/shards/*.jsonl' \
        --rows 3000 \
        --net incumbent=target-cvs/matrix-raw.json \
        --net 400k-v1=target-cvs/400k-v1.json \
        --out-dir tmp-test
"""
from __future__ import annotations

import argparse
import glob
import json
import re
import subprocess
import sys
import time
from pathlib import Path

FINAL_EVAL = re.compile(r"Final evaluation\s+([+-]?[\d.]+)\s+\((white|black) side")


def sample_fens(patterns, want, stride):
    fens, seen = [], set()
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
                    fen = j.get("fen")
                    if not fen or fen in seen:
                        continue
                    seen.add(fen)
                    fens.append(fen)
                    if len(fens) >= want:
                        return fens
    return fens


def sf_static_batch(fens, sf_bin, cmd_path, out_path):
    """SF static eval for every fen, one file-batch process. stm POV cp."""
    with open(cmd_path, "w", encoding="utf-8") as f:
        f.write("uci\nisready\n")
        for fen in fens:
            f.write("position fen " + fen + "\n")
            f.write("eval\n")
        f.write("quit\n")
    with open(cmd_path, "rb") as inp, open(out_path, "w", encoding="utf-8") as out:
        subprocess.run([sf_bin], stdin=inp, stdout=out,
                       stderr=subprocess.DEVNULL, timeout=max(600, len(fens)))
    labels = []
    with open(out_path, encoding="utf-8") as f:
        for line in f:
            m = FINAL_EVAL.search(line)
            if m:
                v = float(m.group(1)) * 100.0
                labels.append(v if m.group(2) == "white" else -v)
    return labels


def engine_evals(fens, engine, flags):
    """Serve-protocol eval per fen; returns (nnue_stm, classic_white) lists."""
    p = subprocess.Popen([engine, "--serve", "--depth", "1"] + flags.split(),
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=1)
    nnue, classic = [], []
    for fen in fens:
        p.stdin.write(json.dumps({"cmd": "eval", "fen": fen}) + "\n")
        p.stdin.flush()
        line = p.stdout.readline()
        if not line:
            break
        j = json.loads(line)
        nnue.append(j.get("nnueStmCp"))
        classic.append(j.get("evalWhiteCp"))
    p.stdin.write("quit\n")
    p.stdin.flush()
    p.kill()
    return nnue, classic


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--shards", default="training/gen10/corpus-d20/shards/*.jsonl")
    ap.add_argument("--rows", type=int, default=3000)
    ap.add_argument("--stride", type=int, default=11)
    ap.add_argument("--sf", default="f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe")
    ap.add_argument("--engine", default="target/release/analyze.exe")
    ap.add_argument("--net", action="append", default=[],
                    help="label=path (repeatable); engine gets '--nnue path'")
    ap.add_argument("--out-dir", default="tmp-test")
    a = ap.parse_args()

    nets = []
    for spec in a.net:
        label, path = spec.split("=", 1)
        nets.append((label, path))

    fens = sample_fens([a.shards], a.rows, a.stride)
    print(f"sampled {len(fens)} positions from {a.shards}", flush=True)

    t0 = time.time()
    statics = sf_static_batch(fens, a.sf,
                              f"{a.out_dir}/sfstatic.cmds", f"{a.out_dir}/sfstatic.out")
    print(f"SF static labels: {len(statics)} in {time.time()-t0:.0f}s", flush=True)
    if len(statics) != len(fens):
        print("WARNING: label count mismatch; aligning by truncation", flush=True)
    n = min(len(fens), len(statics))

    out = Path(a.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    for label, path in nets:
        nnue, classic = engine_evals(fens[:n], a.engine, f"--nnue {path}")
        dump = out / f"current-{label}.dump.jsonl"
        with open(dump, "w", encoding="utf-8") as f:
            for i in range(min(n, len(nnue))):
                stm = fens[i].split()[1]
                f.write(json.dumps({
                    "fen": fens[i],
                    "label_stm": statics[i] if stm == "w" else -statics[i],
                    "nnue_stm": nnue[i],
                    "classic_white": classic[i],
                }) + "\n")
        print(f"{label}: wrote {dump}", flush=True)


if __name__ == "__main__":
    main()
