# Benchmark Suite — baseline `snapshot/gen7-acc-futility-2026-06-11`

> **Champion note (2026-07-01):** the frozen champion for the standing eval
> experiment is **N0 = gen9 `g9.current-default.raw-plus-residual`**, pinned in
> [`N0-identity.json`](N0-identity.json) (see `CLASSICAL_EVAL_EXPERIMENT.md`).
> The gen7 snapshot below remains the historical gate-ladder baseline these
> gate definitions were written against.

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
python benchmarks/scripts/bench_equal_time.py
python benchmarks/scripts/bench_forensic_time.py
python benchmarks/scripts/bench_tactical_sentinel.py
python benchmarks/scripts/bench_sentinel_suite.py
```

Use stable registry IDs, not ad hoc names. See `GENERATION_STANDARD.md` and
`ENGINE_STRENGTH_AUDIT.md`. The frozen clean-suite audit is in
`CLEAN_HOLDOUT_2026-06-19.md`; its first comparison is in
`CLEAN_BASELINE_2026-06-19.md`.

`suite-clean-postmodel-20260619` is built from whole Lichess games played after
the latest current-model artifact. `build_clean_holdout.py` scans every
candidate against the known Gen7-Gen9 corpora using the first four FEN fields,
caps correlated samples per game, and writes a reservation file consumed by
the Gen9 RSI importer. `label_holdout.py` freezes native Stockfish depth-24
MultiPV labels and danger classifications.

The exact Raw versus Hybrid A equal-time decision is recorded in
`EQUAL_TIME_HYBRID_A_2026-06-19.md`. Hybrid A failed the play-mode gate and is
retained for analysis only.

Specialist authority and the non-authoritative tactical sentinel gates are defined in
`SPECIALIST_AUTHORITY_STANDARD.md`.

The first forensic and clean-suite sentinel result is recorded in
`TACTICAL_SENTINEL_V1_2026-06-19.md`. It is a verification-request capability
screen, not a live promotion.

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
| 6 promotion | cutechess + SPRT | 80–100 screen → 200–400 confirm → SPRT (elo0=0, elo1=20, α=β=0.05). PROMOTE requires the SPRT **upper** bound crossed, recorded as a `schemas/sprt-result.schema.json` record and checked by `scripts/lint_promotion.py` (CI). Decisions: PROMOTE / REJECT / HOLD_FOR_MORE_DATA / ANALYSIS_MODE_ONLY / LIVE_DEV_ONLY — fixed-N positives screen only, never promote. **Never say "SPRT pass" unless the bound crossed** |
| 7 bot replay | `arena/bench-ponder-cache.py` (app repo) | opponent-clock systems on real transitions: hit rates, cached-used, verify-reject, avgCP hit/miss, bl100/200, flag risk, helper call/change/improve/regress counts. Must beat plain ponder baseline at same budget |

Telemetry helper: `bench_telemetry.py` aggregates pruning/search counters for a
candidate (`--extra "--rfp"` etc.) before a gate decision: RFP/null/LMP/SEE/delta
cut/attempt pairs, TT hit/cut rates, hash-move and first-move cutoffs, qnode
share, cutoff move index, and effective branching. Use `--base-exe` when
comparing against the current instrumented build; legacy binaries emit only
top-level counters and will be warned as incomplete telemetry rows.

## Fixed-node experiment tooling (2026-07-01)

The deterministic fixed-node control (`CLASSICAL_EVAL_EXPERIMENT.md` #6) runs a
search that stops at an exact node count, cold (fresh searcher, single thread) —
no clock noise, byte-reproducible. It is exposed three ways and consumed by four
tools:

- **Serve**: `{"cmd":"go","fen":...,"nodeBudget":N,"diagnosticIsolation":"cold"}`
  (tests: `tests/serve_diagnostic.rs`). **UCI**: `go nodes N`.
  **benchlib**: `engine_cfg(nodes=N)` + `Engine.search_nodes(...)`;
  `bench_cploss.py --nodes N` runs Gate 3 under the control.
- `scripts/sprt_runner.py` — the canonical SPRT statistics core (BayesElo
  trinomial LLR, sequential stop). Consumes a per-game JSONL stream, emits a
  `schemas/sprt-result.schema.json` record that passes `lint_promotion.py`
  (Gate 6 / INV-1). Tests: `scripts/test_sprt_runner.py`.
- `scripts/match_fixed_nodes.py` — the one-command experiment: cutechess-cli
  A/B match at `tc=inf nodes=N` (color-paired, adjudicated, both sides default
  to the N0 identity), emits candidate-POV JSONL, and with `--sprt` chains into
  the SPRT record. `--cand-nodes/--base-nodes` runs a node-budget SPRT (the
  P-SEARCH follow-up). Tests: `scripts/test_match_fixed_nodes.py`.
- `de/probe_p_search.py` — the P-SEARCH probe (issue #8): decision stability
  across node budgets, cold, per-position trajectories. First committed row:
  `results/20260701-195605-p-search-n0-fixed-node.json` (N0 eval, 92-position
  frozen suite, 10k→640k: moveChangeRate 0.64 — compute is unsaturated).
- `scripts/check_detector_coverage.py` — facts-side CI guard: fails when
  `data/motif-taxonomy.json` claims a detector the facts registry does not
  register (Phase 8 docs-vs-registry discipline).

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
`Decision: PROMOTE | REJECT | HOLD_FOR_MORE_DATA | ANALYSIS_MODE_ONLY | LIVE_DEV_ONLY`.
PROMOTE is valid only with a linked `schemas/sprt-result.schema.json` record whose
`boundary` is `upper`; `scripts/lint_promotion.py` enforces this in CI (INV-1, issue #5).
`ACCEPTED WITH NOTE` survives only in the historical records above — it is **not** a valid
decision for new reports.
