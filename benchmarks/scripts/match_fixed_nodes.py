#!/usr/bin/env python3
"""Fixed-node A/B match driver (the P-SEARCH follow-up slice; CLASSICAL_EVAL_EXPERIMENT.md
Phase 6). Plays candidate vs baseline with cutechess-cli at a NODE budget per move (tc=inf +
nodes=N: deterministic effort, no clock noise — the match analogue of the #6 fixed-node
diagnostic control), then emits the per-game JSONL stream sprt_runner.py consumes and,
with --sprt, chains straight into the canonical SPRT record (INV-1 promotion evidence).

One canonical experiment command:

  python benchmarks/scripts/match_fixed_nodes.py \
      --cand-net CAND.json --base-net N0.json --nodes 40000 --games 200 \
      --sprt --experiment-id exp42 --candidate-id cand/h1 --baseline-id snapshot/N0 \
      --out-record exp42-sprt.json

Division of labour (house pattern, mirrors probe_p_search / sprt_runner):
  * pure, unit-tested pieces: cutechess argv composition and PGN -> candidate-POV result rows;
  * the CLI wires in the real cutechess-cli subprocess and file I/O.

Both sides default to THIS repo's uci.exe with the frozen N0 weights (candidate overrides the
net/flags/nodes it is testing), so an eval candidate changes exactly one thing. Colors are
paired (-repeat: each opening played twice with colors swapped).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path


def _sha16(path) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()[:16]

CUTECHESS = "F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe"
OPENINGS = "F:/tools/openings.epd"
REPO = Path(__file__).resolve().parents[2]
UCI = str(REPO / "target/release/uci.exe")

# The frozen N0 identity (benchmarks/N0-identity.json). Defaults for BOTH sides so a
# candidate run varies exactly the thing under test.
N0_NET = str(REPO / "target-cvs/matrix-raw.json")
N0_HELPER = str(REPO / "target-cvs/matrix-residual.json")
N0_BASE_W = "f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json"
N0_RUNG2_W = "f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json"
N0_FLAGS = ["--futility", "--rfp", "--tt-prune-store", "--qtt", "--histmalus", "--histlmr", "--lmp"]


# ── pure: cutechess argv composition ─────────────────────────────────────────


def engine_args(name: str, exe: str, net: str, helper: str | None, flags: list[str],
                nodes: int) -> list[str]:
    """One -engine block. nodes= is the per-move node budget (with tc=inf it is the only
    stop condition — the fixed-node control)."""
    args = ["-engine", f"name={name}", f"cmd={exe}"]
    for a in (["--base", N0_BASE_W, "--rung2", N0_RUNG2_W, "--nnue", net]
              + (["--helper-nnue", helper] if helper else [])
              + flags):
        args.append(f"arg={a}")
    args.append(f"nodes={nodes}")
    return args


def build_cutechess_cmd(cand: dict, base: dict, games: int, pgnout: str,
                        openings: str = OPENINGS, concurrency: int = 1,
                        maxmoves: int = 200, cutechess: str = CUTECHESS) -> list[str]:
    """Full cutechess-cli argv. -repeat pairs colors; draw/resign adjudication bounds
    runaway games; sequential openings keep the pairing reproducible."""
    return (
        [cutechess]
        + engine_args(cand["name"], cand["exe"], cand["net"], cand.get("helper"),
                      cand.get("flags", []), cand["nodes"])
        + engine_args(base["name"], base["exe"], base["net"], base.get("helper"),
                      base.get("flags", []), base["nodes"])
        + ["-each", "proto=uci", "tc=inf",
           "-games", str(games), "-repeat",
           "-concurrency", str(concurrency),
           "-openings", f"file={openings}", "format=epd", "order=sequential",
           "-draw", "movenumber=40", "movecount=8", "score=10",
           "-resign", "movecount=4", "score=900",
           "-maxmoves", str(maxmoves),
           "-pgnout", pgnout]
    )


# ── pure: PGN -> candidate-POV result rows ───────────────────────────────────

_TAG = re.compile(r'^\[(\w+)\s+"([^"]*)"\]')


def pgn_to_results(pgn_text: str, candidate_name: str) -> list[dict]:
    """Parse a cutechess PGN into sprt_runner rows: {"result", "candidateColor"} per finished
    game, in file order. Unfinished games (Result "*") are skipped. Raises if the candidate
    name appears on neither side of a game (a mislabeled match must not silently count)."""
    rows: list[dict] = []
    white = black = result = None
    def flush():
        nonlocal white, black, result
        if result is None:
            return
        if result != "*":
            if candidate_name == white:
                color = "white"
            elif candidate_name == black:
                color = "black"
            else:
                raise ValueError(f"candidate {candidate_name!r} not in game {white!r} vs {black!r}")
            rows.append({"result": result, "candidateColor": color})
        white = black = result = None

    for line in pgn_text.splitlines():
        m = _TAG.match(line.strip())
        if not m:
            continue
        key, value = m.group(1), m.group(2)
        if key == "White":
            # a new game's tag block begins; emit the previous game
            if white is not None or result is not None:
                flush()
            white = value
        elif key == "Black":
            black = value
        elif key == "Result":
            result = value
    flush()
    return rows


def wld(rows: list[dict]) -> tuple[int, int, int]:
    """Candidate-POV wins/losses/draws for a quick summary line."""
    w = l = d = 0
    for r in rows:
        if r["result"] == "1/2-1/2":
            d += 1
        elif (r["result"] == "1-0") == (r["candidateColor"] == "white"):
            w += 1
        else:
            l += 1
    return w, l, d


# ── real runner ──────────────────────────────────────────────────────────────


def main(argv) -> int:
    ap = argparse.ArgumentParser(description="Fixed-node A/B match -> JSONL (-> SPRT record).")
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--nodes", type=int, default=40000, help="per-move node budget (both sides)")
    ap.add_argument("--cand-nodes", type=int, default=None, help="candidate budget override (node-budget SPRT)")
    ap.add_argument("--base-nodes", type=int, default=None)
    ap.add_argument("--cand-net", default=N0_NET)
    ap.add_argument("--base-net", default=N0_NET)
    ap.add_argument("--cand-flag", action="append", default=None, help="candidate flag (repeatable; replaces N0 flags)")
    ap.add_argument("--exe", default=UCI)
    ap.add_argument("--openings", default=OPENINGS)
    ap.add_argument("--concurrency", type=int, default=1)
    ap.add_argument("--pgnout", default=None)
    ap.add_argument("--out-jsonl", default=None)
    # SPRT chaining (the canonical one-command experiment)
    ap.add_argument("--sprt", action="store_true")
    ap.add_argument("--elo0", type=float, default=0.0)
    ap.add_argument("--elo1", type=float, default=5.0)
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--experiment-id", default="adhoc")
    ap.add_argument("--candidate-id", default="cand")
    ap.add_argument("--baseline-id", default="snapshot/N0")
    ap.add_argument("--out-record", default=None)
    args = ap.parse_args(argv)

    cand = {"name": "cand", "exe": args.exe, "net": args.cand_net, "helper": N0_HELPER,
            "flags": args.cand_flag if args.cand_flag is not None else list(N0_FLAGS),
            "nodes": args.cand_nodes or args.nodes}
    base = {"name": "base", "exe": args.exe, "net": args.base_net, "helper": N0_HELPER,
            "flags": list(N0_FLAGS), "nodes": args.base_nodes or args.nodes}

    pgnout = args.pgnout or f"match-{args.experiment_id}.pgn"
    cmd = build_cutechess_cmd(cand, base, args.games, pgnout,
                              openings=args.openings, concurrency=args.concurrency)
    print("running:", " ".join(cmd), file=sys.stderr)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    tail = "\n".join(proc.stdout.splitlines()[-6:])
    print(tail, file=sys.stderr)
    if proc.returncode != 0:
        print(proc.stderr[-2000:], file=sys.stderr)
        return proc.returncode

    rows = pgn_to_results(Path(pgnout).read_text(encoding="utf-8", errors="replace"), "cand")
    w, l, d = wld(rows)
    print(f"candidate W-L-D {w}-{l}-{d} over {len(rows)} finished games", file=sys.stderr)
    out_jsonl = args.out_jsonl or f"match-{args.experiment_id}.jsonl"
    Path(out_jsonl).write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")
    print(out_jsonl)

    if args.sprt:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        import sprt_runner as sr

        run = sr.run_sprt((sr.score_from_result_row(r) for r in rows),
                          elo0=args.elo0, elo1=args.elo1, alpha=args.alpha, beta=args.beta)
        provenance = {
            "engineSha": _sha16(args.exe),
            "netSha": _sha16(cand["net"]),
            "candArgs": cand["flags"] + [f"nodes={cand['nodes']}", f"net={Path(cand['net']).name}"],
            "baseArgs": base["flags"] + [f"nodes={base['nodes']}", f"net={Path(base['net']).name}"],
            "tc": f"nodes:{cand['nodes']}" if cand["nodes"] == base["nodes"]
                  else f"nodes:{cand['nodes']}vs{base['nodes']}",
            "nodes": cand["nodes"],
            "threads": 1,
            "openingsSha": _sha16(args.openings),
        }
        rec = sr.build_record(run, experiment_id=args.experiment_id,
                              baseline_id=args.baseline_id, candidate_id=args.candidate_id,
                              elo0=args.elo0, elo1=args.elo1, alpha=args.alpha, beta=args.beta,
                              provenance=provenance,
                              pgn_sha256=hashlib.sha256(Path(pgnout).read_bytes()).hexdigest())
        text = json.dumps(rec, indent=2)
        if args.out_record:
            Path(args.out_record).write_text(text + "\n", encoding="utf-8")
        print(text)
        print(f"SPRT {rec['decision']} (boundary={rec['boundary']}) LLR {rec['llr']:.3f}",
              file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
