#!/usr/bin/env python3
"""Tests for two-axis gap-candidate routing (#11). Standalone, no deps:
`python benchmarks/scripts/test_route_candidates.py`."""
import json
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import route_candidates as x  # noqa: E402

CONFIDENT = {"stabilizationStatus": "stable-at-budget", "provenanceOk": True, "labelStatus": "usable"}


def test_signal_sets_are_disjoint():
    # Orthogonality is structural: the two axes must read non-overlapping evidence.
    assert set(x.CALIBRATION_SIGNALS).isdisjoint(x.DIFFICULTY_SIGNALS)


def test_axes_are_orthogonal():
    base = {"evidence": {"engine_oracle_delta_cp": 100, "reply_viable_fraction_50cp": 0.5}}
    r0 = x.route_candidate(base)
    # Bumping a DIFFICULTY signal changes only the difficulty score.
    rd = x.route_candidate({"evidence": {**base["evidence"], "forcing_corridor_len": 9}})
    assert rd["calibration"]["score"] == r0["calibration"]["score"]
    assert rd["difficulty"]["score"] != r0["difficulty"]["score"]
    # Bumping a CALIBRATION signal changes only the calibration score.
    rc = x.route_candidate({"evidence": {**base["evidence"], "move_regret_cp": 200}})
    assert rc["difficulty"]["score"] == r0["difficulty"]["score"]
    assert rc["calibration"]["score"] != r0["calibration"]["score"]


def test_independent_axes_high_low():
    # Big calibration hole, practically easy (many viable replies).
    cal = x.route_candidate({"evidence": {"engine_oracle_delta_cp": 300, "reply_viable_fraction_50cp": 0.9}})
    assert cal["calibration"]["score"] >= 300 and cal["difficulty"]["score"] < 20
    # Hard to play (one viable reply, long corridor) but the engine evaluates it fine (no delta).
    diff = x.route_candidate(
        {"evidence": {"engine_oracle_delta_cp": 0, "reply_viable_fraction_50cp": 0.05, "forcing_corridor_len": 5}}
    )
    assert diff["difficulty"]["score"] > 100 and diff["calibration"]["score"] == 0


def test_eligible_when_all_gates_pass():
    r = x.route_candidate({**CONFIDENT, "evidence": {}})
    assert r["corpusEligible"] is True and r["ineligibleReasons"] == []


def test_selection_only_when_unstable():
    # Flagged FOR instability -> a SELECTION signal, not yet a usable label.
    r = x.route_candidate(
        {"stabilizationStatus": "unstable-trajectory", "provenanceOk": True, "labelStatus": "usable",
         "evidence": {"trajectory_instability": 0.9}}
    )
    assert r["corpusEligible"] is False
    assert any("stabilization" in s for s in r["ineligibleReasons"])
    assert r["calibration"]["score"] > 0  # the instability still scores the calibration axis


def test_fail_closed_on_each_missing_gate():
    for missing in ("stabilizationStatus", "provenanceOk", "labelStatus"):
        cand = {**CONFIDENT}
        del cand[missing]
        assert x.route_candidate(cand)["corpusEligible"] is False, f"missing {missing} must fail closed"


def test_snake_case_status_alias():
    r = x.route_candidate({"stabilizationStatus": "stable_at_budget", "provenanceOk": True, "labelStatus": "usable", "evidence": {}})
    assert r["corpusEligible"] is True


def test_missing_evidence_scores_zero_no_crash():
    r = x.route_candidate({"candidateId": "c1"})  # no evidence at all
    assert r["calibration"]["score"] == 0.0
    assert r["difficulty"]["score"] == 0.0  # missing reply fraction -> treated as 'not hard'


def test_bool_evidence_is_not_numeric():
    r = x.route_candidate({"evidence": {"repeated_error_count": True}})  # bool != a count
    assert r["calibration"]["signals"]["repeated_error_count"] == 0.0


def test_non_finite_evidence_is_clamped():
    r = x.route_candidate({"evidence": {"engine_oracle_delta_cp": float("inf"), "move_regret_cp": float("nan")}})
    assert r["calibration"]["score"] == 0.0  # inf + NaN both clamped -> finite
    assert "Infinity" not in json.dumps(r)  # strict-JSON valid for downstream parsers


def test_difficulty_echo_matches_score_semantics():
    # A missing reply-viable-fraction is treated as 1.0 ('not hard') by BOTH the score and the echo.
    r = x.route_candidate({"evidence": {}})
    assert r["difficulty"]["signals"]["reply_viable_fraction_50cp"] == 1.0
    assert r["difficulty"]["score"] == 0.0


def test_main_round_trip_and_parse_tolerance():
    with tempfile.TemporaryDirectory() as d:
        inp, outp = Path(d) / "cand.jsonl", Path(d) / "out.jsonl"
        inp.write_text(
            json.dumps({**CONFIDENT, "candidateId": "c1", "evidence": {"engine_oracle_delta_cp": 50}})
            + "\n{bad json}\n"
            + json.dumps({"candidateId": "c2", "stabilizationStatus": "unstable-trajectory", "evidence": {}})
            + "\n",
            encoding="utf8",
        )
        rc = x.main(["prog", str(inp), str(outp)])
        assert rc == 0
        rows = [json.loads(line) for line in outp.read_text(encoding="utf8").splitlines()]
        assert len(rows) == 2  # two valid candidates routed; the bad line skipped
        assert rows[0]["corpusEligible"] is True and rows[1]["corpusEligible"] is False


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"ok   {fn.__name__}")
        except AssertionError as e:
            print(f"FAIL {fn.__name__}: {e}")
            failed += 1
    print(f"\n{len(fns)} tests, {failed} failed")
    sys.exit(1 if failed else 0)
