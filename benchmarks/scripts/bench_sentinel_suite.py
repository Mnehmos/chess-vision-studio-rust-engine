#!/usr/bin/env python3
"""Clean-suite precision screen for tactical-sentinel verification requests."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import bench_equal_time as E
import bench_tactical_sentinel as S
import benchlib as B


DEFAULT_EQUAL_TIME_RESULT = (
    "benchmarks/results/20260619-201102-equal-time-paired.json"
)


def summarize(rows: list[dict], loss_threshold: int) -> dict:
    alarms = [row for row in rows if row["evidenceClass"] != "none"]
    actual = [row for row in rows if row["cpLoss"] >= loss_threshold]
    return {
        "positions": len(rows),
        "alarms": len(alarms),
        "exactMateAlarms": sum(
            row["evidenceClass"] == "exact-mate" for row in rows
        ),
        "verifiedMajorLossRequests": sum(
            row["evidenceClass"] == "verified-major-loss" for row in rows
        ),
        "actualMajorLosses": len(actual),
        "truePositives": sum(
            row["evidenceClass"] != "none"
            and row["cpLoss"] >= loss_threshold
            for row in rows
        ),
        "falsePositives": sum(
            row["evidenceClass"] != "none"
            and row["cpLoss"] < loss_threshold
            for row in rows
        ),
        "falseNegatives": sum(
            row["evidenceClass"] == "none"
            and row["cpLoss"] >= loss_threshold
            for row in rows
        ),
        "trueNegatives": sum(
            row["evidenceClass"] == "none"
            and row["cpLoss"] < loss_threshold
            for row in rows
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", default=E.DEFAULT_RAW)
    parser.add_argument("--equal-time-result", default=DEFAULT_EQUAL_TIME_RESULT)
    parser.add_argument("--principal-budget", default="100")
    parser.add_argument("--sentinel-budgets", default="5,10,25")
    parser.add_argument("--major-loss-cp", type=int, default=300)
    parser.add_argument("--decision-margin-cp", type=int, default=50)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--allow-background-load", action="store_true")
    args = parser.parse_args()

    E.assert_idle(args.allow_background_load)
    source_path = Path(args.equal_time_result)
    source = json.loads(source_path.read_text(encoding="utf-8-sig"))
    source_rows = source["rows"][str(args.principal_budget)]
    if args.limit:
        source_rows = source_rows[: args.limit]

    registry = B.load_engine_registry()
    raw_cfg = B.registered_engine(args.raw, registry, depth=30, threads=1)
    sentinel_cfg = S.sentinel_config(raw_cfg)
    budgets = [int(value) for value in args.sentinel_budgets.split(",")]

    sentinel_engine = B.Engine(sentinel_cfg)
    verifier_engine = B.Engine(raw_cfg)
    baseline_engine = B.Engine(raw_cfg)
    rows_by_budget: dict[str, list[dict]] = {}
    try:
        E.warm(sentinel_engine, min(budgets))
        E.warm(verifier_engine, min(budgets))
        E.warm(baseline_engine, min(budgets))
        for budget_index, budget in enumerate(budgets):
            rows = []
            for sequence_index, source_row in enumerate(source_rows):
                fen = source_row["fen"]
                candidate = source_row["raw"]["move"]
                child = S.child_fen(fen, candidate)
                sentinel, verifier, baseline, engine_order = S.run_triplet(
                    sentinel_engine,
                    verifier_engine,
                    baseline_engine,
                    fen,
                    child,
                    candidate,
                    budget,
                    budget_index + sequence_index,
                )
                rows.append(
                    {
                        "positionIndex": source_row["positionIndex"],
                        "fen": fen,
                        "candidate": candidate,
                        "cpLoss": source_row["raw"]["cpLoss"],
                        "engineOrder": engine_order,
                        "sentinel": sentinel,
                        "verifier": verifier,
                        "baseline": baseline,
                        "evidenceClass": S.evidence_class(
                            sentinel,
                            verifier,
                            baseline,
                            candidate,
                            args.major_loss_cp,
                            args.decision_margin_cp,
                        ),
                    }
                )
            rows_by_budget[str(budget)] = rows
            summary = summarize(rows, args.major_loss_cp)
            print(
                f"{budget:4d}ms alarms={summary['alarms']} "
                f"TP/FP/FN/TN={summary['truePositives']}/"
                f"{summary['falsePositives']}/{summary['falseNegatives']}/"
                f"{summary['trueNegatives']}",
                flush=True,
            )
    finally:
        sentinel_engine.close()
        verifier_engine.close()
        baseline_engine.close()

    B.write_result(
        "tactical-sentinel-suite",
        {
            "schemaVersion": 1,
            "sourceResult": str(source_path),
            "sourceSuite": source["suite"],
            "principalBudgetMs": int(args.principal_budget),
            "sentinelBudgetsMs": budgets,
            "majorLossCp": args.major_loss_cp,
            "decisionMarginCp": args.decision_margin_cp,
            "raw": B.provenance(raw_cfg),
            "sentinel": B.provenance(sentinel_cfg),
            "summaries": {
                budget: summarize(rows, args.major_loss_cp)
                for budget, rows in rows_by_budget.items()
            },
            "authority": (
                "verified major loss is a re-search request only; "
                "exact mate requires independent confirmation"
            ),
            "rows": rows_by_budget,
        },
    )


if __name__ == "__main__":
    main()
