#!/usr/bin/env python3
"""Split a JSONL file into fixed-size shards for parallel Stockfish relabeling."""

from __future__ import annotations

import argparse
from pathlib import Path


def shard(args: argparse.Namespace) -> tuple[int, int]:
    src = Path(args.input)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    shard_index = -1
    row_in_shard = 0
    total = 0
    out = None
    try:
        with src.open(encoding="utf-8") as fh:
            for line in fh:
                if not line.strip():
                    continue
                if out is None or row_in_shard >= args.rows_per_shard:
                    if out is not None:
                        out.close()
                    shard_index += 1
                    row_in_shard = 0
                    path = out_dir / f"{args.prefix}-{shard_index:04d}.jsonl"
                    out = path.open("w", encoding="utf-8", newline="\n")
                out.write(line)
                row_in_shard += 1
                total += 1
                if args.max_rows and total >= args.max_rows:
                    break
    finally:
        if out is not None:
            out.close()
    return max(shard_index + 1, 0), total


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--prefix", default="shard")
    parser.add_argument("--rows-per-shard", type=int, default=250_000)
    parser.add_argument("--max-rows", type=int, default=0)
    args = parser.parse_args()
    shards, rows = shard(args)
    print(f"wrote {shards} shards / {rows} rows -> {args.out_dir}")


if __name__ == "__main__":
    main()
