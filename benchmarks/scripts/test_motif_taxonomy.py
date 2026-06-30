#!/usr/bin/env python3
"""Tests for the motif taxonomy + its gap-router integration. Standalone:
`python benchmarks/scripts/test_motif_taxonomy.py`."""
import json
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import motif_taxonomy as t  # noqa: E402
import route_candidates as r  # noqa: E402


def test_loads_and_has_all_categories():
    tax = t.load_taxonomy()
    cats = {m["category"] for m in tax["motifs"]}
    assert cats == {"tactical", "mate", "positional"}
    assert len(tax["motifs"]) >= 120  # tactical + mate + positional combined


def test_slugs_unique_and_validate():
    assert t.validate_slug("fork") and t.validate_slug("isolated-pawn")
    assert not t.validate_slug("not-a-motif")
    slugs = [m["slug"] for m in t.motifs()]
    assert len(slugs) == len(set(slugs))


def test_coverage_partitions_total():
    cov = t.coverage()
    assert cov["detected"] + cov["gaps"] == cov["total"]
    assert sum(cov[c]["total"] for c in t.CATEGORIES) == cov["total"]
    # known detectors exist (fork/pin/pawn-structure are wired); and there ARE gaps (the roadmap)
    assert cov["detected"] >= 5 and cov["gaps"] >= 1


def test_gaps_have_no_detector_detected_have_one():
    assert all(m.get("detectedBy") is None for m in t.gaps())
    assert all(m.get("detectedBy") for m in t.detected())
    # a couple of known-wired motifs must be in `detected`
    detected_slugs = {m["slug"] for m in t.detected()}
    assert {"fork", "absolute-pin", "isolated-pawn"} <= detected_slugs


def test_duplicate_slug_fails_closed():
    with tempfile.TemporaryDirectory() as d:
        bad = Path(d) / "tax.json"
        bad.write_text(json.dumps({"motifs": [
            {"slug": "x", "name": "X", "category": "tactical"},
            {"slug": "x", "name": "X2", "category": "tactical"},
        ]}), encoding="utf8")
        try:
            t.load_taxonomy(str(bad))
            assert False, "expected duplicate slugs to raise"
        except ValueError as e:
            assert "duplicate" in str(e)


def test_bad_category_fails_closed():
    with tempfile.TemporaryDirectory() as d:
        bad = Path(d) / "tax.json"
        bad.write_text(json.dumps({"motifs": [{"slug": "x", "name": "X", "category": "nonsense"}]}), encoding="utf8")
        try:
            t.load_taxonomy(str(bad))
            assert False, "expected bad category to raise"
        except ValueError:
            pass


def test_router_tags_candidate_with_validated_motif():
    # A candidate carrying a known motif is echoed + flagged known; an unknown one is flagged.
    known = r.route_candidate({"candidateId": "c1", "motif": "fork", "evidence": {}})
    assert known["motif"] == "fork" and known["motifKnown"] is True
    unknown = r.route_candidate({"candidateId": "c2", "motif": "made-up", "evidence": {}})
    assert unknown["motif"] == "made-up" and unknown["motifKnown"] is False
    none = r.route_candidate({"candidateId": "c3", "evidence": {}})
    assert none["motif"] is None and none["motifKnown"] is False


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
