#!/bin/bash
# Continuous INV-1 gate chain: run the remaining candidates back-to-back on the fixed
# harness (independent positions, fail-closed book). Each gate is a single variable and
# writes its own record; a crossing is promoted by hand after review.
#
# Order: validate the unproven promoted defaults first (a lower crossing means the flag
# HURTS and should be removed = a real upgrade), then the untested candidates.
set -u
cd /f/Github/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/chess-vision-studio-rust-engine-gatefix/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260909b.epd
GATES="seeprune-regate caphist-regate iid improving-regate tt2-regate kingact-regate delta conthist"

# Wait for any in-flight gate (e.g. countermove) to finish before starting the chain.
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -match 'gate_ladder' -and \$_.Name -eq 'python.exe' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 30
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
echo "=== chain complete $(date '+%H:%M:%S') ==="
