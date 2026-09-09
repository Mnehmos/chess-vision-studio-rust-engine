"""Paired, cold fixed-node parity/throughput gate for accumulator changes.

Alternates execution order; records raw responses and full artifact hashes.
Timing improvements are operational evidence, never a strength promotion.
"""
import argparse
import hashlib
import json
import platform
import statistics
import time
from pathlib import Path

import benchlib as B

ROOT = Path(__file__).resolve().parents[2]
FLAGS = ["--futility", "--rfp", "--tt-prune-store", "--qtt", "--histmalus", "--histlmr",
         "--no-lmp", "--no-seeprune", "--no-delta", "--no-countermove", "--no-conthist",
         "--no-rule50", "--no-caphist", "--no-tt2", "--no-improving", "--no-king-activity",
         "--no-singular", "--no-syzygy", "--no-book", "--cvs-helpers", "0"]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def semantic(value):
    if isinstance(value, dict):
        return {k: semantic(v) for k, v in value.items() if k not in {"timeMs", "nps"}}
    if isinstance(value, list):
        return [semantic(v) for v in value]
    return value


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--net", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--phase", choices=["parity", "speed"], default="parity")
    ap.add_argument("--nodes", type=int, default=80000)
    ap.add_argument("--repeats", type=int, default=1)
    args = ap.parse_args()
    canonical = json.loads((ROOT / "benchmarks/suites/canonical.json").read_text())["positions"]
    fresh = (ROOT / "benchmarks/suites/suite-fresh-100.txt").read_text().splitlines()
    fresh = [f for f in fresh if f.strip() and not f.startswith("#")]
    fens = [r["fen"] for r in canonical] + fresh
    if args.phase == "speed":
        fens = [r["fen"] for r in canonical[:6]] + fresh[:6]
    fens = list(dict.fromkeys(fens))
    configs = {name: B.engine_cfg(name, exe=exe, net=args.net, depth=64, threads=1, extra=FLAGS)
               for name, exe in [("baseline", args.baseline), ("candidate", args.candidate)]}
    engines = {name: B.Engine(cfg) for name, cfg in configs.items()}
    rows = []
    try:
        for engine in engines.values():
            engine.search_nodes(fens[0], 10000)  # excluded warmup; measured searches remain cold
        for repeat in range(args.repeats):
            for index, fen in enumerate(fens):
                row = {"repeat": repeat, "position": index, "fen": fen}
                order = ["baseline", "candidate"] if (repeat + index) % 2 == 0 else ["candidate", "baseline"]
                for name in order:
                    start = time.perf_counter()
                    reply = engines[name].search_nodes(fen, args.nodes)
                    row[name] = {"wallMs": (time.perf_counter() - start) * 1000, "response": reply}
                    if "error" in reply or "uci" not in reply:
                        raise RuntimeError(reply)
                row["exact"] = semantic(row["baseline"]["response"]) == semantic(row["candidate"]["response"])
                rows.append(row)
            print(f"repeat {repeat + 1}/{args.repeats}: {sum(r['exact'] for r in rows)}/{len(rows)} exact", flush=True)
    finally:
        for engine in engines.values():
            engine.close()
    totals = {name: sum(r[name]["response"]["timeMs"] for r in rows) for name in configs}
    repeat_ratios = []
    for repeat in range(args.repeats):
        subset = [r for r in rows if r["repeat"] == repeat]
        base = sum(r["baseline"]["response"]["timeMs"] for r in subset)
        cand = sum(r["candidate"]["response"]["timeMs"] for r in subset)
        repeat_ratios.append(base / cand if cand else None)
    report = {
        "phase": args.phase, "positions": len(fens), "nodesPerPosition": args.nodes,
        "repeats": args.repeats, "exact": sum(r["exact"] for r in rows), "comparisons": len(rows),
        "searchTimeMs": totals, "throughputRatio": totals["baseline"] / totals["candidate"],
        "repeatThroughputRatios": repeat_ratios,
        "medianRepeatThroughputRatio": statistics.median(r for r in repeat_ratios if r is not None),
        "identity": {"gitHead": B.git(["rev-parse", "HEAD"]), "machine": platform.platform(),
                     "flags": FLAGS, "threads": 1, "net": args.net, "netSha256": sha(args.net),
                     "binaries": {k: {"path": v["exe"], "sha256": sha(v["exe"])} for k, v in configs.items()},
                     "suiteSha256": hashlib.sha256("\n".join(fens).encode()).hexdigest()},
        "decision": "RUNTIME_SCREEN_ONLY" if all(r["exact"] for r in rows) else "REJECT_PARITY",
        "rows": rows,
    }
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k not in {"rows", "identity"}}, indent=2))
    return 0 if all(r["exact"] for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
