#!/usr/bin/env python3
"""Extract gen8 seed rows from benchmark PGNs.

Rows are relabel inputs, not direct trainer rows. They preserve provenance and
keep tactical/noisy flags so later stages can choose quiet-only value training
or side-model training.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Iterable

try:
    import chess
    import chess.pgn
except ImportError as exc:  # pragma: no cover - local environment guard
    raise SystemExit(
        "python-chess is required. Install with: python -m pip install chess"
    ) from exc


PIECE_VALUES = {
    chess.PAWN: 1,
    chess.KNIGHT: 3,
    chess.BISHOP: 3,
    chess.ROOK: 5,
    chess.QUEEN: 9,
}


def result_white(result: str) -> float:
    if result == "1-0":
        return 1.0
    if result == "0-1":
        return 0.0
    return 0.5


def side_name(color: bool) -> str:
    return "white" if color == chess.WHITE else "black"


def material_summary(board: chess.Board) -> dict[str, int]:
    out: dict[str, int] = {}
    non_pawn = 0
    total = 0
    for color, prefix in [(chess.WHITE, "w"), (chess.BLACK, "b")]:
        for piece_type in PIECE_VALUES:
            count = len(board.pieces(piece_type, color))
            value = count * PIECE_VALUES[piece_type]
            out[f"{prefix}{chess.piece_symbol(piece_type).upper()}"] = count
            total += value
            if piece_type != chess.PAWN:
                non_pawn += value
    out["totalValue"] = total
    out["nonPawnValue"] = non_pawn
    return out


def phase_of(board: chess.Board) -> str:
    material = material_summary(board)
    queens = len(board.pieces(chess.QUEEN, chess.WHITE)) + len(board.pieces(chess.QUEEN, chess.BLACK))
    non_pawn = material["nonPawnValue"]
    if queens >= 2 and non_pawn >= 44 and board.fullmove_number <= 12:
        return "opening"
    if queens == 0 or non_pawn <= 20:
        return "endgame"
    return "middlegame"


def header_tags(game: chess.pgn.Game) -> list[str]:
    raw = game.headers.get("BenchmarkFocus", "")
    return [t.strip() for t in raw.split(";") if t.strip()]


def game_id(game: chess.pgn.Game, path: Path, index: int) -> str:
    return (
        game.headers.get("GameId")
        or game.headers.get("Site", "").rstrip("/").split("/")[-1]
        or f"{path.stem}-{index}"
    )


def source_key(path: Path, game: chess.pgn.Game, index: int) -> str:
    gid = game_id(game, path, index)
    return f"pgn:{path.as_posix()}:{gid}"


def iter_games(paths: Iterable[Path]) -> Iterable[tuple[Path, int, chess.pgn.Game]]:
    for path in paths:
        with path.open(encoding="utf-8", errors="replace") as fh:
            index = 0
            while True:
                game = chess.pgn.read_game(fh)
                if game is None:
                    break
                index += 1
                yield path, index, game


def extract(args: argparse.Namespace) -> tuple[int, int]:
    pgn_paths = sorted(Path(args.pgn_dir).glob(args.glob))
    if not pgn_paths:
        raise SystemExit(f"no PGNs matched {args.pgn_dir}/{args.glob}")

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    seen: set[str] = set()
    rows = 0
    games = 0
    with out_path.open("w", encoding="utf-8", newline="\n") as out:
        for path, index, game in iter_games(pgn_paths):
            games += 1
            board = game.board()
            gid = game_id(game, path, index)
            skey = source_key(path, game, index)
            tags = sorted(set(header_tags(game) + args.tag))
            res = game.headers.get("Result", "*")
            res_white = result_white(res)
            move_no = 0
            for move in game.mainline_moves():
                move_no += 1
                fen = board.fen()
                san = board.san(move)
                is_quiet = (
                    not board.is_check()
                    and not board.is_capture(move)
                    and not board.gives_check(move)
                    and move.promotion is None
                )
                if move_no < args.min_ply:
                    board.push(move)
                    continue
                if args.quiet_only and not is_quiet:
                    board.push(move)
                    continue
                if args.dedupe and fen in seen:
                    board.push(move)
                    continue
                seen.add(fen)
                before = board.copy(stack=False)
                board.push(move)
                row = {
                    "fen": fen,
                    "fenAfter": board.fen(),
                    "sourceBucket": args.source_bucket,
                    "sourceFile": path.as_posix(),
                    "sourceKey": skey,
                    "splitKey": skey,
                    "gameId": gid,
                    "event": game.headers.get("Event", ""),
                    "site": game.headers.get("Site", ""),
                    "date": game.headers.get("Date", ""),
                    "ply": move_no,
                    "moveNumber": before.fullmove_number,
                    "sideToMove": side_name(before.turn),
                    "moveUci": move.uci(),
                    "moveSan": san,
                    "isQuiet": is_quiet,
                    "phase": phase_of(before),
                    "material": material_summary(before),
                    "result": res,
                    "resultWhite": res_white,
                    "res": res_white,
                    "tags": tags,
                }
                out.write(json.dumps(row, separators=(",", ":")) + "\n")
                rows += 1
    return games, rows


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pgn-dir", default="benchmarks/games")
    parser.add_argument("--glob", default="*.pgn")
    parser.add_argument("--out", default="training/gen8/seeds/hard-pgn-seeds.jsonl")
    parser.add_argument("--source-bucket", default="hard_pgn")
    parser.add_argument("--tag", action="append", default=[])
    parser.add_argument("--min-ply", type=int, default=1)
    parser.add_argument("--quiet-only", action="store_true")
    parser.add_argument("--no-dedupe", dest="dedupe", action="store_false")
    parser.set_defaults(dedupe=True)
    args = parser.parse_args()
    games, rows = extract(args)
    print(f"wrote {rows} rows from {games} games -> {args.out}")


if __name__ == "__main__":
    main()
