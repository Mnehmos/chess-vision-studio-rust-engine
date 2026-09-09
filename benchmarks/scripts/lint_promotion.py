#!/usr/bin/env python3
"""Promotion-policy linter (issue #5).

Enforces INV-1 for strength changes and INV-2 for performance-only changes.

INV-1: a candidate may be PROMOTED only when a declared SPRT test crossed the
*upper* boundary under its declared hypothesis. Fixed-N results, Elo point estimates, LOS,
and screens justify more testing or analysis-only decisions -- never promotion.

This lints the machine-readable SPRT result records (benchmarks/schemas/sprt-result.schema.json):
field/type validation plus the logic checks that catch policy violations:
  * decision 'promote'  requires boundary 'upper'  (crossed upper SPRT bound)
  * decision 'reject'   requires boundary 'lower'
  * a non-crossed test (boundary 'none') may only HOLD -- never promote/reject
  * the recorded boundary must match the LLR vs the declared bounds
  * wins + losses + draws == games

INV-2 (performance-only changes): a change that makes the engine compute the SAME search
result faster is accepted on measurement, not on Elo. It may be marked 'accept_performance'
in a perf record (benchmarks/schemas/perf-result.schema.json) only when it proves:
  * full behavioral parity -- every paired cold fixed-node search identical (non-timing)
  * zero failing tests
  * a repeatable speedup: median >= 2%, consistent across repeats (one-sided sign test
    over per-repeat ratios, p <= 0.05, >= 5 repeats), and no repeat worse than -2%
  * no material resource regression (peak RSS / binary size within 5%)
  * strengthClaim false -- a perf record never claims Elo
Anything that changes what the engine DECIDES (search, eval, pruning, ordering, move
selection) is not performance-only: it needs an SPRT record under INV-1.

Usage:
    python lint_promotion.py [PATH ...]          # default: benchmarks/
Each PATH is a file or directory. Any .json that looks like an SPRT record (has
"schemaVersion" and "boundary") or a perf record (has "schemaVersion" and
"changeClass") is checked; other JSON (gate records etc.) is skipped.
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

# INV-2 thresholds for performance-only acceptance.
PERF_MIN_MEDIAN_SPEEDUP_PCT = 2.0
PERF_MIN_PARITY_SEARCHES = 100
PERF_MAX_RESOURCE_DELTA_PCT = 5.0
# Wall-clock repeats are noisy on a shared desktop: one unlucky repeat must not veto a
# real gain, and a lucky one must not carry it. The consistency requirement is therefore
# a sign test over repeats plus a floor on the worst repeat -- not "every repeat wins".
PERF_MIN_REPEATS = 5
PERF_MAX_SIGN_TEST_P = 0.05
PERF_WORST_REPEAT_FLOOR_PCT = -2.0

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


# --------------------------------------------------------------------------------------
# INV-2: performance-only records (schemas/perf-result.schema.json)
# --------------------------------------------------------------------------------------

PERF_REQUIRED = [
    "schemaVersion", "changeClass", "experimentId", "baselineId", "candidateId",
    "parity", "speed", "tests", "resources", "strengthClaim", "decision", "provenance",
]
PERF_OPTIONAL_TOP = {"screen"}
PERF_ALLOWED_DECISIONS = {"accept_performance", "reject_performance", "hold_for_more_data"}
PERF_SECTIONS = {
    "parity": (["searches", "identicalSearches", "nodeCeiling", "excludedFields"], {"artifact"}),
    "speed": (["positions", "repeats", "nodeCeiling", "baselineMs", "candidateMs",
               "aggregateSpeedupPct", "medianSpeedupPct", "minSpeedupPct", "maxSpeedupPct",
               "repeatsPositive", "repeatsTotal", "signTestP"],
              {"artifact"}),
    "tests": (["passed", "failed", "command"], set()),
    "resources": (["peakRssDeltaPct", "binaryBytesDeltaPct"], set()),
}
PERF_REQUIRED_PROV = ["engineSha", "netSha", "candArgs", "baseArgs", "threads", "host"]
PERF_OPTIONAL_PROV = {"toolchain"}


def validate_perf_record(rec) -> list[str]:
    """Schema-level validation of a performance-only record."""
    v: list[str] = []
    if not isinstance(rec, dict):
        return ["record is not a JSON object"]
    for f in PERF_REQUIRED:
        if f not in rec:
            v.append(f"missing required field: {f}")
    if "schemaVersion" in rec and rec["schemaVersion"] != 1:
        v.append("schemaVersion must be 1")
    if rec.get("changeClass") != "performance-only":
        v.append("changeClass must be 'performance-only' for a perf record")
    for f in ("experimentId", "baselineId", "candidateId"):
        if f in rec and (not isinstance(rec[f], str) or not rec[f]):
            v.append(f"{f} must be a non-empty string")
    if "strengthClaim" in rec and rec["strengthClaim"] is not False:
        v.append("strengthClaim must be false -- a perf record never claims Elo (use an SPRT record)")
    if "decision" in rec and rec["decision"] not in PERF_ALLOWED_DECISIONS:
        v.append(f"decision must be one of {sorted(PERF_ALLOWED_DECISIONS)}")
    for k in rec:
        if k not in PERF_REQUIRED and k not in PERF_OPTIONAL_TOP:
            v.append(f"unknown top-level field: {k} (schema is additionalProperties:false)")
    for name, (required, optional) in PERF_SECTIONS.items():
        sec = rec.get(name)
        if name not in rec:
            continue
        if not isinstance(sec, dict):
            v.append(f"{name} must be an object")
            continue
        for f in required:
            if f not in sec:
                v.append(f"{name} missing required field: {f}")
        for k in sec:
            if k not in required and k not in optional:
                v.append(f"unknown {name} field: {k}")
    par = rec.get("parity")
    if isinstance(par, dict):
        for f in ("searches", "identicalSearches", "nodeCeiling"):
            if f in par and (not _is_int(par[f]) or par[f] < 0):
                v.append(f"parity.{f} must be a non-negative integer")
        if "excludedFields" in par and not isinstance(par["excludedFields"], list):
            v.append("parity.excludedFields must be an array")
    sp = rec.get("speed")
    if isinstance(sp, dict):
        for f in ("positions", "repeats", "nodeCeiling"):
            if f in sp and (not _is_int(sp[f]) or sp[f] < 1):
                v.append(f"speed.{f} must be a positive integer")
        for f in ("baselineMs", "candidateMs", "aggregateSpeedupPct", "medianSpeedupPct",
                  "minSpeedupPct", "maxSpeedupPct", "signTestP"):
            if f in sp and not _is_num(sp[f]):
                v.append(f"speed.{f} must be a number")
        for f in ("repeatsPositive", "repeatsTotal"):
            if f in sp and (not _is_int(sp[f]) or sp[f] < 0):
                v.append(f"speed.{f} must be a non-negative integer")
        if _is_int(sp.get("repeatsPositive")) and _is_int(sp.get("repeatsTotal")):
            if sp["repeatsPositive"] > sp["repeatsTotal"]:
                v.append("speed.repeatsPositive cannot exceed speed.repeatsTotal")
        if _is_int(sp.get("repeatsTotal")) and _is_int(sp.get("repeats")):
            if sp["repeatsTotal"] != sp["repeats"]:
                v.append("speed.repeatsTotal must equal speed.repeats")
    ts = rec.get("tests")
    if isinstance(ts, dict):
        for f in ("passed", "failed"):
            if f in ts and (not _is_int(ts[f]) or ts[f] < 0):
                v.append(f"tests.{f} must be a non-negative integer")
        if "command" in ts and (not isinstance(ts["command"], str) or not ts["command"]):
            v.append("tests.command must be a non-empty string")
    res = rec.get("resources")
    if isinstance(res, dict):
        for f in ("peakRssDeltaPct", "binaryBytesDeltaPct"):
            if f in res and not _is_num(res[f]):
                v.append(f"resources.{f} must be a number")
    prov = rec.get("provenance")
    if "provenance" in rec:
        if not isinstance(prov, dict):
            v.append("provenance must be an object")
        else:
            for f in PERF_REQUIRED_PROV:
                if f not in prov:
                    v.append(f"provenance missing required field: {f}")
            if "threads" in prov and (not _is_int(prov["threads"]) or prov["threads"] < 1):
                v.append("provenance.threads must be a positive integer")
            for k in prov:
                if k not in PERF_REQUIRED_PROV and k not in PERF_OPTIONAL_PROV:
                    v.append(f"unknown provenance field: {k}")
    scr = rec.get("screen")
    if isinstance(scr, dict):
        allowed = {"games", "wins", "losses", "draws", "sprtRecord"}
        for k in scr:
            if k not in allowed:
                v.append(f"unknown screen field: {k}")
        if all(_is_int(scr.get(k)) for k in ("wins", "losses", "draws", "games")):
            if scr["wins"] + scr["losses"] + scr["draws"] != scr["games"]:
                v.append("screen wins+losses+draws != games")
    return v


def check_perf_consistency(rec) -> list[str]:
    """INV-2 gate: what 'accept_performance' actually requires."""
    v: list[str] = []
    par = rec.get("parity") or {}
    sp = rec.get("speed") or {}
    ts = rec.get("tests") or {}
    res = rec.get("resources") or {}
    if _is_int(par.get("searches")) and _is_int(par.get("identicalSearches")):
        if par["identicalSearches"] > par["searches"]:
            v.append("parity.identicalSearches cannot exceed parity.searches")
    if all(_is_num(sp.get(k)) for k in ("baselineMs", "candidateMs", "aggregateSpeedupPct")):
        if sp["candidateMs"] > 0:
            derived = (sp["baselineMs"] / sp["candidateMs"] - 1.0) * 100.0
            if abs(derived - sp["aggregateSpeedupPct"]) > 0.05:
                v.append(
                    f"speed.aggregateSpeedupPct {sp['aggregateSpeedupPct']} != "
                    f"baselineMs/candidateMs-1 = {derived:.2f}% -- inconsistent timing record"
                )
    if all(_is_num(sp.get(k)) for k in ("minSpeedupPct", "medianSpeedupPct", "maxSpeedupPct")):
        if not (sp["minSpeedupPct"] <= sp["medianSpeedupPct"] <= sp["maxSpeedupPct"]):
            v.append("speed min/median/max speedups are not ordered min <= median <= max")
    if rec.get("decision") != "accept_performance":
        return v
    # --- INV-2 acceptance conditions ---
    if not (_is_int(par.get("searches")) and _is_int(par.get("identicalSearches"))):
        v.append("accept_performance requires a numeric parity result -- INV-2 violation")
    else:
        if par["identicalSearches"] != par["searches"]:
            v.append(
                f"accept_performance requires FULL behavioral parity; "
                f"{par['searches'] - par['identicalSearches']} of {par['searches']} paired "
                f"searches differ -- the change is not performance-only (INV-2 violation)"
            )
        if par["searches"] < PERF_MIN_PARITY_SEARCHES:
            v.append(
                f"accept_performance requires >= {PERF_MIN_PARITY_SEARCHES} paired parity "
                f"searches; got {par['searches']} -- INV-2 violation"
            )
    if _is_int(ts.get("failed")) and ts["failed"] != 0:
        v.append(f"accept_performance requires zero failing tests; got {ts['failed']} -- INV-2 violation")
    if _is_int(ts.get("passed")) and ts["passed"] <= 0:
        v.append("accept_performance requires a non-empty passing test run -- INV-2 violation")
    if _is_num(sp.get("medianSpeedupPct")):
        if sp["medianSpeedupPct"] < PERF_MIN_MEDIAN_SPEEDUP_PCT:
            v.append(
                f"accept_performance requires medianSpeedupPct >= {PERF_MIN_MEDIAN_SPEEDUP_PCT}; "
                f"got {sp['medianSpeedupPct']} -- INV-2 violation"
            )
    else:
        v.append("accept_performance requires a measured medianSpeedupPct -- INV-2 violation")
    if _is_num(sp.get("minSpeedupPct")) and sp["minSpeedupPct"] < PERF_WORST_REPEAT_FLOOR_PCT:
        v.append(
            f"accept_performance requires no repeat worse than {PERF_WORST_REPEAT_FLOOR_PCT}%; "
            f"worst repeat {sp['minSpeedupPct']}% -- INV-2 violation"
        )
    if _is_int(sp.get("repeatsTotal")) and _is_int(sp.get("repeatsPositive")):
        if sp["repeatsTotal"] < PERF_MIN_REPEATS:
            v.append(
                f"accept_performance requires >= {PERF_MIN_REPEATS} timed repeats; "
                f"got {sp['repeatsTotal']} -- INV-2 violation"
            )
        derived_p = sign_test_p(sp["repeatsPositive"], sp["repeatsTotal"])
        if _is_num(sp.get("signTestP")) and abs(derived_p - sp["signTestP"]) > 1e-6:
            v.append(
                f"speed.signTestP {sp['signTestP']} != one-sided sign test over "
                f"{sp['repeatsPositive']}/{sp['repeatsTotal']} repeats = {derived_p:.6f}"
            )
        if derived_p > PERF_MAX_SIGN_TEST_P:
            v.append(
                f"accept_performance requires a consistent speedup: sign test over "
                f"{sp['repeatsPositive']}/{sp['repeatsTotal']} positive repeats gives "
                f"p={derived_p:.4f} > {PERF_MAX_SIGN_TEST_P} -- INV-2 violation"
            )
    else:
        v.append("accept_performance requires repeatsPositive/repeatsTotal -- INV-2 violation")
    for f in ("peakRssDeltaPct", "binaryBytesDeltaPct"):
        if _is_num(res.get(f)):
            if res[f] > PERF_MAX_RESOURCE_DELTA_PCT:
                v.append(
                    f"accept_performance requires resources.{f} <= {PERF_MAX_RESOURCE_DELTA_PCT}%; "
                    f"got {res[f]}% -- speedup bought with resources (INV-2 violation)"
                )
        else:
            v.append(f"accept_performance requires a measured resources.{f} -- INV-2 violation")
    # A screen is informational: it never promotes, and never blocks a parity-proven speedup.
    return v


def sign_test_p(positive: int, total: int) -> float:
    """One-sided sign test: P(X >= positive) under X ~ Binomial(total, 0.5).

    Answers "could this many repeats have come out faster by chance?" without assuming a
    distribution for wall-clock noise.
    """
    if total <= 0:
        return 1.0
    positive = max(0, min(positive, total))
    tail = sum(math.comb(total, i) for i in range(positive, total + 1))
    return tail / (2 ** total)


def is_perf_record(rec) -> bool:
    return isinstance(rec, dict) and "changeClass" in rec


def lint_record(rec) -> list[str]:
    if is_perf_record(rec):
        viol = validate_perf_record(rec)
        if not any(x.startswith("missing required field") or x.startswith("record is not")
                   for x in viol):
            viol += check_perf_consistency(rec)
        return viol
    viol = validate_sprt_record(rec)
    # Only run logic checks once the structure is sound enough to trust the fields.
    if not any(x.startswith("missing required field") or x.startswith("record is not") for x in viol):
        viol += check_consistency(rec)
    return viol


def is_policy_record(path: Path) -> bool:
    """True for SPRT records (INV-1) and performance-only records (INV-2)."""
    if path.suffix != ".json":
        return False
    try:
        rec = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return False
    if not isinstance(rec, dict) or "schemaVersion" not in rec:
        return False
    return "boundary" in rec or "changeClass" in rec


# Back-compat alias for callers that predate perf records.
is_sprt_record = is_policy_record


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
        if not is_policy_record(path):
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
    print(f"\n{checked} policy record(s) checked, {total} violation(s).")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
