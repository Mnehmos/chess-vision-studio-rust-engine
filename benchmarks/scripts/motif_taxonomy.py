#!/usr/bin/env python3
"""Motif taxonomy — the canonical weakness-category vocabulary for the RSI loop (#11 gap routing).

Loads benchmarks/data/motif-taxonomy.json (tactical + positional motifs). Each motif's `detectedBy`
names the engine component that recognizes it today; `None` is a DETECTION GAP — i.e. an item on the
RSI detector roadmap. The CLI prints a coverage report (detected vs gaps), which IS that roadmap.

    python benchmarks/scripts/motif_taxonomy.py        # coverage report + the undetected roadmap
"""
import json
import os
import sys

_DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data", "motif-taxonomy.json")
CATEGORIES = ("tactical", "mate", "positional")


def load_taxonomy(path=_DATA):
    """Load + validate the taxonomy: unique slugs, known categories, a non-empty name per motif."""
    with open(path, encoding="utf8") as fh:
        data = json.load(fh)
    ms = data["motifs"]
    slugs = [m["slug"] for m in ms]
    dupes = sorted({s for s in slugs if slugs.count(s) > 1})
    if dupes:
        raise ValueError(f"duplicate motif slugs: {dupes}")
    for m in ms:
        if m.get("category") not in CATEGORIES:
            raise ValueError(f"motif {m.get('slug')!r}: category must be one of {CATEGORIES}")
        if not (isinstance(m.get("name"), str) and m["name"].strip()):
            raise ValueError(f"motif {m.get('slug')!r}: missing name")
    return data


def motifs(taxonomy=None):
    return (taxonomy or load_taxonomy())["motifs"]


def by_slug(taxonomy=None):
    return {m["slug"]: m for m in motifs(taxonomy)}


def validate_slug(slug, taxonomy=None):
    """True iff `slug` is a known motif (used to gate candidate.motif tags against the taxonomy)."""
    return slug in by_slug(taxonomy)


def detected(taxonomy=None):
    return [m for m in motifs(taxonomy) if m.get("detectedBy")]


def gaps(taxonomy=None):
    """Motifs with no detector yet — the RSI detector roadmap."""
    return [m for m in motifs(taxonomy) if not m.get("detectedBy")]


def coverage(taxonomy=None):
    tax = taxonomy or load_taxonomy()
    ms = tax["motifs"]

    def split(cat):
        c = [m for m in ms if m["category"] == cat]
        d = sum(1 for m in c if m.get("detectedBy"))
        return {"total": len(c), "detected": d, "gaps": len(c) - d}

    out = {
        "total": len(ms),
        "detected": len(detected(tax)),
        "gaps": len(gaps(tax)),
    }
    for cat in CATEGORIES:
        out[cat] = split(cat)
    return out


def main(argv):
    tax = load_taxonomy()
    cov = coverage(tax)
    parts = ", ".join(f"{cov[c]['total']} {c}" for c in CATEGORIES)
    print(f"motif taxonomy: {cov['total']} motifs ({parts})")
    print(f"  detected by the engine: {cov['detected']}    gaps (RSI roadmap): {cov['gaps']}")
    for cat in CATEGORIES:
        print(f"  {cat + ':':12} {cov[cat]['detected']}/{cov[cat]['total']} detected")
    print("\n  detector roadmap — undetected motifs (build a fact specialist for each):")
    for m in gaps(tax):
        print(f"    [{m['category'][:3]}] {m['slug']:22} ({m['family']})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
