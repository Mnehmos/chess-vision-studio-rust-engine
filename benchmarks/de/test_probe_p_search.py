#!/usr/bin/env python3
"""Tests for the P-SEARCH probe aggregation (issue #8). Uses an injected fake engine, so no
binary is needed. Standalone: `python benchmarks/de/test_probe_p_search.py`."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import probe_p_search as ps  # noqa: E402


def fake_stable(fen, nodes):
    return {"bestMove": "e2e4", "scoreCp": 20, "nodes": nodes, "depth": 10}


def fake_changing(fen, nodes):
    # move flips at the large budget; score + depth grow with budget.
    return {
        "bestMove": "e2e4" if nodes < 50000 else "d2d4",
        "scoreCp": 20 + nodes // 10000,
        "nodes": nodes,
        "depth": 8 + nodes // 20000,
    }


def test_stable_has_zero_change_rate():
    r = ps.run_p_search(["fen-a", "fen-b"], [10000, 80000], fake_stable)
    assert r["aggregate"]["moveChangeRate"] == 0.0
    p = r["perPosition"][0]
    assert p["moveChangedSmallestToLargest"] is False
    assert p["settledAtBudget"] == 10000  # never changed
    assert p["depthDelta"] == 0


def test_changing_has_full_change_rate_and_settles_late():
    r = ps.run_p_search(["fen-a"], [10000, 80000], fake_changing)
    assert r["aggregate"]["moveChangeRate"] == 1.0
    p = r["perPosition"][0]
    assert p["moveChangedSmallestToLargest"] is True
    assert p["settledAtBudget"] == 80000  # the move changed at the largest budget
    assert p["depthDelta"] >= 1
    assert p["scoreDeltaCp"] is not None and p["scoreDeltaCp"] > 0


def test_budgets_sorted_and_deduped():
    r = ps.run_p_search(["fen-a"], [80000, 10000, 10000], fake_stable)
    assert r["budgets"] == [10000, 80000]


def test_report_schema():
    r = ps.run_p_search(["fen-a"], [1, 2], fake_stable)
    assert r["schemaVersion"] == 1
    assert r["probe"] == "P-SEARCH"
    assert set(r["aggregate"]) >= {"moveChangeRate", "meanSettledBudget", "meanDepthGain"}
    assert r["positions"] == 1
    assert "interpretation" in r


def test_none_score_handled():
    r = ps.run_p_search(
        ["fen-a"], [1, 2], lambda f, n: {"bestMove": "e2e4", "scoreCp": None, "nodes": n, "depth": 1}
    )
    assert r["perPosition"][0]["scoreDeltaCp"] is None


def test_single_budget_no_change():
    r = ps.run_p_search(["fen-a"], [10000], fake_changing)
    assert r["aggregate"]["moveChangeRate"] == 0.0  # one budget -> no comparison
    assert r["perPosition"][0]["settledAtBudget"] == 10000


# --- regression: review findings (F1 oscillation, F6 empty budgets, F2/F3/F4 parser) ---
def test_oscillation_is_flagged_unstable():  # F1
    def osc(fen, nodes):
        m = {1: "a2a3", 2: "b2b3", 3: "a2a3"}[nodes]  # A -> B -> A
        return {"bestMove": m, "scoreCp": 0, "nodes": nodes, "depth": 1}

    r = ps.run_p_search(["fen-a"], [1, 2, 3], osc)
    p = r["perPosition"][0]
    assert p["unstable"] is True  # ANY change, not just endpoint
    assert p["oscillated"] is True  # changed and returned
    assert p["moveChangedSmallestToLargest"] is False  # endpoints equal (the old, weaker metric)
    assert p["settledAtBudget"] == 3  # last change was at budget 3
    assert r["aggregate"]["moveChangeRate"] == 1.0
    assert r["aggregate"]["oscillationRate"] == 1.0


def test_empty_budgets_no_crash():  # F6
    r = ps.run_p_search(["fen-a"], [], fake_stable)
    assert r["budgets"] == []
    assert r["perPosition"][0]["settledAtBudget"] is None
    assert r["perPosition"][0]["unstable"] is False
    assert r["aggregate"]["meanSettledBudget"] is None


def test_parse_skips_info_string():  # F2
    out = "info depth 5 nodes 4000 score cp 12 pv e2e4\ninfo string pruned nodes 99 here\nbestmove e2e4\n"
    p = ps.parse_uci_output(out)
    assert p["nodes"] == 4000  # not 99 from the info-string line
    assert p["scoreCp"] == 12
    assert p["bestMove"] == "e2e4"


def test_parse_multipv_uses_pv1():  # F3
    out = (
        "info depth 6 multipv 1 score cp 50 nodes 8000 pv a2a3\n"
        "info depth 6 multipv 2 score cp 20 nodes 8000 pv b2b3\n"
        "bestmove a2a3\n"
    )
    p = ps.parse_uci_output(out)
    assert p["scoreCp"] == 50  # the best PV's score, not multipv 2's
    assert p["bestMove"] == "a2a3"


def test_parse_mate_score():  # F4
    p = ps.parse_uci_output("info depth 10 nodes 5000 score mate 3 pv a2a3\nbestmove a2a3\n")
    assert p["scoreCp"] is not None and p["scoreCp"] > 900_000  # mate -> large positive sentinel
    p2 = ps.parse_uci_output("info depth 10 nodes 5000 score mate -2 pv a2a3\nbestmove a2a3\n")
    assert p2["scoreCp"] is not None and p2["scoreCp"] < -900_000


def test_parse_empty_or_null_move():
    assert ps.parse_uci_output("")["bestMove"] == "0000"
    assert ps.parse_uci_output("bestmove 0000\n")["bestMove"] == "0000"


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
