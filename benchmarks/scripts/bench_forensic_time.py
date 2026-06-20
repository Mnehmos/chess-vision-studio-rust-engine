#!/usr/bin/env python3
"""Millisecond ladder for the clean-holdout horizon failure."""

from __future__ import annotations

import argparse
import json

import bench_equal_time as E
import benchlib as B


def transition_summary(
    rows: list[dict],
    budgets: list[int],
    bad_move: str,
) -> tuple[dict, dict, dict]:
    per_budget = {}
    for budget in budgets:
        budget_rows = [row for row in rows if row["budgetMs"] == budget]
        per_budget[str(budget)] = {
            side: {
                "avoidedBadMove": sum(
                    row[side]["move"] != bad_move for row in budget_rows
                ),
                "repeats": len(budget_rows),
                "moves": [row[side]["move"] for row in budget_rows],
            }
            for side in ("raw", "hybrid")
        }

    first_all_repeats = {}
    first_sustained = {}
    for side in ("raw", "hybrid"):
        clear = [
            per_budget[str(budget)][side]["avoidedBadMove"]
            == per_budget[str(budget)][side]["repeats"]
            for budget in budgets
        ]
        first_all_repeats[side] = next(
            (budget for budget, is_clear in zip(budgets, clear) if is_clear),
            None,
        )
        first_sustained[side] = next(
            (
                budget
                for index, budget in enumerate(budgets)
                if all(clear[index:])
            ),
            None,
        )
    return per_budget, first_all_repeats, first_sustained


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", default=E.DEFAULT_RAW)
    parser.add_argument("--hybrid", default=E.DEFAULT_HYBRID)
    parser.add_argument("--fen", default=E.FORENSIC_FEN)
    parser.add_argument("--budgets", default="10,25,50,100,250,500,1000,2000")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--allow-background-load", action="store_true")
    args = parser.parse_args()

    E.assert_idle(args.allow_background_load)
    registry = B.load_engine_registry()
    raw_cfg = B.registered_engine(args.raw, registry, depth=30, threads=1)
    hybrid_cfg = B.registered_engine(args.hybrid, registry, depth=30, threads=1)
    E.assert_exact_pair(raw_cfg, hybrid_cfg)
    budgets = [int(value) for value in args.budgets.split(",")]
    raw_engine = B.Engine(raw_cfg)
    hybrid_engine = B.Engine(hybrid_cfg)
    rows = []
    try:
        E.warm(raw_engine, min(budgets))
        E.warm(hybrid_engine, min(budgets))
        for budget_index, budget in enumerate(budgets):
            for repeat in range(args.repeats):
                raw_first = (budget_index + repeat) % 2 == 0
                if raw_first:
                    raw = E.result_record(raw_engine.search_time(args.fen, budget))
                    hybrid = E.result_record(
                        hybrid_engine.search_time(args.fen, budget)
                    )
                else:
                    hybrid = E.result_record(
                        hybrid_engine.search_time(args.fen, budget)
                    )
                    raw = E.result_record(raw_engine.search_time(args.fen, budget))
                row = {
                    "budgetMs": budget,
                    "repeat": repeat,
                    "firstEngine": "raw" if raw_first else "hybrid",
                    "raw": raw,
                    "hybrid": hybrid,
                }
                rows.append(row)
                print(
                    f"{budget:4d}ms r{repeat + 1}: "
                    f"raw={raw['move']} d{raw['depth']} {raw['timeMs']}ms | "
                    f"hybrid={hybrid['move']} d{hybrid['depth']} "
                    f"{hybrid['timeMs']}ms",
                    flush=True,
                )
    finally:
        raw_engine.close()
        hybrid_engine.close()

    per_budget, first_all_repeats, first_sustained = transition_summary(
        rows,
        budgets,
        "g2f3",
    )
    B.write_result(
        "forensic-time-ladder",
        {
            "schemaVersion": 1,
            "fen": args.fen,
            "badMove": "g2f3",
            "budgetsMs": budgets,
            "repeats": args.repeats,
            "pair": {
                "raw": B.provenance(raw_cfg),
                "hybrid": B.provenance(hybrid_cfg),
            },
            "firstAllRepeatsAvoidingBadMove": first_all_repeats,
            "firstSustainedBudgetAvoidingBadMove": first_sustained,
            "perBudget": per_budget,
            "rows": rows,
        },
    )


if __name__ == "__main__":
    main()
