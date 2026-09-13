#!/usr/bin/env python3
"""Self-play + geometry-extraction pipeline for Railway deployment.

Runs selfplay binary to generate games from the inv1 opening book, then labels
the positions with the engine's own eval (depth-configurable) and extracts CVS
geometry feature IDs. Output rows match the gen9 shard format:
    {"fen", "cp", "res", "features", "features_bitset"}

Environment variables:
    SELFPLAY_GAMES      number of games to generate (default 5000)
    SELFPLAY_DEPTH      search depth for self-play (default 6)
    LABEL_DEPTH         depth for position labeling (default 16)
    THREADS             selfplay concurrency (default 6)
    OUTPUT_DIR          where to write training shards (default /data)

Each output shard is ~100K rows, written incrementally. The container exits
when the game target is reached. Restart to continue (the opening book rotation
ensures new positions).
"""
import json
import os
import subprocess
import sys
import time

GAMES = int(os.environ.get("SELFPLAY_GAMES", "5000"))
DEPTH = int(os.environ.get("SELFPLAY_DEPTH", "6"))
THREADS = int(os.environ.get("THREADS", "6"))
OUTPUT_DIR = os.environ.get("OUTPUT_DIR", "/data")
LABEL_DEPTH = int(os.environ.get("LABEL_DEPTH", "16"))

def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)

    # Phase 1: self-play game generation
    print(f"Phase 1: generating {GAMES} self-play games at depth {DEPTH}, {THREADS} threads")
    t0 = time.time()
    cmd = [
        "/app/selfplay",
        "--games", str(GAMES),
        "--depth", str(DEPTH),
        "--threads", str(THREADS),
        "--out", os.path.join(OUTPUT_DIR, "selfplay-games.pgn"),
    ]
    subprocess.run(cmd, check=True, timeout=3600 * 8)
    print(f"Phase 1 done in {time.time()-t0:.0f}s")

    # Phase 2: extract FENs from PGN
    print("Phase 2: extracting positions from PGN")
    fens = []
    import chess.pgn
    with open(os.path.join(OUTPUT_DIR, "selfplay-games.pgn")) as f:
        while True:
            game = chess.pgn.read_game(f)
            if game is None:
                break
            board = game.board()
            result = game.headers.get("Result", "1/2-1/2")
            res_map = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}
            res = res_map.get(result, 0.5)
            for move in game.mainline_moves():
                fen = board.fen()
                if is_quiet_fen(fen):
                    fens.append((fen, res))
                board.push(move)
    print(f"extracted {len(fens)} quiet positions with results")

    # Phase 3: label with the engine's own eval at LABEL_DEPTH
    print(f"Phase 3: labeling {len(fens)} positions at depth {LABEL_DEPTH}")
    proc = subprocess.Popen(
        ["/app/analyze", "--serve", "--depth", str(LABEL_DEPTH),
         "--nnue", "/data/matrix-raw.json"] if os.path.exists("/data/matrix-raw.json") else
        ["/app/analyze", "--serve", "--depth", str(LABEL_DEPTH)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, text=True, bufsize=1)

    labeled = 0
    t1 = time.time()
    with open(os.path.join(OUTPUT_DIR, "training-shard.jsonl"), "a", encoding="utf-8") as f:
        for fen, res in fens:
            proc.stdin.write(json.dumps({"cmd": "go", "fen": fen,
                                         "nodeBudget": 1000000}) + "\n")
            proc.stdin.flush()
            score = None
            while True:
                line = proc.stdout.readline()
                if not line or line.startswith("bestmove"):
                    break
                if line.startswith("info") and " score cp " in line:
                    parts = line.split()
                    score = int(parts[parts.index("cp") + 1])
            if score is None:
                continue
            stm_white = fen.split()[1] == "w"
            cp_white = score if stm_white else -score
            f.write(json.dumps({"fen": fen, "cp": cp_white, "res": res}) + "\n")
            labeled += 1
    proc.stdin.write("quit\n")
    proc.stdin.flush()
    proc.kill()
    print(f"labeled {labeled} positions in {time.time()-t1:.0f}s")

    # Phase 4: extract CVS geometry features (the unique part)
    print("Phase 4: extracting CVS geometry features")
    proc2 = subprocess.Popen(
        ["/app/analyze", "--serve", "--depth", "1", "--cvs-core-ids"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, text=True, bufsize=1)

    final = 0
    with open(os.path.join(OUTPUT_DIR, "training-final.jsonl"), "a", encoding="utf-8") as f:
        for line in open(os.path.join(OUTPUT_DIR, "training-shard.jsonl"), encoding="utf-8"):
            row = json.loads(line)
            fen = row["fen"]
            proc2.stdin.write(json.dumps({"fen": fen}) + "\n")
            proc2.stdin.flush()
            resp = json.loads(proc2.stdout.readline())
            feat_ids = resp.get("cvsCoreIds") or resp.get("features") or []
            row["features"] = feat_ids
            f.write(json.dumps(row) + "\n")
            final += 1
    proc2.stdin.write("quit\n")
    proc2.stdin.flush()
    proc2.kill()
    print(f"DONE: {final} rows with geometry features -> {OUTPUT_DIR}/training-final.jsonl")


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


if __name__ == "__main__":
    main()
