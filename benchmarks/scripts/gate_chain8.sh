#!/bin/bash
# Eighth chain: high-power re-gates of the flags that are ON in the champion but only have
# 2000-game HOLD evidence. book3 (4910 positions) at cap 4000 -> SE ~5.6 Elo (vs ~8 at the
# 2000-game gates), so a real +10 Elo flag can actually cross.
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/chess-vision-studio-rust-engine/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260910.epd

# wait for any in-flight gate (e.g. lmp-regate) — python processes only
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.Name -eq 'python.exe' -and \$_.CommandLine -match 'gate_ladder' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 60
done

for name in seeprune-regate caphist-regate improving-regate tt2-regate kingact-regate; do
  out="benchmarks/results/${name}-hp-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $name (record exists) ==="; continue; }
  echo "=== gate $name cap=4000 $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "$name" --batch 120 --cap 4000 --concurrency 8
  echo "=== done $name $(date '+%H:%M:%S') ==="
done
echo "=== chain8 complete $(date '+%H:%M:%S') ==="
