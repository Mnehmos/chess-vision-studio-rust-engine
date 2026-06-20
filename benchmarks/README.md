# Benchmark Suite — baseline `snapshot/gen7-acc-futility-2026-06-11`

## Standard Entry Point

Cross-generation work starts from `engines.json`:

```bash
python benchmarks/scripts/bench_generations.py list
python benchmarks/scripts/bench_generations.py validate
python benchmarks/scripts/bench_generations.py smoke --depth 2
python benchmarks/scripts/bench_generations.py speed --times 50,250 --threads 1 --repeats 3
python benchmarks/scripts/bench_generations.py decision --limit 20 --decision-depth 8
python benchmarks/scripts/test_bench_registry.py
python benchmarks/scripts/build_clean_holdout.py
python benchmarks/scripts/label_holdout.py
```

Use stable registry IDs, not ad hoc names. See `GENERATION_STANDARD.md` and
`ENGINE_STRENGTH_AUDIT.md`. The frozen clean-suite audit is in
`CLEAN_HOLDOUT_2026-06-19.md`.

`suite-clean-postmodel-20260619` is built from whole Lichess games played after
the latest current-model artifact. `build_clean_holdout.py` scans every
candidate against the known Gen7-Gen9 corpora using the first four FEN fields,
caps correlated samples per game, and writes a reservation file consumed by
the Gen9 RSI importer. `label_holdout.py` freezes native Stockfish depth-24
MultiPV labels and danger classifications.

Live-dev discipline, not a laboratory paper: catch regressions fast, preserve
provenance, compare every change against the same clean baseline.

## Core rule

Every future change must answer:

> **Did this beat `snapshot/gen7-acc-futility-2026-06-11` under the same budget?**

**One variable per gate. Never bundle.** Allowed: RFP on/off, SEE-prune on/off,
LMP on/off, null-move tune, TT replacement, move ordering, qsearch, ponder
width 3v5, helper H1 on/off, gen8-net vs gen7-net. Disallowed: eval+pruning,
helper+clock, multiple prunings at once, model+search-refactor, anything
"all together".

Every clever idea must beat the boring control: **same budget spent on plain
raw gen7 depth.** If it cannot, it belongs in analysis mode, not play mode.

## The baseline

| artifact | file | sha256[:16] |
|---|---|---|
| match/UCI engine | `f:/tools/cvs-baselines/uci-gen7-acc-futility.exe` | `d5233c07` |
| serve/bot engine | `f:/tools/cvs-baselines/analyze-gen7-acc-futility.exe` | `3376c2ac` |
| eval net | `f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json` | `4cc9765c` |

git tag `snapshot/gen7-acc-futility-2026-06-11`, commit `f07caae`. Stack: gen7
champion eval, NNUE accumulator, MoveList refactor, futility pruning
(ACCEPTED-WITH-NOTE — runs by flag, so **every result must record futility
ON/OFF**), ponder (formal SPRT +91). Rollbacks: `uci-gen7-ponder.exe`
(`cb28a544`), gen6 `uci-gen6-full.exe` (`59ead1ff`).

## Gates (scripts/)

