#!/usr/bin/env python3
"""Tests for the RSI experiment-ledger linter (issue #12). Standalone:
`python benchmarks/scripts/test_lint_ledger.py`."""
from __future__ import annotations

import copy
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lint_ledger as ll  # noqa: E402

SPRT_UPPER = {
    "boundary": "upper",
    "decision": "promote",
    "llr": 2.96,
    "lowerBound": -2.9444,
    "upperBound": 2.9444,
    "games": 612,
    "wins": 201,
    "losses": 173,
    "draws": 238,
}


def base_run(run_id="r1", parent=None, decision="hold", authority="agent", sprt=None, created_by="agent-x"):
    return {
        "schemaVersion": 1,
        "runId": run_id,
        "parentRunId": parent,
        "issue": {"repo": "Mnehmos/chess-vision-studio-rust-engine", "number": 4},
        "observe": {"deReport": None, "bindingDirection": "SEARCH", "evidenceHash": None},
        "orient": {"failureClass": "search", "assumptions": [], "risks": []},
        "decide": {
            "oneVariable": "lmr",
            "hypothesis": "log-LMR helps fixed-node depth",
            "expectedSignal": None,
            "rollbackArtifact": "champion.json",
        },
        "act": {"commands": [], "artifacts": [], "invariantReport": None, "gateReports": [], "sprtReport": sprt},
        "decision": decision,
        "decisionAuthority": authority,
        "createdBy": created_by,
    }


def test_valid_ledger():
    led = ll.build_ledger([base_run("r1"), base_run("r2", parent="r1")])
    assert ll.lint_ledger(led) == []


def test_tampered_row_breaks_chain():
    led = ll.build_ledger([base_run("r1"), base_run("r2", parent="r1")])
    led[0]["decide"]["hypothesis"] = "TAMPERED"  # mutate content, keep the old hash
    assert any("appendOnlyHash mismatch" in x for x in ll.lint_ledger(led))


def test_agent_cannot_self_authorize_promote():
    led = ll.build_ledger([base_run("r1", decision="promote", authority="agent", sprt=SPRT_UPPER)])
    assert any("self-authorize promotion" in x for x in ll.lint_ledger(led))


def test_promote_requires_crossed_upper_sprt():
    led = ll.build_ledger([base_run("r1", decision="promote", authority="policy-engine", sprt=None)])
    assert any("crossed-UPPER SPRT" in x for x in ll.lint_ledger(led))


def test_valid_policy_engine_promote():
    led = ll.build_ledger([base_run("r1", decision="promote", authority="policy-engine", sprt=SPRT_UPPER)])
    assert ll.lint_ledger(led) == []


def test_reject_with_upper_sprt_is_inconsistent():
    led = ll.build_ledger([base_run("r1", decision="reject", authority="policy-engine", sprt=SPRT_UPPER)])
    assert any("inconsistent" in x for x in ll.lint_ledger(led))


def test_duplicate_runid():
    led = ll.build_ledger([base_run("r1"), base_run("r1")])
    assert any("duplicate runId" in x for x in ll.lint_ledger(led))


def test_missing_parent():
    led = ll.build_ledger([base_run("r2", parent="ghost")])
    assert any("not found earlier" in x for x in ll.lint_ledger(led))


def test_unknown_field_rejected():
    runs = [base_run("r1")]
    runs[0]["extra"] = 1
    led = ll.build_ledger(runs)
    assert any("unknown field 'extra'" in x for x in ll.lint_ledger(led))


def test_bad_decision_enum():
    led = ll.build_ledger([base_run("r1", decision="ship_it")])
    assert any("decision must be one of" in x for x in ll.lint_ledger(led))


def test_schemaversion_bool_rejected():
    runs = [base_run("r1")]
    runs[0]["schemaVersion"] = True
    led = ll.build_ledger(runs)
    assert any("schemaVersion" in x for x in ll.lint_ledger(led))


def test_chain_is_order_sensitive():
    led = ll.build_ledger([base_run("r1"), base_run("r2", parent="r1")])
    swapped = [copy.deepcopy(led[1]), copy.deepcopy(led[0])]  # reorder -> chain + lineage break
    viol = ll.lint_ledger(swapped)
    assert any("appendOnlyHash mismatch" in x or "not found earlier" in x for x in viol)


# --- regression: review findings (F1 forged SPRT, F3 re-genesis taint, F4 type gaps) ---
def test_forged_minimal_sprt_is_rejected():  # F1 — the headline bypass
    forged = {"boundary": "upper", "decision": "promote"}  # label-only, no llr/bounds/counts
    led = ll.build_ledger([base_run("r1", decision="promote", authority="policy-engine", sprt=forged)])
    assert any("crossed-UPPER SPRT" in x for x in ll.lint_ledger(led))


def test_sprt_llr_below_upper_is_rejected():  # F1
    weak = dict(SPRT_UPPER, llr=1.0)  # llr < upperBound -> not actually crossed
    led = ll.build_ledger([base_run("r1", decision="promote", authority="policy-engine", sprt=weak)])
    assert any("crossed-UPPER SPRT" in x for x in ll.lint_ledger(led))


def test_broken_row_then_regenesis_tail_is_tainted():  # F3
    led = ll.build_ledger([base_run("r1"), base_run("r2", parent="r1"), base_run("r3", parent="r2")])
    del led[1]["appendOnlyHash"]  # attacker cuts the chain at r2
    led[2]["appendOnlyHash"] = ll.chain_hash("", led[2])  # then re-anchors r3 as a fresh genesis
    assert any("follows a broken chain link" in x for x in ll.lint_ledger(led))


def test_type_gaps_rejected():  # F4
    runs = [base_run("r1")]
    runs[0]["decide"]["pGridCell"] = "yes"  # not bool
    runs[0]["act"]["commands"] = "do-the-thing"  # not array
    runs[0]["observe"]["bindingDirection"] = ""  # empty (minLength)
    runs[0]["parentRunId"] = 12345  # not str/null
    led = ll.build_ledger(runs)
    viol = ll.lint_ledger(led)
    assert any("pGridCell must be a boolean" in x for x in viol)
    assert any("act.commands must be an array" in x for x in viol)
    assert any("bindingDirection must be a non-empty string" in x for x in viol)
    assert any("parentRunId must be a string" in x for x in viol)


def test_one_variable_comma_smuggle_rejected():  # F4
    runs = [base_run("r1")]
    runs[0]["decide"]["oneVariable"] = "lmr,nullmove,futility"
    led = ll.build_ledger(runs)
    assert any("multiple variables" in x for x in ll.lint_ledger(led))


def test_pgrid_cell_allows_declared_grid():
    runs = [base_run("r1")]
    runs[0]["decide"]["pGridCell"] = True
    runs[0]["decide"]["oneVariable"] = "lmr x hidden-size"  # a declared two-axis grid cell (no comma)
    led = ll.build_ledger(runs)
    assert ll.lint_ledger(led) == []


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
