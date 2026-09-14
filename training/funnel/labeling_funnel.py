#!/usr/bin/env python3
"""Information-gain labeling funnel (#111).

Cheap computation decides where expensive computation is spent:

  sample  deterministic, order-independent sample of unique positions from corpus shards
  tier0   deterministic board facts (legal-move counts + TeachingFactBundle `before` block,
          summarised and mapped onto motif-taxonomy slugs/families) -- no search
  tier1   shallow cold fixed-node CVS searches at 2+ budgets (score/move/PV stability,
          stabilization verdict, selectivity telemetry) -- a data-generation profile
  triage  versioned, decomposable priority score (priority-v1) + deep/audit/uniform/holdout
          selection; writes coverage counts usable as the next run's prior
  tier3   deep cold fixed-node CVS labels for the selected subsets only
  tier4   sparse Stockfish oracle audit (priority / uniform / low-priority audit / holdout)
  report  cost per tier, coverage, spend by family/phase/priority bucket, audit miss rate,
          and the equal-budget priority-vs-uniform comparison

Every record keeps its provenance class as a top-level key (deterministic_geometry,
bounded_tactical_proof, search_derived, game_outcome, external_oracle) so training code can
include/exclude authorities separately. Only Tier-0 records may feed static model inputs.

Run everything:
  python training/funnel/labeling_funnel.py run --config training/funnel/funnel-config.v1.json \\
      --run-dir training/funnel/runs/<name> [--positions N] [--workers N] [--artifact-root DIR]
Or one stage at a time with the same --run-dir (stages resume: finished ids are skipped).
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import heapq
import json
import math
import multiprocessing as mp
import os
import platform
import statistics
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TAXONOMY = REPO / "benchmarks" / "data" / "motif-taxonomy.json"

FUNNEL_SCHEMA_VERSION = 1
PRIORITIZER_VERSION = "priority-v1"
CP_CLAMP = 2000  # mate scores and runaway evals are clamped here before any delta is taken
STATIC_INPUT_CLASSES = ("deterministic_geometry", "bounded_tactical_proof")
TIER0_KEYS = {"id", "schemaVersion", "stage", "status", "deterministic_geometry",
              "bounded_tactical_proof", "taxonomy", "uncomputed", "factsErrors",
              "factsRegistryVersion", "cost"}

STATUS_INSTABILITY = {
    "stable-at-budget": 0.0,
    "exact-tablebase": 0.0,
    "verified-forced-mate": 0.0,
    "unresolved-at-budget": 0.75,
    "omission-risk": 0.75,
    "verifier-conflict": 1.0,
    "unstable-trajectory": 1.0,
}

# ---------------------------------------------------------------------------------------------
# small helpers


def stable_hash(*parts) -> int:
    return int(hashlib.sha256(":".join(str(p) for p in parts).encode()).hexdigest()[:16], 16)


def unit_hash(*parts) -> float:
    return stable_hash(*parts) / float(1 << 64)


def position_id(fen: str) -> str:
    return hashlib.sha1(" ".join(fen.split()[:4]).encode()).hexdigest()[:16]


def clamp_cp(v) -> int:
    if v is None:
        return 0
    return max(-CP_CLAMP, min(CP_CLAMP, int(v)))


def sha256_file(path) -> str | None:
    try:
        h = hashlib.sha256()
        with open(path, "rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
        return h.hexdigest()
    except OSError:
        return None


def read_jsonl(path) -> list[dict]:
    rows = []
    if not Path(path).exists():
        return rows
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # a torn last line from a killed run; the id is simply redone
    return rows


def by_id(path) -> dict[str, dict]:
    return {r["id"]: r for r in read_jsonl(path) if "id" in r}


def dumps(obj) -> str:
    return json.dumps(obj, separators=(",", ":"), sort_keys=True)


def white_pov(fen: str, stm_cp) -> int | None:
    if stm_cp is None:
        return None
    return stm_cp if fen.split()[1] == "w" else -stm_cp


def expected_score(cp) -> float:
    """logistic-400: stm expected score from a centipawn value (documented in the manifest)."""
    return 1.0 / (1.0 + 10 ** (-clamp_cp(cp) / 400.0))


def below_normal() -> int:
    return getattr(subprocess, "BELOW_NORMAL_PRIORITY_CLASS", 0) if os.name == "nt" else 0


# ---------------------------------------------------------------------------------------------
# taxonomy mapping (Tier 0)

PIN_KIND = {"absolute": "absolute-pin", "relative": "relative-pin"}
KIND_FIRST = {"Motifs", "Discoveries", "MatePatterns"}
COLLECTION_DEFAULT = {
    "Skewers": "skewer",
    "Discoveries": "discovered-attack",
    "DiscoveredDefense": "discovered-defense",
    "RemoveGuard": "capturing-defender",
    "Trapped": "trapped-piece",
    "Desperado": "desperado",
    "Overload": "overloading",
    "AttackDefender": "attacking-the-defender",
    "Deflection": "distraction",
    "LureDefender": "luring-the-defender",
    "Interference": "interference",
    "DoubleAttack": "double-attack",
    "XrayAttack": "xray-attack",
    "XrayDefense": "xray-defense",
    "WinExchange": "win-the-exchange",
}
PAWN_SLUG = {"doubled": "doubled-pawns", "isolated": "isolated-pawn", "passed": "passed-pawn"}


def load_taxonomy(path=TAXONOMY) -> dict:
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    return {
        "schemaVersion": data.get("schemaVersion"),
        "sha256": sha256_file(path),
        "family": {m["slug"]: m.get("family") for m in data["motifs"]},
    }


def resolve_slug(collection: str, kind: str | None, slugs) -> str:
    if collection == "Pins" and kind in PIN_KIND:
        return PIN_KIND[kind]
    default = COLLECTION_DEFAULT.get(collection)
    candidates = []
    if kind:
        k = kind.replace("_", "-")
        candidates = [k, k + "-mate"]
    order = candidates + [default] if collection in KIND_FIRST else [default] + candidates
    for cand in order:
        if cand and cand in slugs:
            return cand
    return f"unmapped:{collection}:{kind}"


def _items(coll):
    if isinstance(coll, dict) and coll.get("status") == "computed":
        return coll.get("items") or []
    return None


def summarize_facts(bundle: dict, families: dict) -> dict:
    """TeachingFactBundleV1 `before` block -> compact Tier-0 fields. Opportunity sides are
    who can EXECUTE ("stm"/"opp"); structure sides are who OWNS the structure."""
    before = bundle["before"]
    stm = before["sideToMove"]
    owner = lambda side: "stm" if side == stm else "opp"  # noqa: E731
    other = {"stm": "opp", "opp": "stm"}

    uncomputed: list[str] = []
    opportunities = {"stm": set(), "opp": set()}
    structures = {"stm": set(), "opp": set()}
    kind_counts: Counter = Counter()
    captures = {"stm": {"count": 0, "seePositive": 0}, "opp": {"count": 0, "seePositive": 0}}

    for key, coll in before.items():
        if key.startswith("opponentAvailable"):
            side, name = "opp", key[len("opponentAvailable"):]
        elif key.startswith("available"):
            side, name = "stm", key[len("available"):]
        else:
            continue
        items = _items(coll)
        if items is None:
            uncomputed.append(key)
            continue
        if name == "Captures":
            captures[side]["count"] += len(items)
            captures[side]["seePositive"] += sum(1 for it in items if (it.get("seeCp") or 0) > 0)
            continue
        for it in items:
            kind = it.get("kind") if isinstance(it, dict) else None
            kind_counts[f"{side}:{name}:{kind}"] += 1
            opportunities[side].add(resolve_slug(name, kind, families))

    pieces = {"stm": 0, "opp": 0}
    attacked = {"stm": 0, "opp": 0}
    loose = {"stm": 0, "opp": 0}
    hanging = {"stm": 0, "opp": 0}
    for pc in before.get("pieces") or []:
        o = owner(pc.get("side"))
        pieces[o] += 1
        attacked[o] += bool(pc.get("attacked"))
        loose[o] += bool(pc.get("loose"))
        if pc.get("pieceType") != "king" and pc.get("attacked") and pc.get("loose"):
            hanging[o] += 1
            opportunities[other[o]].add("hanging-piece")

    pawns: dict = {}
    ps = before.get("pawnStructure") or {}
    for key, slug in PAWN_SLUG.items():
        val = ps.get(key)
        if isinstance(val, list):
            counts = {"stm": 0, "opp": 0}
            for it in val:
                if isinstance(it, dict) and it.get("side"):
                    counts[owner(it["side"])] += 1
                    structures[owner(it["side"])].add(slug)
            pawns[key] = counts
        else:
            uncomputed.append(f"pawnStructure.{key}")
    islands = ps.get("islands")
    if isinstance(islands, list):
        pawns["islands"] = {s: sum(1 for it in islands if owner(it.get("side")) == s)
                            for s in ("stm", "opp")}

    king = {}
    for it in _items(before.get("kingSafety")) or []:
        esc = _items(it.get("legalEscapeSquares"))
        king[owner(it.get("side"))] = {
            "inCheck": bool(it.get("inCheck")),
            "attackers": len(it.get("attackers") or []),
            "pressuredSquares": len(it.get("pressuredSquares") or []),
            "escapeSquares": None if esc is None else len(esc),
        }

    control = {"stm": 0, "opp": 0}
    squares = _items(before.get("squareFacts"))
    if squares is None:
        uncomputed.append("squareFacts")
    else:
        white = "stm" if stm == "white" else "opp"
        black = other[white]
        for sq in squares:
            control[white] += bool(sq.get("controlledByWhite"))
            control[black] += bool(sq.get("controlledByBlack"))

    hazards: Counter = Counter()
    hz = _items(before.get("hazards"))
    if hz is None:
        uncomputed.append("hazards")
    else:
        for it in hz:
            hazards[it.get("kind")] += 1
            if it.get("kind") == "mate_threat" and it.get("side"):
                # hazard `side` is the threatened side; the beneficiary can execute it
                opportunities[other[owner(it["side"])]].add("mate-threat")

    all_slugs = sorted(opportunities["stm"] | opportunities["opp"]
                       | structures["stm"] | structures["opp"])
    return {
        "deterministic_geometry": {
            "pieces": pieces, "attacked": attacked, "loose": loose,
            "pawns": pawns, "kingSafety": king, "squareControl": control,
            "structures": {s: sorted(v) for s, v in structures.items()},
        },
        "bounded_tactical_proof": {
            "opportunities": {s: sorted(v) for s, v in opportunities.items()},
            "kindCounts": dict(sorted(kind_counts.items())),
            "captures": captures,
            "hanging": hanging,
            "hazards": dict(sorted(hazards.items())),
            "tacticalItems": sum(kind_counts.values()) + sum(hazards.values()),
        },
        "taxonomy": {
            "slugs": [s for s in all_slugs if not s.startswith("unmapped:")],
            "families": sorted({families[s] for s in all_slugs if s in families and families[s]}),
            "unmapped": [s for s in all_slugs if s.startswith("unmapped:")],
        },
        "uncomputed": sorted(uncomputed),
        "factsErrors": len(bundle.get("errors") or []),
        "factsRegistryVersion": (bundle.get("provenance") or {}).get("factsRegistryVersion"),
    }


def board_geometry(fen: str) -> tuple[dict, str]:
    """python-chess legal-move structure, material phase and material bucket."""
    import chess

    b = chess.Board(fen)
    legal = list(b.legal_moves)
    counts = {}
    for color, name in ((chess.WHITE, "w"), (chess.BLACK, "b")):
        counts[name] = {p: len(b.pieces(t, color)) for p, t in
                        (("Q", chess.QUEEN), ("R", chess.ROOK), ("B", chess.BISHOP),
                         ("N", chess.KNIGHT), ("P", chess.PAWN))}
    npm = sum(c["N"] + c["B"] + 2 * c["R"] + 4 * c["Q"] for c in counts.values())
    phase = "opening" if npm >= 22 else "endgame" if npm <= 8 else "middlegame"

    def sig(c):
        return f"Q{c['Q']}R{c['R']}m{c['B'] + c['N']}P{min(c['P'], 8) // 3}"

    bucket = f"{sig(counts['w'])}-{sig(counts['b'])}"
    return {
        "sideToMove": "white" if b.turn else "black",
        "legalMoves": len(legal),
        "legalCaptures": sum(1 for m in legal if b.is_capture(m)),
        "legalChecks": sum(1 for m in legal if b.gives_check(m)),
        "inCheck": b.is_check(),
        "nonPawnMaterial": npm,
        "phase": phase,
        "materialBucket": bucket,
    }, (min(m.uci() for m in legal) if legal else "")


def assert_static_input_safe(tier0_record: dict) -> None:
    """Leakage guard: a Tier-0 record may only carry deterministic/bounded-proof fields."""
    extra = set(tier0_record) - TIER0_KEYS
    if extra:
        raise ValueError(f"tier0 record carries non-static fields: {sorted(extra)}")


# ---------------------------------------------------------------------------------------------
# engine processes


class Serve:
    """One persistent `analyze --serve`; one JSON request -> one JSON line."""

    def __init__(self, cmd: list[str]):
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, encoding="utf-8",
                                  bufsize=1, creationflags=below_normal())

    def request(self, obj: dict) -> dict:
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError("analyze --serve exited")
        return json.loads(line)


_W: dict = {}


def _init_worker(serve_cmd, families):
    _W["serve"] = Serve(serve_cmd) if serve_cmd else None
    _W["families"] = families


def compact_search(resp: dict, pv_plies: int) -> dict:
    st = resp.get("stabilization") or {}
    tel = resp.get("telemetry") or {}
    return {
        "nodes": resp.get("nodes"),
        "depth": resp.get("depth"),
        "scoreCpStm": resp.get("scoreCp"),
        "mate": resp.get("mate"),
        "bestMove": resp.get("uci"),
        "pv": (resp.get("pv") or [])[:pv_plies],
        "termination": resp.get("termination"),
        "resultSource": resp.get("resultSource"),
        "stabilization": {k: st.get(k) for k in (
            "status", "bestMoveChanges", "maxAdjacentSwingCp", "scoreRangeCp", "signFlips",
            "oddEvenOscillation", "mateFlip", "seePruneSkips", "reasons")},
        "trajectory": [[it.get("depth"), it.get("uci"), it.get("scoreCp")]
                       for it in resp.get("iterations") or []],
        "avgCutoffMoveIndex": tel.get("avgCutoffMoveIndex"),
        "avgLegalMoves": tel.get("avgLegalMoves"),
        "qNodes": resp.get("qNodes"),
    }


def common_prefix(a: list, b: list) -> int:
    n = 0
    for x, y in zip(a, b):
        if x != y:
            break
        n += 1
    return n


def _tier0_job(job):
    pos, include_motifs = job
    t0 = time.perf_counter()
    try:
        geo, first_move = board_geometry(pos["fen"])
    except ValueError as e:
        return {"id": pos["id"], "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier0",
                "status": f"invalid-fen: {e}"}
    rec = {"id": pos["id"], "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier0"}
    if not first_move:
        rec.update(status="no-legal-moves", deterministic_geometry=geo)
        return rec
    # Facts need a move; the position-level `before` block is independent of which one, so a
    # fixed, search-free choice (lexicographically first legal move) keeps Tier 0 search-free.
    bundle = _W["serve"].request({
        "cmd": "facts", "schemaVersion": 1, "fenBefore": pos["fen"],
        "playedMoveUci": first_move,
        "options": {"includeMotifOpportunities": include_motifs, "includeCounterfactual": False},
    })
    if "error" in bundle:
        rec.update(status=f"facts-error: {bundle['error']}", deterministic_geometry=geo)
        return rec
    summary = summarize_facts(bundle, _W["families"])
    summary["deterministic_geometry"] = {**geo, **summary["deterministic_geometry"]}
    rec.update(status="ok", **summary)
    rec["cost"] = {"wallMs": round((time.perf_counter() - t0) * 1000, 3)}
    assert_static_input_safe(rec)
    return rec


def _tier1_job(job):
    pos, budgets, pv_plies = job
    t0 = time.perf_counter()
    runs = []
    for nb in budgets:
        resp = _W["serve"].request({"cmd": "go", "fen": pos["fen"], "nodeBudget": nb})
        c = compact_search(resp, pv_plies)
        c["nodeBudget"] = nb
        runs.append(c)
    lo, hi = runs[0], runs[-1]
    return {
        "id": pos["id"], "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier1",
        "search_derived": {
            "profile": {"nodeBudgets": budgets, "isolation": "cold"},
            "budgets": runs,
            "derived": {
                "scoreDeltaCp": clamp_cp(hi["scoreCpStm"]) - clamp_cp(lo["scoreCpStm"]),
                "bestMoveChanged": lo["bestMove"] != hi["bestMove"],
                "pvCommonPrefix": common_prefix(lo["pv"], hi["pv"]),
                "scoreCpWhite": white_pov(pos["fen"], hi["scoreCpStm"]),
            },
        },
        "cost": {"wallMs": round((time.perf_counter() - t0) * 1000, 3),
                 "nodes": sum(r["nodes"] or 0 for r in runs)},
    }


def _tier3_job(job):
    pos, shallow, node_budget, pv_plies = job
    t0 = time.perf_counter()
    resp = _W["serve"].request({"cmd": "go", "fen": pos["fen"], "nodeBudget": node_budget})
    deep = compact_search(resp, pv_plies)
    return {
        "id": pos["id"], "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier3",
        "search_derived": {
            "profile": {"nodeBudget": node_budget, "isolation": "cold"},
            "deep": deep,
            "shallowToDeep": {
                "shallowNodeBudget": shallow["nodeBudget"],
                "scoreDeltaCp": clamp_cp(deep["scoreCpStm"]) - clamp_cp(shallow["scoreCpStm"]),
                "bestMoveChanged": deep["bestMove"] != shallow["bestMove"],
            },
            "targets": {
                "scoreCpStm": deep["scoreCpStm"],
                "scoreCpWhite": white_pov(pos["fen"], deep["scoreCpStm"]),
                "expectedScoreStm": round(expected_score(deep["scoreCpStm"]), 6),
            },
        },
        "cost": {"wallMs": round((time.perf_counter() - t0) * 1000, 3), "nodes": deep["nodes"]},
    }


def parse_sf_output(text: str) -> list[dict | None]:
    """File-batch Stockfish output -> one result per `bestmove` (None if no score line)."""
    out: list[dict | None] = []
    cur: dict | None = None
    for line in text.splitlines():
        if line.startswith("bestmove"):
            parts = line.split()
            if cur is not None:
                cur["bestMove"] = parts[1] if len(parts) > 1 and parts[1] != "(none)" else None
            out.append(cur)
            cur = None
            continue
        if not line.startswith("info") or " score " not in line:
            continue
        p = line.split()
        try:
            rec = {"depth": int(p[p.index("depth") + 1])}
            i = p.index("score")
            if p[i + 1] == "cp":
                rec["scoreCpStm"], rec["mate"] = int(p[i + 2]), None
            else:
                m = int(p[i + 2])
                rec["mate"] = m
                rec["scoreCpStm"] = CP_CLAMP if m > 0 else -CP_CLAMP
            if "upperbound" in p or "lowerbound" in p:
                continue  # aspiration bound, not a score
            for key, name in (("nodes", "nodes"), ("time", "timeMs")):
                if key in p:
                    rec[name] = int(p[p.index(key) + 1])
            rec["pv"] = p[p.index("pv") + 1:] if "pv" in p else []
            cur = rec
        except (ValueError, IndexError):
            continue
    return out


def _tier4_job(job):
    chunk, sf_bin, hash_mb, depth, movetime, pv_plies, tmp_dir, tag = job
    cmd_path = Path(tmp_dir) / f"sf-{tag}.in"
    out_path = Path(tmp_dir) / f"sf-{tag}.out"
    with open(cmd_path, "w", encoding="utf-8") as f:
        f.write(f"uci\nsetoption name Threads value 1\nsetoption name Hash value {hash_mb}\nisready\n")
        for pos in chunk:
            f.write(f"ucinewgame\nisready\nposition fen {pos['fen']}\ngo depth {depth} movetime {movetime}\n")
        f.write("quit\n")
    with open(cmd_path, "rb") as inp, open(out_path, "w", encoding="utf-8") as out:
        subprocess.run([sf_bin], stdin=inp, stdout=out, stderr=subprocess.DEVNULL,
                       timeout=max(600, len(chunk) * (movetime / 1000 + 5)),
                       creationflags=below_normal())
    results = parse_sf_output(out_path.read_text(encoding="utf-8"))
    cmd_path.unlink(missing_ok=True)
    out_path.unlink(missing_ok=True)
    recs = []
    for pos, res in zip(chunk, results):
        if res is None:
            continue
        recs.append({
            "id": pos["id"], "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier4",
            "external_oracle": {
                "engine": "stockfish", "depthLimit": depth, "movetimeMs": movetime,
                "reachedDepth": res["depth"], "scoreCpStm": res["scoreCpStm"], "mate": res["mate"],
                "scoreCpWhite": white_pov(pos["fen"], res["scoreCpStm"]),
                "bestMove": res.get("bestMove"), "pv": res.get("pv", [])[:pv_plies],
            },
            "cost": {"wallMs": res.get("timeMs"), "nodes": res.get("nodes")},
        })
    return recs


def run_pool(jobs, fn, workers, out_path, serve_cmd, families, label):
    """Fan jobs out to worker processes, append results as they finish (resumable)."""
    if not jobs:
        print(f"{label}: nothing to do", flush=True)
        return 0
    t0 = time.time()
    done = 0
    workers = max(1, min(workers, len(jobs)))
    pool = mp.Pool(workers, initializer=_init_worker, initargs=(serve_cmd, families))
    try:
        with open(out_path, "a", encoding="utf-8") as out:
            for res in pool.imap_unordered(fn, jobs, chunksize=4):
                for rec in res if isinstance(res, list) else [res]:
                    out.write(dumps(rec) + "\n")
                done += 1
                if done % 500 == 0 or done == len(jobs):
                    out.flush()
                    rate = done / max(1e-9, time.time() - t0)
                    print(f"  {label}: {done}/{len(jobs)} ({rate:.1f}/s, "
                          f"ETA {(len(jobs) - done) / max(rate, 1e-9) / 60:.1f} min)", flush=True)
        pool.close()
        pool.join()
    except BaseException:
        pool.terminate()
        raise
    return done


# ---------------------------------------------------------------------------------------------
# run directory / manifest


class Run:
    def __init__(self, run_dir, artifact_root=None):
        self.dir = Path(run_dir)
        self.dir.mkdir(parents=True, exist_ok=True)
        self.cfg_path = self.dir / "funnel-config.json"
        self.root = Path(artifact_root) if artifact_root else REPO

    def path(self, name):
        return self.dir / name

    def cfg(self) -> dict:
        return json.loads(self.cfg_path.read_text(encoding="utf-8"))

    def resolve(self, p) -> Path:
        q = Path(p)
        return q if q.is_absolute() else self.root / q

    def serve_cmd(self) -> list[str]:
        eng = self.cfg()["engine"]
        args = []
        for a in eng["args"]:
            args.append(str(self.resolve(a)) if not a.startswith("-") and ("/" in a or "\\" in a) else a)
        return [str(self.resolve(eng["analyze"])), "--serve", *args]

    def manifest(self) -> dict:
        p = self.path("manifest.json")
        return json.loads(p.read_text(encoding="utf-8")) if p.exists() else {}

    def record_stage(self, stage, info):
        m = self.manifest()
        m.setdefault("stages", {})[stage] = {**info, "finishedAt": time.strftime("%Y-%m-%dT%H:%M:%S")}
        self.path("manifest.json").write_text(json.dumps(m, indent=2), encoding="utf-8")


def git_state(root: Path) -> dict:
    def git(*a):
        try:
            return subprocess.run(["git", "-C", str(root), *a], capture_output=True, text=True,
                                  timeout=30).stdout.strip()
        except (OSError, subprocess.SubprocessError):
            return None
    return {"commit": git("rev-parse", "HEAD"),
            "dirty": bool(git("status", "--porcelain", "--untracked-files=no"))}


def write_identity(run: Run, overrides: dict):
    cfg = run.cfg()
    serve = Serve(run.serve_cmd())
    try:
        identity = serve.request({"cmd": "identity"})
    finally:
        serve.p.kill()
    tax = load_taxonomy()
    m = run.manifest()
    m.update({
        "funnelSchemaVersion": FUNNEL_SCHEMA_VERSION,
        "funnelConfigVersion": cfg.get("funnelConfigVersion"),
        "prioritizerVersion": PRIORITIZER_VERSION,
        "configSha256": sha256_file(run.cfg_path),
        "overrides": overrides,
        "createdAt": m.get("createdAt") or time.strftime("%Y-%m-%dT%H:%M:%S"),
        "toolingGit": git_state(REPO),
        "artifactRoot": str(run.root),
        "engine": {
            "serveCommand": run.serve_cmd(),
            "binarySha256": sha256_file(run.resolve(cfg["engine"]["analyze"])),
            "artifactSha256": {a: sha256_file(run.resolve(a)) for a in cfg["engine"].get("hashArtifacts", [])},
            "identity": identity,
            "artifactRootGit": git_state(run.root),
        },
        "stockfish": {"binary": cfg["stockfish"]["binary"],
                      "binarySha256": sha256_file(cfg["stockfish"]["binary"])},
        "taxonomy": {"path": str(TAXONOMY.relative_to(REPO)), "schemaVersion": tax["schemaVersion"],
                     "sha256": tax["sha256"]},
        "provenanceClasses": {
            "deterministic_geometry": "tier0 (static-input eligible)",
            "bounded_tactical_proof": "tier0 (static-input candidate, validator-backed)",
            "search_derived": "tier1, tier3 (targets/triage only; never static input)",
            "game_outcome": "positions.gameOutcome (targets/triage only)",
            "external_oracle": "tier4 (audit/targets only)",
        },
        "targets": {"expectedScoreStm": "logistic-400: 1/(1+10^(-cp/400)), cp clamped to +/-%d" % CP_CLAMP},
        "python": platform.python_version(),
        "host": platform.platform(),
    })
    run.path("manifest.json").write_text(json.dumps(m, indent=2), encoding="utf-8")


# ---------------------------------------------------------------------------------------------
# stages


def stage_sample(run: Run):
    import chess

    cfg = run.cfg()
    src, seed = cfg["source"], cfg["seed"]
    want = int(src["positions"])
    files = sorted({f for pat in src["shards"] for f in glob.glob(str(run.resolve(pat)))})
    if not files:
        raise SystemExit(f"no source shards match {src['shards']}")
    keep = int(want * 1.02) + 16  # headroom for rows python-chess rejects
    heap: list = []  # max-heap on key via negation: keeps the `keep` smallest hash keys
    seen: set[str] = set()
    scanned = 0
    for f in files:
        with open(f, encoding="utf-8") as fd:
            for n, line in enumerate(fd, 1):
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                fen = row.get("fen")
                if not fen:
                    continue
                scanned += 1
                pid = position_id(fen)
                if pid in seen:
                    continue
                seen.add(pid)
                key = stable_hash(seed, "sample", pid)
                item = (-key, pid, fen, str(Path(f).relative_to(run.root)) if Path(f).is_relative_to(run.root) else f,
                        n, row.get(src.get("outcomeField", "res")))
                if len(heap) < keep:
                    heapq.heappush(heap, item)
                elif -heap[0][0] > key:
                    heapq.heapreplace(heap, item)
    chosen = sorted(heap, key=lambda it: -it[0])
    rows = []
    for _, pid, fen, fname, line_no, res in chosen:
        try:
            full = chess.Board(fen).fen()
        except ValueError:
            continue
        rows.append({
            "id": pid, "fen": full, "source": {"file": fname, "line": line_no},
            "gameOutcome": ({"provenance": "game_outcome", "whiteScore": res}
                            if res in (0, 0.5, 1, 0.0, 1.0) else None),
        })
        if len(rows) >= want:
            break
    with open(run.path("positions.jsonl"), "w", encoding="utf-8") as out:
        for r in rows:
            out.write(dumps(r) + "\n")
    run.record_stage("sample", {"files": len(files), "rowsScanned": scanned,
                                "uniquePositions": len(seen), "sampled": len(rows)})
    print(f"sample: {len(rows)} positions from {len(seen)} unique ({scanned} rows, {len(files)} files)")


def _pending(run, stage_file, ids=None):
    positions = read_jsonl(run.path("positions.jsonl"))
    done = set(by_id(run.path(stage_file)))
    return [p for p in positions if p["id"] not in done and (ids is None or p["id"] in ids)]


def stage_tier0(run: Run, workers=None):
    cfg = run.cfg()["tiers"]["tier0"]
    jobs = [(p, cfg.get("includeMotifOpportunities", True)) for p in _pending(run, "tier0.jsonl")]
    t0 = time.time()
    n = run_pool(jobs, _tier0_job, workers or cfg["workers"], run.path("tier0.jsonl"),
                 run.serve_cmd(), load_taxonomy()["family"], "tier0")
    run.record_stage("tier0", {"processed": n, "elapsedSec": round(time.time() - t0, 1)})


def stage_tier1(run: Run, workers=None):
    cfg = run.cfg()["tiers"]["tier1"]
    budgets = sorted(cfg["nodeBudgets"])
    jobs = [(p, budgets, cfg.get("pvPlies", 8)) for p in _pending(run, "tier1.jsonl")]
    t0 = time.time()
    n = run_pool(jobs, _tier1_job, workers or cfg["workers"], run.path("tier1.jsonl"),
                 run.serve_cmd(), None, "tier1")
    run.record_stage("tier1", {"processed": n, "elapsedSec": round(time.time() - t0, 1)})


# ---------------------------------------------------------------------------------------------
# Tier 2: priority-v1


def coverage_counts(tier0_records) -> Counter:
    c: Counter = Counter()
    for r in tier0_records:
        if r.get("status") != "ok":
            continue
        for s in r["taxonomy"]["slugs"]:
            c[s] += 1
        for fam in r["taxonomy"]["families"]:
            c["family:" + fam] += 1
        geo = r["deterministic_geometry"]
        c["phase:" + geo["phase"]] += 1
        c["material:" + geo["materialBucket"]] += 1
        c["__positions__"] += 1
    return c


def priority_components(t0: dict, t1: dict, outcome, counts: Counter, cfg: dict) -> dict:
    caps = cfg["caps"]
    sd = t1["search_derived"]
    hi, lo, der = sd["budgets"][-1], sd["budgets"][0], sd["derived"]
    geo = t0["deterministic_geometry"]
    tac = t0["bounded_tactical_proof"]

    cov = cfg.get("coverage", {})
    total = counts.get("__positions__", 0)
    target = max(cov.get("minTarget", 5), cov.get("targetShare", 0.02) * total)
    slug_rarity = max((min(1.0, target / max(1, counts.get(s, 0))) for s in t0["taxonomy"]["slugs"]),
                      default=0.0)
    bucket_rarity = min(1.0, target / max(1, counts.get("material:" + geo["materialBucket"], 0)))

    shorter = min(len(lo["pv"]), len(hi["pv"]))
    pv_dis = 1.0 - der["pvCommonPrefix"] / shorter if shorter else 0.0

    outcome_v = 0.0
    if outcome is not None and der.get("scoreCpWhite") is not None:
        s = clamp_cp(der["scoreCpWhite"])
        against = -s if outcome == 1 else s if outcome == 0 else abs(s) * 0.5
        outcome_v = min(1.0, max(0.0, against - caps["outcomeMarginCp"]) / caps["outcomeScaleCp"])

    stab = hi["stabilization"] or {}
    selectivity = min(1.0, (stab.get("seePruneSkips") or 0) / caps["seePruneSkips"])
    if "partial-iteration-move-shift" in (stab.get("reasons") or []):
        selectivity = 1.0

    return {
        "scoreInstability": min(1.0, abs(der["scoreDeltaCp"]) / caps["scoreDeltaCp"]),
        "bestMoveChange": 1.0 if der["bestMoveChanged"] else 0.0,
        "trajectoryUnstable": STATUS_INSTABILITY.get(stab.get("status"), 0.5),
        "pvDisagreement": pv_dis,
        "tacticalDensity": min(1.0, tac["tacticalItems"] / caps["tacticalItems"]),
        "rarity": max(slug_rarity, 0.5 * bucket_rarity),
        "outcomeDisagreement": outcome_v,
        "selectivityEdge": selectivity,
        "forcedness": 1.0 if geo["inCheck"] or geo["legalMoves"] <= caps["forcedLegalMoves"] else 0.0,
    }


def score_priority(components: dict, weights: dict) -> tuple[float, dict, list[str]]:
    wsum = sum(weights.values()) or 1.0
    detail = {}
    for name, w in sorted(weights.items()):
        v = components.get(name, 0.0)
        detail[name] = {"value": round(v, 6), "weight": w, "contribution": round(w * v / wsum, 6)}
    total = sum(d["contribution"] for d in detail.values())
    reasons = [n for n, d in sorted(detail.items(), key=lambda kv: (-kv[1]["contribution"], kv[0]))
               if d["contribution"] > 0][:4]
    return round(total, 6), detail, reasons


def select(items: list[dict], t2: dict, t4: dict, seed: int) -> list[dict]:
    """items: [{id, priority, components, reasons}] -> triage records with selection flags.
    Holdout membership is a pure function of (holdoutSeed, id): stable across pools, runs and
    prioritizer versions, and excluded from every training-eligible arm."""
    n = len(items)
    hseed = t2["holdoutSeed"]
    holdout = {it["id"] for it in items if unit_hash(hseed, "holdout", it["id"]) < t2["holdoutFraction"]}
    pool = [it for it in items if it["id"] not in holdout]
    ranked = sorted(pool, key=lambda it: (-it["priority"], stable_hash(seed, "tie", it["id"])))
    k_deep = round(t2["deepFraction"] * n)
    deep = [it["id"] for it in ranked[:k_deep]]
    remainder = ranked[k_deep:]
    audit = [it["id"] for it in sorted(remainder, key=lambda it: stable_hash(seed, "audit", it["id"]))
             [:round(t2["auditFraction"] * n)]]
    uniform = [it["id"] for it in sorted(pool, key=lambda it: stable_hash(seed, "uniform", it["id"]))[:k_deep]]

    sf: dict[str, list[str]] = defaultdict(list)
    if t4.get("enabled", True):
        for tag, ids, frac in (("priority", deep, t4["priorityFraction"]),
                               ("uniform", uniform, t4["uniformFraction"]),
                               ("audit", audit, t4["auditFraction"])):
            for i in ids[:round(frac * n)]:
                sf[i].append(tag)
        for i in sorted(holdout):
            sf[i].append("holdout")

    rank = {it["id"]: r for r, it in enumerate(ranked, 1)}
    deep_s, audit_s, uniform_s = set(deep), set(audit), set(uniform)
    out = []
    for it in sorted(items, key=lambda it: (rank.get(it["id"], n + 1), it["id"])):
        i = it["id"]
        out.append({
            "id": i, "schemaVersion": FUNNEL_SCHEMA_VERSION, "stage": "tier2",
            "prioritizerVersion": PRIORITIZER_VERSION,
            "priority": it["priority"], "rank": rank.get(i),
            "components": it["components"], "reasons": it["reasons"],
            "selection": {"deep": i in deep_s, "uniform": i in uniform_s, "audit": i in audit_s,
                          "holdout": i in holdout, "stockfish": sf.get(i, [])},
            "trainEligible": i not in holdout,
        })
    return out


def stage_triage(run: Run):
    cfg = run.cfg()
    t2, t4 = cfg["tiers"]["tier2"], cfg["tiers"]["tier4"]
    positions = by_id(run.path("positions.jsonl"))
    t0s = {k: v for k, v in by_id(run.path("tier0.jsonl")).items() if v.get("status") == "ok"}
    t1s = by_id(run.path("tier1.jsonl"))
    pool_counts = coverage_counts(t0s.values())
    counts = Counter(pool_counts)
    prior_path = t2.get("coverage", {}).get("priorCountsPath")
    if prior_path:
        counts.update(json.loads(run.resolve(prior_path).read_text(encoding="utf-8"))["counts"])
    items = []
    for pid in sorted(set(t0s) & set(t1s)):
        outcome = (positions[pid].get("gameOutcome") or {}).get("whiteScore")
        comps = priority_components(t0s[pid], t1s[pid], outcome, counts, t2)
        pr, detail, reasons = score_priority(comps, t2["weights"])
        items.append({"id": pid, "priority": pr, "components": detail, "reasons": reasons})
    recs = select(items, t2, t4, cfg["seed"])
    with open(run.path("triage.jsonl"), "w", encoding="utf-8") as out:
        for r in recs:
            out.write(dumps(r) + "\n")
    run.path("coverage.json").write_text(json.dumps({
        "note": "position counts per taxonomy slug / family / phase / material bucket for this pool; "
                "pass as tiers.tier2.coverage.priorCountsPath to make the next run coverage-aware",
        "prioritizerVersion": PRIORITIZER_VERSION,
        "counts": dict(sorted(pool_counts.items())),
    }, indent=1), encoding="utf-8")
    sel = Counter(k for r in recs for k, v in r["selection"].items() if v and k != "stockfish")
    sf = sum(1 for r in recs if r["selection"]["stockfish"])
    run.record_stage("triage", {"ranked": len(recs), "selected": dict(sel), "stockfish": sf,
                                "priorCountsPath": prior_path})
    print(f"triage: {len(recs)} ranked; selected {dict(sel)}; stockfish {sf}")


def stage_tier3(run: Run, workers=None):
    cfg = run.cfg()["tiers"]["tier3"]
    tri = read_jsonl(run.path("triage.jsonl"))
    want = {r["id"] for r in tri if any(r["selection"][k] for k in ("deep", "uniform", "audit", "holdout"))}
    t1s = by_id(run.path("tier1.jsonl"))
    jobs = [(p, t1s[p["id"]]["search_derived"]["budgets"][-1], cfg["nodeBudget"], cfg.get("pvPlies", 12))
            for p in _pending(run, "tier3.jsonl", want)]
    t0 = time.time()
    n = run_pool(jobs, _tier3_job, workers or cfg["workers"], run.path("tier3.jsonl"),
                 run.serve_cmd(), None, "tier3")
    run.record_stage("tier3", {"selected": len(want), "processed": n, "elapsedSec": round(time.time() - t0, 1)})


def stage_tier4(run: Run, workers=None):
    cfg = run.cfg()
    t4 = cfg["tiers"]["tier4"]
    if not t4.get("enabled", True):
        run.record_stage("tier4", {"skipped": "disabled"})
        return
    sf_bin = os.environ.get("CVS_SF_EXE") or cfg["stockfish"]["binary"]
    tri = read_jsonl(run.path("triage.jsonl"))
    want = {r["id"] for r in tri if r["selection"]["stockfish"]}
    pending = _pending(run, "tier4.jsonl", want)
    size = t4.get("chunkSize", 20)
    tmp = run.path("tmp")
    tmp.mkdir(exist_ok=True)
    jobs = [(pending[i:i + size], sf_bin, cfg["stockfish"].get("hashMb", 16), t4["depth"],
             t4["movetimeMs"], 12, str(tmp), f"{os.getpid()}-{i // size:05d}")
            for i in range(0, len(pending), size)]
    t0 = time.time()
    n = run_pool(jobs, _tier4_job, workers or t4["workers"], run.path("tier4.jsonl"), None, None, "tier4 chunks")
    run.record_stage("tier4", {"selected": len(want), "chunks": n, "elapsedSec": round(time.time() - t0, 1)})


# ---------------------------------------------------------------------------------------------
# report


def _q(values, q):
    if not values:
        return None
    s = sorted(values)
    return s[min(len(s) - 1, int(q * (len(s) - 1) + 0.5))]


def arm_stats(ids, t3s, t4s, tier1_cost, cfg_r) -> dict:
    ids = [i for i in ids if i in t3s]
    if not ids:
        return {"n": 0}
    d = [t3s[i]["search_derived"] for i in ids]
    delta = [abs(x["shallowToDeep"]["scoreDeltaCp"]) for x in d]
    moved = [x["shallowToDeep"]["bestMoveChanged"] for x in d]
    # Score-only on purpose: between shallow and deep budgets the best move changes in most
    # positions (near-equal quiet alternatives), so counting move changes makes every position
    # "informative". Move churn is reported separately as moveChangeRate.
    informative = [dl >= cfg_r["informativeDeltaCp"] for dl in delta]
    secs = sum(t3s[i]["cost"]["wallMs"] for i in ids) / 1000
    nodes = sum(t3s[i]["cost"]["nodes"] or 0 for i in ids)
    out = {
        "n": len(ids), "deepNodes": nodes, "deepEngineSec": round(secs, 1),
        "informative": sum(informative), "informativeRate": round(sum(informative) / len(ids), 4),
        "informativePerDeepEngineSec": round(sum(informative) / max(secs, 1e-9), 4),
        "moveChangeRate": round(sum(moved) / len(ids), 4),
        "absShallowDeepDeltaCp": {"mean": round(statistics.fmean(delta), 1), "p50": _q(delta, .5), "p90": _q(delta, .9)},
    }
    sf_ids = [i for i in ids if i in t4s]
    if sf_ids:
        err, miss, same_move = [], 0, 0
        for i in sf_ids:
            deep = t3s[i]["search_derived"]["deep"]
            sf = t4s[i]["external_oracle"]
            e = abs(clamp_cp(deep["scoreCpStm"]) - clamp_cp(sf["scoreCpStm"]))
            err.append(e)
            miss += e >= cfg_r["oracleDeltaCp"]
            same_move += deep["bestMove"] == sf["bestMove"]
        out["oracle"] = {"n": len(sf_ids), "cvsDeepVsSfAbsCp": {"mean": round(statistics.fmean(err), 1),
                                                                "p50": _q(err, .5), "p90": _q(err, .9)},
                         "disagreementRate": round(miss / len(sf_ids), 4),
                         "moveAgreementRate": round(same_move / len(sf_ids), 4)}
    return out


def stage_report(run: Run):
    cfg = run.cfg()
    rcfg = cfg["report"]
    m = run.manifest()
    positions = by_id(run.path("positions.jsonl"))
    t0s = by_id(run.path("tier0.jsonl"))
    t1s = by_id(run.path("tier1.jsonl"))
    tri = read_jsonl(run.path("triage.jsonl"))
    t3s = by_id(run.path("tier3.jsonl"))
    t4s = by_id(run.path("tier4.jsonl"))
    t0ok = {k: v for k, v in t0s.items() if v.get("status") == "ok"}

    def tier_cost(recs):
        ms = [r["cost"]["wallMs"] for r in recs if r.get("cost") and r["cost"].get("wallMs") is not None]
        nodes = sum((r.get("cost") or {}).get("nodes") or 0 for r in recs)
        return {"positions": len(recs), "engineSec": round(sum(ms) / 1000, 1),
                "msPerPosition": round(sum(ms) / max(1, len(ms)), 2), "nodes": nodes}

    arms = {k: [r["id"] for r in tri if r["selection"][k]] for k in ("deep", "uniform", "audit", "holdout")}
    report: dict = {
        "funnelSchemaVersion": FUNNEL_SCHEMA_VERSION, "prioritizerVersion": PRIORITIZER_VERSION,
        "manifest": {k: m.get(k) for k in ("configSha256", "toolingGit", "taxonomy", "overrides")},
        "engineBinarySha256": (m.get("engine") or {}).get("binarySha256"),
        "factsRegistryVersion": next((r.get("factsRegistryVersion") for r in t0ok.values()), None),
        "tiers": {
            "tier0": {**tier_cost(t0s.values()), "statuses": dict(Counter(r.get("status", "?").split(":")[0] for r in t0s.values()))},
            "tier1": tier_cost(t1s.values()),
            "tier3": tier_cost(t3s.values()),
            "tier4": tier_cost(t4s.values()),
        },
        "wallClockSec": {s: v.get("elapsedSec") for s, v in (m.get("stages") or {}).items() if "elapsedSec" in v},
    }
    total_engine = sum(report["tiers"][t]["engineSec"] for t in ("tier0", "tier1", "tier3", "tier4"))
    report["tiers"]["totalEngineSec"] = round(total_engine, 1)

    # coverage (semantic corpus)
    cov = coverage_counts(t0ok.values())
    n0 = max(1, cov.get("__positions__", 0))
    t2cfg = cfg["tiers"]["tier2"]["coverage"]
    target = max(t2cfg.get("minTarget", 5), t2cfg.get("targetShare", 0.02) * n0)
    slugs = {k: v for k, v in cov.items() if ":" not in k and k != "__positions__"}
    report["coverage"] = {
        "positions": n0,
        "slugs": dict(sorted(slugs.items(), key=lambda kv: -kv[1])),
        "families": {k[7:]: v for k, v in sorted(cov.items(), key=lambda kv: -kv[1]) if k.startswith("family:")},
        "phases": {k[6:]: v for k, v in cov.items() if k.startswith("phase:")},
        "materialBucketsTop": dict(Counter({k[9:]: v for k, v in cov.items() if k.startswith("material:")}).most_common(12)),
        "underrepresented": sorted([s for s, v in slugs.items() if v < target], key=lambda s: slugs[s]),
        "unmapped": dict(Counter(u for r in t0ok.values() for u in r["taxonomy"]["unmapped"])),
        "uncomputed": dict(Counter(u for r in t0ok.values() for u in r["uncomputed"])),
        "provenanceRecords": {"deterministic_geometry": len(t0ok), "bounded_tactical_proof": len(t0ok),
                              "search_derived": len(t1s) + len(t3s), "external_oracle": len(t4s),
                              "game_outcome": sum(1 for p in positions.values() if p.get("gameOutcome"))},
    }

    # deep-label spend breakdowns
    tri_by = {r["id"]: r for r in tri}
    spend: dict = {"family": defaultdict(lambda: [0, 0.0]), "phase": defaultdict(lambda: [0, 0.0]),
                   "priorityDecile": defaultdict(lambda: [0, 0.0]), "arm": defaultdict(lambda: [0, 0.0]),
                   "materialBucket": defaultdict(lambda: [0, 0.0])}
    n_ranked = max(1, sum(1 for r in tri if r["rank"]))
    for i, r3 in t3s.items():
        sec = r3["cost"]["wallMs"] / 1000
        t0r = t0ok.get(i)
        keys = {
            "family": (t0r["taxonomy"]["families"] or ["(none)"]) if t0r else ["(no-tier0)"],
            "phase": [t0r["deterministic_geometry"]["phase"]] if t0r else ["?"],
            "materialBucket": [t0r["deterministic_geometry"]["materialBucket"]] if t0r else ["?"],
            "priorityDecile": [f"d{min(9, (tri_by[i]['rank'] - 1) * 10 // n_ranked)}" if tri_by[i]["rank"] else "holdout"],
            "arm": [k for k in ("deep", "uniform", "audit", "holdout") if tri_by[i]["selection"][k]],
        }
        for dim, vals in keys.items():
            for v in vals:
                spend[dim][v][0] += 1
                spend[dim][v][1] += sec
    report["deepSpend"] = {
        dim: {k: {"positions": c, "engineSec": round(s, 1)}
              for k, (c, s) in sorted(vals.items(), key=lambda kv: -kv[1][1])[:(15 if dim == "materialBucket" else 50)]}
        for dim, vals in spend.items()
    }

    # arms + experiment
    stats = {k: arm_stats(v, t3s, t4s, report["tiers"]["tier1"], rcfg) for k, v in arms.items()}
    report["arms"] = stats
    sf_arm = {tag: [r["id"] for r in tri if tag in r["selection"]["stockfish"]]
              for tag in ("priority", "uniform", "audit", "holdout")}
    report["oracleArms"] = {tag: arm_stats(ids, t3s, t4s, None, rcfg).get("oracle") for tag, ids in sf_arm.items()}

    remainder = sum(1 for r in tri if r["rank"] and not r["selection"]["deep"])
    audit = stats.get("audit", {})
    deep = stats.get("deep", {})
    if audit.get("n") and deep.get("n"):
        est_missed = audit["informativeRate"] * remainder
        est_total = deep["informative"] + est_missed
        report["auditMissEstimate"] = {
            "lowPriorityPool": remainder, "auditN": audit["n"],
            "auditInformativeRate": audit["informativeRate"],
            "estimatedInformativeMissed": round(est_missed),
            "informativeCaptured": deep["informative"],
            "estimatedRecall": round(deep["informative"] / max(1e-9, est_total), 4),
            # recall is bounded by the deep budget: k deep slots can hold at most k informative positions
            "maxRecallAtBudget": round(min(1.0, deep["n"] / max(1e-9, est_total)), 4),
            "triageLift": round(deep["informativeRate"] / max(1e-9, audit["informativeRate"]), 3),
        }
    uni = stats.get("uniform", {})
    if deep.get("n") and uni.get("n"):
        overlap = len(set(arms["deep"]) & set(arms["uniform"]))
        report["experiment"] = {
            "question": "equal deep-label budget: priority-selected vs uniformly sampled positions",
            "equalBudget": {"positionsEach": deep["n"], "nodeBudgetEach": cfg["tiers"]["tier3"]["nodeBudget"],
                            "deepNodes": {"priority": deep["deepNodes"], "uniform": uni["deepNodes"]},
                            "overlapPositions": overlap},
            "informativeYieldRatio": round(deep["informative"] / max(1, uni["informative"]), 3),
            "informativeRate": {"priority": deep["informativeRate"], "uniform": uni["informativeRate"]},
            "oracleDisagreementRate": {k: (report["oracleArms"].get(k) or {}).get("disagreementRate")
                                       for k in ("priority", "uniform", "audit", "holdout")},
            "caveat": "informativeRate measures shallow->deep CVS change, and the prioritizer reads shallow "
                      "instability, so that ratio is partly circular. oracleDisagreementRate (CVS deep vs "
                      "Stockfish on equal-size samples) is the non-circular signal. Neither is the downstream "
                      "training-value test the research question ultimately needs.",
        }
    misses = [i for i in arms["audit"] if i in t3s and
              abs(t3s[i]["search_derived"]["shallowToDeep"]["scoreDeltaCp"]) >= rcfg["informativeDeltaCp"]]
    with open(run.path("triage-misses.jsonl"), "w", encoding="utf-8") as out:
        for i in misses:
            out.write(dumps({"id": i, "fen": positions[i]["fen"], "triage": tri_by[i],
                             "shallowToDeep": t3s[i]["search_derived"]["shallowToDeep"]}) + "\n")

    run.path("report.json").write_text(json.dumps(report, indent=2, default=list), encoding="utf-8")
    run.path("report.md").write_text(render_markdown(report), encoding="utf-8")
    print(render_markdown(report))


def render_markdown(r: dict) -> str:
    L = [f"# Labeling funnel report ({r['prioritizerVersion']})", ""]
    L.append(f"facts registry v{r.get('factsRegistryVersion')} · engine sha256 `{(r.get('engineBinarySha256') or '')[:16]}` · "
             f"config sha256 `{(r['manifest'].get('configSha256') or '')[:16]}`")
    L += ["", "## Cost per tier", "", "| tier | positions | engine-sec | ms/position | nodes |", "|---|---:|---:|---:|---:|"]
    for t in ("tier0", "tier1", "tier3", "tier4"):
        c = r["tiers"][t]
        L.append(f"| {t} | {c['positions']} | {c['engineSec']} | {c['msPerPosition']} | {c['nodes']} |")
    L.append(f"\nTotal single-thread engine-seconds: {r['tiers']['totalEngineSec']}")
    L += ["", "## Arms (deep CVS labels)", "",
          "| arm | n | informative rate | inform./deep-sec | move-change | mean abs(delta) cp | oracle n | CVS-vs-SF disagree |",
          "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for k, s in r["arms"].items():
        if not s.get("n"):
            continue
        o = s.get("oracle") or {}
        L.append(f"| {k} | {s['n']} | {s['informativeRate']} | {s['informativePerDeepEngineSec']} | {s['moveChangeRate']} | "
                 f"{s['absShallowDeepDeltaCp']['mean']} | {o.get('n', '')} | {o.get('disagreementRate', '')} |")
    if r.get("oracleArms"):
        L += ["", "Stockfish samples by selection reason: " + ", ".join(
            f"{k}: n={v['n']} disagree={v['disagreementRate']} mean abs(delta)={v['cvsDeepVsSfAbsCp']['mean']}"
            for k, v in r["oracleArms"].items() if v)]
    if r.get("experiment"):
        e = r["experiment"]
        L += ["", "## Equal-budget experiment", "", f"- {e['question']}",
              f"- positions each: {e['equalBudget']['positionsEach']} (overlap {e['equalBudget']['overlapPositions']}), "
              f"deep nodes priority/uniform: {e['equalBudget']['deepNodes']['priority']}/{e['equalBudget']['deepNodes']['uniform']}",
              f"- informative yield ratio (priority/uniform): **{e['informativeYieldRatio']}**",
              f"- oracle disagreement rate: {e['oracleDisagreementRate']}", f"- caveat: {e['caveat']}"]
    if r.get("auditMissEstimate"):
        a = r["auditMissEstimate"]
        L += ["", "## Triage false negatives (low-priority audit)", "",
              f"- audit n={a['auditN']}, informative rate {a['auditInformativeRate']} over a low-priority pool of {a['lowPriorityPool']}",
              f"- estimated informative positions missed: {a['estimatedInformativeMissed']}; captured: {a['informativeCaptured']}; "
              f"estimated recall **{a['estimatedRecall']}** (max achievable at this deep budget: {a['maxRecallAtBudget']})",
              f"- triage lift (deep informative rate / low-priority informative rate): **{a['triageLift']}**"]
    c = r["coverage"]
    L += ["", "## Coverage (semantic corpus)", "", f"positions with facts: {c['positions']}; phases: {c['phases']}", "",
          "| taxonomy slug | positions |", "|---|---:|"]
    L += [f"| {k} | {v} |" for k, v in c["slugs"].items()]
    L += ["", f"underrepresented (< target): {', '.join(c['underrepresented']) or '-'}",
          f"unmapped fact kinds: {c['unmapped'] or '-'}", f"uncomputed collections: {c['uncomputed'] or '-'}"]
    L += ["", "## Deep-label spend by family", "", "| family | positions | engine-sec |", "|---|---:|---:|"]
    L += [f"| {k} | {v['positions']} | {v['engineSec']} |" for k, v in r["deepSpend"]["family"].items()]
    L += ["", "## Deep-label spend by priority decile", "", "| decile | positions | engine-sec |", "|---|---:|---:|"]
    L += [f"| {k} | {v['positions']} | {v['engineSec']} |" for k, v in sorted(r["deepSpend"]["priorityDecile"].items())]
    return "\n".join(L) + "\n"


# ---------------------------------------------------------------------------------------------
# CLI

STAGES = ("sample", "tier0", "tier1", "triage", "tier3", "tier4", "report")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("stage", choices=STAGES + ("run",))
    ap.add_argument("--run-dir", required=True)
    ap.add_argument("--config", help="funnel config (copied into the run dir on first use; later stages read the copy)")
    ap.add_argument("--artifact-root", help="base for relative engine/data paths (default: repo root)")
    ap.add_argument("--positions", type=int, help="override source.positions")
    ap.add_argument("--workers", type=int, help="override every tier's worker count")
    a = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")  # Windows consoles are cp1252

    run = Run(a.run_dir, a.artifact_root)
    overrides = {k: v for k, v in (("positions", a.positions), ("workers", a.workers)) if v is not None}
    if not run.cfg_path.exists():
        if not a.config:
            ap.error("--config is required for a new run dir")
        cfg = json.loads(Path(a.config).read_text(encoding="utf-8"))
        if a.positions:
            cfg["source"]["positions"] = a.positions
        run.cfg_path.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
        write_identity(run, overrides)
    elif a.config:
        print(f"note: {run.cfg_path} already exists; --config ignored (frozen per run)", flush=True)

    stages = STAGES if a.stage == "run" else (a.stage,)
    for s in stages:
        print(f"== {s}", flush=True)
        if s == "sample":
            if run.path("positions.jsonl").exists() and a.stage == "run":
                print("positions.jsonl exists; keeping it", flush=True)
                continue
            stage_sample(run)
        elif s == "tier0":
            stage_tier0(run, a.workers)
        elif s == "tier1":
            stage_tier1(run, a.workers)
        elif s == "triage":
            stage_triage(run)
        elif s == "tier3":
            stage_tier3(run, a.workers)
        elif s == "tier4":
            stage_tier4(run, a.workers)
        elif s == "report":
            stage_report(run)
    return 0


if __name__ == "__main__":
    sys.exit(main())
