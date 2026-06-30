#!/usr/bin/env python3
"""Tests for the promotion-policy linter (issue #5).

Runs with pytest, or standalone: `python test_lint_promotion.py` (no pytest needed).
Pins the two acceptance criteria from #5:
  * a report with PROMOTE and no crossed bound fails
  * a fixed-N +50-ish point estimate that did not cross the bound resolves to HOLD
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
