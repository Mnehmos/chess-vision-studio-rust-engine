#!/usr/bin/env python3
"""Build a performance-only record (INV-2) from measured parity/speed artifacts.

Consumes the JSON written by `bench_acc_fusion.py` (or any producer emitting the same
shape: `exact`, `comparisons`, `searchTimeMs`, `repeatThroughputRatios`, `identity`) plus
the measured test counts and resource deltas, and emits a record matching
`benchmarks/schemas/perf-result.schema.json`.

The record is only as honest as its inputs: this script does no rounding that would move a
number across an INV-2 threshold, and it never sets `strengthClaim`.

Usage:
  python make_perf_record.py --parity P.json --speed S.json \
      --experiment-id ID --baseline-id BASE --candidate-id CAND \
      --tests-passed N --tests-failed N --tests-command "cargo test ..." \
      --peak-rss-delta-pct X --binary-bytes-delta-pct Y \
      [--screen W,L,D] [--decision accept_performance] --out R.json
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from lint_promotion import sign_test_p  # noqa: E402  (single source of truth for the gate)


def pct(ratio: float) -> float:
    return round((ratio - 1.0) * 100.0, 4)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--parity", required=True)
    ap.add_argument("--speed", required=True)
    ap.add_argument("--experiment-id", required=True)
    ap.add_argument("--baseline-id", required=True)
    ap.add_argument("--candidate-id", required=True)
    ap.add_argument("--tests-passed", type=int, required=True)
    ap.add_argument("--tests-failed", type=int, required=True)
    ap.add_argument("--tests-command", required=True)
    ap.add_argument("--peak-rss-delta-pct", type=float, required=True)
    ap.add_argument("--binary-bytes-delta-pct", type=float, required=True)
    ap.add_argument("--screen", help="W,L,D from an optional equal-clock screen")
    ap.add_argument("--screen-sprt", default=None, help="path to the screen's SPRT record")
    ap.add_argument("--decision", default="accept_performance")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    par = json.loads(Path(args.parity).read_text(encoding="utf-8"))
    sp = json.loads(Path(args.speed).read_text(encoding="utf-8"))
    ratios = sp["repeatThroughputRatios"]
    ident = sp["identity"]

    rec = {
        "schemaVersion": 1,
        "changeClass": "performance-only",
        "experimentId": args.experiment_id,
        "baselineId": args.baseline_id,
        "candidateId": args.candidate_id,
        "parity": {
            "searches": par["comparisons"],
            "identicalSearches": par["exact"],
            "nodeCeiling": par["nodesPerPosition"],
            "excludedFields": ["timeMs", "nps"],
            "artifact": Path(args.parity).as_posix(),
        },
        "speed": {
            "positions": sp["positions"],
            "repeats": sp["repeats"],
            "nodeCeiling": sp["nodesPerPosition"],
            "baselineMs": sp["searchTimeMs"]["baseline"],
            "candidateMs": sp["searchTimeMs"]["candidate"],
            "aggregateSpeedupPct": pct(sp["searchTimeMs"]["baseline"] / sp["searchTimeMs"]["candidate"]),
            "medianSpeedupPct": pct(statistics.median(ratios)),
            "minSpeedupPct": pct(min(ratios)),
            "maxSpeedupPct": pct(max(ratios)),
            "repeatsPositive": sum(1 for r in ratios if r > 1.0),
            "repeatsTotal": len(ratios),
            "signTestP": sign_test_p(sum(1 for r in ratios if r > 1.0), len(ratios)),
            "artifact": Path(args.speed).as_posix(),
        },
        "tests": {
            "passed": args.tests_passed,
            "failed": args.tests_failed,
            "command": args.tests_command,
        },
        "resources": {
            "peakRssDeltaPct": args.peak_rss_delta_pct,
            "binaryBytesDeltaPct": args.binary_bytes_delta_pct,
        },
        "strengthClaim": False,
        "decision": args.decision,
        "provenance": {
            "engineSha": ident["binaries"]["candidate"]["sha256"],
            "netSha": ident["netSha256"],
            "candArgs": " ".join(ident["flags"]),
            "baseArgs": " ".join(ident["flags"]),
            "threads": ident["threads"],
            "host": ident["machine"],
            "toolchain": ident.get("toolchain", "cargo release --locked"),
        },
    }
    if args.screen:
        w, l, d = (int(x) for x in args.screen.split(","))
        rec["screen"] = {"games": w + l + d, "wins": w, "losses": l, "draws": d,
                         "sprtRecord": args.screen_sprt}
    Path(args.out).write_text(json.dumps(rec, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(rec["speed"], indent=2))
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
