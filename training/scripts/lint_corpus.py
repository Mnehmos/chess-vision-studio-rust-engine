#!/usr/bin/env python3
"""Corpus-invariant linter (issue #9).

Machine-enforces the data invariants that the pipeline previously only described:

  INV-3 (source split): every seed carries a stable source/game key; a row missing one is
        rejected (no silent FEN fallback). Across train/validation/test slices, one
        source/game key must appear in exactly one slice.
  INV-4 (finder != labeler): the engine that FOUND a seed may not be the oracle that LABELS
        it. Joined by seedId, label.labeler.engineId must differ from seed.finder.engineId.

Plus schema validation of candidate-seed and oracle-label rows (pure Python, no deps —
the hand-rolled equivalent of the JSON Schemas in training/schemas/).

Usage:
    python lint_corpus.py --seeds seeds.jsonl [--labels labels.jsonl] \
                          [--slice train=train.jsonl --slice validation=val.jsonl ...]
Exits non-zero on any violation, so it can gate CI.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SOURCE_KINDS = {"real_game", "maia_game", "engine_game", "bounded_perturbation", "benchmark_fixture"}
SEED_REASONS = {"oracle_disagreement", "eval_volatility", "collapse", "noconvert", "pressure_intersection"}
LABEL_STATUSES = {"usable", "quarantined", "conflict"}


def _is_int(x) -> bool:
    return isinstance(x, int) and not isinstance(x, bool)


def _str(x) -> bool:
    # whitespace-only is NOT a value — it must not pass as a stable key (review F4).
    return isinstance(x, str) and len(x.strip()) > 0


def stable_source_key(seed: dict) -> str | None:
    """The stable source/game key for splitting — NEVER the FEN (INV-3). splitKey is the
    authoritative per-game unit (this matches split_by_source.key_for's precedence and the
    schema's own description), with source.sourceId/gameId as the fallback. Whitespace is
    trimmed so "X" and " X " are the same key. Returns None when no stable key exists, so
    callers fail closed instead of position-splitting."""
    sk = seed.get("splitKey")
    if _str(sk):
        return sk.strip()
    src = seed.get("source") or {}
    sid, gid = src.get("sourceId"), src.get("gameId")
    if _str(gid):
        return f"{sid.strip()}/{gid.strip()}" if _str(sid) else gid.strip()
    return None


def validate_seed(row, idx: int = -1) -> list[str]:
    p = f"seed[{idx}]" if idx >= 0 else "seed"
    v: list[str] = []
    if not isinstance(row, dict):
        return [f"{p}: not a JSON object"]
    sv = row.get("schemaVersion")
    if not (_is_int(sv) and sv == 1):
        v.append(f"{p}: schemaVersion must be the integer 1")
    for f in ("seedId", "fen", "splitKey"):
        if not _str(row.get(f)):
            v.append(f"{p}: {f} must be a non-empty string")
    if row.get("reason") not in SEED_REASONS:
        v.append(f"{p}: reason must be one of {sorted(SEED_REASONS)}")
    src = row.get("source")
    if not isinstance(src, dict):
        v.append(f"{p}: source must be an object")
    else:
        if src.get("kind") not in SOURCE_KINDS:
            v.append(f"{p}: source.kind must be one of {sorted(SOURCE_KINDS)}")
        for f in ("sourceId", "gameId"):
            if not _str(src.get(f)):
                v.append(f"{p}: source.{f} must be a non-empty string")
        if not _is_int(src.get("ply")) or src.get("ply", -1) < 0:
            v.append(f"{p}: source.ply must be a non-negative integer")
        if "perturbationDepth" in src and (not _is_int(src["perturbationDepth"]) or src["perturbationDepth"] < 0):
            v.append(f"{p}: source.perturbationDepth must be a non-negative integer")
    fnd = row.get("finder")
    if not isinstance(fnd, dict):
        v.append(f"{p}: finder must be an object")
    else:
        for f in ("engineId", "binarySha256", "modelSha256", "optionsHash"):
            if not _str(fnd.get(f)):
                v.append(f"{p}: finder.{f} must be a non-empty string")
    # additionalProperties: false — reject unknown/typo'd fields.
    for k in row:
        if k not in {"schemaVersion", "seedId", "fen", "source", "splitKey", "finder", "reason", "stabilization"}:
            v.append(f"{p}: unknown field '{k}'")
    if isinstance(src, dict):
        for k in src:
            if k not in {"kind", "sourceId", "gameId", "ply", "parentSeedId", "perturbationDepth"}:
                v.append(f"{p}: unknown source field '{k}'")
    if isinstance(fnd, dict):
        for k in fnd:
            if k not in {"engineId", "binarySha256", "modelSha256", "optionsHash", "budget"}:
                v.append(f"{p}: unknown finder field '{k}'")
    # INV-3: a stable source/game key must exist (independent of the FEN).
    if stable_source_key(row) is None:
        v.append(f"{p}: no stable source/game key (INV-3) — source.gameId or splitKey required")
    return v


def validate_label(row, idx: int = -1) -> list[str]:
    p = f"label[{idx}]" if idx >= 0 else "label"
    v: list[str] = []
    if not isinstance(row, dict):
        return [f"{p}: not a JSON object"]
    sv = row.get("schemaVersion")
    if not (_is_int(sv) and sv == 1):
        v.append(f"{p}: schemaVersion must be the integer 1")
    if not _str(row.get("seedId")):
        v.append(f"{p}: seedId must be a non-empty string")
    if not isinstance(row.get("score"), dict):
        v.append(f"{p}: score must be an object")
    if "pv" in row and not isinstance(row.get("pv"), list):
        v.append(f"{p}: pv must be an array")
    if row.get("labelStatus") not in LABEL_STATUSES:
        v.append(f"{p}: labelStatus must be one of {sorted(LABEL_STATUSES)}")
    lbl = row.get("labeler")
    if not isinstance(lbl, dict):
        v.append(f"{p}: labeler must be an object")
    else:
        for f in ("engineId", "binarySha256", "version"):
            if not _str(lbl.get(f)):
                v.append(f"{p}: labeler.{f} must be a non-empty string")
    for k in row:
        if k not in {"schemaVersion", "seedId", "labeler", "score", "pv", "stabilization", "labelStatus"}:
            v.append(f"{p}: unknown field '{k}'")
    if isinstance(lbl, dict):
        for k in lbl:
            if k not in {"engineId", "binarySha256", "version", "options", "budget"}:
                v.append(f"{p}: unknown labeler field '{k}'")
    return v


def finder_labeler_violations(seeds: list[dict], labels: list[dict]) -> list[str]:
    """INV-4: the engine that FOUND a seed must not be the one that LABELS it. Joined by
    seedId. Flags duplicate seedIds (which would otherwise shadow a collision) and labels
    that join to no seed (where INV-4 cannot be verified)."""
    v: list[str] = []
    finders_by_seed: dict[str, set] = {}
    dup_seed_ids: set = set()
    for s in seeds:
        sid = s.get("seedId")
        if not _str(sid):
            continue
        if sid in finders_by_seed:
            dup_seed_ids.add(sid)
        finders_by_seed.setdefault(sid, set()).add((s.get("finder") or {}).get("engineId"))
    for sid in sorted(dup_seed_ids):
        v.append(f"duplicate seedId '{sid}' across seeds — seedIds must be unique")
    for i, lab in enumerate(labels):
        sid = lab.get("seedId")
        labeler = (lab.get("labeler") or {}).get("engineId")
        finders = finders_by_seed.get(sid)
        if finders is None:
            v.append(
                f"label[{i}] seedId='{sid}' joins to no seed — finder != labeler cannot be "
                f"verified (INV-4)"
            )
            continue
        if labeler is not None and labeler in finders:
            v.append(
                f"label[{i}] seedId={sid}: labeler engine '{labeler}' also found this seed — "
                f"finder != labeler required (INV-4)"
            )
    return v


def cross_slice_violations(slices: dict[str, list[dict]]) -> list[str]:
    """INV-3: a source/game key must appear in at most one slice."""
    seen: dict[str, str] = {}
    v: list[str] = []
    for name, rows in slices.items():
        for r in rows:
            key = stable_source_key(r)
            if key is None:
                continue  # validate_seed already flags missing keys
            prior = seen.get(key)
            if prior is None:
                seen[key] = name
            elif prior != name:
                v.append(f"source key '{key}' spans slices '{prior}' and '{name}' (INV-3 leak)")
    return v


def _load_jsonl(path: str) -> list[dict]:
    rows = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def main(argv) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds")
    ap.add_argument("--labels")
    ap.add_argument("--slice", action="append", default=[], metavar="name=path.jsonl")
    args = ap.parse_args(argv[1:])

    viol: list[str] = []
    seeds: list[dict] = []
    if args.seeds:
        seeds = _load_jsonl(args.seeds)
        for i, r in enumerate(seeds):
            viol += validate_seed(r, i)
    if args.labels:
        labels = _load_jsonl(args.labels)
        for i, r in enumerate(labels):
            viol += validate_label(r, i)
        viol += finder_labeler_violations(seeds, labels)
    if args.slice:
        slices = {}
        for spec in args.slice:
            name, _, path = spec.partition("=")
            slices[name] = _load_jsonl(path)
        viol += cross_slice_violations(slices)

    for x in viol:
        print(f"  - {x}")
    print(f"\n{len(viol)} corpus-invariant violation(s).")
    return 1 if viol else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
