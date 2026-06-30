#!/usr/bin/env python3
"""Promotion-policy linter (issue #5).

Enforces INV-1: a candidate may be PROMOTED only when a declared SPRT test crossed the
*upper* boundary under its declared hypothesis. Fixed-N results, Elo point estimates, LOS,
and screens justify more testing or analysis-only decisions -- never promotion.

This lints the machine-readable SPRT result records (benchmarks/schemas/sprt-result.schema.json):
field/type validation plus the logic checks that catch policy violations:
  * decision 'promote'  requires boundary 'upper'  (crossed upper SPRT bound)
  * decision 'reject'   requires boundary 'lower'
  * a non-crossed test (boundary 'none') may only HOLD -- never promote/reject
  * the recorded boundary must match the LLR vs the declared bounds
  * wins + losses + draws == games

Usage:
    python lint_promotion.py [PATH ...]          # default: benchmarks/
Each PATH is a file or directory. Any .json that looks like an SPRT record (has
"schemaVersion" and "boundary") is checked; other JSON (gate records etc.) is skipped.
Exits non-zero if any violation is found, so it can gate CI.
"""
from __future__ import annotations

import json
import math
import sys
from pathlib import Path

OPTIONAL_TOP = {"pgnSha256"}
OPTIONAL_PROV = {"nodes", "openingsSha"}
BOUND_TOL = 0.01  # tolerance for bounds vs the alpha/beta-derived SPRT thresholds

ALLOWED_DECISIONS = {"promote", "reject", "hold_for_more_data", "analysis_mode_only", "live_dev_only"}
ALLOWED_BOUNDARY = {"upper", "lower", "none"}
REQUIRED = [
    "schemaVersion", "experimentId", "baselineId", "candidateId", "elo0", "elo1", "alpha",
    "beta", "games", "wins", "losses", "draws", "llr", "lowerBound", "upperBound",
    "boundary", "decision", "provenance",
]
REQUIRED_PROV = ["engineSha", "netSha", "candArgs", "baseArgs", "tc", "threads"]


def _is_num(x) -> bool:
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def _is_int(x) -> bool:
    return isinstance(x, int) and not isinstance(x, bool)


def validate_sprt_record(rec) -> list[str]:
    """Schema-level validation (required fields, types, ranges)."""
    v: list[str] = []
    if not isinstance(rec, dict):
        return ["record is not a JSON object"]
    for f in REQUIRED:
        if f not in rec:
            v.append(f"missing required field: {f}")
    if "schemaVersion" in rec and rec["schemaVersion"] != 1:
        v.append("schemaVersion must be 1")
    for f in ("experimentId", "baselineId", "candidateId"):
        if f in rec and (not isinstance(rec[f], str) or not rec[f]):
            v.append(f"{f} must be a non-empty string")
    for f in ("elo0", "elo1", "alpha", "beta", "llr", "lowerBound", "upperBound"):
        if f in rec and not _is_num(rec[f]):
            v.append(f"{f} must be a number")
    for f in ("games", "wins", "losses", "draws"):
        if f in rec and (not _is_int(rec[f]) or rec[f] < 0):
            v.append(f"{f} must be a non-negative integer")
    for f in ("alpha", "beta"):
        if _is_num(rec.get(f)) and not (0 < rec[f] < 0.5):
            v.append(f"{f} must be in (0, 0.5)")
    if "boundary" in rec and rec["boundary"] not in ALLOWED_BOUNDARY:
        v.append(f"boundary must be one of {sorted(ALLOWED_BOUNDARY)}")
    if "decision" in rec and rec["decision"] not in ALLOWED_DECISIONS:
        v.append(f"decision must be one of {sorted(ALLOWED_DECISIONS)}")
    # additionalProperties: false (the hand-rolled equivalent of the schema constraint) —
    # an unknown/typo'd field (e.g. "upperBnd" while "upperBound" is wrong) must not pass.
    for k in rec:
        if k not in REQUIRED and k not in OPTIONAL_TOP:
            v.append(f"unknown top-level field: {k} (schema is additionalProperties:false)")
    prov = rec.get("provenance")
    if not isinstance(prov, dict):
        v.append("provenance must be an object")
    else:
        for f in REQUIRED_PROV:
            if f not in prov:
                v.append(f"provenance missing required field: {f}")
        if "threads" in prov and (not _is_int(prov["threads"]) or prov["threads"] < 1):
            v.append("provenance.threads must be a positive integer")
        for k in prov:
            if k not in REQUIRED_PROV and k not in OPTIONAL_PROV:
                v.append(f"unknown provenance field: {k}")
    return v


def derive_boundary(llr: float, lower: float, upper: float) -> str:
    if llr >= upper:
        return "upper"
    if llr <= lower:
        return "lower"
    return "none"


