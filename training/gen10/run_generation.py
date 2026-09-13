#!/usr/bin/env python3
"""Self-play + geometry-extraction pipeline for Railway deployment.

Runs the selfplay binary in durable BATCHES: each batch plays BATCH_GAMES
games from the inv1 opening book, labels the quiet positions with the
champion NNUE (200k-v10 + calibration) at LABEL_DEPTH, attaches CVS
geometry feature IDs, and writes one immutable shard file per batch.
A crash loses at most the in-flight batch; completed shards persist.

Environment variables:
    TOTAL_GAMES         stop after this many games (default 100_000_000)
    BATCH_GAMES         games per durable batch (default 1000)
    SELFPLAY_DEPTH      search depth for self-play (default 6)
    LABEL_DEPTH         depth for position labeling (default 16)
    LABEL_WORKERS       parallel analyze processes for labeling (default 4)
    THREADS             selfplay concurrency (default 6)
    OUTPUT_DIR          where to write shards (default /data)

Output: OUTPUT_DIR/training-<replica>-<batch:05d>.jsonl, one JSON row per
position: {"fen", "cp", "res", "features", "features_bitset"}.
"""
import json
import os
import subprocess
import sys
import threading
import time

TOTAL_GAMES = int(os.environ.get("TOTAL_GAMES", "100000000"))
BATCH_GAMES = int(os.environ.get("BATCH_GAMES", "1000"))
DEPTH = int(os.environ.get("SELFPLAY_DEPTH", "6"))
THREADS = int(os.environ.get("THREADS", "6"))
OUTPUT_DIR = os.environ.get("OUTPUT_DIR", "/data")
LABEL_DEPTH = int(os.environ.get("LABEL_DEPTH", "16"))
LABEL_WORKERS = int(os.environ.get("LABEL_WORKERS", "4"))
REPLICA = os.environ.get("RAILWAY_REPLICA_ID", os.environ.get("HOSTNAME", "0"))[:12]

NET = "/app/matrix-raw.json"
CAL = "/app/eval-cal.json"
HELPER = "/app/matrix-residual.json"


class AnalyzeProc:
    """One persistent `analyze --serve` process, one job at a time."""

    def __init__(self, args):
        self.proc = subprocess.Popen(
            ["/app/analyze", "--serve"] + args,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1)

    def eval_cp(self, fen, node_budget=1000000):
        self.proc.stdin.write(json.dumps(
            {"cmd": "go", "fen": fen, "nodeBudget": node_budget}) + "\n")
        self.proc.stdin.flush()
        score = None
        while True:
            line = self.proc.stdout.readline()
            if not line or line.startswith("bestmove"):
                break
            if line.startswith("info") and " score cp " in line:
                parts = line.split()
                score = int(parts[parts.index("cp") + 1])
        return score

    def cvs_ids(self, fen):
        self.proc.stdin.write(json.dumps({"fen": fen}) + "\n")
        self.proc.stdin.flush()
        resp = json.loads(self.proc.stdout.readline())
        return resp.get("cvsCoreIds") or resp.get("features") or []

    def close(self):
        try:
            self.proc.stdin.write("quit\n")
            self.proc.stdin.flush()
            self.proc.kill()
        except Exception:
            pass


def eval_worker(args):
    """Thread body: own one analyze process, drain a fen queue, emit rows."""
    fen_q, out_q, done = args
    proc = AnalyzeProc(["--depth", str(LABEL_DEPTH), "--nnue", NET,
                        "--nnue-cal", CAL, "--helper-nnue", HELPER])
    while True:
        try:
            fen, res = fen_q.get_nowait()
        except Exception:
            break
        try:
            score = proc.eval_cp(fen)
        except Exception:
            score = None
        if score is not None:
            stm_white = fen.split()[1] == "w"
            cp_white = score if stm_white else -score
            out_q.append({"fen": fen, "cp": cp_white, "res": res})
    proc.close()
    done.set()


