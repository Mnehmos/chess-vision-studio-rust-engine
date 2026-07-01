#!/usr/bin/env python3
"""Phase-8 detector-coverage guard (CLASSICAL_EVAL_EXPERIMENT.md).

Cross-checks the motif taxonomy's `detectedBy` claims against the ACTUAL detector
registry in the engine source (the validators registered in move_bundle.rs). Fails CI
when the taxonomy claims a `facts::motifs(...)`/`facts::mate_patterns(...)` detector that
the registry does NOT register — i.e. "documentation claims a detector the registry marks
missing". Also prints per-category / per-subsystem coverage counts straight from the
registry so coverage is never asserted from prose.

Ground truth = the source, not a running binary (no build required). Exit 0 = consistent.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # chess-vision-studio-rust-engine
TAX = ROOT / "benchmarks" / "data" / "motif-taxonomy.json"
MOVE_BUNDLE = ROOT / "src" / "facts" / "move_bundle.rs"

# detectedBy inner-name (for facts::motifs(...)) -> the validator it must register.
# Names differ from the validator prefix in two historical cases; keep this explicit.
MOTIF_TO_VALIDATOR = {
    "fork": "fork_validation",
    "pin": "pin_validation",
    "skewer": "skewer_validation",
    "discovery": "discovery_validation",
    "remove_guard": "remove_guard_validation",
    "trapped_piece": "trapped_piece_validation",
    "overloading": "overload_validation",
    "attacking_the_defender": "attack_defender_validation",
    "interference": "interference_validation",
}
# facts::<subsystem> that are NOT the motif/mate validators but are real engine subsystems.
SUBSYSTEM_TO_VALIDATOR = {"pawn_structure": "pawn_structure", "piece_safety": "king_safety",
                          "square_control": "square_control"}
# subsystems referenced by detectedBy that are not part of the facts-validator registry
# (eval terms, promotion-pressure issue refs, raw SEE) — informational, not guarded here.
NON_FACTS = {"eval", "promotion_pressure", "SEE", "see", "hazards", "search"}


def registered_validators() -> set:
    """Every validator string registered in move_bundle.rs (base vec + pushes)."""
    src = MOVE_BUNDLE.read_text(encoding="utf-8")
    # the validators are all `"..."` string literals inside the base vec and the
    # `validators.push("...")` lines; grab every quoted token that is a validator name.
    names = set(re.findall(r'"([a-z_]+)"\.to_string\(\)', src))
    return names


def parse_detected_by(s: str):
    """Yield (subsystem, inner) for each `subsystem(inner)` term in a detectedBy string."""
    for m in re.finditer(r"([A-Za-z_]+(?:::[A-Za-z_]+)?)\(([^)]*)\)", s):
        sub = m.group(1)
        sub = sub.split("::")[-1] if "::" in sub else sub
        yield sub, m.group(2).strip()


def main() -> int:
    tax = json.loads(TAX.read_text(encoding="utf-8"))
    motifs = tax["motifs"]
    reg = registered_validators()

    detected = [m for m in motifs if m.get("detectedBy")]
    gaps = [m for m in motifs if not m.get("detectedBy")]

    # coverage counts from the registry side
    from collections import Counter
    by_cat = Counter(m["category"] for m in motifs)
    det_by_cat = Counter(m["category"] for m in detected)
    subsys = Counter()

    failures = []
    for m in detected:
        for sub, inner in parse_detected_by(m["detectedBy"]):
            if sub == "motifs":
                subsys[f"facts::motifs({inner})"] += 1
                val = MOTIF_TO_VALIDATOR.get(inner)
                if val is None:
                    failures.append(f"{m['slug']}: unknown facts::motifs({inner}) — no validator mapping")
                elif val not in reg:
                    failures.append(f"{m['slug']}: claims facts::motifs({inner}) -> {val}, but {val} is NOT registered")
            elif sub == "mate_patterns":
                subsys["facts::mate_patterns"] += 1
                if "mate_pattern_validation" not in reg:
                    failures.append(f"{m['slug']}: claims a mate pattern but mate_pattern_validation is NOT registered")
            elif sub in SUBSYSTEM_TO_VALIDATOR:
                subsys[f"facts::{sub}"] += 1
                val = SUBSYSTEM_TO_VALIDATOR[sub]
                if val not in reg:
                    failures.append(f"{m['slug']}: claims facts::{sub} -> {val}, but {val} is NOT registered")
            elif sub in NON_FACTS:
                subsys[f"{sub} (non-facts)"] += 1
            else:
                subsys[f"?{sub}"] += 1

    print("=== detector coverage (from registry) ===")
    print(f"total motifs: {len(motifs)}   detected: {len(detected)}   gaps: {len(gaps)}")
    for cat in ("tactical", "mate", "positional"):
        print(f"  {cat:11s}: {det_by_cat.get(cat,0):3d} / {by_cat.get(cat,0):3d} detected "
              f"({by_cat.get(cat,0)-det_by_cat.get(cat,0)} gaps)")
    print("\n=== detectedBy subsystem usage ===")
    for k, v in sorted(subsys.items(), key=lambda x: -x[1]):
        print(f"  {v:3d}  {k}")
    print(f"\n=== registered facts validators ({len(reg)}) ===")
    print("  " + ", ".join(sorted(reg)))

    if failures:
        print(f"\nFAIL: {len(failures)} taxonomy claim(s) not backed by the registry:")
        for f in failures:
            print("  ✗ " + f)
        return 1
    print("\nOK: every facts::motifs / mate_patterns / pawn_structure detectedBy claim is "
          "backed by a registered validator.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
