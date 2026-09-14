#!/usr/bin/env python3
"""Tests for the labeling funnel (#111). Standalone, no engine needed:
`python training/funnel/test_labeling_funnel.py` (also runs under pytest)."""
import json
import random
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import labeling_funnel as f  # noqa: E402

CFG = json.loads((HERE / "funnel-config.v1.json").read_text(encoding="utf-8"))
T2, T4 = CFG["tiers"]["tier2"], CFG["tiers"]["tier4"]
FAMILIES = f.load_taxonomy()["family"]


def coll(*items):
    return {"status": "computed", "items": list(items)}


def bundle(**before):
    base = {"sideToMove": "white", "pieces": [], "pawnStructure": {"doubled": [], "isolated": [], "passed": [], "islands": []},
            "kingSafety": coll(), "availableCaptures": coll(), "opponentAvailableCaptures": coll(),
            "squareFacts": coll(), "hazards": coll()}
    base.update(before)
    return {"before": base, "provenance": {"factsRegistryVersion": 23}, "errors": []}


def t0(slugs=(), phase="middlegame", bucket="Q1R2m4P2-Q1R2m4P2", tactical=0, legal=30, in_check=False):
    return {"status": "ok", "taxonomy": {"slugs": list(slugs), "families": [FAMILIES[s] for s in slugs], "unmapped": []},
            "deterministic_geometry": {"phase": phase, "materialBucket": bucket, "legalMoves": legal, "inCheck": in_check},
            "bounded_tactical_proof": {"tacticalItems": tactical}}


def t1(lo_score=10, hi_score=10, lo_move="e2e4", hi_move="e2e4", status="stable-at-budget", white=10):
    run = lambda s, m: {"scoreCpStm": s, "bestMove": m, "pv": [m, "e7e5"], "stabilization": {"status": status, "reasons": []}}  # noqa: E731
    return {"search_derived": {"budgets": [run(lo_score, lo_move), run(hi_score, hi_move)],
                               "derived": {"scoreDeltaCp": f.clamp_cp(hi_score) - f.clamp_cp(lo_score),
                                           "bestMoveChanged": lo_move != hi_move,
                                           "pvCommonPrefix": 2 if lo_move == hi_move else 0, "scoreCpWhite": white}}}


def items_for(n, seed=1):
    rng = random.Random(seed)
    out = []
    for i in range(n):
        comps = {k: rng.random() for k in T2["weights"]}
        pr, detail, reasons = f.score_priority(comps, T2["weights"])
        out.append({"id": f"{i:016x}", "priority": pr, "components": detail, "reasons": reasons})
    return out


def test_priority_is_sum_of_component_contributions():
    comps = {k: 0.5 for k in T2["weights"]}
    comps["bestMoveChange"] = 1.0
    pr, detail, reasons = f.score_priority(comps, T2["weights"])
    assert abs(pr - sum(d["contribution"] for d in detail.values())) < 1e-5
    assert set(detail) == set(T2["weights"])
    assert 0.0 <= pr <= 1.0
    assert reasons[0] == "scoreInstability" or detail[reasons[0]]["contribution"] >= detail["scoreInstability"]["contribution"]


def test_selection_is_order_independent_and_deterministic():
    items = items_for(500)
    a = f.select(items, T2, T4, seed=7)
    shuffled = items[:]
    random.Random(3).shuffle(shuffled)
    b = f.select(shuffled, T2, T4, seed=7)
    assert [r["id"] for r in a] == [r["id"] for r in b]
    assert [r["selection"] for r in a] == [r["selection"] for r in b]


def test_arm_sizes_and_disjointness():
    n = 1000
    recs = f.select(items_for(n), T2, T4, seed=7)
    sel = lambda k: {r["id"] for r in recs if r["selection"][k]}  # noqa: E731
    deep, audit, uniform, holdout = sel("deep"), sel("audit"), sel("uniform"), sel("holdout")
    assert len(deep) == round(T2["deepFraction"] * n)
    assert len(uniform) == len(deep), "equal-budget arms must be the same size"
    assert len(audit) == round(T2["auditFraction"] * n)
    assert not deep & audit, "audit is drawn from the low-priority remainder only"
    assert not holdout & (deep | audit | uniform), "holdout never enters a training-eligible arm"
    assert all(not r["trainEligible"] for r in recs if r["selection"]["holdout"])
    # every deep position outranks every audit position
    rank = {r["id"]: r["rank"] for r in recs}
    assert max(rank[i] for i in deep) < min(rank[i] for i in audit)


