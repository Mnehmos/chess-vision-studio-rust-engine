#!/usr/bin/env python3
"""Curation probe: what would the bot SAY, per move, and is it worth reading?

Feeds real engine moves through the full TeachingFactBundleV1 (played = our move), builds
the candidate claims a chat layer could make, ranks them, and renders one line per move.
Design questions this answers, on real output:

  * how often is there anything worth saying at all
  * which topics dominate, and how often the *rich* ones occur
  * whether the sentences read like a coach or like telemetry

Rendering rule learned the hard way: render the FACT with its own participants, never the
hazard summary. Hazards are lossy ("pin_constraint" carries the whole ray as `squares`,
which produced "pins a piece (d1, e2, f3)"), while the pin/skewer/fork facts carry
pinner/pinned/anchor, forkingPiece/targets and a validator-proven materialGain.

Ranking: proof class first (mate > validator-proven material > quantified position >
structure > hygiene), then magnitude, then novelty (created beats removed; nothing is
repeated within a run).

Run:  python benchmarks/scripts/chat_curate_probe.py --positions 30
"""
from __future__ import annotations

import argparse
import json
import subprocess
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FACTS_OPTS = {"includeMotifOpportunities": True, "includeCounterfactual": True}
PIECE_NAME = {"p": "pawn", "n": "knight", "b": "bishop", "r": "rook", "q": "queen", "k": "king"}

# motif families that carry a validator-proven materialGain, keyed by collection field
MATERIAL_GROUPS = (
    ("availableSkewers", "skewers"),
    ("availableTrapped", "traps"),
    ("availableDesperado", "desperado"),
    ("availableXrayAttack", "wins by x-ray"),
    ("availableDeflection", "deflects the defender"),
    ("availableLureDefender", "lures the defender away"),
    ("availableInterference", "cuts the defence"),
    ("availableDoubleAttack", "double-attacks"),
    ("availableWinExchange", "wins the exchange"),
    ("availableRemoveGuard", "removes the guard"),
    ("availableDiscoveredDefense", "unveils a defence"),
    ("availableOverload", "overloads the defender"),
)

# hygiene topics: they are true, but they read as the engine talking to itself. Only
# speak them when they were materially significant.
SUPPRESS_HYGIENE_BELOW_CP = 100

PRINTED: list[str] = []


def items(coll) -> list:
    return coll.get("items", []) if isinstance(coll, dict) and coll.get("status") == "computed" else []


def name(ref: dict, with_side: bool = False) -> str:
    pt = PIECE_NAME.get(str(ref.get("pieceType", ""))[:1], ref.get("pieceType", "?"))
    sq = ref.get("square")
    return f"{pt} on {sq}" if not with_side else f"{ref.get('side')} {pt} on {sq}"