def check_consistency(rec) -> list[str]:
    """Logic checks: the promotion-policy gate (INV-1) and arithmetic sanity."""
    v: list[str] = []
    # Bounds/hypotheses must be finite, well-ordered, and — critically — match the declared
    # alpha/beta. Without this a fabricated record could "cross" a forged bound (e.g.
    # lower=upper=0, llr=0) and promote with no genuine test crossing.
    nums = {k: rec.get(k) for k in ("llr", "lowerBound", "upperBound", "alpha", "beta", "elo0", "elo1")}
    if all(_is_num(x) for x in nums.values()):
        if not all(math.isfinite(x) for x in nums.values()):
            v.append("llr / bounds / alpha / beta / elo must all be finite")
        else:
            if nums["upperBound"] <= nums["lowerBound"]:
                v.append(f"upperBound ({nums['upperBound']}) must be > lowerBound ({nums['lowerBound']})")
            if nums["elo1"] <= nums["elo0"]:
                v.append(f"elo1 ({nums['elo1']}) must be > elo0 ({nums['elo0']}) for a promotion test")
            a, b = nums["alpha"], nums["beta"]
            if 0 < a < 1 and 0 < b < 1:
                exp_upper = math.log((1 - b) / a)
                exp_lower = math.log(b / (1 - a))
                if abs(nums["upperBound"] - exp_upper) > BOUND_TOL:
                    v.append(
                        f"upperBound {nums['upperBound']} != log((1-beta)/alpha) = {exp_upper:.4f} "
                        f"(alpha={a}, beta={b}) -- forged or inconsistent bound"
                    )
                if abs(nums["lowerBound"] - exp_lower) > BOUND_TOL:
                    v.append(
                        f"lowerBound {nums['lowerBound']} != log(beta/(1-alpha)) = {exp_lower:.4f} "
                        f"(alpha={a}, beta={b})"
                    )
    if all(_is_int(rec.get(k)) for k in ("wins", "losses", "draws", "games")):
        if rec["wins"] + rec["losses"] + rec["draws"] != rec["games"]:
            v.append(
                f"wins+losses+draws ({rec['wins']}+{rec['losses']}+{rec['draws']}) != games ({rec['games']})"
            )
    if all(_is_num(rec.get(k)) for k in ("llr", "lowerBound", "upperBound")) and "boundary" in rec:
        derived = derive_boundary(rec["llr"], rec["lowerBound"], rec["upperBound"])
        if rec["boundary"] != derived:
            v.append(
                f"boundary '{rec['boundary']}' contradicts llr={rec['llr']} vs "
                f"[{rec['lowerBound']}, {rec['upperBound']}] (derived '{derived}')"
            )
    dec, bnd = rec.get("decision"), rec.get("boundary")
    if dec == "promote" and bnd != "upper":
        v.append(
            f"decision 'promote' requires a crossed UPPER SPRT bound (boundary 'upper'); "
            f"got '{bnd}' -- INV-1 violation"
        )
    if dec == "reject" and bnd != "lower":
        v.append(f"decision 'reject' requires boundary 'lower'; got '{bnd}'")
    if bnd == "none" and dec in ("promote", "reject"):
        v.append(
            f"decision '{dec}' on a non-crossed test (boundary 'none') is invalid -- "
            f"use 'hold_for_more_data'"
        )
    return v


def lint_record(rec) -> list[str]:
    viol = validate_sprt_record(rec)
    # Only run logic checks once the structure is sound enough to trust the fields.
    if not any(x.startswith("missing required field") or x.startswith("record is not") for x in viol):
        viol += check_consistency(rec)
    return viol


def is_sprt_record(path: Path) -> bool:
    if path.suffix != ".json":
        return False
    try:
        rec = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return False
    return isinstance(rec, dict) and "schemaVersion" in rec and "boundary" in rec


def _iter_json(paths):
    for p in paths:
        p = Path(p)
        if p.is_dir():
            for f in sorted(p.rglob("*.json")):
                # Skip test fixtures (records that live DIRECTLY in a fixtures/ dir): they
                # include intentionally-invalid records, linted explicitly by the tests. The
                # check is the immediate parent only (not any "fixtures" anywhere in the
                # path), so a real record can't be silently skipped by an unlucky path. An
                # explicit file path is always linted.
                if f.parent.name == "fixtures":
                    continue
                yield f
        elif p.suffix == ".json":
            yield p


def main(argv) -> int:
    paths = argv[1:] or ["benchmarks"]
    checked = 0
    total = 0
    for path in _iter_json(paths):
        if not is_sprt_record(path):
            continue
        checked += 1
        try:
            rec = json.loads(path.read_text(encoding="utf-8"))
        except Exception as e:
            print(f"FAIL {path}\n  - unreadable/invalid JSON: {e}")
            total += 1
            continue
        viol = lint_record(rec)
        if viol:
            print(f"FAIL {path}")
            for x in viol:
                print(f"  - {x}")
            total += len(viol)
        else:
            print(f"ok   {path}")
    print(f"\n{checked} SPRT record(s) checked, {total} violation(s).")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
