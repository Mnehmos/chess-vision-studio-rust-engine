#!/usr/bin/env python3
"""Label a frozen FEN suite with native Stockfish at a recorded fixed depth."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

import chess
import chess.engine

import benchlib as B


def score_cp(info: dict, turn: chess.Color) -> int:
    return info["score"].pov(turn).score(mate_score=10000)


def classify_danger(board: chess.Board, infos: list[dict]) -> tuple[bool, list[str]]:
    best = infos[0]["pv"][0]
    reasons: list[str] = []
    if board.is_check():
        reasons.append("in_check")
    if board.is_capture(best):
        reasons.append("best_is_capture")
    if board.gives_check(best):
        reasons.append("best_gives_check")
    if len(infos) > 1:
        gap = score_cp(infos[0], board.turn) - score_cp(infos[1], board.turn)
        if gap >= 100:
            reasons.append("multipv_gap_100")
    return bool(reasons), reasons


def load_cache(path: Path) -> dict[str, dict]:
    if not path.exists():
        return {}
    rows = {}
    with path.open(encoding="utf-8-sig") as handle:
        for line in handle:
            if line.strip():
                row = json.loads(line)
                rows[row["key"]] = row
    return rows


def seed_from_labels(path: Path, cache: dict[str, dict]) -> None:
    if not path.exists():
        return
    payload = json.loads(path.read_text(encoding="utf8"))
    for row in payload.get("positions", []):
        cache.setdefault(row["key"], row)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--suite",
        default="benchmarks/suites/suite-clean-postmodel-20260619.txt",
    )
    parser.add_argument("--depth", type=int, default=B.DEFAULT_STOCKFISH_REVIEW_DEPTH)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--hash-mb", type=int, default=256)
    parser.add_argument("--stockfish", default=B.STOCKFISH)
    parser.add_argument("--cache", default="benchmarks/results/holdout-label-cache.jsonl")
    parser.add_argument("--max-abs-score-cp", type=int, default=3000)
    args = parser.parse_args()

    suite_path = Path(args.suite)
    fens = [line.strip() for line in suite_path.read_text().splitlines() if line.strip()]
    stem = suite_path.with_suffix("")
    moves_path = stem.with_suffix(".moves.json")
    danger_path = stem.with_suffix(".danger.json")
    labels_path = stem.with_suffix(".labels.json")
    rejected_path = stem.with_suffix(".rejected.json")
    source_path = stem.with_suffix(".source.json")
    cache_path = Path(args.cache)
    cache = load_cache(cache_path)
    seed_from_labels(labels_path, cache)
    engine = chess.engine.SimpleEngine.popen_uci(args.stockfish)
    engine.configure({"Threads": args.threads, "Hash": args.hash_mb})
    try:
        with cache_path.open("a", encoding="utf8", newline="\n") as cache_out:
            for index, fen in enumerate(fens, start=1):
                key = " ".join(fen.split()[:4])
                if key in cache:
                    continue
                board = chess.Board(fen)
                infos = engine.analyse(
                    board,
                    chess.engine.Limit(depth=args.depth),
                    multipv=2,
                    info=chess.engine.INFO_ALL,
                )
                danger, reasons = classify_danger(board, infos)
                row = {
                    "key": key,
                    "fen": fen,
                    "bestMove": infos[0]["pv"][0].uci(),
                    "scoreCp": score_cp(infos[0], board.turn),
                    "secondMove": (
                        infos[1]["pv"][0].uci()
                        if len(infos) > 1 and infos[1].get("pv")
                        else None
                    ),
                    "secondScoreCp": (
                        score_cp(infos[1], board.turn) if len(infos) > 1 else None
                    ),
                    "danger": danger,
                    "dangerReasons": reasons,
                    "depth": args.depth,
                    "nodes": infos[0].get("nodes"),
                    "timeSec": infos[0].get("time"),
                }
                cache[key] = row
                cache_out.write(json.dumps(row, separators=(",", ":")) + "\n")
                cache_out.flush()
                print(
                    f"{index}/{len(fens)} {row['bestMove']} cp={row['scoreCp']} "
                    f"danger={danger}",
                    flush=True,
                )
    finally:
        engine.quit()

    rows = [cache[" ".join(fen.split()[:4])] for fen in fens]
    accepted = [
        row for row in rows if abs(row["scoreCp"]) <= args.max_abs_score_cp
    ]
    rejected = [
        row for row in rows if abs(row["scoreCp"]) > args.max_abs_score_cp
    ]
    accepted_keys = {row["key"] for row in accepted}
    suite_path.write_text(
        "\n".join(row["fen"] for row in accepted) + "\n",
        encoding="utf8",
    )
    if source_path.exists():
        source = json.loads(source_path.read_text(encoding="utf8"))
        source["positions"] = [
            row for row in source["positions"] if row["key"] in accepted_keys
        ]
        source["selection"]["selectedByPhase"] = dict(
            Counter(row["phase"] for row in source["positions"])
        )
        source["selection"]["selectedGames"] = len(
            {row["gameId"] for row in source["positions"]}
        )
        source["finalization"] = {
            "initialLabels": len(rows),
            "accepted": len(accepted),
            "rejected": len(rejected),
            "maxAbsScoreCp": args.max_abs_score_cp,
            "reason": "exclude forced-mate and already-decided positions",
        }
        source_path.write_text(
            json.dumps(source, indent=2) + "\n",
            encoding="utf8",
        )
    stockfish_path = Path(args.stockfish)
    payload = {
        "schemaVersion": 1,
        "suiteSha256": hashlib.sha256(suite_path.read_bytes()).hexdigest(),
        "stockfish": {
            "path": str(stockfish_path),
            "sha256": B.sha256(stockfish_path, n=64),
            "depth": args.depth,
            "threads": args.threads,
            "hashMb": args.hash_mb,
            "multipv": 2,
        },
        "finalization": {
            "initialLabels": len(rows),
            "accepted": len(accepted),
            "rejected": len(rejected),
            "maxAbsScoreCp": args.max_abs_score_cp,
        },
        "positions": accepted,
    }
    moves_path.write_text(
        json.dumps(
            {
                "ORACLE": [row["bestMove"] for row in accepted],
                "stockfishDepth": args.depth,
                "suiteSha256": payload["suiteSha256"],
            },
            indent=2,
        )
        + "\n",
        encoding="utf8",
    )
    danger_path.write_text(
        json.dumps([row["danger"] for row in accepted], indent=2) + "\n",
        encoding="utf8",
    )
    labels_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf8")
    rejected_path.write_text(
        json.dumps(
            {
                "maxAbsScoreCp": args.max_abs_score_cp,
                "positions": rejected,
            },
            indent=2,
        )
        + "\n",
        encoding="utf8",
    )
    cache_path.unlink(missing_ok=True)
    print(
        f"wrote competitive holdout: {len(accepted)} accepted, "
        f"{len(rejected)} rejected"
    )


if __name__ == "__main__":
    main()
