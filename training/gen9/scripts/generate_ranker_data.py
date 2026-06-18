#!/usr/bin/env python3
"""Generate CVS deltas dataset for Quiet-Hybrid-B Ranker training.

Extracts FENs from the training dataset, runs the Rust analyzer binary in
--cvs-deltas mode, and writes the JSON lines containing quiet move features
and scores to the output file.
"""

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description="Generate Quiet-Hybrid-B Ranker training data")
    parser.add_argument("--input", default="F:/tools/gen9-train.jsonl", help="Input jsonl file with FENs")
    parser.add_argument("--nnue", default="target-cvs/matrix-raw.json", help="Path to raw NNUE model")
    parser.add_argument("--engine", default="target/release/analyze.exe", help="Path to compiled engine analyze binary")
    parser.add_argument("--output", default="training/gen9/gen9-cvs-deltas.jsonl", help="Output jsonl file")
    parser.add_argument("--limit", type=int, default=100000, help="Maximum number of positions to process (0 for unlimited)")
    args = parser.parse_args()

    input_path = Path(args.input)
    if not input_path.exists():
        print(f"Error: input file {input_path} does not exist.")
        return

    nnue_path = Path(args.nnue)
    if not nnue_path.exists():
        print(f"Error: NNUE model {nnue_path} does not exist.")
        return

    engine_path = Path(args.engine)
    if not engine_path.exists():
        # Fall back to target-cvs/release/analyze.exe if it's there
        fallback = Path("target-cvs/release/analyze.exe")
        if fallback.exists():
            engine_path = fallback
        else:
            print(f"Error: engine binary {engine_path} does not exist. Please run cargo build --release --bin analyze")
            return

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    print(f"Reading FENs from {input_path}...")
    fens = []
    with input_path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
                if "fen" in row:
                    fens.append(row["fen"])
            except Exception:
                continue
            
            if args.limit > 0 and len(fens) >= args.limit:
                break

    print(f"Loaded {len(fens)} FENs. Writing to temp file for batch extraction...")
    
    with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".fens", encoding="utf-8") as tmp:
        tmp_name = tmp.name
        for fen in fens:
            tmp.write(fen + "\n")

    print(f"Running batch extraction via {engine_path} --cvs-deltas...")
    try:
        cmd = [
            str(engine_path),
            "--fens", tmp_name,
            "--cvs-deltas",
            "--nnue", str(nnue_path),
            "--depth", "1",
            "--allow-unverified-net"
        ]
        
        # We can stream stdout directly to the output file
        with output_path.open("w", encoding="utf-8", newline="\n") as out_f:
            proc = subprocess.run(
                cmd,
                stdout=out_f,
                stderr=subprocess.PIPE,
                text=True,
                check=True
            )
        
        print(f"Successfully generated {output_path}")
        
    except subprocess.CalledProcessError as e:
        print(f"Error: Engine extraction failed with code {e.returncode}")
        print(f"Stderr: {e.stderr}")
    finally:
        if os.path.exists(tmp_name):
            os.remove(tmp_name)


if __name__ == "__main__":
    main()