| gate | script | purpose / pass condition |
|---|---|---|
| 0 identity | `bench_identity.py` | boots, net loads loudly, full provenance record (commit, shas, flags, threads, suite hash, date, machine) |
| 1 parity | `bench_parity.py` | fixed-depth canonical positions. Speed patches: same move/score/nodes/PV, time not worse. Pruning patches: differences allowed → Gate 3 |
| 2 ladder | `bench_ladder.py` | movetime 5ms–5s × 1/4/8T. Speed: NPS/time-to-depth up. Pruning: depth up at equal movetime. NPS-similar-but-depth-lags ⇒ inspect selectivity/ordering/TT/qsearch |
| 3 cp-loss | `bench_cploss.py` | suites dev/fresh/hard-100, SF-d24 child scoring by default. avgCP/median/p90/p95/bl100/**bl200 (must not increase)**/danger (must not regress). Exact-match alone is NOT enough — gen7's win was calibration, not match% |
| 4 same-budget | (methodology) | mandatory for helpers/split-eval/lanes/arbiter/smart-clock/ponder changes: compare vs plain gen7 search at the SAME time/threads/openings. No runtime promotion unless it beats same-budget raw depth; otherwise analysis mode only |
| 5 screen | `bench_screen.py` | 20 games 5+0.05. ≥60% green / 50–60 neutral / 40–50 suspicious (read PGNs) / <40 red. Pruning: min 10/20, preferred 11.5/20, red flag 8/20. **Never quote Elo from 20 games** |
| 6 promotion | cutechess + SPRT | 80–100 screen → 200–400 confirm → SPRT (elo0=0, elo1=20, α=β=0.05) when warranted. Labels: formal SPRT pass / fixed-N positive / accepted with note / rejected / inconclusive. **Never say "SPRT pass" unless the bound crossed** |
| 7 bot replay | `arena/bench-ponder-cache.py` (app repo) | opponent-clock systems on real transitions: hit rates, cached-used, verify-reject, avgCP hit/miss, bl100/200, flag risk, helper call/change/improve/regress counts. Must beat plain ponder baseline at same budget |

Telemetry helper: `bench_telemetry.py` aggregates pruning/search counters for a
candidate (`--extra "--rfp"` etc.) before a gate decision: RFP/null/LMP/SEE/delta
cut/attempt pairs, TT hit/cut rates, hash-move and first-move cutoffs, qnode
share, cutoff move index, and effective branching. Use `--base-exe` when
comparing against the current instrumented build; legacy binaries emit only
top-level counters and will be warned as incomplete telemetry rows.

Suite builder: `build_hard_suite.py` mines `suite-hard-100` = positions where
the snapshot itself loses ≥50cp vs SF — "hard" defined relative to the baseline.

## Suites (suites/)

`canonical.json` (Gate 1/2 positions incl. kiwipete, midgame-r1, preBd6-danger,
549-d4d5-blunder, castling/ep/promotion), `suite-dev-100`, `suite-fresh-100`
(82/18 danger/quiet, saved SF-d14 oracle moves), `suite-hard-100` (mined).
`SHA256SUMS` pins them; results embed the suite hash.

## Play mode vs analysis mode

Play mode (strongest runtime path): gen7 main, plain depth, ponder cache,
futility, future gated selectivity. Analysis mode: CVS helpers, lane fanout,
arbiter reports, Control Lens, candidate-diversity, SF comparison, What
Changed, danger explanations.

> Raw depth owns the clock. CVS helpers rent unused time. Gen7 remains judge.

## Known decisions (recorded)

- **gen7**: champion hot-path eval; formally beat gen6 (+101.9 SPRT); wins by
  calibration / catastrophic-blunder reduction, not exact-match.
- **CVS-NNUE**: did not beat raw-v3 as hot-path eval; analysis/diversity only.
- **Split eval H1**: raw-v3 8T control scored 55.6% over 80 games vs
  raw+CVS-H1; H1 rejected for play mode.
- **Lanes/arbiter**: better candidate recall and fixed-depth cp-loss, but lost
  to same-budget plain deeper search at the bot layer → analysis tools.
- **Futility**: accepted with note (fixed-N +34, NOT formal SPRT; was −188 on
  gen6/classical). Fixed-depth sanity ON vs OFF identical (avgCP 10.3/10.2,
  bl200 0% both): gain is pure depth, not traded judgment.
- **RFP-v2** (depth≤4): ACCEPTED WITH NOTE (user ruling 2026-06-11 — standard
  engine practice, non-degrading). On record: formal SPRT bound crossing
  (+68.8, llr 2.97, 220g @10+0.1) and 100-game GREEN screen (63.5% @5+0.05)
  that resolved a suspicious 45% 20-game start as sampling noise (PGNs clean,
  build parity 0-diff). Live on the bot via CVS_RUST_RFP=1.

## Candidate change report

Copy `results/TEMPLATE.md`. Every report ends with exactly one of:
`Decision: PROMOTE | REJECT | ACCEPTED WITH NOTE | HOLD FOR MORE DATA |
LIVE-DEV ONLY | ANALYSIS MODE ONLY`.
