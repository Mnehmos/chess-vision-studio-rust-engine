#!/usr/bin/env python3
"""Tests for the corpus-invariant linter + the fail-closed source splitter (issue #9).
Standalone (no pytest needed): `python training/scripts/test_lint_corpus.py`."""
from __future__ import annotations

import copy
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "gen8" / "scripts"))
import lint_corpus as lc  # noqa: E402
import split_by_source as sbs  # noqa: E402

VALID_SEED = {
    "schemaVersion": 1,
    "seedId": "seed-1",
    "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "source": {"kind": "real_game", "sourceId": "lichess", "gameId": "g123", "ply": 42},
    "splitKey": "lichess/g123",
    "finder": {"engineId": "cvs-finder", "binarySha256": "aa", "modelSha256": "bb", "optionsHash": "cc"},
    "reason": "oracle_disagreement",
}
VALID_LABEL = {
    "schemaVersion": 1,
    "seedId": "seed-1",
    "labeler": {"engineId": "stockfish-16", "binarySha256": "dd", "version": "16"},
    "score": {"cp": 35},
    "labelStatus": "usable",
}


def test_valid_seed_and_label():
    assert lc.validate_seed(VALID_SEED) == []
    assert lc.validate_label(VALID_LABEL) == []


def test_seed_missing_source_key_is_rejected():
    s = copy.deepcopy(VALID_SEED)
    del s["splitKey"]
    s["source"].pop("gameId")
    assert any("INV-3" in x for x in lc.validate_seed(s))


def test_finder_equals_labeler_rejected():
    s = copy.deepcopy(VALID_SEED)
    lab = copy.deepcopy(VALID_LABEL)
    lab["labeler"]["engineId"] = s["finder"]["engineId"]  # same engine finds AND labels
    assert any("INV-4" in x for x in lc.finder_labeler_violations([s], [lab]))


def test_finder_different_from_labeler_ok():
    assert lc.finder_labeler_violations([VALID_SEED], [VALID_LABEL]) == []


def test_source_spanning_two_slices_rejected():
    a, b = copy.deepcopy(VALID_SEED), copy.deepcopy(VALID_SEED)  # same gameId -> same key
    assert any("spans slices" in x for x in lc.cross_slice_violations({"train": [a], "validation": [b]}))


def test_same_source_one_slice_ok():
    a, b = copy.deepcopy(VALID_SEED), copy.deepcopy(VALID_SEED)
    assert lc.cross_slice_violations({"train": [a, b]}) == []


def test_bad_enums_rejected():
    s = copy.deepcopy(VALID_SEED)
    s["reason"] = "because"
    assert any("reason" in x for x in lc.validate_seed(s))
    s2 = copy.deepcopy(VALID_SEED)
    s2["source"]["kind"] = "twitch"
    assert any("kind" in x for x in lc.validate_seed(s2))
    lab = copy.deepcopy(VALID_LABEL)
    lab["labelStatus"] = "great"
    assert any("labelStatus" in x for x in lc.validate_label(lab))


def test_unknown_field_rejected():
    s = copy.deepcopy(VALID_SEED)
    s["sourceId"] = "oops"  # belongs under source, not top-level
    assert any("unknown field" in x for x in lc.validate_seed(s))
    s2 = copy.deepcopy(VALID_SEED)
    s2["source"]["gameld"] = "x"  # typo of gameId
    assert any("unknown source field" in x for x in lc.validate_seed(s2))


# --- the fail-closed source splitter (INV-3) ---
def test_splitter_key_for_fails_closed():
    assert sbs.key_for({"fen": "abc"}) is None  # no source key -> None, NO FEN fallback
    assert sbs.key_for({"fen": "abc"}, allow_fen_fallback=True) == "abc"  # legacy escape
    assert sbs.key_for({"splitKey": "k"}) == "k"
    assert sbs.key_for({"gameId": "g"}) == "||g"  # join of (sourceBucket, sourceFile, gameId)


def test_splitter_empty_join_is_none():
    # all three join fields absent -> "||" -> no stable key -> None.
    assert sbs.key_for({"sourceBucket": "", "sourceFile": "", "gameId": ""}) is None


# --- regression: review bypasses (F1, F2, F4, F5, F6, F8, F9) ---
def test_duplicate_seedid_rejected():  # F1
    a, b = copy.deepcopy(VALID_SEED), copy.deepcopy(VALID_SEED)
    a["finder"]["engineId"] = "engine-A"
    b["finder"]["engineId"] = "engine-B"  # same seedId, different finder
    lab = copy.deepcopy(VALID_LABEL)
    lab["labeler"]["engineId"] = "engine-A"  # collides with seed a's finder
    viol = lc.finder_labeler_violations([a, b], [lab])
    assert any("duplicate seedId" in x for x in viol), viol
    assert any("INV-4" in x for x in viol), viol  # the shadowed collision is still caught


def test_orphan_label_rejected():  # F2
    lab = copy.deepcopy(VALID_LABEL)
    lab["seedId"] = "ghost"
    assert any("joins to no seed" in x for x in lc.finder_labeler_violations([VALID_SEED], [lab]))


def test_whitespace_values_are_not_valid_keys():  # F4
    s = copy.deepcopy(VALID_SEED)
    s["splitKey"] = "   "
    s["source"]["gameId"] = "   "
    assert lc.stable_source_key(s) is None
    assert any("INV-3" in x for x in lc.validate_seed(s))
    assert sbs.key_for({"splitKey": "   "}) is None
    assert sbs.key_for({"gameId": "   "}) is None


def test_splitkey_precedence_same_game_one_key():  # F5
    a, b = copy.deepcopy(VALID_SEED), copy.deepcopy(VALID_SEED)
    a["source"]["gameId"] = "g1"
    b["source"]["gameId"] = "g2"  # different gameId but SAME splitKey
    assert lc.stable_source_key(a) == lc.stable_source_key(b)  # splitKey wins, not gameId
    assert any("spans slices" in x for x in lc.cross_slice_violations({"train": [a], "test": [b]}))


def test_splitter_none_field_fails_closed():  # F6
    assert sbs.key_for({"gameId": None}) is None
    assert sbs.key_for({"sourceFile": None, "gameId": None}) is None


def test_schemaversion_bool_rejected():  # F8
    s = copy.deepcopy(VALID_SEED)
    s["schemaVersion"] = True
    assert any("schemaVersion" in x for x in lc.validate_seed(s))
    lab = copy.deepcopy(VALID_LABEL)
    lab["schemaVersion"] = True
    assert any("schemaVersion" in x for x in lc.validate_label(lab))


def test_perturbation_depth_type_checked():  # F9
    s = copy.deepcopy(VALID_SEED)
    s["source"]["perturbationDepth"] = "deep"
    assert any("perturbationDepth" in x for x in lc.validate_seed(s))


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
