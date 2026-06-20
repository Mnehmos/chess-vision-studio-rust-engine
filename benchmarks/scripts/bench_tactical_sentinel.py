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


def evidence_class(
    sentinel: dict,
    verifier: dict,
    baseline: dict | None = None,
    candidate: str | None = None,
    major_loss_cp: int = 300,
    decision_margin_cp: int = 50,
) -> str:
    sentinel_mate = sentinel.get("mate")
    verifier_mate = verifier.get("mate")
    if (
        isinstance(sentinel_mate, int)
        and sentinel_mate > 0
        and isinstance(verifier_mate, int)
        and verifier_mate < 0
    ):
        return "exact-mate"
    sentinel_score = sentinel.get("scoreCp")
    verifier_score = verifier.get("scoreCp")
    if (
        isinstance(sentinel_score, int)
        and sentinel_score >= major_loss_cp
        and isinstance(verifier_score, int)
        and verifier_score <= -major_loss_cp
        and baseline is not None
        and candidate is not None
        and baseline.get("move") != candidate
        and isinstance(baseline.get("scoreCp"), int)
        and baseline["scoreCp"] - verifier_score >= decision_margin_cp
    ):
        return "verified-major-loss"
    return "none"


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


def run_triplet(
    sentinel_engine: B.Engine,
    verifier_engine: B.Engine,
    baseline_engine: B.Engine,
    fen: str,
    child: str,
    candidate: str,
    budget: int,
    rotation: int,
) -> tuple[dict, dict, dict, list[str]]:
    output = {}
    actions = {
        "sentinel": lambda: E.result_record(
            sentinel_engine.search_time(child, budget)
        ),
        "verifier": lambda: E.result_record(
            verifier_engine.search_time(
                fen,
                budget,
                forced_move=candidate,
            )
        ),
        "baseline": lambda: E.result_record(
            baseline_engine.search_time(fen, budget)
        ),
    }
    base_order = ["sentinel", "verifier", "baseline"]
    order = base_order[rotation % 3 :] + base_order[: rotation % 3]
    for name in order:
        output[name] = actions[name]()
    return output["sentinel"], output["verifier"], output["baseline"], order


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", default=E.DEFAULT_RAW)
    parser.add_argument("--fen", default=E.FORENSIC_FEN)
    parser.add_argument("--candidate", default="g2f3")
    parser.add_argument("--budgets", default="5,10,25,50,100,250,500")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--major-loss-cp", type=int, default=300)
    parser.add_argument("--decision-margin-cp", type=int, default=50)
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
    baseline_engine = B.Engine(raw_cfg)
    rows = []
    try:
        E.warm(sentinel_engine, min(budgets))
        E.warm(verifier_engine, min(budgets))
        E.warm(baseline_engine, min(budgets))
        for budget_index, budget in enumerate(budgets):
            for repeat in range(args.repeats):
                sentinel, verifier, baseline, engine_order = run_triplet(
                    sentinel_engine,
                    verifier_engine,
                    baseline_engine,
                    args.fen,
                    child,
                    args.candidate,
                    budget,
                    budget_index + repeat,
                )
                evidence = evidence_class(
                    sentinel,
                    verifier,
                    baseline,
                    args.candidate,
                    args.major_loss_cp,
                    args.decision_margin_cp,
                )
                row = {
                    "budgetMs": budget,
                    "repeat": repeat,
                    "engineOrder": engine_order,
                    "sentinel": sentinel,
                    "verifier": verifier,
                    "baseline": baseline,
                    "sentinelMateAlarm": (
                        isinstance(sentinel.get("mate"), int)
                        and sentinel["mate"] > 0
                    ),
                    "evidenceClass": evidence,
                    "verifiedMateAlarm": evidence == "exact-mate",
                    "verifiedConcern": evidence != "none",
                }
                rows.append(row)
                print(
                    f"{budget:4d}ms r{repeat + 1}: "
                    f"sentinel={sentinel['move']} mate={sentinel.get('mate')} "
                    f"d{sentinel['depth']} | "
                    f"verify mate={verifier.get('mate')} d{verifier['depth']} "
                    f"evidence={evidence}",
                    flush=True,
                )
    finally:
        sentinel_engine.close()
        verifier_engine.close()
        baseline_engine.close()

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
    first_concern = all_repeats_transition(
        rows,
        budgets,
        "verifiedConcern",
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
            "majorLossCp": args.major_loss_cp,
            "decisionMarginCp": args.decision_margin_cp,
            "raw": B.provenance(raw_cfg),
            "sentinel": B.provenance(sentinel_cfg),
            "firstAllRepeatsSentinelMateAlarmMs": first_sentinel,
            "firstAllRepeatsVerifiedMateAlarmMs": first_verified,
            "firstAllRepeatsVerifiedConcernMs": first_concern,
            "authority": "experimental proof-only; no live move authority",
            "rows": rows,
        },
    )


if __name__ == "__main__":
    main()
