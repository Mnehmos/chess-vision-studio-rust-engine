#!/bin/bash
# Fifth chain: the standard-engine techniques this search was missing (razoring, ProbCut,
# recapture extension), each behind its own flag on the feature/std-techniques build.
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix/target-std/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260910.epd
# wait for any in-flight gate (chains 3/4) to finish
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -match 'gate_ladder' -and \$_.Name -eq 'python.exe' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 60
done
for gate in razoring probcut recapture; do
  out="benchmarks/results/${gate}-gate-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $gate (record exists) ==="; continue; }
  echo "=== gate $gate $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "$gate" --batch 120 --cap 2000 --concurrency 8
  echo "=== done $gate $(date '+%H:%M:%S') ==="
done
echo "=== chain5 complete $(date '+%H:%M:%S') ==="
