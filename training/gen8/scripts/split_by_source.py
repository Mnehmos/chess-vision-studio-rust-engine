#!/usr/bin/env python3
"""Split JSONL by source/game key, never by random position.

Rows sharing `splitKey` stay in the same split. This prevents near-duplicate
positions from the same game/source leaking across train/validation/test.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


SPLITS = ("train", "validation", "test")


def key_for(row: dict, allow_fen_fallback: bool = False) -> str | None:
    """Stable source/game key for splitting. Returns None when the row carries no stable
    key — callers MUST fail closed (INV-3, issue #9). Falling back to the FEN silently turns
    a source split into a position split, leaking near-duplicates across train/val/test."""
    raw = (
        row.get("splitKey")
        or row.get("sourceKey")
        or "|".join(str(row.get(k) or "") for k in ("sourceBucket", "sourceFile", "gameId"))
    )
    key = raw.strip() if isinstance(raw, str) else ""
    # require real content beyond pipe separators and whitespace (review F4/F6).
    if key.replace("|", "").strip():
        return key
    if allow_fen_fallback:
        return row["fen"]  # LEGACY / non-promotable: a position split, NOT a source split
    return None


def bucket(key: str, salt: str) -> float:
    digest = hashlib.sha256((salt + "\0" + key).encode("utf-8")).digest()
    return int.from_bytes(digest[:8], "big") / float(1 << 64)


def split_name(key: str, train_pct: float, validation_pct: float, salt: str) -> str:
    x = bucket(key, salt)
    if x < train_pct:
        return "train"
    if x < train_pct + validation_pct:
        return "validation"
    return "test"


def split(args: argparse.Namespace) -> dict[str, dict[str, int]]:
    total_pct = args.train_pct + args.validation_pct + args.test_pct
    if abs(total_pct - 1.0) > 1e-9:
        raise SystemExit("--train-pct + --validation-pct + --test-pct must equal 1.0")

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    writers = {
        name: (out_dir / f"{args.prefix}-{name}.jsonl").open("w", encoding="utf-8", newline="\n")
        for name in SPLITS
    }
    assignment: dict[str, str] = {}
    stats = {name: {"sources": 0, "rows": 0} for name in SPLITS}
    try:
        with Path(args.input).open(encoding="utf-8") as src:
            for line_no, line in enumerate(src, 1):
                if not line.strip():
                    continue
                row = json.loads(line)
                key = key_for(row, args.allow_fen_fallback)
                if key is None:
                    raise SystemExit(
                        f"row {line_no} has no stable source/game key (INV-3); add source "
                        f"fields or pass --allow-fen-fallback to force a non-promotable "
                        f"position split"
                    )
                name = assignment.get(key)
                if name is None:
                    name = split_name(key, args.train_pct, args.validation_pct, args.salt)
                    assignment[key] = name
                    stats[name]["sources"] += 1
                writers[name].write(json.dumps(row, separators=(",", ":")) + "\n")
                stats[name]["rows"] += 1
    finally:
        for writer in writers.values():
            writer.close()

    assignment_path = out_dir / f"{args.prefix}-source-splits.json"
    assignment_path.write_text(
        json.dumps(
            {
                "input": args.input,
                "salt": args.salt,
                "allowFenFallback": args.allow_fen_fallback,
                "trainPct": args.train_pct,
                "validationPct": args.validation_pct,
                "testPct": args.test_pct,
                "stats": stats,
                "assignments": assignment,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return stats


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--prefix", default="gen8")
    parser.add_argument("--train-pct", type=float, default=0.80)
    parser.add_argument("--validation-pct", type=float, default=0.10)
    parser.add_argument("--test-pct", type=float, default=0.10)
    parser.add_argument("--salt", default="gen8-source-split-v1")
    parser.add_argument(
        "--allow-fen-fallback",
        action="store_true",
        help="LEGACY/non-promotable: split rows lacking a source key by FEN (position split)",
    )
    args = parser.parse_args()
    stats = split(args)
    print(json.dumps(stats, sort_keys=True))


if __name__ == "__main__":
    main()
