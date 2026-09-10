#!/bin/bash
# Second continuous gate chain: the remaining untested default-off flags, run after
# gate_chain.sh finishes. Same fixed harness (independent positions, fail-closed book).
set -u
cd /f/Github/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/chess-vision-studio-rust-engine-gatefix/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260909b.epd
GATES="seeverify rootsafequiet"

# Wait for the first chain to finish.
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -match 'gate_ladder' -and \$_.Name -eq 'python.exe' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 60
done

for gate in $GATES; do
  out="benchmarks/results/${gate}-gate-20260909"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $gate (record exists) ==="; continue; }
  echo "=== gate $gate $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "$gate" --batch 120 --cap 2000 --concurrency 8
  echo "=== done $gate $(date '+%H:%M:%S') ==="
done
echo "=== chain2 complete $(date '+%H:%M:%S') ==="