def test_holdout_membership_ignores_prioritizer_and_pool():
    items = items_for(800)
    heavy = dict(T2, weights={k: (10.0 if k == "rarity" else 0.1) for k in T2["weights"]})
    h1 = {r["id"] for r in f.select(items, T2, T4, seed=7) if r["selection"]["holdout"]}
    h2 = {r["id"] for r in f.select(items[:400], heavy, T4, seed=99) if r["selection"]["holdout"]}
    assert h2 == {i for i in h1 if i in {it["id"] for it in items[:400]}}


def test_stockfish_tags_follow_their_arms():
    recs = f.select(items_for(1000), T2, T4, seed=7)
    for r in recs:
        for tag in r["selection"]["stockfish"]:
            key = {"priority": "deep"}.get(tag, tag)
            assert r["selection"][key], (tag, r["selection"])
    n_pri = sum("priority" in r["selection"]["stockfish"] for r in recs)
    assert n_pri == round(T4["priorityFraction"] * 1000)
    # the priority SF sample is the top of the deep arm
    top = sorted((r for r in recs if r["selection"]["deep"]), key=lambda r: r["rank"])[:n_pri]
    assert all("priority" in r["selection"]["stockfish"] for r in top)


def test_rarity_prefers_underrepresented_motif():
    counts = Counter({"fork": 900, "interference": 3, "__positions__": 1000, "material:Q1R2m4P2-Q1R2m4P2": 1000})
    common = f.priority_components(t0(["fork"]), t1(), None, counts, T2)
    rare = f.priority_components(t0(["interference"]), t1(), None, counts, T2)
    assert rare["rarity"] == 1.0 and common["rarity"] < 0.1
    assert f.score_priority(rare, T2["weights"])[0] > f.score_priority(common, T2["weights"])[0]


def test_instability_components():
    counts = Counter({"__positions__": 10})
    stable = f.priority_components(t0(), t1(), None, counts, T2)
    unstable = f.priority_components(t0(), t1(0, 300, "e2e4", "d2d4", "unstable-trajectory"), None, counts, T2)
    assert stable["scoreInstability"] == 0 and stable["bestMoveChange"] == 0 and stable["trajectoryUnstable"] == 0
    assert unstable["scoreInstability"] == 1 and unstable["bestMoveChange"] == 1 and unstable["trajectoryUnstable"] == 1
    assert unstable["pvDisagreement"] == 1


def test_outcome_disagreement_uses_game_result_direction():
    counts = Counter({"__positions__": 10})
    agree = f.priority_components(t0(), t1(white=400), 1, counts, T2)
    clash = f.priority_components(t0(), t1(white=400), 0, counts, T2)
    assert agree["outcomeDisagreement"] == 0 and clash["outcomeDisagreement"] > 0.5


