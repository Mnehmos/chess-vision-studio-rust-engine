#!/usr/bin/env python3
"""Self-play + geometry-extraction pipeline for Railway deployment.

Each durable BATCH plays BATCH_GAMES NNUE-backed games with our engine
(champion net + calibration + helper), then labels every quiet position with
a fast deep labeler, then attaches CVS geometry feature IDs (our unique
signal) and writes one immutable shard per batch. A crash loses at most the
in-flight batch.

Two labelers:
  LABELER=sf   Stockfish (default): `go depth LABEL_DEPTH` per position,
               single-thread per worker, N workers. d20 ≈ 0.5s/pos/worker.
  LABELER=cvs  our own engine at fixed nodes (LABEL_NODES); best for
               shallow depths only — d20 with our engine is ~100x slower
               than SF, so SF is the default for deep labels.

Environment variables:
    LABELER             sf | cvs (default sf)
    TOTAL_GAMES         stop after this many games (default 100_000_000)
    BATCH_GAMES         games per durable batch (default 1000)
    SELFPLAY_DEPTH      self-play depth for our engine (default 6)
    LABEL_DEPTH         labeler depth (default 20)
    LABEL_NODES         cvs-labeler node budget per position (default 150000)
    LABEL_WORKERS       parallel labeler processes (default 8)
    THREADS             selfplay concurrency (default 6)
    OUTPUT_DIR          where to write shards (default /data)
    SF_BIN              stockfish path (default /app/stockfish)

Output: OUTPUT_DIR/training-<replica>-<batch:05d>.jsonl, one JSON row per
position: {"fen", "cp", "cp_play", "res", "features", "features_bitset"}.
"""
import itertools
import json
import os
import queue
import subprocess
import threading
import time

LABELER = os.environ.get("LABELER", "sf").lower()
TOTAL_GAMES = int(os.environ.get("TOTAL_GAMES", "100000000"))
BATCH_GAMES = int(os.environ.get("BATCH_GAMES", "1000"))
DEPTH = int(os.environ.get("SELFPLAY_DEPTH", "6"))
THREADS = int(os.environ.get("THREADS", "6"))
OUTPUT_DIR = os.environ.get("OUTPUT_DIR", "/data")
LABEL_DEPTH = int(os.environ.get("LABEL_DEPTH", "20"))
LABEL_NODES = int(os.environ.get("LABEL_NODES", "150000"))
LABEL_WORKERS = int(os.environ.get("LABEL_WORKERS", "8"))
REPLICA = os.environ.get("RAILWAY_REPLICA_ID", os.environ.get("HOSTNAME", "0"))[:12]

SELFPLAY_BIN = os.environ.get("SELFPLAY_BIN", "/app/selfplay")
ANALYZE_BIN = os.environ.get("ANALYZE_BIN", "/app/analyze")
SF_BIN = os.environ.get("SF_BIN", "/usr/local/bin/stockfish")
NET = os.environ.get("NET", "/app/matrix-raw.json")
CAL = os.environ.get("CAL", "/app/eval-cal.json")
HELPER = os.environ.get("HELPER", "/app/matrix-residual.json")

NNUE_FLAGS = ["--nnue", NET, "--nnue-cal", CAL, "--helper-nnue", HELPER]

MATE_CP = 10000  # mate scores saturate here (kept ordered by distance)


def ids_to_bitset(ids):
    """Core feature ids -> [u64; 3] word array (same encoding as the gen9
    corpus: word = id // 64, bit = id % 64)."""
    words = [0, 0, 0]
    for i in ids:
        w, b = divmod(int(i), 64)
        if w < 3:
            words[w] |= 1 << b
    return words


