#!/usr/bin/env python3
"""Build a source-isolated holdout and audit it against known training data."""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path

import chess
import chess.pgn

import benchlib as B


DEFAULT_SOURCE = (
    "benchmarks/sources/"
    "lichess-post-model-20260618T194300Z-20260620T000430Z.pgn"
)
DEFAULT_STEM = "benchmarks/suites/suite-clean-postmodel-20260619"
DEFAULT_RESERVED = "benchmarks/suites/holdout-reserved-20260619.txt"
DEFAULT_EXCLUSIONS = "benchmarks/holdout-exclusions.json"
DEFAULT_SALT = "cvs-clean-holdout-20260619-v1"


def position_key(fen: str) -> str:
    fields = fen.strip().split()
    if len(fields) < 4:
        raise ValueError(f"invalid FEN: {fen!r}")
    return " ".join(fields[:4])


def resolve_pattern(pattern: str) -> str:
    path = Path(pattern)
    return str(path if path.is_absolute() else Path(B.REPO) / path)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def phase_for(board: chess.Board, ply: int) -> str:
    piece_count = len(board.piece_map())
    nonpawn = sum(
        1
        for piece in board.piece_map().values()
        if piece.piece_type not in (chess.PAWN, chess.KING)
    )
    if ply < 24 and piece_count >= 26:
        return "opening"
    if piece_count <= 12 or nonpawn <= 4:
        return "endgame"
    return "middlegame"


def load_games(path: Path, min_ply: int) -> tuple[list[dict], list[dict], list[dict]]:
    candidates: list[dict] = []
    reserved: list[dict] = []
    games: list[dict] = []
    seen_keys: set[str] = set()
    reserved_keys: set[str] = set()
    with path.open(encoding="utf-8-sig") as handle:
        while True:
            game = chess.pgn.read_game(handle)
            if game is None:
                break
            game_id = game.headers.get("GameId")
            if not game_id:
                raise ValueError("source PGN game lacks GameId")
            game_rows = 0
            board = game.board()
            for ply, move in enumerate(game.mainline_moves(), start=1):
                board.push(move)
                if board.is_game_over():
                    continue
                key = position_key(board.fen())
                if key not in reserved_keys:
                    reserved_keys.add(key)
                    reserved.append(
                        {
                            "fen": board.fen(),
                            "key": key,
                            "gameId": game_id,
                            "ply": ply,
                        }
                    )
                if ply < min_ply:
                    continue
                if key in seen_keys:
                    continue
                seen_keys.add(key)
                candidates.append(
                    {
                        "fen": board.fen(),
                        "key": key,
                        "gameId": game_id,
                        "ply": ply,
                        "phase": phase_for(board, ply),
                        "result": game.headers.get("Result"),
                        "timeControl": game.headers.get("TimeControl"),
                        "utcDate": game.headers.get("UTCDate"),
                        "utcTime": game.headers.get("UTCTime"),
                    }
                )
                game_rows += 1
            games.append(
                {
                    "gameId": game_id,
                    "site": game.headers.get("Site"),
                    "date": game.headers.get("UTCDate"),
                    "time": game.headers.get("UTCTime"),
                    "result": game.headers.get("Result"),
                    "timeControl": game.headers.get("TimeControl"),
                    "candidatePositions": game_rows,
                }
            )
    return candidates, games, reserved


def exclusion_files(manifest_path: Path) -> list[dict]:
    manifest = json.loads(manifest_path.read_text(encoding="utf8"))
    files: list[dict] = []
    for spec in manifest["inputs"]:
        matches = sorted(glob.glob(resolve_pattern(spec["pattern"])))
        if not matches:
            raise FileNotFoundError(f"exclusion pattern matched nothing: {spec['pattern']}")
        for match in matches:
            files.append({**spec, "path": Path(match)})
    return files


def scan_exclusions(candidates: list[dict], files: list[dict]) -> tuple[set[str], list[dict]]:
    candidate_keys = {row["key"] for row in candidates}
    contaminated: set[str] = set()
    reports: list[dict] = []
    for spec in files:
        path: Path = spec["path"]
        rows = malformed = 0
        matched: set[str] = set()
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for raw in handle:
                digest.update(raw)
                if not raw.strip():
                    continue
                rows += 1
                try:
                    text = raw.decode("utf8").strip()
                    fen = json.loads(text)["fen"] if spec["format"] == "jsonl" else text
                    key = position_key(fen)
                except Exception:
                    malformed += 1
                    continue
                if key in candidate_keys:
                    matched.add(key)
                    contaminated.add(key)
        reports.append(
            {
                "generation": spec["generation"],
                "path": str(path),
                "sha256": digest.hexdigest(),
                "rows": rows,
                "malformed": malformed,
                "matchedCandidates": len(matched),
            }
        )
        print(f"scanned {path}: {rows} rows, {len(matched)} candidate overlaps", flush=True)
    return contaminated, reports


def deterministic_rank(row: dict, salt: str) -> str:
    value = f"{salt}\0{row['gameId']}\0{row['ply']}\0{row['key']}"
    return hashlib.sha256(value.encode("utf8")).hexdigest()


