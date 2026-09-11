#!/usr/bin/env python3
"""Time-control A/B match — the referee for changes that trade nodes for depth.

The fixed-node gate (match_fixed_nodes / gate_ladder) is the right instrument for
*node efficiency*: both engines get the same nodes and the deeper/more selective
one must be stronger per node. It is the WRONG instrument for a change that also
moves the nodes-per-second rate (e.g. SF's SEE pruning costs CPU: ~1.8x slower per
node). What the live bot experiences is strength per SECOND, so those changes need
an equal-time control: `-each tc=<base>+<inc>`, no node stop.

Reuses match_fixed_nodes' PGN parsing + sprt_runner for the verdict, so the record
format and the SPRT bounds are identical to the fixed-node gates.

Usage:
  python match_timed.py --exe target-sf/release/uci.exe --tc 8+0.08 \
      --cand-flag --sfprune --games 300 --concurrency 8 \
      --out-pgn benchmarks/results/sfprune-tc-20260910/batch-001.pgn --sprt
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import match_fixed_nodes as M  # noqa: E402
import sprt_runner as sr  # noqa: E402

CUTECHESS = M.CUTECHESS


def build_cmd(cand: dict, base: dict, games: int, pgnout: str, openings: str,
              tc: str, concurrency: int, maxmoves: int = 300) -> list[str]:
    def block(name: str, exe: str, flags: list[str]) -> list[str]:
        args = ["-engine", f"name={name}", f"cmd={exe}"]
        for a in (["--base", M.N0_BASE_W, "--rung2", M.N0_RUNG2_W, "--nnue", M.N0_NET]
                  + (["--helper-nnue", M.N0_HELPER] if M.N0_HELPER else []) + flags):
            args.append(f"arg={a}")
        return args

    return (
        [CUTECHESS]
        + block("cand", cand["exe"], cand["flags"])
        + block("base", base["exe"], base["flags"])
        + ["-each", "proto=uci", f"tc={tc}",
           "-games", str(games), "-repeat",
           "-concurrency", str(concurrency),
           "-openings", f"file={openings}", "format=epd", "order=sequential",
           "-draw", "movenumber=40", "movecount=8", "score=10",
           "-resign", "movecount=4", "score=900",
           "-maxmoves", str(maxmoves),
           "-pgnout", pgnout]
    )


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", default=str(M.REPO / "target/release/uci.exe"))
    ap.add_argument("--cand-flag", action="append", default=None,
                    help="candidate flag (repeatable); default: the champion flags")
    ap.add_argument("--base-flag", action="append", default=None,
                    help="baseline flag override (default: the champion flags)")
    ap.add_argument("--book", default=str(M.OPENINGS))
    ap.add_argument("--book-offset", type=int, default=0)
    ap.add_argument("--tc", default="8+0.08")
    ap.add_argument("--games", type=int, default=300)
    ap.add_argument("--concurrency", type=int, default=8)
    ap.add_argument("--out-pgn", required=True)
    ap.add_argument("--sprt", action="store_true")
    ap.add_argument("--elo0", type=float, default=0.0)
    ap.add_argument("--elo1", type=float, default=10.0)
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--experiment-id", default=None)
    a = ap.parse_args(argv)

    base_flags = M.N0_FLAGS if a.base_flag is None else a.base_flag
    cand_flags = (base_flags + a.cand_flag) if a.cand_flag else list(base_flags)
    books = M.load_book(Path(a.book)) if hasattr(M, "load_book") else None
    # Disjoint slice: same anti-repeat discipline as the ladder (a --book-offset
    # selects a fresh window; the seen-set refuses repeats inside one run).
    lines = [ln.strip() for ln in Path(a.book).read_text().splitlines() if ln.strip()]
    lines = [" ".join(ln.split()[:4]) for ln in lines]
    need = (a.games // 2) + 1
    slice_ = lines[a.book_offset:a.book_offset + need]
    if len(slice_) < need:
        raise SystemExit(f"book slice too short: need {need}, have {len(slice_)}")
    out = Path(a.out_pgn)
    out.parent.mkdir(parents=True, exist_ok=True)
    slice_path = out.parent / "openings-slice.epd"
    slice_path.write_text("\n".join(slice_) + "\n", encoding="utf-8")

    cand = {"exe": a.exe, "flags": cand_flags}
    base = {"exe": a.exe, "flags": list(base_flags)}
    cmd = build_cmd(cand, base, a.games, str(out), str(slice_path), a.tc, a.concurrency)
    print(f"tc={a.tc} games={a.games} cand_flags={' '.join(a.cand_flag or []) or '(champion)'}")
    t0 = time.time()
    import subprocess
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr[-2000:])
        raise SystemExit(proc.returncode)
    text = out.read_text(encoding="utf-8", errors="replace")
    for bad in ("illegal move", "disconnect", "stall", "loses on time"):
        if bad in text.lower():
            print(f"WARNING: match log mentions {bad!r}")
    rows = M.pgn_to_results(text, "cand")
    run = sr.run_sprt((sr.score_from_result_row(r) for r in rows),
                      elo0=a.elo0, elo1=a.elo1, alpha=a.alpha, beta=a.beta)
    print(f"{run['games']}g  {run['wins']}-{run['losses']}-{run['draws']}  "
          f"llr {run['llr']:+.3f}  [{run['lowerBound']:.2f},{run['upperBound']:.2f}]  "
          f"{run['boundary']}  ({time.time()-t0:.0f}s)")
    if a.sprt:
        rec = sr.build_record(run, experiment_id=a.experiment_id or out.parent.name,
                              baseline_id="champion", candidate_id="champion+" + " ".join(a.cand_flag or []),
                              elo0=a.elo0, elo1=a.elo1, alpha=a.alpha, beta=a.beta,
                              provenance={"engineSha": M._sha16(a.exe),
                                          "netSha": M._sha16(M.N0_NET),
                                          "candArgs": cand_flags, "baseArgs": base_flags,
                                          "tc": a.tc, "threads": 1,
                                          "book": Path(a.book).name, "bookOffset": a.book_offset})
        rec_path = out.parent / "sprt.json"
        rec_path.write_text(json.dumps(rec, indent=2) + "\n", encoding="utf-8")
        print(f"record: {rec_path}  decision {rec['decision']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
