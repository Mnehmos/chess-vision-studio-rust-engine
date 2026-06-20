#!/usr/bin/env python3
"""Proof-only tactical sentinel capability gate.

The sentinel never chooses a live move. It searches the child position after a
proposed move with reduced pruning, and a separate Raw forced-move search must
confirm the same mate before the result is counted as a verified alarm.
"""

from __future__ import annotations

import argparse
import copy
import json

import chess

import bench_equal_time as E
import benchlib as B


SENTINEL_OFF_FLAGS = (
    "--no-rfp",
    "--no-futility",
    "--no-null",
    "--no-lmr",
    "--no-lmp",
    "--no-seeprune",
    "--no-delta",
    "--no-singular",
    "--no-book",
    "--no-syzygy",
)


def child_fen(fen: str, move: str) -> str:
    board = chess.Board(fen)
    candidate = chess.Move.from_uci(move)
    if candidate not in board.legal_moves:
        raise ValueError(f"illegal candidate {move} for {fen}")
    board.push(candidate)
    return board.fen()


def sentinel_config(raw_cfg: dict) -> dict:
    cfg = copy.deepcopy(raw_cfg)
    cfg["id"] = f"{raw_cfg['id']}+tactical-sentinel-v1"
    cfg["name"] = f"{raw_cfg['name']} Tactical Sentinel v1"
    cfg["status"] = "experimental"
    cfg["architecture"] = f"{raw_cfg['architecture']}+proof-only-sentinel"
    cfg["extra"] = list(raw_cfg.get("extra", [])) + list(SENTINEL_OFF_FLAGS)
    cfg["policy"] = {
        "id": "tactical-sentinel-v1",
        "authority": "proof-only; cannot choose or reorder live moves",
        "scope": "child position after proposed principal move",
        "alarm": "sentinel positive mate and Raw forced-move negative mate",
        "pruning": "rfp/futility/null/lmr/lmp/see/delta/singular off",
    }
    cfg["policy_sha"] = B.canonical_hash(cfg["policy"])
    return cfg


def is_verified_mate_alarm(sentinel: dict, verifier: dict) -> bool:
    sentinel_mate = sentinel.get("mate")
    verifier_mate = verifier.get("mate")
    return (
        isinstance(sentinel_mate, int)
        and sentinel_mate > 0
        and isinstance(verifier_mate, int)
        and verifier_mate < 0
    )


def all_repeats_transition(
    rows: list[dict],
    budgets: list[int],
    field: str,
) -> int | None:
    for budget in budgets:
        budget_rows = [row for row in rows if row["budgetMs"] == budget]
        if budget_rows and all(row[field] for row in budget_rows):
            return budget
    return None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", default=E.DEFAULT_RAW)
    parser.add_argument("--fen", default=E.FORENSIC_FEN)
    parser.add_argument("--candidate", default="g2f3")
    parser.add_argument("--budgets", default="5,10,25,50,100,250,500")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--allow-background-load", action="store_true")
    args = parser.parse_args()

    E.assert_idle(args.allow_background_load)
    registry = B.load_engine_registry()
    raw_cfg = B.registered_engine(args.raw, registry, depth=30, threads=1)
    sentinel_cfg = sentinel_config(raw_cfg)
    child = child_fen(args.fen, args.candidate)
    budgets = [int(value) for value in args.budgets.split(",")]

    sentinel_engine = B.Engine(sentinel_cfg)
    verifier_engine = B.Engine(raw_cfg)
    rows = []
    try:
        E.warm(sentinel_engine, min(budgets))
        E.warm(verifier_engine, min(budgets))
        for budget_index, budget in enumerate(budgets):
            for repeat in range(args.repeats):
                sentinel_first = (budget_index + repeat) % 2 == 0
                if sentinel_first:
                    sentinel = E.result_record(
                        sentinel_engine.search_time(child, budget)
                    )
                    verifier = E.result_record(
                        verifier_engine.search_time(
                            args.fen,
                            budget,
                            forced_move=args.candidate,
                        )
                    )
                else:
                    verifier = E.result_record(
                        verifier_engine.search_time(
                            args.fen,
                            budget,
                            forced_move=args.candidate,
                        )
                    )
                    sentinel = E.result_record(
                        sentinel_engine.search_time(child, budget)
                    )
                verified = is_verified_mate_alarm(sentinel, verifier)
                row = {
                    "budgetMs": budget,
                    "repeat": repeat,
                    "firstEngine": "sentinel" if sentinel_first else "verifier",
                    "sentinel": sentinel,
                    "verifier": verifier,
                    "sentinelMateAlarm": (
                        isinstance(sentinel.get("mate"), int)
                        and sentinel["mate"] > 0
                    ),
                    "verifiedMateAlarm": verified,
                }
                rows.append(row)
                print(
                    f"{budget:4d}ms r{repeat + 1}: "
                    f"sentinel={sentinel['move']} mate={sentinel.get('mate')} "
                    f"d{sentinel['depth']} | "
                    f"verify mate={verifier.get('mate')} d{verifier['depth']} "
                    f"verified={verified}",
                    flush=True,
                )
    finally:
        sentinel_engine.close()
        verifier_engine.close()

    first_sentinel = all_repeats_transition(
        rows,
        budgets,
        "sentinelMateAlarm",
    )
    first_verified = all_repeats_transition(
        rows,
        budgets,
        "verifiedMateAlarm",
    )
    B.write_result(
        "tactical-sentinel-forensic",
        {
            "schemaVersion": 1,
            "fen": args.fen,
            "candidate": args.candidate,
            "childFen": child,
            "budgetsMs": budgets,
            "repeats": args.repeats,
            "raw": B.provenance(raw_cfg),
            "sentinel": B.provenance(sentinel_cfg),
            "firstAllRepeatsSentinelMateAlarmMs": first_sentinel,
            "firstAllRepeatsVerifiedMateAlarmMs": first_verified,
            "authority": "experimental proof-only; no live move authority",
            "rows": rows,
        },
    )


if __name__ == "__main__":
    main()
