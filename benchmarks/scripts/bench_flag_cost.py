#!/usr/bin/env python3
"""Throughput cost of the promoted INV-1 flags: nodes/second with the flag on vs off.

The live bot plays on the clock, so a flag that is strength-neutral at fixed nodes but
costs nodes/second is a net loss at equal time. The fixed-node gates measure strength at
equal nodes; this measures the other half of the trade.

Caveat: NPS is only comparable between configs that search the same way. A flag that
changes pruning changes the search shape (and therefore the node mix), so a NPS delta is
not a strength claim either way — use the fixed-node SPRT for strength and an equal-time
match for the combined effect. What this script *is* good for is catching a flag that
costs time for nothing, and it refuses to report without a warmup and a champion control
at both ends (an early no-warmup sweep showed a spurious +15-26% for every flag).

Runs `analyze --serve` (one thread, movetime control) over the canonical ladder positions
and reports median MNPS per configuration plus the ratio against the champion.

Usage:
  python bench_flag_cost.py --exe target/release/analyze.exe --net target-cvs/matrix-raw.json \
      [--helper target-cvs/matrix-residual.json] [--ms 500] [--repeats 3] [--positions 8]
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import benchlib as B  # noqa: E402

PROMOTED = {
    "caphist": "--no-caphist",
    "seeprune": "--no-seeprune",
    "improving": "--no-improving",
    "tt2": "--no-tt2",
    "kingact": "--no-king-activity",
}


def measure(exe: str, net: str, helper: str | None, extra: list[str], positions: list[dict],
            ms: int, repeats: int, warmup: int = 2) -> list[float]:
    cfg = B.engine_cfg("cfg", exe=exe, net=net,
                       extra=(["--helper-nnue", helper] if helper else []) + extra)
    engine = B.Engine(cfg)
    try:
        for _ in range(warmup):  # warm the net / page cache before timing
            for p in positions:
                engine.search_time(p["fen"], ms)
        runs = []
        for _ in range(repeats):
            nodes = elapsed = 0
            for p in positions:
                res = engine.search_time(p["fen"], ms)
                nodes += int(res.get("nodes", 0))
                elapsed += max(1, int(res.get("timeMs", ms)))
            runs.append(nodes / elapsed / 1000.0)
        return runs
    finally:
        engine.close()


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", required=True)
    ap.add_argument("--net", required=True)
    ap.add_argument("--helper", default=None)
    ap.add_argument("--ms", type=int, default=500)
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--positions", type=int, default=8)
    ap.add_argument("--out", default=None)
    args = ap.parse_args(argv)

    canon = json.loads((Path(B.SUITES) / "canonical.json").read_text())
    by_name = {p["name"]: p for p in canon["positions"]}
    subset = canon.get("ladder_subset") or list(by_name)
    positions = [by_name[n] for n in subset][:args.positions]

    results: dict[str, dict] = {}
    runs = measure(args.exe, args.net, args.helper, [], positions, args.ms, args.repeats)
    results["champion"] = {"medianMnps": statistics.median(runs), "runs": runs}
    champion = results["champion"]["medianMnps"]
    for name, off in PROMOTED.items():
        runs = measure(args.exe, args.net, args.helper, [off], positions, args.ms, args.repeats)
        results[f"no-{name}"] = {"medianMnps": statistics.median(runs), "runs": runs,
                                 "pctVsChampion": 100.0 * (statistics.median(runs) / champion - 1.0)}
    # Champion control at the end: a per-flag delta is only credible if this reproduces
    # the opening champion run (guards against drift/contention during the sweep).
    runs = measure(args.exe, args.net, args.helper, [], positions, args.ms, args.repeats)
    results["champion-control"] = {"medianMnps": statistics.median(runs), "runs": runs,
                                   "pctVsChampion": 100.0 * (statistics.median(runs) / champion - 1.0)}
    print(f"{'config':>16s} {'median MNPS':>12s} {'vs champion':>12s}")
    for name, r in results.items():
        delta = "" if "pctVsChampion" not in r else f"{r['pctVsChampion']:+.2f}%"
        print(f"{name:>16s} {r['medianMnps']:>12.2f} {delta:>12s}")
    if args.out:
        Path(args.out).write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