def test_summarize_facts_maps_to_taxonomy_and_sides():
    piece = lambda side, t, sq, attacked, loose: {"id": f"{side}-{t}-{sq}", "side": side, "pieceType": t,  # noqa: E731
                                                  "square": sq, "attacked": attacked, "loose": loose}
    b = bundle(
        availableMotifs=coll({"kind": "fork"}),
        availablePins=coll({"kind": "absolute"}),
        opponentAvailablePins=coll({"kind": "relative"}),
        availableDeflection=coll({"kind": "deflection"}),
        availableMatePatterns=coll({"kind": "back_rank"}),
        availableSkewers={"status": "uncomputed", "reason": "not_requested"},
        opponentAvailableMotifs=coll({"kind": "brand_new_motif"}),
        pieces=[piece("black", "knight", "c6", True, True), piece("white", "king", "e1", True, True),
                piece("white", "bishop", "c4", False, True)],
        pawnStructure={"doubled": [{"side": "black", "file": "f"}], "isolated": [{"side": "white"}], "passed": [],
                       "islands": [{"side": "white"}, {"side": "black"}, {"side": "black"}]},
        hazards=coll({"kind": "mate_threat", "side": "black"}, {"kind": "pin_constraint", "side": "white"}),
    )
    s = f.summarize_facts(b, FAMILIES)
    opp = s["bounded_tactical_proof"]["opportunities"]
    assert opp["stm"] == sorted(["fork", "absolute-pin", "distraction", "back-rank-mate", "hanging-piece", "mate-threat"])
    assert "relative-pin" in opp["opp"] and "unmapped:Motifs:brand_new_motif" in opp["opp"]
    assert s["taxonomy"]["unmapped"] == ["unmapped:Motifs:brand_new_motif"]
    assert "unmapped:Motifs:brand_new_motif" not in s["taxonomy"]["slugs"]
    assert "availableSkewers" in s["uncomputed"], "uncomputed stays distinct from computed-empty"
    geo = s["deterministic_geometry"]
    assert geo["structures"] == {"stm": ["isolated-pawn"], "opp": ["doubled-pawns"]}
    assert geo["pawns"]["islands"] == {"stm": 1, "opp": 2}
    assert s["bounded_tactical_proof"]["hanging"] == {"stm": 0, "opp": 1}, "kings are never 'hanging'"
    assert "multiple-attack" in s["taxonomy"]["families"] and "pin" in s["taxonomy"]["families"]


def test_static_input_leak_guard():
    rec = {"id": "x", "schemaVersion": 1, "stage": "tier0", "status": "ok", "deterministic_geometry": {},
           "bounded_tactical_proof": {}, "taxonomy": {}, "uncomputed": [], "factsErrors": 0,
           "factsRegistryVersion": 23, "cost": {}}
    f.assert_static_input_safe(rec)
    for leaked in ("search_derived", "external_oracle", "game_outcome", "priority"):
        try:
            f.assert_static_input_safe({**rec, leaked: {}})
        except ValueError:
            continue
        raise AssertionError(f"{leaked} leaked into a tier0 record")


def test_board_geometry_phase_and_bucket():
    start, first = f.board_geometry("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
    assert start["phase"] == "opening" and start["legalMoves"] == 20 and first == "a2a3"
    assert start["materialBucket"] == "Q1R2m4P2-Q1R2m4P2"
    kp, _ = f.board_geometry("8/5k2/8/8/8/8/4P3/4K3 w - - 0 1")
    assert kp["phase"] == "endgame" and kp["nonPawnMaterial"] == 0
    mated, first = f.board_geometry("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1")
    assert mated["legalMoves"] == 0 and first == ""


def test_parse_sf_output():
    text = "\n".join([
        "id name Stockfish", "uciok", "readyok",
        "info depth 10 seldepth 12 multipv 1 score cp 35 nodes 1000 nps 1 time 5 pv e2e4 e7e5",
        "info depth 11 seldepth 12 multipv 1 score cp 50 lowerbound nodes 1500 time 6 pv e2e4",
        "info depth 11 seldepth 14 multipv 1 score cp 41 nodes 2000 nps 1 time 9 pv d2d4 d7d5",
        "bestmove d2d4 ponder d7d5",
        "info depth 3 seldepth 3 multipv 1 score mate -2 nodes 40 time 1 pv h8g8",
        "bestmove h8g8",
        "bestmove (none)",
    ])
    out = f.parse_sf_output(text)
    assert len(out) == 3
    assert out[0] == {"depth": 11, "scoreCpStm": 41, "mate": None, "nodes": 2000, "timeMs": 9,
                      "pv": ["d2d4", "d7d5"], "bestMove": "d2d4"}
    assert out[1]["mate"] == -2 and out[1]["scoreCpStm"] == -f.CP_CLAMP
    assert out[2] is None


def test_position_id_ignores_move_counters():
    a = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
    b = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 7 30"
    assert f.position_id(a) == f.position_id(b)


if __name__ == "__main__":
    tests = [(n, fn) for n, fn in sorted(globals().items()) if n.startswith("test_") and callable(fn)]
    failed = 0
    for name, fn in tests:
        try:
            fn()
            print(f"ok   {name}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"FAIL {name}: {e!r}")
    print(f"{len(tests) - failed}/{len(tests)} passed")
    sys.exit(1 if failed else 0)
