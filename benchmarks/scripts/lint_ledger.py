#!/usr/bin/env python3
"""RSI experiment-ledger linter + append-only chain (issue #12).

The ledger is a JSONL list of ExperimentRunV1 records (benchmarks/schemas/experiment-run.schema.json).
This module:

  * validate_run(record)  -- schema validation (required fields, enums, additionalProperties).
  * chain_hash / build_ledger -- the append-only hash chain so producers write valid ledgers and
    any mutation of a prior row is detectable.
  * lint_ledger(runs) -- verifies the chain, runId uniqueness, parent lineage, and the harness's
    authority invariants:
        - decision 'promote' requires decisionAuthority 'policy-engine' (an agent may PROPOSE and
          EXECUTE but cannot self-authorize promotion) AND an embedded crossed-UPPER SPRT report (#5).
        - decision 'reject' may not also carry a crossed-upper SPRT (that would be a promote).

Limitations (follow-up slices of #12):
  * Append-only is per-row + chained: a prior-row content tamper, a partial re-chain, and a
    structurally-broken row (which taints the rest of the tail) are all detected. A FULL tail
    re-chain (recomputing every downstream hash) is NOT detectable without an external anchor —
    a published, notarized ledger HEAD is the fix.
  * decisionAuthority is trusted as written: the linter cannot tell whether the real policy engine
    stamped 'policy-engine' or an agent typed the string. Binding it needs a signature/MAC from the
    policy engine over the record (out of scope here).
  * _sprt_crossed_upper is an inline subset of #5's sprt-result validation; full reuse of
    lint_promotion lands when #5 merges.

Usage:
    python lint_ledger.py <ledger.jsonl> [...]
Exits non-zero on any violation (CI-gateable).
"""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

DECISIONS = {"promote", "reject", "hold", "analysis_only", "blocked"}
AUTHORITIES = {"policy-engine", "agent"}
FAILURE_CLASSES = {"search", "capacity", "coverage", "label", "interaction", "unknown"}

TOP = {
    "schemaVersion", "runId", "parentRunId", "issue", "observe", "orient", "decide", "act",
    "decision", "decisionAuthority", "createdBy", "appendOnlyHash",
}
REQ_TOP = TOP - {"parentRunId"}
NESTED = {
    "issue": ({"repo", "number"}, {"repo", "number"}),
    "observe": ({"bindingDirection"}, {"deReport", "bindingDirection", "evidenceHash"}),
    "orient": ({"failureClass"}, {"failureClass", "assumptions", "risks"}),
    "decide": (
        {"oneVariable", "hypothesis", "rollbackArtifact"},
        {"oneVariable", "hypothesis", "expectedSignal", "rollbackArtifact", "pGridCell"},
    ),
    "act": ({"commands", "artifacts"}, {"commands", "artifacts", "invariantReport", "gateReports", "sprtReport"}),
}


def _is_int(x):
    return isinstance(x, int) and not isinstance(x, bool)


def _str(x):
    return isinstance(x, str) and len(x.strip()) > 0


def _canonical(run: dict) -> str:
    """Canonical JSON of a record EXCLUDING appendOnlyHash (the field the hash protects)."""
    return json.dumps({k: v for k, v in run.items() if k != "appendOnlyHash"}, sort_keys=True, separators=(",", ":"))


def chain_hash(prev_hash: str, run: dict) -> str:
    return hashlib.sha256((prev_hash + _canonical(run)).encode("utf-8")).hexdigest()


def build_ledger(runs_without_hash: list[dict]) -> list[dict]:
    """Stamp each record's appendOnlyHash, chaining from a "" genesis. For producers + tests."""
    out, prev = [], ""
    for r in runs_without_hash:
        r = dict(r)
        r.pop("appendOnlyHash", None)
        h = chain_hash(prev, r)
        r["appendOnlyHash"] = h
        out.append(r)
        prev = h
    return out