def cvs_worker(args):
    """Thread body: own one analyze process, label rows with geometry IDs."""
    row_q, out, done = args
    proc = AnalyzeProc(["--depth", "1", "--cvs-core-ids"])
    while True:
        try:
            row = row_q.get_nowait()
        except Exception:
            break
        try:
            feats = proc.cvs_ids(row["fen"])
        except Exception:
            feats = []
        row["features"] = feats
        row["features_bitset"] = sum(1 << int(f) for f in feats if str(f).isdigit())
        out.append(row)
    proc.close()
    done.set()


def run_batch(batch_no):
    t0 = time.time()
    pgn = os.path.join(OUTPUT_DIR, f"selfplay-{REPLICA}-{batch_no:05d}.pgn")

    # Phase 1: self-play
    subprocess.run([
        "/app/selfplay", "--games", str(BATCH_GAMES),
        "--depth", str(DEPTH), "--threads", str(THREADS),
        "--out", pgn], check=True, timeout=3600 * 8)
    t1 = time.time()

    # Phase 2: extract quiet FENs + results
    import chess.pgn
    fens = []
    with open(pgn) as f:
        while True:
            game = chess.pgn.read_game(f)
            if game is None:
                break
            board = game.board()
            res_map = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}
            res = res_map.get(game.headers.get("Result", "1/2-1/2"), 0.5)
            for move in game.mainline_moves():
                fen = board.fen()
                if is_quiet_fen(fen):
                    fens.append((fen, res))
                board.push(move)
    t2 = time.time()

    # Phase 3: parallel labeling with the champion eval
    import queue
    fen_q = queue.Queue()
    for item in fens:
        fen_q.put(item)
    out_rows, dones = [], []
    threads = []
    for _ in range(min(LABEL_WORKERS, max(1, len(fens)))):
        done = threading.Event()
        t = threading.Thread(target=eval_worker, args=((fen_q, out_rows, done),))
        dones.append(done)
        t.start()
        threads.append(t)
    for t in threads:
        t.join()
    t3 = time.time()

    # Phase 4: parallel CVS geometry extraction
    row_q = queue.Queue()
    for row in out_rows:
        row_q.put(row)
    final_rows, threads = [], []
    for _ in range(min(LABEL_WORKERS, max(1, out_rows and len(out_rows) or 1))):
        done = threading.Event()
        t = threading.Thread(target=cvs_worker, args=((row_q, final_rows, done),))
        dones.append(done)
        t.start()
        threads.append(t)
    for t in threads:
        t.join()
    t4 = time.time()

    # Phase 5: immutable shard, then drop the PGN
    shard = os.path.join(OUTPUT_DIR, f"training-{REPLICA}-{batch_no:05d}.jsonl")
    with open(shard, "w", encoding="utf-8") as f:
        for row in final_rows:
            f.write(json.dumps(row) + "\n")
    os.remove(pgn)

    print(f"batch {batch_no}: {BATCH_GAMES} games, {len(final_rows)} rows "
          f"(extract {t1-t0:.0f}s | quiet {t2-t1:.0f}s | label {t3-t2:.0f}s | "
          f"geom {t4-t3:.0f}s) -> {shard}", flush=True)
    return len(final_rows)


def is_quiet_fen(fen: str) -> bool:
    """No check, no captures available — the quiet filter."""
    import chess
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


def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    games_done = 0
    batch_no = 0
    rows_total = 0
    while games_done < TOTAL_GAMES:
        batch_no += 1
        try:
            rows_total += run_batch(batch_no)
        except subprocess.CalledProcessError as e:
            print(f"batch {batch_no} selfplay failed ({e}); retrying", flush=True)
            time.sleep(5)
            continue
        games_done += BATCH_GAMES
        secs = time.time()
        print(f"total: {games_done} games, {rows_total} rows", flush=True)
    print("TOTAL_GAMES reached; exiting", flush=True)


if __name__ == "__main__":
    main()
