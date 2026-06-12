#!/usr/bin/env python3
"""Convert relabeled gen8 rows into the existing raw NNUE trainer format.

The app-side Stockfish relabel worker stores `sfCp` in side-to-move POV. The
raw trainer expects `cp` in White-POV and `res` as White's game result.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


DEFAULT_MATE_CP = 32000
DEFAULT_CLIP_CP = 2000


def stm(fen: str) -> str:
    parts = fen.split()
    if len(parts) < 2 or parts[1] not in ("w", "b"):
        raise ValueError(f"bad FEN side-to-move: {fen}")
    return parts[1]


def cp_from_row(row: dict, mate_cp: int) -> int | None:
    if isinstance(row.get("sfCp"), int):
        cp_stm = int(row["sfCp"])
    elif isinstance(row.get("cp"), int):
        return int(row["cp"])
    elif isinstance(row.get("sfMate"), int):
        mate = int(row["sfMate"])
        sign = 1 if mate > 0 else -1
        cp_stm = sign * (mate_cp - min(abs(mate), 100))
    else:
        return None

    return cp_stm if stm(row["fen"]) == "w" else -cp_stm


def clip_cp(cp: int, limit: int) -> int:
    if limit <= 0:
        return cp
    return max(-limit, min(limit, cp))


def res_from_row(row: dict) -> float:
    for key in ("res", "resultWhite"):
        value = row.get(key)
        if isinstance(value, (int, float)):
            return float(value)
    result = row.get("result")
    if result == "1-0":
        return 1.0
    if result == "0-1":
        return 0.0
    if result == "1/2-1/2":
        return 0.5
    return 0.5


def convert(args: argparse.Namespace) -> tuple[int, int, int]:
    in_path = Path(args.input)
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    read = wrote = skipped = 0
    with in_path.open(encoding="utf-8") as src, out_path.open("w", encoding="utf-8", newline="\n") as out:
        for line in src:
            if not line.strip():
                continue
            read += 1
            try:
                row = json.loads(line)
                fen = row["fen"]
                cp = cp_from_row(row, args.mate_cp)
                if cp is None:
                    skipped += 1
                    continue
                cp = clip_cp(cp, args.clip_cp)
                item = {"fen": fen, "cp": cp, "res": res_from_row(row)}
            except Exception:
                skipped += 1
                continue
            out.write(json.dumps(item, separators=(",", ":")) + "\n")
            wrote += 1
    return read, wrote, skipped


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument(
        "--mate-cp",
        type=int,
        default=DEFAULT_MATE_CP,
        help="side-to-move centipawn value used for mate labels before POV conversion",
    )
    parser.add_argument(
        "--clip-cp",
        type=int,
        default=DEFAULT_CLIP_CP,
        help="absolute White-POV cp clipping limit; use 0 to disable",
    )
    args = parser.parse_args()
    read, wrote, skipped = convert(args)
    print(f"read {read}, wrote {wrote}, skipped {skipped} -> {args.out}")


if __name__ == "__main__":
    main()
