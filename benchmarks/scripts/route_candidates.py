#!/usr/bin/env python3
"""Two-axis gap-candidate routing (#11, slice 1).

A gap candidate (a position flagged from diverse-opponent play) carries EVIDENCE. This routes each
candidate onto TWO ORTHOGONAL axes — never collapsing them into one "interesting" score:

  - CALIBRATION-HOLE axis  (consumer: training corpus): how wrong is the engine vs an INDEPENDENT
    oracle? Evidence: engine-vs-oracle score delta, move regret under oracle child scoring,
    eval-trajectory instability, repeated error across related source positions.
  - PRACTICAL-DIFFICULTY axis (consumer: product difficulty model): how hard is the position to
    PLAY? Evidence: viable-reply compression (#1), forcing-corridor length, promotion/defender
    commitment (#2).

Orthogonality is STRUCTURAL: each axis reads a DISJOINT evidence set (`CALIBRATION_SIGNALS` vs
`DIFFICULTY_SIGNALS`), so a position can be a large calibration hole yet practically easy, or a hard
practical puzzle the engine already evaluates correctly. Collapsing them would conflate "the engine
is wrong here" with "this is hard to play".

An UNSTABLE evaluation is a candidate-SELECTION signal, NOT a training label. A candidate is
`corpusEligible` only after it passes the #7 stabilization gate AND #9 provenance AND carries a
usable oracle label — recorded here as gates (fail-closed), never recomputed. Selection != labeling.

Pure + deterministic; no engine calls, no external binaries.

    python benchmarks/scripts/route_candidates.py <candidates.jsonl> [routed.jsonl]
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
try:
    from motif_taxonomy import validate_slug as _motif_known
except Exception:  # taxonomy absent -> a motif tag simply can't be confirmed as known
    def _motif_known(_slug):
        return False

# Disjoint evidence sets — the structural guarantee of orthogonality.
CALIBRATION_SIGNALS = (
    "engine_oracle_delta_cp",   # |engine - oracle| score delta (cp)
    "move_regret_cp",           # regret of the engine's move under oracle child scoring (cp)
    "trajectory_instability",   # eval-trajectory volatility (0..1); a SELECTION signal
    "repeated_error_count",     # related source positions showing the same error
)
DIFFICULTY_SIGNALS = (
    "reply_viable_fraction_50cp",  # #1: fraction of replies within 50cp of best (LOWER = harder)
    "forcing_corridor_len",        # length of the only-move corridor
    "promotion_commitment",        # #2: defender commitment to stopping promotion
)

# The three confident stabilization statuses (rust #7 / app #35 policy), kebab-case as the engine
# emits them. A candidate flagged FOR instability is not yet usable — it must stabilize first.
CONFIDENT_STABILIZATION = frozenset({"exact-tablebase", "verified-forced-mate", "stable-at-budget"})


def _num(evidence, key):
    """Evidence value as a float; missing / non-numeric / NaN -> 0.0 (fail-soft for scoring)."""
    v = evidence.get(key)
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return 0.0
    if not math.isfinite(v):  # NaN or ±inf -> 0 (keeps scores finite + JSON-valid for strict parsers)
        return 0.0
    return float(v)


def _frac(evidence, key):
    """A [0,1] evidence value; missing -> 1.0 (treat unknown reply-compression as 'not hard')."""
    if key not in evidence:
        return 1.0
    return min(1.0, max(0.0, _num(evidence, key)))


def calibration_score(evidence):
    """Bigger = a worse calibration hole. Reads ONLY `CALIBRATION_SIGNALS`."""
    return (
        abs(_num(evidence, "engine_oracle_delta_cp")) * 1.0
        + _num(evidence, "move_regret_cp") * 1.0
        + _num(evidence, "trajectory_instability") * 50.0
        + _num(evidence, "repeated_error_count") * 25.0
    )


def difficulty_score(evidence):
    """Bigger = harder to play. Reads ONLY `DIFFICULTY_SIGNALS` (reply compression is inverted)."""
    return (
        (1.0 - _frac(evidence, "reply_viable_fraction_50cp")) * 100.0
        + _num(evidence, "forcing_corridor_len") * 20.0
        + _num(evidence, "promotion_commitment") * 15.0
    )


def corpus_eligibility(candidate):
    """A candidate is usable for the TRAINING CORPUS only after it passes #7 stabilization + #9
    provenance + carries a usable oracle label. Returns (eligible, reasons). Fail-closed: any missing
    gate is a reason. Instability selects a candidate; it does not make it a label."""
    reasons = []
    status = candidate.get("stabilizationStatus")
    norm = status.replace("_", "-").strip().lower() if isinstance(status, str) else ""
    if norm not in CONFIDENT_STABILIZATION:
        reasons.append(f"stabilization not confident (#7): {status!r}")
    if candidate.get("provenanceOk") is not True:
        reasons.append("provenance not validated (#9)")
    if candidate.get("labelStatus") != "usable":
        reasons.append(f"labelStatus not usable: {candidate.get('labelStatus')!r}")
    return (not reasons, reasons)


def route_candidate(candidate):
    """Route one candidate onto the two orthogonal axes + record corpus eligibility."""
    ev = candidate.get("evidence") if isinstance(candidate.get("evidence"), dict) else {}
    eligible, reasons = corpus_eligibility(candidate)
    motif = candidate.get("motif")
    return {
        "candidateId": candidate.get("candidateId"),
        # The motif (weakness category) this candidate exercises, validated against the taxonomy so
        # the RSI loop can route/aggregate by tactical/positional motif (motif-taxonomy.json).
        "motif": motif,
        "motifKnown": bool(isinstance(motif, str) and _motif_known(motif)),
        "calibration": {
            "score": calibration_score(ev),
            "signals": {k: _num(ev, k) for k in CALIBRATION_SIGNALS},
        },
        "difficulty": {
            "score": difficulty_score(ev),
            # Echo with the SAME semantics the score uses (reply fraction via _frac so a missing
            # value echoes as 1.0, matching the score) so the echo can reconstruct the difficulty.
            "signals": {
                k: (_frac(ev, k) if k == "reply_viable_fraction_50cp" else _num(ev, k))
                for k in DIFFICULTY_SIGNALS
            },
        },
        # Calibration axis feeds the corpus ONLY when eligible; the difficulty axis (product
        # difficulty model) is computed regardless — it does not require a usable training label.
        "corpusEligible": eligible,
        "ineligibleReasons": reasons,
    }


def main(argv):
    if len(argv) < 2:
        sys.exit("usage: route_candidates.py <candidates.jsonl> [routed.jsonl]")
    in_path = argv[1]
    out_path = argv[2] if len(argv) > 2 else os.path.splitext(in_path)[0] + ".routed.jsonl"

    routed, parse_errors = [], 0
    with open(in_path, encoding="utf8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                routed.append(route_candidate(json.loads(line)))
            except (ValueError, AttributeError):
                parse_errors += 1

    with open(out_path, "w", encoding="utf8") as fh:
        for r in routed:
            fh.write(json.dumps(r) + "\n")

    eligible = sum(1 for r in routed if r["corpusEligible"])
    print(
        f"{in_path}: routed {len(routed)} candidates -> {out_path} "
        f"({eligible} corpus-eligible, {len(routed) - eligible} selection-only, "
        f"{parse_errors} unparseable lines)",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
