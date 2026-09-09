#!/usr/bin/env python3
"""Sequential INV-1 gate ladder: one variable per gate, batched SPRT, hard game cap.

Every candidate in the ladder is a STRENGTH change, so INV-1 applies: it may be promoted
only if a declared SPRT crosses the upper boundary. This runs the gates one at a time
(never concurrently -- a match is a timing-free fixed-node control, but CPU contention
would still halve throughput and distort nothing except the wall clock), playing
color-paired fixed-node games in batches and recomputing the LLR after each batch so a
decided test stops early instead of burning the full cap.

Integrity requirement (2026-09-09): every game pair must start from a position no other
pair in the run used. The first ladder used one 12-position book for every batch, so each
120-game batch replayed the same 24 distinct games and the SPRT counted dependent repeats
as independent samples. This harness therefore REQUIRES `--book` and feeds cutechess a
disjoint slice per batch (`openings-batch-NNN.epd`); it refuses to run without a book and
stops (HOLD) rather than repeat a position when the book runs out. The per-batch slices are
checked against a seen-set, and `gate.json` records the book hash and positions used.

Each gate writes, under --out-dir/<gate-id>/:
  match.pgn / match.jsonl   every finished game, candidate POV
  openings-batch-NNN.epd    the exact disjoint slice that batch played from
  sprt.json                 the canonical SPRT record (schemas/sprt-result.schema.json)
  gate.json                 batch-by-batch LLR trajectory and the stop reason
and the ladder appends one line per finished gate to --out-dir/ladder.jsonl.

Decisions follow lint_promotion.py exactly: upper -> promote, lower -> reject, no crossing
at the cap -> hold_for_more_data. This script never writes 'promote' on its own judgment.

Usage:
  python gate_ladder.py --out-dir benchmarks/results/inv1-ladder-<date> \
      --exe path/to/uci.exe --book benchmarks/suites/openings-inv1-<date>.epd \
      [--gate singular --gate iid ...] \
      [--nodes 40000] [--batch 120] [--cap 2000] [--concurrency 12] \
      [--elo0 0 --elo1 10] [--dry-run]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import match_fixed_nodes as M  # noqa: E402
import sprt_runner as sr  # noqa: E402

REPO = HERE.parents[1]

# The play-mode baseline flag set (match_fixed_nodes.N0_FLAGS). Every gate's baseline is
# exactly this; every candidate is exactly this plus ONE change.
BASE_FLAGS = list(M.N0_FLAGS)

# The ladder. Ordered by expected gain in a conventional engine: ordering history first
# (it makes every later pruning decision better), then pruning, then extensions.
# `flag` gates toggle one runtime flag; `exe` gates compare two builds of the same source
# tree differing by one commit.
LADDER: dict[str, dict] = {
    "conthist":    {"kind": "flag", "add": ["--conthist"],
                    "what": "continuation history in quiet move ordering"},
    "countermove": {"kind": "flag", "add": ["--countermove"],
                    "what": "countermove heuristic in quiet move ordering"},
    "caphist":     {"kind": "flag", "add": ["--caphist"],
                    "what": "capture history in capture ordering and LMR"},
    "seeprune":    {"kind": "flag", "add": ["--seeprune"],
                    "what": "SEE pruning of losing captures in the main search"},
    "delta":       {"kind": "flag", "add": ["--delta"],
                    "what": "delta pruning in quiescence"},
    "improving":   {"kind": "flag", "add": ["--improving"],
                    "what": "improving flag in LMR / pruning margins"},
    "loglmr":      {"kind": "flag", "add": ["--loglmr"],
                    "what": "log-based LMR (r = 0.75 + ln d * ln i / 2.25) vs the flat 1-ply tier"},
    "singular":    {"kind": "flag", "add": ["--singular"],
                    "what": "singular extensions"},
    "iid":         {"kind": "flag", "add": ["--iid"],
                    "what": "internal iterative deepening"},
    "tt2":         {"kind": "flag", "add": ["--tt2"],
                    "what": "two-tier transposition table"},
    "rule50":      {"kind": "flag", "add": ["--rule50"],
                    "what": "rule-50 score scaling"},
    "kingact":     {"kind": "flag", "add": ["--king-activity"],
                    "what": "king activity term in endgames"},
    "futility-pv": {"kind": "exe", "what": "futility pruning skipped at PV nodes (#75)"},
}

# Re-gates: the candidate is the CURRENT champion (flag already default-on), the baseline is
# the champion with that one flag disabled. These exist because the 2026-09-09 promotions
# were gated on repeated games; a promotion keeps its claim only if the re-gate reproduces
# it on independent positions. `bundle-inv1` is the collective confirmation of the promoted
# set (documented as a confirmation, not a single-variable promotion record).
PROMOTED_FLAGS = {
    "caphist": "--no-caphist",
    "seeprune": "--no-seeprune",
    "improving": "--no-improving",
    "tt2": "--no-tt2",
    "kingact": "--no-king-activity",
}
REGATES: dict[str, dict] = {
    f"{name}-regate": {"kind": "flag", "add": [], "base_add": [off],
                       "what": f"re-gate {name}: champion vs champion with {name} disabled"}
    for name, off in PROMOTED_FLAGS.items()
}
REGATES["bundle-inv1"] = {
    "kind": "flag", "add": [], "base_add": sorted(PROMOTED_FLAGS.values()),
    "what": "confirmation: current champion vs champion with the promoted INV-1 set disabled",
}
ALL_GATES = {**LADDER, **REGATES}


# ── book handling (fail closed on repeats) ───────────────────────────────────


def load_book(path: Path) -> list[str]:
    """Read an EPD book as four-field position keys (deduped, order preserved)."""
    seen: set[str] = set()
    lines: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key = " ".join(line.split()[:4])
        if key in seen:
            continue
        seen.add(key)
        lines.append(key)
    if not lines:
        raise SystemExit(f"empty opening book: {path}")
    return lines


def take_slice(book: list[str], cursor: int, need_positions: int,
               used: set[str]) -> tuple[list[str], int]:
    """Take the next `need_positions` unused book entries; raise if any repeat.

    Returns (positions, new_cursor). A short tail is allowed (the caller plays fewer
    games); an exhausted book returns an empty slice so the gate stops with HOLD.
    """
    slice_: list[str] = []
    while cursor < len(book) and len(slice_) < need_positions:
        pos = book[cursor]
        cursor += 1
        if pos in used:
            raise SystemExit(f"book repeat detected for {pos!r} -- refusing to play it twice")
        used.add(pos)
        slice_.append(pos)
    return slice_, cursor


def write_slice(out: Path, batch_no: int, positions: list[str]) -> Path:
    path = out / f"openings-batch-{batch_no:03d}.epd"
    path.write_text("\n".join(positions) + "\n", encoding="utf-8")
    return path


# ── gate runner ──────────────────────────────────────────────────────────────


def play_batch(gate_id: str, spec: dict, out: Path, args, batch_games: int, batch_no: int,
               openings: Path):
    """One cutechess batch. Returns the candidate-POV rows it produced."""
    pgn = out / f"batch-{batch_no:03d}.pgn"
    cand_flags = BASE_FLAGS + spec.get("add", [])
    base_flags = BASE_FLAGS + spec.get("base_add", [])
    cand = {"name": "cand", "exe": spec.get("cand_exe", args.exe), "net": args.net,
            "helper": M.N0_HELPER, "flags": cand_flags, "nodes": args.nodes}
    base = {"name": "base", "exe": spec.get("base_exe", args.exe), "net": args.net,
            "helper": M.N0_HELPER, "flags": base_flags, "nodes": args.nodes}
    cmd = M.build_cutechess_cmd(cand, base, batch_games, str(pgn),
                                openings=str(openings), concurrency=args.concurrency)
    if args.dry_run:
        print(" ".join(cmd))
        return []
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(f"[{gate_id}] cutechess exit {proc.returncode}\n{proc.stderr[-1500:]}\n")
        raise SystemExit(proc.returncode)
    text = pgn.read_text(encoding="utf-8", errors="replace")
    for bad in ("illegal move", "disconnect", "stall", "loses on time"):
        if bad in text.lower():
            sys.stderr.write(f"[{gate_id}] WARNING: match log mentions {bad!r}\n")
    return M.pgn_to_results(text, "cand")


def run_gate(gate_id: str, spec: dict, args, book: list[str]) -> dict:
    out = Path(args.out_dir) / gate_id
    out.mkdir(parents=True, exist_ok=True)
    rows: list[dict] = []
    trajectory = []
    started = time.time()
    batch_no = 0
    cursor = args.book_offset
    used: set[str] = set()
    run = None
    while len(rows) < args.cap:
        want = min(args.batch, args.cap - len(rows))
        want += want % 2  # keep color pairs whole
        positions, cursor = take_slice(book, cursor, want // 2, used)
        if not positions:
            break  # book exhausted -> HOLD, never repeat
        batch_no += 1
        slice_path = write_slice(out, batch_no, positions)
        rows += play_batch(gate_id, spec, out, args, 2 * len(positions), batch_no, slice_path)
        if args.dry_run:
            return {"gate": gate_id, "dryRun": True}
        run = sr.run_sprt((sr.score_from_result_row(r) for r in rows),
                          elo0=args.elo0, elo1=args.elo1, alpha=args.alpha, beta=args.beta)
        trajectory.append({"batch": batch_no, "games": run["games"], "wins": run["wins"],
                           "losses": run["losses"], "draws": run["draws"],
                           "llr": round(run["llr"], 4), "boundary": run["boundary"]})
        print(f"[{gate_id}] {run['games']:5d}g  {run['wins']}-{run['losses']}-{run['draws']}  "
              f"llr {run['llr']:+.3f}  [{run['lowerBound']:.2f},{run['upperBound']:.2f}]  "
              f"{run['boundary']}", flush=True)
        if run["boundary"] != "none":
            break
    elapsed = round(time.time() - started, 1)
    if run is None:
        raise SystemExit(f"[{gate_id}] no games played (book exhausted before batch 1)")
    (out / "match.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n",
                                     encoding="utf-8")
    cand_flags = BASE_FLAGS + spec.get("add", [])
    provenance = {
        "engineSha": M._sha16(spec.get("cand_exe", args.exe)),
        "netSha": M._sha16(args.net),
        "candArgs": cand_flags + [f"nodes={args.nodes}", f"exe={Path(spec.get('cand_exe', args.exe)).parent.parent.parent.name}"],
        "baseArgs": BASE_FLAGS + spec.get("base_add", []) + [f"nodes={args.nodes}", f"exe={Path(spec.get('base_exe', args.exe)).parent.parent.parent.name}"],
        "tc": f"nodes:{args.nodes}",
        "nodes": args.nodes,
        "threads": 1,
        "openingsSha": M._sha16(args.book),
        "book": Path(args.book).name,
        "bookPositions": len(book),
        "positionsUsed": len(used),
    }
    rec = sr.build_record(run, experiment_id=f"{Path(args.out_dir).name}/{gate_id}",
                          baseline_id=args.baseline_id,
                          candidate_id=f"{args.baseline_id}+{gate_id}",
                          elo0=args.elo0, elo1=args.elo1,
                          alpha=args.alpha, beta=args.beta,
                          provenance=provenance)
    (out / "sprt.json").write_text(json.dumps(rec, indent=2) + "\n", encoding="utf-8")
    gate = {"gate": gate_id, "what": spec["what"], "kind": spec["kind"],
            "games": run["games"], "wins": run["wins"], "losses": run["losses"], "draws": run["draws"],
            "llr": round(run["llr"], 4), "boundary": run["boundary"],
            "decision": rec["decision"], "elapsedSec": elapsed,
            "cappedAt": args.cap if run["boundary"] == "none" else None,
            "bookExhausted": cursor >= len(book),
            "positionsUsed": len(used), "book": Path(args.book).name,
            "trajectory": trajectory, "record": str((out / "sprt.json").as_posix())}
    (out / "gate.json").write_text(json.dumps(gate, indent=2) + "\n", encoding="utf-8")
    return gate


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--exe", required=True, help="baseline/candidate uci.exe for flag gates")
    ap.add_argument("--futpv-exe", help="uci.exe built with the #75 patch (futility-pv gate)")
    ap.add_argument("--book", required=True,
                    help="EPD opening book; each batch gets a disjoint slice (no repeats)")
    ap.add_argument("--book-offset", type=int, default=0,
                    help="skip this many book positions before the first batch")
    ap.add_argument("--net", default=M.N0_NET)
    ap.add_argument("--gate", action="append", help="gate id (repeatable); default: the whole ladder")
    ap.add_argument("--nodes", type=int, default=40000)
    ap.add_argument("--batch", type=int, default=120)
    ap.add_argument("--cap", type=int, default=2000, help="max games per gate before HOLD")
    ap.add_argument("--concurrency", type=int, default=12)
    ap.add_argument("--elo0", type=float, default=0.0)
    ap.add_argument("--elo1", type=float, default=10.0)
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--baseline-id", default="master")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args(argv)

    if args.batch % 2:
        raise SystemExit("--batch must be even (color-paired games)")
    book = load_book(Path(args.book))
    print(f"book {args.book}: {len(book)} distinct positions "
          f"(sha {hashlib.sha256(Path(args.book).read_bytes()).hexdigest()[:16]})", flush=True)

    gates = args.gate or list(LADDER)
    unknown = [g for g in gates if g not in ALL_GATES]
    if unknown:
        raise SystemExit(f"unknown gate(s): {unknown}; known: {sorted(ALL_GATES)}")
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    ledger = out_dir / "ladder.jsonl"

    for gate_id in gates:
        spec = dict(ALL_GATES[gate_id])
        if spec["kind"] == "exe":
            if not args.futpv_exe:
                sys.stderr.write(f"[{gate_id}] skipped: --futpv-exe not given\n")
                continue
            spec["cand_exe"] = args.futpv_exe
        print(f"=== gate {gate_id}: {spec['what']} ===", flush=True)
        gate = run_gate(gate_id, spec, args, book)
        if args.dry_run:
            continue
        with ledger.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps({k: v for k, v in gate.items() if k != "trajectory"}) + "\n")
        print(f"--- {gate_id}: {gate['decision']} ({gate['games']} games, "
              f"llr {gate['llr']:+.3f}, {gate['elapsedSec']}s)\n", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