def validate_run(run, idx: int = -1) -> list[str]:
    p = f"run[{idx}]" if idx >= 0 else "run"
    v: list[str] = []
    if not isinstance(run, dict):
        return [f"{p}: not a JSON object"]
    sv = run.get("schemaVersion")
    if not (_is_int(sv) and sv == 1):
        v.append(f"{p}: schemaVersion must be the integer 1")
    for f in REQ_TOP:
        if f not in run:
            v.append(f"{p}: missing required field '{f}'")
    for k in run:
        if k not in TOP:
            v.append(f"{p}: unknown field '{k}'")
    for f in ("runId", "createdBy", "appendOnlyHash"):
        if not _str(run.get(f)):
            v.append(f"{p}: {f} must be a non-empty string")
    if run.get("decision") not in DECISIONS:
        v.append(f"{p}: decision must be one of {sorted(DECISIONS)}")
    if run.get("decisionAuthority") not in AUTHORITIES:
        v.append(f"{p}: decisionAuthority must be one of {sorted(AUTHORITIES)}")
    for key, (req, allowed) in NESTED.items():
        obj = run.get(key)
        if not isinstance(obj, dict):
            v.append(f"{p}: {key} must be an object")
            continue
        for f in req:
            if f not in obj:
                v.append(f"{p}: {key}.{f} is required")
        for f in obj:
            if f not in allowed:
                v.append(f"{p}: unknown {key} field '{f}'")
    orient = run.get("orient")
    if isinstance(orient, dict) and orient.get("failureClass") not in FAILURE_CLASSES:
        v.append(f"{p}: orient.failureClass must be one of {sorted(FAILURE_CLASSES)}")
    decide = run.get("decide")
    if isinstance(decide, dict):
        for f in ("oneVariable", "hypothesis", "rollbackArtifact"):
            if not _str(decide.get(f)):
                v.append(f"{p}: decide.{f} must be a non-empty string")
    issue = run.get("issue")
    if isinstance(issue, dict) and not _is_int(issue.get("number")):
        v.append(f"{p}: issue.number must be an integer")
    if isinstance(issue, dict) and not _str(issue.get("repo")):
        v.append(f"{p}: issue.repo must be a non-empty string")
    observe = run.get("observe")
    if isinstance(observe, dict) and not _str(observe.get("bindingDirection")):
        v.append(f"{p}: observe.bindingDirection must be a non-empty string")
    if isinstance(decide, dict):
        if "pGridCell" in decide and not isinstance(decide["pGridCell"], bool):
            v.append(f"{p}: decide.pGridCell must be a boolean")
        # one-variable invariant: a single declared variable unless a P-GRID cell. A comma
        # smuggles several past the single-string field (heuristic — name-level, not semantic).
        if not decide.get("pGridCell") and isinstance(decide.get("oneVariable"), str) and "," in decide["oneVariable"]:
            v.append(f"{p}: decide.oneVariable declares multiple variables (comma) without pGridCell")
    act = run.get("act")
    if isinstance(act, dict):
        for f in ("commands", "artifacts", "gateReports"):
            if f in act and not isinstance(act[f], list):
                v.append(f"{p}: act.{f} must be an array")
    if isinstance(orient, dict):
        for f in ("assumptions", "risks"):
            if f in orient and not isinstance(orient[f], list):
                v.append(f"{p}: orient.{f} must be an array")
    pid = run.get("parentRunId")
    if pid is not None and not isinstance(pid, str):
        v.append(f"{p}: parentRunId must be a string or null")
    return v


