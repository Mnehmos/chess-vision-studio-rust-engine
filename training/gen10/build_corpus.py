#!/usr/bin/env python3
"""Gen10 corpus v2: fast, resumable, observable labelling.

What the three gen7-9 corpora did not do, each of which caps a trained net:

  1. **Fresh labels.** The stored gen9 `cp` values disagree with a fresh search by a
     median of 48cp and a p90 of 1156cp (measured on 120 sampled rows) — the same
     order as the documented "gen7 d12 vs d20 = 65cp" teacher noise. Noisy targets
     are why the trained net's output came out compressed (the MSE-optimal predictor
     under heavy-tailed target noise shrinks toward the mean).
  2. **Quiet positions** (no check, no capture) — the classic eval-training filter.
  3. **Stable labels**: keep a position only when a shallow and a deeper search agree
     within `--stable-max`; the disagreement tail is where labels are unreliable.

v2 exists because v1 wedged: 12 workers x (d24 + d16) per position is a multi-hour
job with no checkpoints and no progress output, and it died silently when a worker's
engine went away. v2:

  * writes one `part-NN.jsonl` per worker, flushed per row, and `--resume` skips
    positions already present (kill/restart costs at most the in-flight position);
  * caps every search with `movetime` so a starved engine can never block a worker;
  * prints rows/sec + ETA from the part-file sizes while running.

Run (smoke):   python training/gen10/build_corpus.py --positions 1200 --workers 12
Run (corpus):  python training/gen10/build_corpus.py --positions 200000 --workers 12
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import random
import subprocess
import sys
import time
from pathlib import Path

import chess

REPO = Path(__file__).resolve().parents[2]
SF = "f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe"


def is_quiet(fen: str) -> bool:
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
    for f in sorted(f for pat in shards for f in glob.glob(pat)):
        with open(f, encoding="utf-8") as fd:
            for line in fd:
                try:
                    fen = json.loads(line).get("fen", "")
                except json.JSONDecodeError:
                    continue
                if not fen or fen in seen:
                    continue
                seen.add(fen)
                if is_quiet(fen):
                    out.append(fen)
        if len(out) >= want * 3:
            break
    rng.shuffle(out)
    return out[:want]


class Engine:
    """A Stockfish process that cannot wedge a worker: every search carries a
    movetime cap, and a dead process is restarted before the next request."""

    def __init__(self, movetime_ms: int):
        self.movetime = movetime_ms
        self.p: subprocess.Popen | None = None

    def start(self):
        self.p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self.p.stdin.write("uci\n"); self.p.stdin.flush()
        while self.p.stdout.readline().strip() != "uciok":
            pass

    def eval_cmd(self, fen: str, depth: int) -> int | None:
        if self.p is None or self.p.poll() is not None:
            self.start()
        try:
            self.p.stdin.write(f"position fen {fen}\ngo depth {depth} movetime {self.movetime}\n")
            self.p.stdin.flush()
        except (BrokenPipeError, OSError):
            self.p = None
            return None
        score = None
        while True:
            try:
                line = self.p.stdout.readline()
            except (ValueError, OSError):
                self.p = None
                return None
            if not line or line.startswith("bestmove"):
                break
            if line.startswith("info") and " score cp " in line and " pv " in line:
                parts = line.split()
                score = int(parts[parts.index("cp") + 1])
        return score

    def static_eval(self, fen: str) -> int | None:
        """SF's own static evaluation (UCI `eval` -> "Final evaluation"), white POV cp."""
        if self.p is None or self.p.poll() is not None:
            self.start()
        try:
            self.p.stdin.write("position fen " + fen + "\neval\n")
            self.p.stdin.flush()
        except (BrokenPipeError, OSError):
            self.p = None
            return None
        import re as _re
        while True:
            try:
                line = self.p.stdout.readline()
            except (ValueError, OSError):
                self.p = None
                return None
            if not line:
                return None
            m = _re.search(r"Final evaluation" + r"\s+" + r"([+-]?[\d.]+)" + r"\s+\((white|black) side", line)
            if m:
                v = float(m.group(1)) * 100.0
                return v if m.group(2) == "white" else -v
            if line.startswith("bestmove"):
                return None

    def stop(self):
        if self.p and self.p.poll() is None:
            try:
                self.p.stdin.write("quit\n"); self.p.stdin.flush()
            except OSError:
                pass
            self.p.kill()
        self.p = None


