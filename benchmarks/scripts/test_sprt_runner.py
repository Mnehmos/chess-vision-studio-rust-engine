#!/usr/bin/env python3
"""Tests for the canonical SPRT match runner (issue #5). Pure — an injected list of game scores
stands in for real games, so no binary is needed. Standalone:
`python benchmarks/scripts/test_sprt_runner.py`, or via pytest."""
from __future__ import annotations

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import sprt_runner as sr  # noqa: E402
import lint_promotion as lp  # noqa: E402

ALPHA, BETA = 0.05, 0.05
ELO0, ELO1 = 0.0, 5.0

PROV = {
    "engineSha": "deadbeefcafef00d",
    "netSha": "feedface12345678",
    "candArgs": ["--nnue", "cand.json"],
    "baseArgs": ["--nnue", "champion.json"],
    "tc": "nodes:80000",
    "nodes": 80000,
    "threads": 1,
    "openingsSha": "aabbccddeeff0011",
}


def _record(run):
    return sr.build_record(
        run,
        experiment_id="test",
        baseline_id="snapshot/N0",
        candidate_id="cand/x",
        elo0=ELO0,
        elo1=ELO1,
        alpha=ALPHA,
        beta=BETA,
        provenance=PROV,
    )


def test_bounds_match_lint_promotion_formulas():
    lower, upper = sr.sprt_bounds(ALPHA, BETA)
    assert abs(upper - math.log((1 - BETA) / ALPHA)) < 1e-12
    assert abs(lower - math.log(BETA / (1 - ALPHA))) < 1e-12
    assert upper > 0 > lower


def test_llr_is_zero_until_all_three_outcomes_seen():
    # No draws yet -> draw_elo undefined -> keep sampling (never crash).
    assert sr.llr_trinomial(10, 0, 0, ELO0, ELO1) == 0.0
    assert sr.llr_trinomial(0, 5, 0, ELO0, ELO1) == 0.0
    assert sr.llr_trinomial(5, 5, 0, ELO0, ELO1) == 0.0


def test_winning_evidence_gives_positive_llr_losing_gives_negative():
    winning = sr.llr_trinomial(60, 40, 20, ELO0, ELO1)
    losing = sr.llr_trinomial(20, 40, 60, ELO0, ELO1)
    assert winning > 0.0, "a candidate that wins more than it loses supports H1"
    assert losing < 0.0, "a candidate that loses more supports H0"


def test_strong_candidate_crosses_upper_and_promotes():
    # A decisive +score stream must terminate at the UPPER boundary -> promote.
    scores = [sr.WIN, sr.DRAW, sr.LOSS] * 5 + [sr.WIN] * 5000
    run = sr.run_sprt(scores, ELO0, ELO1, ALPHA, BETA)
    assert run["boundary"] == "upper"
    assert run["decision"] == "promote"
    assert run["llr"] >= run["upperBound"]
    # Sequential: it must STOP at the crossing, not consume all 5000+ games.
    assert run["games"] < 5015


def test_weak_candidate_crosses_lower_and_rejects():
    scores = [sr.WIN, sr.DRAW, sr.LOSS] * 5 + [sr.LOSS] * 5000
    run = sr.run_sprt(scores, ELO0, ELO1, ALPHA, BETA)
    assert run["boundary"] == "lower"
    assert run["decision"] == "reject"
    assert run["llr"] <= run["lowerBound"]


def test_inconclusive_stream_holds_at_the_game_cap():
    # Perfectly balanced -> LLR hovers near 0, never crosses; cap forces a HOLD.
    scores = [sr.WIN, sr.LOSS, sr.DRAW] * 40
    run = sr.run_sprt(scores, ELO0, ELO1, ALPHA, BETA, max_games=120)
    assert run["games"] == 120
    assert run["boundary"] == "none"
    assert run["decision"] == "hold_for_more_data"


def test_rejects_malformed_hypotheses():
    for kwargs in (
        {"elo0": 5.0, "elo1": 0.0},  # elo1 must exceed elo0
        {"alpha": 0.9},  # out of (0, 0.5)
        {"beta": 0.0},
    ):
        base = {"elo0": ELO0, "elo1": ELO1, "alpha": ALPHA, "beta": BETA}
        base.update(kwargs)
        try:
            sr.run_sprt([sr.DRAW], **base)
            raise AssertionError(f"expected ValueError for {kwargs}")
        except ValueError:
            pass


def test_produced_records_pass_the_promotion_linter():
    # The closed loop: whatever this runner emits must satisfy the existing promotion gate.
    for scores in (
        [sr.WIN, sr.DRAW, sr.LOSS] * 5 + [sr.WIN] * 5000,  # promote/upper
        [sr.WIN, sr.DRAW, sr.LOSS] * 5 + [sr.LOSS] * 5000,  # reject/lower
        [sr.WIN, sr.LOSS, sr.DRAW] * 40,  # hold/none
    ):
        run = sr.run_sprt(scores, ELO0, ELO1, ALPHA, BETA, max_games=200)
        rec = _record(run)
        violations = lp.lint_record(rec)
        assert violations == [], f"linter rejected a produced record: {violations}"
        # counts are internally consistent
        assert rec["wins"] + rec["losses"] + rec["draws"] == rec["games"]


def test_result_row_parsing_cutechess_and_score():
    assert sr.score_from_result_row({"score": 1.0}) == sr.WIN
    assert sr.score_from_result_row({"result": "1/2-1/2"}) == sr.DRAW
    # candidate as white winning
    assert sr.score_from_result_row({"result": "1-0", "candidateColor": "white"}) == sr.WIN
    # candidate as white but black won -> loss
    assert sr.score_from_result_row({"result": "0-1", "candidateColor": "white"}) == sr.LOSS
    # candidate as black winning
    assert sr.score_from_result_row({"result": "0-1", "candidateColor": "black"}) == sr.WIN


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for fn in fns:
        fn()
        print(f"ok  {fn.__name__}")
    print(f"\n{len(fns)} passed")


if __name__ == "__main__":
    _run_all()
