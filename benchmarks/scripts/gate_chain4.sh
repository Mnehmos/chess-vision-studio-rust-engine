#!/bin/bash
# Fourth chain: high-power gates for the small-but-positive effects the 2000-game gates could
# not resolve. Book3 (4910 positions) supports ~9800 games -> SE ~3.6 Elo (was ~8).
#   1. smallpos-bundle (4000 games): is delta+conthist together worth anything?
#   2. delta-big / conthist-big (9800 games each): attribute it.
set -u
cd /f/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix || exit 1
EXE=F:/Github/_worktrees/chess-vision-studio-rust-engine/chess-vision-studio-rust-engine-gatefix/target/release/uci.exe
BOOK=benchmarks/suites/openings-inv1-20260910.epd
run() { # name cap
  local name="$1" cap="$2"
  local out="benchmarks/results/${name}-gate-20260910"
  [ -f "$out/ladder.jsonl" ] && { echo "=== skip $name (record exists) ==="; return; }
  echo "=== gate $name cap=$cap $(date '+%H:%M:%S') ==="
  python benchmarks/scripts/gate_ladder.py \
    --out-dir "$out" --exe "$EXE" --book "$BOOK" \
    --gate "${name%-big}" --batch 120 --cap "$cap" --concurrency 8
  echo "=== done $name $(date '+%H:%M:%S') ==="
}
# wait for any in-flight chain
while true; do
  running=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -match 'gate_ladder' -and \$_.Name -eq 'python.exe' } | Measure-Object).Count" | tr -d ' \r')
  [ "$running" = "0" ] && break
  sleep 60
done
run smallpos-bundle 4000
run delta-big 9800
run conthist-big 9800
echo "=== chain4 complete $(date '+%H:%M:%S') ==="
