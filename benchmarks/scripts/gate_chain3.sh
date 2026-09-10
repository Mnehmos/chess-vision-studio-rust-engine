#!/bin/bash
# Third gate chain: tune the log-LMR divisor (the promoted loglmr's one free constant).
# Each gate is champion (div 2.25) vs champion with one divisor, fresh book positions.
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260909b.epd
GATES="lmrdiv20 lmrdiv25 lmrdiv175 lmrdiv275"
for gate in $GATES; do
  out="benchmarks/results/${gate}-gate-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $gate (record exists) ==="; continue; }
  echo "=== gate $gate $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" --book-offset 1200 \
    --gate "$gate" --batch 120 --cap 2000 --concurrency 8
  echo "=== done $gate $(date '+%H:%M:%S') ==="
done
echo "=== chain3 complete $(date '+%H:%M:%S') ==="
