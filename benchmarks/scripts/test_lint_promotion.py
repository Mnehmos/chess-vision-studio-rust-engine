#!/usr/bin/env python3
"""Tests for the promotion-policy linter (issue #5).

Runs with pytest, or standalone: `python test_lint_promotion.py` (no pytest needed).
Pins the two acceptance criteria from #5:
  * a report with PROMOTE and no crossed bound fails
  * a fixed-N +50-ish point estimate that did not cross the bound resolves to HOLD
and the INV-2 performance-only tier:
  * a parity-proven, repeatable speedup is accepted without an Elo proof
  * one noisy repeat does not veto a gain that is consistent across repeats
  * any behavioral difference, test failure, small/inconsistent speedup, resource cost,
    or Elo claim blocks accept_performance
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lint_promotion as lp  # noqa: E402

FIX = Path(__file__).resolve().parent.parent / "results" / "fixtures"


def load(name):
    return json.loads((FIX / name).read_text(encoding="utf-8"))


def test_valid_promote_passes():
    assert lp.lint_record(load("sprt-promote-valid.json")) == []


def test_promote_without_crossed_bound_fails():
    viol = lp.lint_record(load("sprt-promote-invalid.json"))
    assert viol, "expected violations for a promote with no crossed bound"
    assert any("INV-1" in x or "promote" in x for x in viol), viol


def test_fixed_n_plus50_holds_cleanly():
    rec = load("sprt-fixedn-plus50-hold.json")
    # 60% score (90+30 over 200) / ~+70 Elo point estimate, but the SPRT never crossed
    # -> HOLD, lint clean.
    assert rec["decision"] == "hold_for_more_data"
    assert rec["boundary"] == "none"
    assert lp.lint_record(rec) == []


def test_forged_zero_bounds_rejected():
    # The headline bypass: lower=upper=llr=0, boundary "upper" -> no genuine crossing.
    rec = load("sprt-promote-valid.json")
    rec["lowerBound"] = 0.0
    rec["upperBound"] = 0.0
    rec["llr"] = 0.0
    assert lp.lint_record(rec), "all-zero bounds must not promote"


def test_inverted_bounds_rejected():
    rec = load("sprt-promote-valid.json")
    rec["upperBound"] = -1.0
    rec["lowerBound"] = 1.0
    assert any("must be >" in x for x in lp.lint_record(rec))


def test_bounds_must_match_alpha_beta():
    rec = load("sprt-promote-valid.json")
    rec["upperBound"] = 1.0  # alpha=beta=0.05 => expected ~2.944
    assert any("forged or inconsistent bound" in x or "upperBound" in x for x in lp.lint_record(rec))


def test_non_finite_llr_rejected():
    rec = load("sprt-promote-valid.json")
    rec["llr"] = float("inf")
    assert any("finite" in x for x in lp.lint_record(rec))


def test_unknown_field_rejected():
    rec = load("sprt-promote-valid.json")
    rec["sprtPass"] = True
    assert any("unknown top-level field" in x for x in lp.lint_record(rec))


def test_unknown_provenance_field_rejected():
    rec = load("sprt-promote-valid.json")
    rec["provenance"]["fudge"] = 1
    assert any("unknown provenance field" in x for x in lp.lint_record(rec))


def test_elo1_must_exceed_elo0():
    rec = load("sprt-promote-valid.json")
    rec["elo1"] = rec["elo0"]
    assert any("elo1" in x for x in lp.lint_record(rec))


def test_derive_boundary():
    assert lp.derive_boundary(3.0, -2.94, 2.94) == "upper"
    assert lp.derive_boundary(-3.0, -2.94, 2.94) == "lower"
    assert lp.derive_boundary(1.0, -2.94, 2.94) == "none"


def test_boundary_must_match_llr():
    rec = load("sprt-promote-valid.json")
    rec["llr"] = 0.5  # below upper, but boundary still claims 'upper'
    assert any("contradicts" in x for x in lp.lint_record(rec))


def test_counts_must_sum_to_games():
    rec = load("sprt-promote-valid.json")
    rec["draws"] += 1
    assert any("!= games" in x for x in lp.lint_record(rec))


def test_missing_field_flagged():
    rec = load("sprt-promote-valid.json")
    del rec["provenance"]
    assert any("missing required field: provenance" in x for x in lp.lint_record(rec))


# --------------------------------------------------------------------------------------
# INV-2: performance-only tier
# --------------------------------------------------------------------------------------

def test_perf_accept_valid_passes():
    # Full parity + repeatable speedup + green tests: accepted with no Elo evidence at all.
    assert lp.lint_record(load("perf-accept-valid.json")) == []


def test_perf_accept_requires_full_parity():
    viol = lp.lint_record(load("perf-accept-invalid-parity.json"))
    assert any("FULL behavioral parity" in x for x in viol), viol


def test_perf_accept_rejects_failing_tests():
    rec = load("perf-accept-valid.json")
    rec["tests"]["failed"] = 1
    assert any("zero failing tests" in x for x in lp.lint_record(rec))


def test_perf_accept_rejects_small_speedup():
    rec = load("perf-accept-valid.json")
    rec["speed"]["medianSpeedupPct"] = 0.4
    rec["speed"]["minSpeedupPct"] = 0.1
    assert any("medianSpeedupPct" in x for x in lp.lint_record(rec))


def test_perf_accept_tolerates_one_noisy_repeat():
    # 8/9 repeats faster, one -1% dip: desktop wall-clock noise, not a regression.
    rec = load("perf-accept-valid.json")
    rec["speed"].update({"repeats": 9, "repeatsTotal": 9, "repeatsPositive": 8,
                         "signTestP": lp.sign_test_p(8, 9), "minSpeedupPct": -1.01})
    assert lp.lint_record(rec) == []


def test_perf_accept_rejects_deep_repeat_regression():
    rec = load("perf-accept-valid.json")
    rec["speed"]["minSpeedupPct"] = -9.0
    assert any("no repeat worse than" in x for x in lp.lint_record(rec))


def test_perf_accept_rejects_inconsistent_repeats():
    # 5/9 faster: a coin flip. Median can still look good; the sign test says nothing proven.
    rec = load("perf-accept-valid.json")
    rec["speed"].update({"repeats": 9, "repeatsTotal": 9, "repeatsPositive": 5,
                         "signTestP": lp.sign_test_p(5, 9)})
    assert any("sign test" in x for x in lp.lint_record(rec))


def test_perf_accept_rejects_too_few_repeats():
    rec = load("perf-accept-valid.json")
    rec["speed"].update({"repeats": 3, "repeatsTotal": 3, "repeatsPositive": 3,
                         "signTestP": lp.sign_test_p(3, 3)})
    assert any("timed repeats" in x for x in lp.lint_record(rec))


def test_perf_sign_test_p_must_match_counts():
    rec = load("perf-accept-valid.json")
    rec["speed"]["signTestP"] = 0.001
    assert any("one-sided sign test" in x for x in lp.lint_record(rec))


def test_sign_test_p_values():
    assert lp.sign_test_p(5, 5) == 1 / 32
    assert abs(lp.sign_test_p(8, 9) - 10 / 512) < 1e-12
    assert lp.sign_test_p(0, 9) == 1.0


def test_perf_accept_rejects_resource_purchase():
    rec = load("perf-accept-valid.json")
    rec["resources"]["peakRssDeltaPct"] = 40.0
    assert any("bought with resources" in x for x in lp.lint_record(rec))


def test_perf_record_may_not_claim_elo():
    rec = load("perf-accept-valid.json")
    rec["strengthClaim"] = True
    assert any("never claims Elo" in x for x in lp.lint_record(rec))


def test_perf_accept_requires_enough_parity_searches():
    rec = load("perf-accept-valid.json")
    rec["parity"]["searches"] = 12
    rec["parity"]["identicalSearches"] = 12
    assert any("paired parity" in x for x in lp.lint_record(rec))


def test_perf_speedup_arithmetic_must_be_consistent():
    rec = load("perf-accept-valid.json")
    rec["speed"]["aggregateSpeedupPct"] = 25.0  # baselineMs/candidateMs says ~5.24%
    assert any("inconsistent timing record" in x for x in lp.lint_record(rec))


def test_perf_hold_does_not_need_the_gate():
    # A short/failed measurement may still be recorded honestly -- as a hold, not an accept.
    rec = load("perf-accept-invalid-parity.json")
    rec["decision"] = "hold_for_more_data"
    assert lp.lint_record(rec) == []


def test_perf_unknown_field_rejected():
    rec = load("perf-accept-valid.json")
    rec["eloGain"] = 20
    assert any("unknown top-level field" in x for x in lp.lint_record(rec))


def test_perf_screen_is_informational_only():
    # A 40-game screen that did not cross a bound must not block an accepted speedup.
    rec = load("perf-accept-valid.json")
    rec["screen"] = {"games": 40, "wins": 18, "losses": 14, "draws": 8, "sprtRecord": None}
    assert lp.lint_record(rec) == []


def test_sprt_record_still_linted_as_sprt():
    # A record without changeClass keeps going through the INV-1 path.
    assert lp.is_perf_record(load("sprt-promote-valid.json")) is False


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
