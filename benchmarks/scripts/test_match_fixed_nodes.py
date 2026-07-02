#!/usr/bin/env python3
"""Tests for the fixed-node match driver's pure pieces (argv composition, PGN -> rows).
Standalone: `python benchmarks/scripts/test_match_fixed_nodes.py`, or via pytest."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import match_fixed_nodes as mf  # noqa: E402

PGN = """\
[Event "?"]
[White "cand"]
[Black "base"]
[Result "1-0"]

1. e4 e5 2. Nf3 1-0

[Event "?"]
[White "base"]
[Black "cand"]
[Result "1-0"]

1. d4 d5 1-0

[Event "?"]
[White "cand"]
[Black "base"]
[Result "1/2-1/2"]

1. c4 c5 1/2-1/2

[Event "?"]
[White "base"]
[Black "cand"]
[Result "*"]

1. g3 *
"""


def test_pgn_rows_candidate_pov_and_unfinished_skipped():
    rows = mf.pgn_to_results(PGN, "cand")
    assert rows == [
        {"result": "1-0", "candidateColor": "white"},
        {"result": "1-0", "candidateColor": "black"},
        {"result": "1/2-1/2", "candidateColor": "white"},
    ], rows
    assert mf.wld(rows) == (1, 1, 1)


def test_pgn_rejects_a_game_without_the_candidate():
    bad = PGN.replace('[White "cand"]', '[White "other"]').replace('[Black "cand"]', '[Black "other"]')
    try:
        mf.pgn_to_results(bad, "cand")
        raise AssertionError("expected ValueError for a game without the candidate")
    except ValueError:
        pass


def test_rows_feed_sprt_runner():
    import sprt_runner as sr

    rows = mf.pgn_to_results(PGN, "cand")
    scores = [sr.score_from_result_row(r) for r in rows]
    assert scores == [1.0, 0.0, 0.5]


def test_cutechess_cmd_carries_fixed_node_control():
    cand = {"name": "cand", "exe": "uci.exe", "net": "c.json", "helper": None,
            "flags": ["--lmp"], "nodes": 40000}
    base = {"name": "base", "exe": "uci.exe", "net": "b.json", "helper": "h.json",
            "flags": [], "nodes": 10000}
    cmd = mf.build_cutechess_cmd(cand, base, games=8, pgnout="out.pgn", openings="open.epd")
    joined = " ".join(cmd)
    # fixed-node control: infinite clock + per-engine node budgets
    assert "tc=inf" in joined
    assert "nodes=40000" in joined and "nodes=10000" in joined
    # color pairing + reproducible opening order + adjudication bounds
    assert "-repeat" in cmd and "order=sequential" in joined
    assert "-maxmoves" in cmd and "-pgnout" in cmd
    # candidate flag propagated as an engine arg
    assert "arg=--lmp" in cmd
    # helper only for the side that has one
    assert cmd.count("arg=--helper-nnue") == 1


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for fn in fns:
        fn()
        print(f"ok  {fn.__name__}")
    print(f"\n{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
