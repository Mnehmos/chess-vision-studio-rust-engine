"""Equal-clock screen for two frozen binaries; writes PGN, log, manifest and SPRT record."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import benchlib as B
from bench_acc_fusion import FLAGS, sha
from match_fixed_nodes import pgn_to_results, wld
import sprt_runner as S


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--net", required=True)
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--tc", default="5+0.05")
    ap.add_argument("--concurrency", type=int, default=2)
    ap.add_argument("--openings", default="F:/tools/openings.epd")
    ap.add_argument("--cutechess", default="F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe")
    args = ap.parse_args()
    if args.games < 2 or args.games % 2:
        ap.error("--games must be positive and even for paired openings")
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    paths = {name: out / name for name in ["match.pgn", "match.log", "match-plan.json", "match.jsonl", "match-sprt.json"]}
    if any(p.exists() for p in paths.values()):
        ap.error("output already exists; use a new directory")
    weights = ["--base", B.BASELINE["base_weights"], "--rung2", B.BASELINE["rung2_weights"],
               "--nnue", args.net, "--threads", "1"]
    cmd = [args.cutechess]
    binaries = {}
    for name, exe in [("candidate", args.candidate), ("baseline", args.baseline)]:
        exe = str(Path(exe).resolve())
        binaries[name] = {"path": exe, "sha256": sha(exe)}
        cmd += ["-engine", f"name={name}", f"cmd={exe}"] + [f"arg={x}" for x in weights + FLAGS]
    cmd += ["-each", "proto=uci", f"tc={args.tc}", "-games", str(args.games), "-repeat",
            "-concurrency", str(args.concurrency), "-openings", f"file={args.openings}", "format=epd", "order=sequential",
            "-draw", "movenumber=40", "movecount=8", "score=10", "-resign", "movecount=4", "score=900",
            "-maxmoves", "200", "-pgnout", str(paths["match.pgn"])]
    plan = {"experimentId": "llm-acc-fusion-20260908", "baselineSource": B.git(["rev-parse", "HEAD"]),
            "hypothesis": "Identical fixed-node decisions, faster accumulator updates, improved equal-clock play",
            "elo0": 0, "elo1": 20, "alpha": 0.05, "beta": 0.05,
            "gamesCap": args.games, "tc": args.tc, "threads": 1, "concurrency": args.concurrency,
            "binaries": binaries, "netSha256": sha(args.net), "openingsSha256": sha(args.openings),
            "baseWeightsSha256": sha(B.BASELINE["base_weights"]), "rung2WeightsSha256": sha(B.BASELINE["rung2_weights"]),
            "cargoLockSha256": sha("Cargo.lock"), "machine": B.machine_info(), "command": cmd}
    paths["match-plan.json"].write_text(json.dumps(plan, indent=2) + "\n")
    with paths["match.log"].open("w", encoding="utf8") as log:
        with subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as proc:
            for line in proc.stdout:
                log.write(line)
                log.flush()
                if "Score of" in line or "Warning" in line or "Error" in line:
                    print(line.strip(), flush=True)
            if proc.wait() != 0:
                raise RuntimeError(f"cutechess failed: see {paths['match.log']}")
    pgn = paths["match.pgn"].read_text(encoding="utf8")
    rows = pgn_to_results(pgn, "candidate")
    if len(rows) != args.games:
        raise RuntimeError(f"expected {args.games} completed games, found {len(rows)}")
    paths["match.jsonl"].write_text("".join(json.dumps(r) + "\n" for r in rows))
    run = S.run_sprt((S.score_from_result_row(r) for r in rows), elo0=0, elo1=20, alpha=0.05, beta=0.05)
    record = S.build_record(run, experiment_id=plan["experimentId"], baseline_id=plan["baselineSource"],
                            candidate_id="accumulator-copy-fusion", elo0=0, elo1=20, alpha=0.05, beta=0.05,
                            provenance={"engineSha": binaries["candidate"]["sha256"], "netSha": plan["netSha256"],
                                        "candArgs": weights + FLAGS, "baseArgs": weights + FLAGS,
                                        "tc": args.tc, "threads": 1, "openingsSha": plan["openingsSha256"]},
                            pgn_sha256=hashlib.sha256(paths["match.pgn"].read_bytes()).hexdigest())
    paths["match-sprt.json"].write_text(json.dumps(record, indent=2) + "\n")
    w, l, d = wld(rows)
    print(json.dumps({"wins": w, "losses": l, "draws": d, "scorePct": 100 * (w + d / 2) / len(rows),
                      "sprtDecision": record["decision"], "llr": record["llr"], "boundary": record["boundary"]}))


if __name__ == "__main__":
    main()
