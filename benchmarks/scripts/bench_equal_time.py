#!/usr/bin/env python3
"""Paired equal-time Raw versus Hybrid benchmark on a frozen suite."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import statistics
import subprocess
from pathlib import Path

import benchlib as B


FORENSIC_FEN = "4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32"
DEFAULT_RAW = "g9.raw-control.matrix-raw"
DEFAULT_HYBRID = "g9.hybrid-a.raw-plus-residual"
DEFAULT_SUITE = "suite-clean-postmodel-20260619"


def assert_exact_pair(raw: dict, hybrid: dict) -> None:
    checks = {
        "binary": (raw["exe"], hybrid["exe"]),
        "main network": (raw["net"], hybrid["net"]),
        "base weights": (raw["base_weights"], hybrid["base_weights"]),
        "rung2 weights": (raw["rung2_weights"], hybrid["rung2_weights"]),
        "search profile": (
            raw["search_profile_sha"],
            hybrid["search_profile_sha"],
        ),
        "threads": (raw["threads"], hybrid["threads"]),
    }
    mismatches = [
        f"{name}: {left!r} != {right!r}"
        for name, (left, right) in checks.items()
        if left != right
    ]
    if raw.get("helper_net"):
        mismatches.append("Raw control unexpectedly has a helper network")
    if not hybrid.get("helper_net"):
        mismatches.append("Hybrid candidate has no helper network")
    if mismatches:
        raise SystemExit("invalid equal-time pair:\n  " + "\n  ".join(mismatches))


def assert_idle(allow_background_load: bool) -> None:
    if allow_background_load or os.name != "nt":
        return
    script = (
        "$rows=Get-CimInstance Win32_Process | Where-Object { "
        "$_.Name -eq 'analyze.exe' -or "
        "$_.CommandLine -match 'arena[/\\\\]lichess[/\\\\]run.ts|lichess:bot' };"
        "$rows | Select-Object Name,ProcessId,CommandLine | ConvertTo-Json -Compress"
    )
    output = subprocess.check_output(
        ["powershell", "-NoProfile", "-Command", script],
        text=True,
        stderr=subprocess.STDOUT,
    ).strip()
    if output:
        raise SystemExit(
            "background engine/bot processes detected; stop them before timing:\n"
            + output
        )


def load_score_cache(path: Path) -> dict[tuple[str, str, int], int | None]:
    rows: dict[tuple[str, str, int], int | None] = {}
    if not path.exists():
        return rows
    with path.open(encoding="utf-8-sig") as handle:
        for line in handle:
            if not line.strip():
                continue
            row = json.loads(line)
            rows[(row["fen"], row["move"], row["depth"])] = row["cp"]
    return rows


class DiskScorer:
    def __init__(self, path: Path, depth: int):
        self.path = path
        self.depth = depth
        self.cache = load_score_cache(path)
        self.engine = B.Stockfish(depth=depth)
        path.parent.mkdir(parents=True, exist_ok=True)
        self.writer = path.open("a", encoding="utf8", newline="\n")

    def child_cp(self, fen: str, move: str | None) -> int | None:
        if not move:
            return None
        key = (fen, move, self.depth)
        if key not in self.cache:
            cp = self.engine.child_cp(fen, move)
            self.cache[key] = cp
            self.writer.write(
                json.dumps(
                    {"fen": fen, "move": move, "depth": self.depth, "cp": cp},
                    separators=(",", ":"),
                )
                + "\n"
            )
            self.writer.flush()
        return self.cache[key]

    def close(self) -> None:
        self.writer.close()
        self.engine.close()


def stable_final_depth(iterations: list[dict], final_move: str | None) -> int | None:
    if not final_move:
        return None
    for index, iteration in enumerate(iterations):
        if iteration.get("uci") == final_move and all(
            later.get("uci") == final_move for later in iterations[index:]
        ):
            return iteration.get("depth")
    return None


def result_record(result: dict) -> dict:
    elapsed = max(1, result.get("timeMs") or 0)
    iterations = result.get("iterations") or []
    moves = [row.get("uci") for row in iterations]
    final_move = result.get("uci")
    return {
        "move": final_move,
        "scoreCp": result.get("scoreCp"),
        "depth": result.get("depth"),
        "nodes": result.get("nodes"),
        "qNodes": result.get("qNodes"),
        "timeMs": result.get("timeMs"),
        "nps": (result.get("nodes") or 0) * 1000 / elapsed,
        "pv": result.get("pv") or [],
        "rootOrder": result.get("rootOrder") or [],
        "iterations": iterations,
        "firstStableFinalDepth": stable_final_depth(iterations, final_move),
        "bestMoveChanges": sum(
            moves[index] != moves[index - 1]
            for index in range(1, len(moves))
        ),
    }


def bootstrap_mean_ci(
    values: list[float],
    seed: str,
    samples: int = 20000,
) -> tuple[float | None, float | None]:
    if not values:
        return None, None
    rng = random.Random(seed)
    count = len(values)
    means = sorted(
        statistics.mean(values[rng.randrange(count)] for _ in range(count))
        for _ in range(samples)
    )
    return means[int(samples * 0.025)], means[int(samples * 0.975)]


def paired_summary(rows: list[dict], seed: str) -> dict:
    scored = [
        row for row in rows
        if row["raw"].get("cpLoss") is not None
        and row["hybrid"].get("cpLoss") is not None
    ]
    deltas = [
        row["hybrid"]["cpLoss"] - row["raw"]["cpLoss"]
        for row in scored
    ]
    low, high = bootstrap_mean_ci(deltas, seed)
    return {
        "positions": len(rows),
        "scored": len(scored),
        "meanPairedDeltaCp": statistics.mean(deltas) if deltas else None,
        "medianPairedDeltaCp": statistics.median(deltas) if deltas else None,
        "bootstrap95MeanDeltaCp": [low, high],
        "hybridWins": sum(delta < 0 for delta in deltas),
        "rawWins": sum(delta > 0 for delta in deltas),
        "ties": sum(delta == 0 for delta in deltas),
        "catastrophicSaves1000": sum(
            row["raw"]["cpLoss"] >= 1000
            and row["hybrid"]["cpLoss"] < 200
            for row in scored
        ),
        "catastrophicRegressions1000": sum(
            row["hybrid"]["cpLoss"] >= 1000
            and row["raw"]["cpLoss"] < 200
            for row in scored
        ),
        "rawFailures": {
            str(threshold): sum(row["raw"]["cpLoss"] >= threshold for row in scored)
            for threshold in (100, 200, 1000)
        },
        "hybridFailures": {
            str(threshold): sum(
                row["hybrid"]["cpLoss"] >= threshold for row in scored
            )
            for threshold in (100, 200, 1000)
        },
        "rawAvgCp": statistics.mean(
            row["raw"]["cpLoss"] for row in scored
        ) if scored else None,
        "hybridAvgCp": statistics.mean(
            row["hybrid"]["cpLoss"] for row in scored
        ) if scored else None,
        "rawMedianDepth": statistics.median(
            row["raw"]["depth"] for row in scored
        ) if scored else None,
        "hybridMedianDepth": statistics.median(
            row["hybrid"]["depth"] for row in scored
        ) if scored else None,
        "rawMedianNodes": statistics.median(
            row["raw"]["nodes"] for row in scored
        ) if scored else None,
        "hybridMedianNodes": statistics.median(
            row["hybrid"]["nodes"] for row in scored
        ) if scored else None,
    }


def warm(engine: B.Engine, ms: int) -> None:
    start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    for _ in range(2):
        engine.search_time(start, ms)


def run_searches(
    raw_engine: B.Engine,
    hybrid_engine: B.Engine,
    fens: list[str],
    oracle: list[str],
    budgets: list[int],
    seed: str,
) -> dict[str, list[dict]]:
    order = list(range(len(fens)))
    random.Random(seed).shuffle(order)
    output: dict[str, list[dict]] = {}
    for budget_index, budget in enumerate(budgets):
        rows = []
        for sequence_index, position_index in enumerate(order):
            fen = fens[position_index]
            first_raw = (sequence_index + budget_index) % 2 == 0
            if first_raw:
                raw = result_record(raw_engine.search_time(fen, budget))
                hybrid = result_record(hybrid_engine.search_time(fen, budget))
            else:
                hybrid = result_record(hybrid_engine.search_time(fen, budget))
                raw = result_record(raw_engine.search_time(fen, budget))
            rows.append(
                {
                    "positionIndex": position_index,
                    "sequenceIndex": sequence_index,
                    "firstEngine": "raw" if first_raw else "hybrid",
                    "fen": fen,
                    "oracleMove": oracle[position_index],
                    "raw": raw,
                    "hybrid": hybrid,
                }
            )
        output[str(budget)] = rows
        print(f"search complete: {budget}ms x {len(rows)} positions", flush=True)
    return output


def checkpoint_identity(
    raw_cfg: dict,
    hybrid_cfg: dict,
    suite: dict,
    count: int,
    budgets: list[int],
    seed: str,
) -> dict:
    return {
        "raw": raw_cfg["id"],
        "hybrid": hybrid_cfg["id"],
        "suite": suite["name"],
        "suiteHash": suite["hash"],
        "positions": count,
        "budgetsMs": budgets,
        "seed": seed,
        "rawPolicySha": raw_cfg["policy_sha"],
        "hybridPolicySha": hybrid_cfg["policy_sha"],
    }


def load_checkpoint(path: Path, identity: dict) -> dict[str, list[dict]] | None:
    if not path.exists():
        return None
    payload = json.loads(path.read_text(encoding="utf-8-sig"))
    if payload.get("identity") != identity:
        raise SystemExit(
            f"checkpoint identity mismatch: {path}\n"
            "remove it or choose a different --checkpoint path"
        )
    print(f"resuming completed searches from {path}", flush=True)
    return payload["rows"]


def save_checkpoint(
    path: Path,
    identity: dict,
    rows: dict[str, list[dict]],
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {"schemaVersion": 1, "identity": identity, "rows": rows},
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"search checkpoint written: {path}", flush=True)


def score_rows(
    rows_by_budget: dict[str, list[dict]],
    scorer: DiskScorer,
) -> None:
    for budget, rows in rows_by_budget.items():
        for index, row in enumerate(rows, start=1):
            expected = scorer.child_cp(row["fen"], row["oracleMove"])
            for side in ("raw", "hybrid"):
                candidate = scorer.child_cp(row["fen"], row[side]["move"])
                row[side]["oracleChildCp"] = expected
                row[side]["candidateChildCp"] = candidate
                row[side]["cpLoss"] = (
                    None
                    if expected is None or candidate is None
                    else max(0, expected - candidate)
                )
            if index % 10 == 0:
                print(f"scored {budget}ms: {index}/{len(rows)}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", default=DEFAULT_RAW)
    parser.add_argument("--hybrid", default=DEFAULT_HYBRID)
    parser.add_argument("--suite", default=DEFAULT_SUITE)
    parser.add_argument("--budgets", default="25,50,100,250,500,1000,2000")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--seed", default="equal-time-clean-20260619-v1")
    parser.add_argument("--sf-depth", type=int, default=24)
    parser.add_argument(
        "--score-cache",
        default="benchmarks/results/equal-time-sf24-cache.jsonl",
    )
    parser.add_argument(
        "--checkpoint",
        default="benchmarks/results/equal-time-search-checkpoint.json",
    )
    parser.add_argument("--allow-background-load", action="store_true")
    args = parser.parse_args()

    assert_idle(args.allow_background_load)
    registry = B.load_engine_registry()
    raw_cfg = B.registered_engine(args.raw, registry, depth=30, threads=1)
    hybrid_cfg = B.registered_engine(args.hybrid, registry, depth=30, threads=1)
    assert_exact_pair(raw_cfg, hybrid_cfg)
    suite = B.load_suite(args.suite)
    count = min(args.limit or len(suite["fens"]), len(suite["fens"]))
    fens = suite["fens"][:count]
    oracle = suite["oracle"][:count]
    budgets = [int(value) for value in args.budgets.split(",")]
    checkpoint_path = Path(args.checkpoint)
    identity = checkpoint_identity(
        raw_cfg,
        hybrid_cfg,
        suite,
        count,
        budgets,
        args.seed,
    )
    rows = load_checkpoint(checkpoint_path, identity)
    if rows is None:
        raw_engine = B.Engine(raw_cfg)
        hybrid_engine = B.Engine(hybrid_cfg)
        try:
            warm(raw_engine, min(budgets))
            warm(hybrid_engine, min(budgets))
            rows = run_searches(
                raw_engine,
                hybrid_engine,
                fens,
                oracle,
                budgets,
                args.seed,
            )
        finally:
            raw_engine.close()
            hybrid_engine.close()
        save_checkpoint(checkpoint_path, identity, rows)

    scorer = DiskScorer(Path(args.score_cache), args.sf_depth)
    try:
        score_rows(rows, scorer)
    finally:
        scorer.close()

    summaries = {
        budget: paired_summary(budget_rows, f"{args.seed}:{budget}")
        for budget, budget_rows in rows.items()
    }
    for budget, summary in summaries.items():
        print(
            f"{budget:>5s}ms delta={summary['meanPairedDeltaCp']:7.2f} "
            f"CI={summary['bootstrap95MeanDeltaCp']} "
            f"H/R/T={summary['hybridWins']}/{summary['rawWins']}/{summary['ties']} "
            f"save/reg={summary['catastrophicSaves1000']}/"
            f"{summary['catastrophicRegressions1000']}"
        )

    B.write_result(
        "equal-time-paired",
        {
            "schemaVersion": 1,
            "suite": {
                "name": args.suite,
                "hash": suite["hash"],
                "positions": count,
                "orderSeed": args.seed,
            },
            "pair": {
                "raw": B.provenance(raw_cfg),
                "hybrid": B.provenance(hybrid_cfg),
            },
            "budgetsMs": budgets,
            "oracle": {"stockfishDepth": args.sf_depth},
            "summaries": summaries,
            "rows": rows,
        },
    )
    Path(args.score_cache).unlink(missing_ok=True)
    checkpoint_path.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
