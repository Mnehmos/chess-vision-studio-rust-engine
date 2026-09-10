#!/bin/bash
# Sixth chain: resumes the interrupted queue at concurrency 6 (leaving cores for the live
# bot's 4+2+2 threads), and finishes the standard-technique + small-effect program.
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix/target-std/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260910.epd
run() { # name cap
  local name="$1" cap="$2"
  local out="benchmarks/results/${name}-gate-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $name (record exists) ==="; return; }
  echo "=== gate $name cap=$cap $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "${name%-big}" --batch 120 --cap "$cap" --concurrency 6
  echo "=== done $name $(date '+%H:%M:%S') ==="
}
run razoring 2000
run probcut 2000
run recapture 2000
run smallpos-bundle 4000
run delta-big 9800
run conthist-big 9800
echo "=== chain6 complete $(date '+%H:%M:%S') ==="
