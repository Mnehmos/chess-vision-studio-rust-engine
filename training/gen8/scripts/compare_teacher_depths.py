#!/usr/bin/env python3
"""Compare two Stockfish relabel files, usually d12 vs d20.

The app-side worker records `sfCp` in side-to-move POV. This script converts to
White-POV before computing deltas, so signs are stable across positions.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def stm(fen: str) -> str:
    return fen.split()[1]


def white_cp(row: dict) -> int | None:
    cp = row.get("sfCp")
    if not isinstance(cp, int):
        return None
    return cp if stm(row["fen"]) == "w" else -cp


def load(path: str) -> dict[str, dict]:
    rows: dict[str, dict] = {}
    with Path(path).open(encoding="utf-8") as fh:
        for line in fh:
            if not line.strip():
                continue
            row = json.loads(line)
            if "fen" in row:
                rows[row["fen"]] = row
    return rows


def percentile(xs: list[int], pct: float) -> int:
    if not xs:
        return 0
    idx = min(len(xs) - 1, round((len(xs) - 1) * pct))
    return xs[idx]


def compare(args: argparse.Namespace) -> dict:
    shallow = load(args.shallow)
    deep = load(args.deep)
    common = sorted(set(shallow) & set(deep))
    out_path = Path(args.out) if args.out else None
    writer = out_path.open("w", encoding="utf-8", newline="\n") if out_path else None
    deltas: list[int] = []
    signed: list[int] = []
    try:
        for fen in common:
            a = white_cp(shallow[fen])
            b = white_cp(deep[fen])
            if a is None or b is None:
                continue
            delta = b - a
            signed.append(delta)
            deltas.append(abs(delta))
            if writer:
                writer.write(
                    json.dumps(
                        {
                            "fen": fen,
                            "shallowDepth": shallow[fen].get("sfDepth"),
                            "deepDepth": deep[fen].get("sfDepth"),
                            "shallowCpWhite": a,
                            "deepCpWhite": b,
                            "deltaCp": delta,
                            "absDeltaCp": abs(delta),
                        },
                        separators=(",", ":"),
                    )
                    + "\n"
                )
    finally:
        if writer:
            writer.close()

    deltas.sort()
    n = len(deltas)
    summary = {
        "commonRows": len(common),
        "comparedRows": n,
        "avgAbsDeltaCp": round(sum(deltas) / n, 3) if n else 0,
        "avgSignedDeltaCp": round(sum(signed) / n, 3) if n else 0,
        "p50AbsDeltaCp": percentile(deltas, 0.50),
        "p90AbsDeltaCp": percentile(deltas, 0.90),
        "p95AbsDeltaCp": percentile(deltas, 0.95),
        "pctAbsDeltaGe25": round(100 * sum(x >= 25 for x in deltas) / n, 2) if n else 0,
        "pctAbsDeltaGe50": round(100 * sum(x >= 50 for x in deltas) / n, 2) if n else 0,
        "pctAbsDeltaGe100": round(100 * sum(x >= 100 for x in deltas) / n, 2) if n else 0,
    }
    return summary


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--shallow", required=True, help="usually d12 relabel JSONL")
    parser.add_argument("--deep", required=True, help="usually d20 relabel JSONL")
    parser.add_argument("--out", default="")
    args = parser.parse_args()
    print(json.dumps(compare(args), sort_keys=True))


if __name__ == "__main__":
    main()
