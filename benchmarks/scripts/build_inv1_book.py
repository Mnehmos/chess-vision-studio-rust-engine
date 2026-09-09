#!/usr/bin/env python3
"""Build the INV-1 gate opening book: distinct, filtered positions for fixed-node A/B.

Why this exists (2026-09-09 integrity finding): the INV-1 ladder reused
`F:/tools/openings.epd` (12 positions) for every batch, so each 120-game batch
replayed the same 24 distinct games. The SPRT counted those repeats as independent
and reported crossings the unique games do not support. A gate book must supply a
position no other game in the run has used.

Sources, deduped on the first four FEN fields:
  --harvest   harvested Lichess positions (arena/out/lichess-dataset.jsonl): real
              game positions carrying a phase label; opening/middlegame only.
  --control   the historical 12-opening control book, kept so the new book still
              covers the positions every previous gate was played from.

Filters (applied to every candidate position):
  * fullmove 4..16 and halfmove clock <= 6 (skip the first moves and shuffling)
  * material difference within 2 pawns (skip already-decided positions)
  * at least 2 non-pawn pieces per side and 6 non-pawn pieces total

Sampling is deterministic (`--seed`), so the book is reproducible from the same
source file. Output: `<out>.epd` (four-field FEN per line, ready for
`cutechess-cli -openings file=... format=epd`) and `<out>.provenance.json`
(counts, filters, source hashes, book sha256).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
from pathlib import Path

PIECE_VALUE = {"P": 1.0, "N": 3.0, "B": 3.25, "R": 5.0, "Q": 9.0, "K": 0.0}
REPO = Path(__file__).resolve().parents[2]
DEFAULT_HARVEST = Path("F:/Github/chess-vision-studio/arena/out/lichess-dataset.jsonl")
DEFAULT_CONTROL = Path("F:/tools/openings.epd")


def fen_key(fen: str) -> str:
    """Dedupe key: board, side to move, castling, en passant (drop the clocks)."""
    return " ".join(fen.split()[:4])


def epd_line(fen: str) -> str:
    """Four-field EPD line (cutechess reads clocks itself and resets them)."""
    return fen_key(fen)


def material(board: str) -> tuple[float, float, int, int]:
    """(white material, black material, white non-pawns, black non-pawns)."""
    wm = bm = 0.0
    wn = bn = 0
    for ch in board:
        if ch == "/" or ch.isdigit():
            continue
        up = ch.upper()
        v = PIECE_VALUE.get(up, 0.0)
        if ch.isupper():
            wm += v
            if up != "K" and up != "P":
                wn += 1
        else:
            bm += v
            if up != "K" and up != "P":
                bn += 1
    return wm, bm, wn, bn


def position_ok(fen: str, min_move: int, max_move: int, max_halfmove: int,
                max_material_diff: float) -> bool:
    parts = fen.split()
    if len(parts) < 4:
        return False
    board = parts[0]
    if board.count("K") != 1 or board.count("k") != 1:
        return False
    if len(parts) >= 6:
        try:
            halfmove, fullmove = int(parts[4]), int(parts[5])
        except ValueError:
            return False
        if halfmove > max_halfmove or not (min_move <= fullmove <= max_move):
            return False
    wm, bm, wn, bn = material(board)
    if abs(wm - bm) > max_material_diff:
        return False
    if wn < 2 or bn < 2 or wn + bn < 6:
        return False
    return True


def load_harvest(path: Path, *, want: int, seed: int, min_move: int, max_move: int,
                 max_halfmove: int, max_material_diff: float) -> list[str]:
    """Reservoir-sample `want` qualifying opening/middlegame positions."""
    rng = random.Random(seed)
    picked: list[str] = []
    seen = 0
    with path.open(encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get("features", {}).get("phase") not in ("opening", "middlegame"):
                continue
            fen = row.get("fen") or ""
            if not position_ok(fen, min_move, max_move, max_halfmove, max_material_diff):
                continue
            seen += 1
            if len(picked) < want:
                picked.append(fen)
            else:
                j = rng.randrange(seen)
                if j < want:
                    picked[j] = fen
    return picked


def load_control(path: Path) -> list[str]:
    if not path.exists():
        return []
    return [ln.strip() for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def build(harvest: Path, control: Path, out: Path, *, want_harvest: int, seed: int,
          min_move: int, max_move: int, max_halfmove: int, max_material_diff: float) -> dict:
    controls = load_control(control)
    sampled = load_harvest(harvest, want=want_harvest, seed=seed, min_move=min_move,
                           max_move=max_move, max_halfmove=max_halfmove,
                           max_material_diff=max_material_diff)
    by_key: dict[str, str] = {}
    for fen in controls + sampled:
        key = fen_key(fen)
        if key not in by_key:
            by_key[key] = fen
    lines = [epd_line(fen) for fen in by_key.values()]
    random.Random(seed).shuffle(lines)

    out.parent.mkdir(parents=True, exist_ok=True)
    text = "\n".join(lines) + "\n"
    out.write_text(text, encoding="utf-8")
    prov = {
        "book": out.name,
        "positions": len(lines),
        "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
        "seed": seed,
        "sources": {
            "control": {"path": str(control), "positions": len(controls)},
            "harvest": {"path": str(harvest), "sampled": len(sampled),
                        "sha256": hashlib.sha256(harvest.read_bytes()).hexdigest()
                        if harvest.exists() else None},
        },
        "filters": {
            "fullmove": [min_move, max_move],
            "maxHalfmoveClock": max_halfmove,
            "maxMaterialDiffPawns": max_material_diff,
            "minNonPawnsPerSide": 2,
            "minNonPawnsTotal": 6,
            "phases": ["opening", "middlegame"],
        },
    }
    out.with_suffix(".provenance.json").write_text(json.dumps(prov, indent=2) + "\n",
                                                   encoding="utf-8")
    return prov


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--harvest", type=Path, default=DEFAULT_HARVEST)
    ap.add_argument("--control", type=Path, default=DEFAULT_CONTROL)
    ap.add_argument("--out", type=Path,
                    default=REPO / "benchmarks/suites/openings-inv1-20260909.epd")
    ap.add_argument("--want-harvest", type=int, default=1600)
    ap.add_argument("--seed", type=int, default=20260909)
    ap.add_argument("--min-move", type=int, default=4)
    ap.add_argument("--max-move", type=int, default=16)
    ap.add_argument("--max-halfmove", type=int, default=6)
    ap.add_argument("--max-material-diff", type=float, default=2.0)
    args = ap.parse_args(argv)
    prov = build(args.harvest, args.control, args.out, want_harvest=args.want_harvest,
                 seed=args.seed, min_move=args.min_move, max_move=args.max_move,
                 max_halfmove=args.max_halfmove, max_material_diff=args.max_material_diff)
    print(json.dumps(prov, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