def collect(before: dict, played: dict, mv: str, min_capture: int) -> list[tuple]:
    """(tier, score, sentence, dedupe_key)"""
    out: list[tuple] = []
    deltas = played.get("deltas", {}) or {}
    stm = before.get("sideToMove")

    # ---- T1: mate ----------------------------------------------------------------
    for h in items(before.get("hazards")):
        if h.get("kind") == "mate_threat" and h.get("moveUci") == mv:
            sq = ", ".join(h.get("squares", [])[:2])
            out.append((1, 900 + int(h.get("magnitudeCp") or 0),
                        f"{mv} creates a mate threat ({sq}).", f"mate|{sq}"))
    for m in items(before.get("availableMatePatterns")):
        if m.get("moveUci") == mv:
            kind = str(m.get("kind", "mate")).replace("_", " ")
            out.append((1, 950, f"{mv} forces {kind}.", f"mp|{kind}"))
    for h in items(deltas.get("createdHazards", {})):
        if h.get("kind") == "mate_threat":
            sq = ", ".join(h.get("squares", [])[:2])
            out.append((1, 880, f"{mv} creates a mate threat ({sq}).", f"mate|{sq}"))

    # ---- T2: validator-proven material motifs -----------------------------------
    for m in items(before.get("availableMotifs")):
        if m.get("moveUci") != mv:
            continue
        gain = int(m.get("materialGain") or 0)
        tg = " and ".join(name(t) for t in (m.get("targets") or [])[:2])
        chk = " with check" if m.get("givesCheck") else ""
        tail = f" — wins about {gain/100:.1f}" if gain else ""
        out.append((2, 800 + gain, f"{mv} forks {tg}{chk}{tail}.",
                    f"fork|{tg}"))
    for pin in items(before.get("availablePins")):
        if pin.get("moveUci") != mv:
            continue
        pinned, anchor = pin.get("pinned", {}), pin.get("anchor", {})
        if pin.get("kind") == "absolute" or str(anchor.get("pieceType")) == "K":
            text = f"{mv} pins the {name(pinned)} to the king"
        else:
            text = f"{mv} pins the {name(pinned)} to the {name(anchor)}"
        if pin.get("pinnedImmobile"):
            text += " — it cannot move"
        out.append((2, 780, text + ".", f"pin|{pinned.get('square')}"))
    for field, label in MATERIAL_GROUPS:
        for m in items(before.get(field)):
            if m.get("moveUci") != mv:
                continue
            gain = int(m.get("materialGain") or m.get("materialGainCp") or 0)
            who = m.get("pinned") or m.get("victim") or m.get("trapped") or m.get("defender") or {}
            detail = f" ({name(who)})" if isinstance(who, dict) and who.get("square") else ""
            tail = f" — wins about {gain/100:.1f}" if gain else ""
            out.append((2, 700 + gain, f"{mv} {label}{detail}{tail}.",
                        f"{field}|{who.get('square') if isinstance(who, dict) else ''}"))
    for c in items(before.get("availableCaptures")):
        if c.get("moveUci") == mv and int(c.get("seeCp") or 0) >= min_capture:
            see = int(c["seeCp"])
            out.append((2, 600 + see, f"{mv} wins the {name(c.get('victim'))} — SEE {see/100:+.1f}.",
                        f"cap|{c.get('victimSquare')}"))

    # ---- T3: quantified position ------------------------------------------------
    for ks in items(before.get("kingSafety")):
        if ks.get("side") != stm:
            continue
        atk = len(ks.get("attackers") or [])
        esc = ks.get("legalEscapeSquares")
        nesc = len(esc.get("items", [])) if isinstance(esc, dict) and esc.get("status") == "computed" else None
        if atk >= 2 or (nesc is not None and nesc <= 2):
            sq = ks.get("kingSquare")
            e = "no escape squares" if nesc == 0 else f"{nesc} escape square(s)"
            out.append((3, 480 + 40 * atk, f"{mv} keeps the king under pressure ({sq}): {atk} attacker(s), {e}.",
                        f"king|{sq}|{atk}|{nesc}"))
    for h in items(deltas.get("createdHazards", {})):
        kind, mag = h.get("kind"), int(h.get("magnitudeCp") or 0)
        sq = ", ".join(h.get("squares", [])[:2])
        if kind == "king_pressure":
            out.append((3, 450, f"{mv} adds pressure around the king ({sq}).", f"kp|{sq}"))
        elif kind == "fork_threat":
            out.append((3, 440, f"{mv} sets up a fork ({sq}).", f"ft|{sq}"))
        elif kind == "losing_material" and mag >= SUPPRESS_HYGIENE_BELOW_CP:
            out.append((3, 430 + mag, f"{mv} threatens to win material ({sq}).", f"lm|{sq}"))
        # pin_constraint deliberately omitted: the pin facts above say it with participants

    # ---- T4: structure ----------------------------------------------------------
    for sd in items(deltas.get("createdStructures", {})):
        kind = str(sd.get("kind", "structure")).replace("_", " ")
        sq = ", ".join(sd.get("squares", [])[:2])
        out.append((4, 300, f"{mv} creates a {kind}{(' on ' + sq) if sq else ''}.", f"st|{kind}|{sq}"))

    # ---- T5: hazards answered (hygiene, gated on magnitude) ----------------------
    for h in items(deltas.get("removedHazards", {})):
        kind, mag = h.get("kind"), int(h.get("magnitudeCp") or 0)
        sq = ", ".join(h.get("squares", [])[:2])
        if kind == "mate_threat" or (kind == "losing_material" and mag >= SUPPRESS_HYGIENE_BELOW_CP):
            what = "the mate threat" if kind == "mate_threat" else "material"
            out.append((5, 250 + mag, f"{mv} answers {what} ({sq}).", f"rm|{kind}|{sq}"))
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=str(REPO / "target-sf3/release/analyze.exe"))
    ap.add_argument("--book", default=str(REPO / "benchmarks/suites/openings-inv1-20260910.epd"))
    ap.add_argument("--positions", type=int, default=30)
    ap.add_argument("--stride", type=int, default=173)
    ap.add_argument("--nodes", type=int, default=40000)
    ap.add_argument("--min-capture-cp", type=int, default=100)
    a = ap.parse_args(argv)

    lines = [l.strip() for l in Path(a.book).read_text().splitlines() if l.strip()]
    fens = [" ".join(l.split()[:4]) for l in lines][:: a.stride][: a.positions]
    p = subprocess.Popen([a.exe, "--serve", "--depth", "12"], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)

    def ask(req):
        p.stdin.write(json.dumps(req) + "\n")
        p.stdin.flush()
        while True:
            line = p.stdout.readline()
            if not line:
                return None
            if line.startswith("{"):
                return json.loads(line)

    tiers, spoken_keys, examples, silent = Counter(), set(), [], 0
    for fen in fens:
        go = ask({"cmd": "go", "fen": fen, "nodeBudget": a.nodes})
        mv = (go or {}).get("uci")
        if not mv:
            continue
        f = ask({"cmd": "facts", "schemaVersion": 1, "fenBefore": fen, "playedMoveUci": mv,
                 "options": FACTS_OPTS})
        if not f or f.get("errors"):
            tiers["no_bundle"] += 1
            continue
        cands = [c for c in collect(f["before"], f.get("played", {}), mv, a.min_capture_cp)
                 if c[3] not in spoken_keys]
        if not cands:
            tiers["silent"] += 1
            silent += 1
            continue
        cands.sort(key=lambda c: (-c[0], -c[1]))
        tier, score, text, key = cands[0]
        spoken_keys.add(key)
        tiers[f"T{tier}"] += 1
        if len(examples) < 16:
            examples.append((tier, len(cands), text))
    p.stdin.write("quit\n"); p.stdin.flush(); p.kill()

    n = sum(tiers.values())
    print(f"\n{n} engine moves, full fact bundle each\n")
    for t in ("T1", "T2", "T3", "T4", "T5", "silent", "no_bundle"):
        if tiers[t]:
            print(f"  {t:>9}  {tiers[t]:4d}  {100*tiers[t]/max(n,1):5.0f}%")
    print(f"\n  lines emitted: {n - silent - tiers['no_bundle']}/{n}  "
          f"({100*(n - silent - tiers['no_bundle'])/max(n,1):.0f}% of moves)")
    print("\nexamples (top-ranked candidate per position):")
    for tier, ncand, text in examples:
        print(f"  [T{tier}, {ncand:2d} cands] {text}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