def select_rows(
    candidates: list[dict],
    target: int,
    max_per_game: int,
    min_ply_gap: int,
    salt: str,
) -> list[dict]:
    quotas = {
        "opening": target // 4,
        "middlegame": target // 2,
        "endgame": target - (target // 4) - (target // 2),
    }
    ranked = sorted(candidates, key=lambda row: deterministic_rank(row, salt))
    selected: list[dict] = []
    selected_keys: set[str] = set()
    game_plies: dict[str, list[int]] = defaultdict(list)

    def eligible(row: dict) -> bool:
        plies = game_plies[row["gameId"]]
        return (
            row["key"] not in selected_keys
            and len(plies) < max_per_game
            and all(abs(row["ply"] - existing) >= min_ply_gap for existing in plies)
        )

    def take(row: dict) -> None:
        selected.append(row)
        selected_keys.add(row["key"])
        game_plies[row["gameId"]].append(row["ply"])

    for phase, quota in quotas.items():
        for row in ranked:
            if sum(item["phase"] == phase for item in selected) >= quota:
                break
            if row["phase"] == phase and eligible(row):
                take(row)

    for row in ranked:
        if len(selected) >= target:
            break
        if eligible(row):
            take(row)

    if len(selected) < target:
        raise RuntimeError(
            f"only selected {len(selected)} of {target}; loosen max-per-game/min-ply-gap"
        )
    return sorted(selected, key=lambda row: (row["gameId"], row["ply"]))


def write_outputs(
    args: argparse.Namespace,
    source_path: Path,
    candidates: list[dict],
    games: list[dict],
    reserved: list[dict],
    contaminated: set[str],
    exclusions: list[dict],
) -> None:
    clean = [row for row in candidates if row["key"] not in contaminated]
    selected = select_rows(
        clean,
        args.target,
        args.max_per_game,
        args.min_ply_gap,
        args.salt,
    )
    stem = Path(args.out_stem)
    reserved_path = Path(args.reserved_out)
    stem.parent.mkdir(parents=True, exist_ok=True)
    reserved_path.parent.mkdir(parents=True, exist_ok=True)

    stem.with_suffix(".txt").write_text(
        "\n".join(row["fen"] for row in selected) + "\n",
        encoding="utf8",
    )
    reserved_rows = sorted(reserved, key=lambda row: (row["gameId"], row["ply"]))
    reserved_path.write_text(
        "\n".join(row["fen"] for row in reserved_rows) + "\n",
        encoding="utf8",
    )
    metadata = {
        "schemaVersion": 1,
        "suite": stem.name,
        "positionKey": "first four FEN fields",
        "source": {
            "path": str(source_path),
            "sha256": sha256_file(source_path),
            "modelCutoffUtc": "2026-06-18T19:43:00.475Z",
            "fetchedUntilUtc": "2026-06-20T00:04:30Z",
            "account": "ChessVisionStudioEng",
            "games": games,
        },
        "selection": {
            "salt": args.salt,
            "target": args.target,
            "minPly": args.min_ply,
            "maxPerGame": args.max_per_game,
            "minPlyGap": args.min_ply_gap,
            "candidatePositions": len(candidates),
            "contaminatedCandidates": len(contaminated),
            "cleanCandidates": len(clean),
            "selectedByPhase": dict(Counter(row["phase"] for row in selected)),
            "selectedGames": len({row["gameId"] for row in selected}),
        },
        "reserved": {
            "path": str(reserved_path),
            "positions": len(reserved_rows),
            "games": len(games),
        },
        "exclusions": exclusions,
        "positions": selected,
    }
    stem.with_suffix(".source.json").write_text(
        json.dumps(metadata, indent=2) + "\n",
        encoding="utf8",
    )
    print(
        json.dumps(
            {
                "suite": str(stem.with_suffix(".txt")),
                "selected": len(selected),
                "phase": metadata["selection"]["selectedByPhase"],
                "games": metadata["selection"]["selectedGames"],
                "reserved": len(reserved_rows),
                "contaminated": len(contaminated),
            },
            indent=2,
        )
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", default=DEFAULT_SOURCE)
    parser.add_argument("--exclusions", default=DEFAULT_EXCLUSIONS)
    parser.add_argument("--out-stem", default=DEFAULT_STEM)
    parser.add_argument("--reserved-out", default=DEFAULT_RESERVED)
    parser.add_argument("--target", type=int, default=120)
    parser.add_argument("--min-ply", type=int, default=12)
    parser.add_argument("--max-per-game", type=int, default=4)
    parser.add_argument("--min-ply-gap", type=int, default=10)
    parser.add_argument("--salt", default=DEFAULT_SALT)
    args = parser.parse_args()

    source_path = Path(resolve_pattern(args.source))
    manifest_path = Path(resolve_pattern(args.exclusions))
    candidates, games, reserved = load_games(source_path, args.min_ply)
    files = exclusion_files(manifest_path)
    contaminated, reports = scan_exclusions(candidates, files)
    write_outputs(
        args,
        source_path,
        candidates,
        games,
        reserved,
        contaminated,
        reports,
    )


if __name__ == "__main__":
    main()
