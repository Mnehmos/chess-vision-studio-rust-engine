#!/bin/bash
# Seventh chain: settle the flags that are ON in the deployed champion but only have
# 2000-game HOLD evidence (or, for lmp, a historical negative note). High-power gates on
# book3 (4910 positions -> SE ~3.6 Elo at 9800 games, ~5.6 at 4000).
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/chess-vision-studio-rust-engine/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260910.epd
run() { # name cap
  local name="$1" cap="$2"
  local out="benchmarks/results/${name}-hp-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $name (record exists) ==="; return; }
  echo "=== gate $name cap=$cap $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "${name%-hp}" --batch 120 --cap "$cap" --concurrency 8
  echo "=== done $name $(date '+%H:%M:%S') ==="
}
# wait for the anchor benchmark to finish (it owns the CPU)
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.Name -eq 'python.exe' -and \$_.CommandLine -match 'bench_anchor' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 60
done
run lmp-regate 9800
run seeprune-regate 4000
run caphist-regate 4000
run improving-regate 4000
run tt2-regate 4000
run kingact-regate 4000
echo "=== chain7 complete $(date '+%H:%M:%S') ==="
