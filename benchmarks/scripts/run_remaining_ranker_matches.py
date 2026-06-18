import subprocess
import os

matches = [
    {
        "name1": "Quiet-Hybrid-B",
        "args1": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-ranker.json --allow-unverified-net",
        "name2": "Quiet-Hybrid-A",
        "args2": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-residual.json --allow-unverified-net",
        "pgn": "f:/tools/match_quiet_hybrid_b_vs_hybrid_a_5.pgn"
    },
    {
        "name1": "Quiet-Hybrid-B",
        "args1": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-ranker.json --allow-unverified-net",
        "name2": "Zero-Bonus",
        "args2": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-ranker.json --allow-unverified-net --no-cvs-bonus",
        "pgn": "f:/tools/match_quiet_hybrid_b_vs_zerobonus_5.pgn"
    },
    {
        "name1": "Quiet-Hybrid-B",
        "args1": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-ranker.json --allow-unverified-net",
        "name2": "Shuffled-Geometry",
        "args2": "--nnue target-cvs/matrix-raw.json --helper-nnue target-cvs/matrix-ranker.json --allow-unverified-net --shuffled-geometry",
        "pgn": "f:/tools/match_quiet_hybrid_b_vs_shuffled_5.pgn"
    }
]

for m in matches:
    cmd = [
        "python", "benchmarks/scripts/run_match.py",
        "--name1", m["name1"],
        "--exe1", "target-deltas/release/uci.exe",
        "--args1", m["args1"],
        "--name2", m["name2"],
        "--exe2", "target-deltas/release/uci.exe",
        "--args2", m["args2"],
        "--games", "100",
        "--tc", "5+0.05",
        "--pgn", m["pgn"]
    ]
    print(f"Starting match: {m['name1']} vs {m['name2']} at 5+0.05")
    subprocess.run(cmd, check=True)