class SfProc:
    """One persistent Stockfish UCI process, one position at a time."""

    def __init__(self):
        self.proc = subprocess.Popen(
            [SF_BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self.send("uci")
        while True:
            line = self.proc.stdout.readline()
            if not line or line.startswith("uciok"):
                break
        self.send("setoption name Threads value 1")
        self.send("setoption name Hash value 32")
        self.send("setoption name MultiPV value 1")
        self.send("isready")
        while True:
            line = self.proc.stdout.readline()
            if not line or line.startswith("readyok"):
                break

    def send(self, cmd):
        self.proc.stdin.write(cmd + "\n")
        self.proc.stdin.flush()

    def label(self, fen, depth):
        """Returns (cp_stm, depth_reached, nodes, best_uci) or None on failure."""
        self.send("position fen " + fen)
        self.send(f"go depth {depth}")
        cp, reached, nodes, best = None, 0, 0, None
        while True:
            line = self.proc.stdout.readline()
            if not line:
                return None
            if line.startswith("bestmove"):
                parts = line.split()
                if len(parts) > 1 and parts[1] != "(none)":
                    best = parts[1]
                break
            if not line.startswith("info") or " score " not in line:
                continue
            parts = line.split()
            try:
                if "depth" in parts:
                    reached = int(parts[parts.index("depth") + 1])
                if "nodes" in parts:
                    nodes = int(parts[parts.index("nodes") + 1])
                if "cp" in parts:
                    cp = int(parts[parts.index("cp") + 1])
                elif "mate" in parts:
                    m = int(parts[parts.index("mate") + 1])
                    cp = (MATE_CP - min(abs(m), 100) * 10) * (1 if m > 0 else -1)
            except (ValueError, IndexError):
                continue
        if cp is None:
            return None
        return cp, reached, nodes, best

    def close(self):
        try:
            self.send("quit")
            self.proc.kill()
        except Exception:
            pass


class AnalyzeProc:
    """One persistent `analyze --serve` process (our engine), one job at a
    time. The nodeBudget request returns exactly one JSON line."""

    def __init__(self, extra):
        self.proc = subprocess.Popen(
            [ANALYZE_BIN, "--serve"] + extra,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1)

    def label(self, fen, node_budget):
        self.proc.stdin.write(json.dumps(
            {"cmd": "go", "fen": fen, "nodeBudget": node_budget}) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            return None
        try:
            resp = json.loads(line)
        except ValueError:
            return None
        cp = resp.get("scoreCp")
        if cp is None:
            return None
        return cp, resp.get("depth", 0), resp.get("nodes", 0), resp.get("uci")

    def close(self):
        try:
            self.proc.stdin.write("quit\n")
            self.proc.stdin.flush()
            self.proc.kill()
        except Exception:
            pass


def make_labeler():
    if LABELER == "sf":
        return SfProc(), (lambda p, fen: p.label(fen, LABEL_DEPTH))
    proc = AnalyzeProc(["--depth", str(LABEL_DEPTH)] + NNUE_FLAGS)
    return proc, (lambda p, fen: p.label(fen, LABEL_NODES))


def label_worker(fen_q, out_q, progress):
    proc, fn = make_labeler()
    while True:
        try:
            row = fen_q.get_nowait()
        except queue.Empty:
            break
        try:
            res = fn(proc, row["fen"])
        except Exception:
            res = None
        if res is not None:
            cp, depth, nodes, best = res
            stm_white = row["fen"].split()[1] == "w"
            row["cp"] = cp if stm_white else -cp
            row["label_depth"] = depth
            row["label_nodes"] = nodes
            if best:
                row["sf_best"] = best
            out_q.append(row)
        if next(progress) % 2000 == 0:
            print(f"  labeled {out_q and len(out_q) or 0}", flush=True)
    proc.close()


def cvs_geometry(fens, batch_no):
    """One batch invocation of the ultra-fast feature extractor."""
    fens_file = os.path.join(OUTPUT_DIR, f"fens-{REPLICA}-{batch_no:05d}.txt")
    with open(fens_file, "w", encoding="utf-8") as f:
        f.write("\n".join(fens) + "\n")
    out = subprocess.run(
        [ANALYZE_BIN, "--depth", "1", "--cvs-core-ids", "--fens", fens_file],
        capture_output=True, text=True, check=True).stdout
    os.remove(fens_file)
    lines = out.splitlines()
    feats = []
    for line in lines:
        s = line.strip()
        if not s or s == "ERR":
            feats.append([])
        else:
            feats.append([int(x) for x in s.split(",") if x.strip().isdigit()])
    return feats


def run_batch(batch_no):
    t0 = time.time()
    raw = os.path.join(OUTPUT_DIR, f"selfplay-{REPLICA}-{batch_no:05d}.jsonl")

    # Phase 1: our engine plays NNUE-backed self-play games.
    subprocess.run([
        SELFPLAY_BIN, "--games", str(BATCH_GAMES),
        "--depth", str(DEPTH), "--threads", str(THREADS),
        "--out", raw] + NNUE_FLAGS, check=True, timeout=3600 * 8)
    t1 = time.time()

    with open(raw, encoding="utf-8") as f:
        rows = [json.loads(line) for line in f if line.strip()]
    for r in rows:
        r["cp_play"] = r.pop("cp")  # keep the play-depth score under its own key
    t2 = time.time()

    # Phase 2: deep labels (SF d20 by default) across many single-thread workers.
    fen_q = queue.Queue()
    for row in rows:
        fen_q.put(row)
    labeled = []
    threads = [
        threading.Thread(target=label_worker,
                         args=(fen_q, labeled, itertools.count(1)))
        for _ in range(max(1, min(LABEL_WORKERS, len(rows))))
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    t3 = time.time()

    # Phase 3: CVS geometry feature IDs (the unique signal), one batch call.
    feats = cvs_geometry([r["fen"] for r in labeled], batch_no)
    if len(feats) != len(labeled):
        raise RuntimeError(
            f"geometry count mismatch: {len(feats)} lines for {len(labeled)} rows")
    for row, ids in zip(labeled, feats):
        row["features"] = ids
        row["features_bitset"] = ids_to_bitset(ids)
    t4 = time.time()

    # Phase 4: immutable shard, then drop the raw selfplay file.
    shard = os.path.join(OUTPUT_DIR, f"training-{REPLICA}-{batch_no:05d}.jsonl")
    with open(shard, "w", encoding="utf-8") as f:
        for row in labeled:
            f.write(json.dumps(row) + "\n")
    os.remove(raw)

    print(f"batch {batch_no}: {BATCH_GAMES} games, {len(labeled)} rows "
          f"(play {t1-t0:.0f}s | read {t2-t1:.0f}s | label {t3-t2:.0f}s | "
          f"geom {t4-t3:.0f}s) -> {shard}", flush=True)
    return len(labeled)


def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    print(f"pipeline: labeler={LABELER} depth={LABEL_DEPTH} "
          f"workers={LABEL_WORKERS} batch={BATCH_GAMES} games", flush=True)
    games_done = 0
    batch_no = 0
    rows_total = 0
    while games_done < TOTAL_GAMES:
        batch_no += 1
        try:
            rows_total += run_batch(batch_no)
        except subprocess.CalledProcessError as e:
            print(f"batch {batch_no} failed ({e}); retrying", flush=True)
            time.sleep(5)
            continue
        games_done += BATCH_GAMES
        print(f"total: {games_done} games, {rows_total} rows", flush=True)
    print("TOTAL_GAMES reached; exiting", flush=True)


if __name__ == "__main__":
    main()