def _sprt_crossed_upper(sprt) -> bool:
    """A genuinely crossed UPPER SPRT bound — an inline subset of #5's sprt-result checks (full
    reuse of lint_promotion lands when #5 merges). Rejects a forged label-only dict: requires the
    bounds + counts and `llr >= upperBound`, not just `boundary`/`decision` strings."""
    if not isinstance(sprt, dict):
        return False
    if any(k not in sprt for k in ("llr", "lowerBound", "upperBound", "games", "wins", "losses", "draws", "boundary", "decision")):
        return False
    if not all(isinstance(sprt[k], (int, float)) and not isinstance(sprt[k], bool) for k in ("llr", "lowerBound", "upperBound")):
        return False
    if not all(isinstance(sprt[k], int) and not isinstance(sprt[k], bool) for k in ("games", "wins", "losses", "draws")):
        return False
    if sprt["wins"] + sprt["losses"] + sprt["draws"] != sprt["games"]:
        return False
    if sprt["upperBound"] <= sprt["lowerBound"]:
        return False
    return sprt["boundary"] == "upper" and sprt["decision"] == "promote" and sprt["llr"] >= sprt["upperBound"]


def authority_violations(run, idx: int = -1) -> list[str]:
    """The harness-authority invariants (issue #12)."""
    p = f"run[{idx}]" if idx >= 0 else "run"
    v: list[str] = []
    decision = run.get("decision")
    act = run.get("act") if isinstance(run.get("act"), dict) else {}
    sprt = act.get("sprtReport")
    if decision == "promote":
        if run.get("decisionAuthority") != "policy-engine":
            v.append(
                f"{p}: decision 'promote' requires decisionAuthority 'policy-engine' — an agent "
                f"may not self-authorize promotion (issue #12)"
            )
        if not _sprt_crossed_upper(sprt):
            v.append(
                f"{p}: decision 'promote' requires an embedded crossed-UPPER SPRT report "
                f"(act.sprtReport.boundary=='upper', decision=='promote') (#5)"
            )
    # a non-promote decision must not carry a crossed-upper SPRT masquerading as something else
    if decision in ("reject", "hold", "analysis_only", "blocked") and _sprt_crossed_upper(sprt):
        v.append(f"{p}: a crossed-upper SPRT report with decision '{decision}' is inconsistent (should be 'promote')")
    return v


def lint_ledger(runs: list[dict]) -> list[str]:
    v: list[str] = []
    seen: set = set()
    prev = ""
    chain_broken = False
    for i, run in enumerate(runs):
        rv = validate_run(run, i)
        v += rv
        structurally_ok = not any("not a JSON object" in x or "missing required field" in x for x in rv)
        h = run.get("appendOnlyHash")
        if structurally_ok and _str(h):
            expect = chain_hash(prev, run)
            if h != expect:
                v.append(
                    f"run[{i}] runId={run.get('runId')}: appendOnlyHash mismatch — ledger mutated "
                    f"or mis-chained (expected {expect[:12]}…)"
                )
                chain_broken = True
            elif chain_broken:
                v.append(
                    f"run[{i}] runId={run.get('runId')}: follows a broken chain link — the tail is "
                    f"untrusted until the ledger is re-anchored"
                )
            v += authority_violations(run, i)
        else:
            chain_broken = True  # an invalid / missing-hash row cuts the chain; taint the tail
        prev = h if _str(h) else ""
        rid = run.get("runId")
        pid = run.get("parentRunId")
        if pid is not None and pid not in seen:
            v.append(f"run[{i}] runId={rid}: parentRunId '{pid}' not found earlier in the ledger")
        if rid in seen:
            v.append(f"run[{i}]: duplicate runId '{rid}'")
        if _str(rid):
            seen.add(rid)
    return v


def main(argv) -> int:
    total = 0
    for path in argv[1:]:
        runs = [json.loads(ln) for ln in Path(path).read_text(encoding="utf-8").splitlines() if ln.strip()]
        viol = lint_ledger(runs)
        if viol:
            print(f"FAIL {path}")
            for x in viol:
                print(f"  - {x}")
            total += len(viol)
        else:
            print(f"ok   {path} ({len(runs)} runs)")
    print(f"\n{total} ledger violation(s).")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