def label_chunk(job) -> tuple[int, int]:
    """Label one chunk into its own part file. Returns (kept, seen).

    Records three numbers per position:
      cp        deep search score (the training label)
      cpShallow shallow search score (stability filter)
      cpStatic  SF's own static `eval` ("Final evaluation") -- ~free, and it is the
                same reference the eval instrument scores us against, so the same
                corpus can train a search-label net or a static-distillation net.
    """
    idx, chunk, part, deep, shallow, cap, stable_max, max_abs = job
    eng = Engine(cap)
    kept = seen = 0
    with open(part, "w", encoding="utf-8") as fd:
        for fen in chunk:
            seen += 1
            white = fen.split()[1] == "w"
            if deep <= 0:
                # static-distillation mode: SF's own static eval only (~1ms/position
                # instead of ~1s), so a corpus can be built at million-position scale.
                # This is the exact reference the eval instrument scores us against.
                st = eng.static_eval(fen)
                if st is None:
                    continue
                fd.write(json.dumps({"fen": " ".join(fen.split()[:4]), "cp": st,
                                     "cpShallow": st, "cpStatic": st, "stable": True}) + chr(10))
                kept += 1
                continue
            d = eng.eval_cmd(fen, deep)
            if d is None:
                continue
            s = eng.eval_cmd(fen, shallow)
            if s is None:
                s = d
            st = eng.static_eval(fen)
            w = (lambda v: v if white else -v)
            cp, cs = w(d), w(s)
            if abs(cp) > max_abs or abs(cp - cs) > stable_max:
                continue
            row = {"fen": " ".join(fen.split()[:4]), "cp": cp, "cpShallow": cs, "stable": True}
            if st is not None:
                # SF's `eval` output is White-POV already (it always reports
                # "(white side)"), so it must NOT go through the stm->white lambda.
                row["cpStatic"] = st
            fd.write(json.dumps(row) + chr(10))
            kept += 1
            if kept % 200 == 0:
                fd.flush()
    eng.stop()
    return kept, seen


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--shards", default=str(REPO / "training/gen9/gen9-cvs/shard-*.jsonl"))
    ap.add_argument("--out-dir", default=str(REPO / "training/gen10/corpus"))
    ap.add_argument("--positions", type=int, default=200000)
    ap.add_argument("--workers", type=int, default=12)
    ap.add_argument("--depth", type=int, default=16)
    ap.add_argument("--shallow-depth", type=int, default=12)
    ap.add_argument("--movetime", type=int, default=4000, help="per-search cap (ms)")
    ap.add_argument("--stable-max", type=int, default=80)
    ap.add_argument("--max-abs", type=int, default=1500)
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--resume", action="store_true")
    a = ap.parse_args(argv)

    out_dir = Path(a.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    done: set[str] = set()
    parts = sorted(out_dir.glob("part-*.jsonl"))
    if a.resume and parts:
        for p in parts:
            with open(p, encoding="utf-8") as fd:
                for line in fd:
                    try:
                        done.add(json.loads(line)["fen"])
                    except (json.JSONDecodeError, KeyError):
                        pass
        print(f"resume: {len(done)} labelled rows already on disk")

    t0 = time.time()
    fens = sample_fens(a.shards.split(","), a.positions, a.seed)
    if done:
        fens = [f for f in fens if f not in done]
    print(f"sampled {len(fens)} quiet deduped FENs to label "
          f"({time.time()-t0:.0f}s); depth {a.depth}/probe {a.shallow_depth}, "
          f"{a.movetime}ms cap, {a.workers} workers", flush=True)

    import multiprocessing as mp
    jobs = []
    for i in range(a.workers):
        jobs.append((i, fens[i::a.workers], str(out_dir / f"part-{i:02d}.jsonl"),
                     a.depth, a.shallow_depth, a.movetime, a.stable_max, a.max_abs))
    total = len(fens)

    def count_rows() -> int:
        n = 0
        for f in out_dir.glob("part-*.jsonl"):
            n += sum(1 for _ in open(f, encoding="utf-8"))
        return n

    with mp.Pool(a.workers) as pool:
        res = pool.map_async(label_chunk, jobs)
        while not res.ready():
            time.sleep(20)
            rows = count_rows()
            rate = rows / max(1e-9, time.time() - t0)
            eta = (total - rows) / rate / 60 if rate > 0 else 0
            print(f"  {rows}/{total} kept  ({rate:.1f}/s, ETA {eta:.1f} min)", flush=True)
        done = sum(k for k, _ in res.get())

    rows = 0
    for p in sorted(out_dir.glob("part-*.jsonl")):
        with open(p, encoding="utf-8") as fd:
            rows += sum(1 for _ in fd)
    print(f"\nlabelled+kept {rows} rows in {(time.time()-t0)/60:.1f} min -> {out_dir}/part-*.jsonl")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
