# Engine Strength Audit

Audit date: 2026-06-19.

## Executive Findings

The engine has a broad modern feature set. The immediate strength risk was
configuration drift, not a missing textbook feature.

The 2026-06-12 champion documentation says several measured negatives remain
off. The pre-audit `SearchOptions::default`, `analyze`, and `uci` enabled nearly
every option unless a `--no-*` flag was supplied. That included rejected or
shelved LMP, countermove, continuation history, rule-50 scaling, king activity,
capture history, two-bucket TT, and improving-informed LMR.

The 2026-06-19 correction makes accepted champion features the code default and
requires positive flags to opt experimental features in. The old all-on stack
remains reproducible as `legacy-all-on-2026-06-19`, but it is not promoted.
`champion-2026-06-12` remains the frozen control profile.

## Capability Checklist

| Area | Current capability | Audit status | Required gate |
|---|---|---|---|
| Legal move generation | Bitboards, make/unmake, perft coverage | Strong baseline | Perft and randomized make/unmake |
| Core search | Iterative deepening, PVS, aspiration windows | Present | Fixed-depth parity and decision quality |
| Tactical leaves | Capture/evasion qsearch, quiet checks, SEE/delta options | Present, default drift | Q-node share, tactical suite, blunder tail |
| Reductions/pruning | NMP, RFP, futility, LMR, LMP, SEE pruning | Mixed evidence | One variable per profile; same-budget games |
| Move ordering | TT, killers, history, counter/continuation/capture history | Mixed evidence | First-cut rate, cutoff index, games |
| TT | Lock-free shared TT, qsearch TT, two buckets | Two-bucket negative | Correctness, occupancy, CPU profile |
| SMP | Lazy and heterogeneous shared-TT helpers | Present | 1/2/4/8-thread scaling and strength |
| Evaluation | HCE, raw NNUE, CVS flat/residual/ranker | Multiple candidates | Non-leaked suite and paired games |
| NNUE inference | Incremental piece-square accumulator | Present | NPS and accumulator parity |
| Extensions | Checks, danger hooks, singular extension | Singular unproven | Tactical gain versus node cost |
| Endgames | Rule handling, optional Syzygy | Present, unstandardized | Probe correctness and conversion suite |
| Openings | Optional Polyglot | Present, unstandardized | Book-off engine tests |
| Time management | Hard/soft limits, ponder | Ponder promoted | Overshoot, flag rate, paired clocks |
| Training | Gen8/Gen9 pipelines, registry-bound CVS features, RSI | Provenance inconsistent | Frozen manifests and clean holdouts |

## Priority Work

### P0: Restore Experimental Control

1. Keep search profiles explicit in benchmark, app, UCI, gauntlet, and bot
   launch paths.
2. Re-gate individual experimental options against `champion-2026-06-12`.
3. Do not call a stack champion unless its exact profile is registered.
4. Require the startup identity response for new binaries and record whether
   historical binaries support it.

### P1: Rebuild A Clean Evaluation Ladder

1. Freeze non-leaked validation suites by source and game hash.
2. Re-run Gen7, Gen8-v2, Gen9 raw, flat, residual, Hybrid A, and Hybrid B under
   the same accepted search profile.
3. Separate main-eval tests from helper-ordering tests.
4. Require model and dataset provenance before promotion.

### P1: Benchmark Correctly

1. Fixed-depth parity for refactors.
2. Fixed-time depth and NPS for throughput and selectivity.
3. Stockfish child-score decision tests for tail risk.
4. Color-paired opening matches for strength.
5. SPRT only after screens and only with frozen artifacts.
6. Repeat speed tests and report dispersion.

### P2: Performance Profiling

1. Profile NNUE accumulator copy and update cost by model architecture.
2. Measure qsearch share and deepest qsearch outliers.
3. Test TT alignment only after a CPU profile shows cache cost.
4. Measure move generation, SEE, attack lookup, and ordering-key percentages.
5. Test PGO as an isolated build experiment.

### P2: Strength Experiments

Run independently:

- Singular extension off/on.
- SEE pruning off/on.
- Delta pruning off/on.
- Safe-check and threat-escape ordering bonuses.
- LMR table tuning instead of adding reductions.
- Hybrid helpers versus equal-budget raw depth.
- Syzygy probe policy and DTZ conversion behavior.

## 2026-06-19 Equal-Time Decision

Hybrid A's quiet-root residual ordering policy failed its mandatory same-budget
gate against the exact matrix-raw control. It produced no statistically
reliable gain from 25-2000 ms, no median gain, no depth gain, and one
catastrophic 100 ms tail regression. Hybrid A is analysis-only; matrix-raw is
the live candidate. See `EQUAL_TIME_HYBRID_A_2026-06-19.md`.

## 2026-06-19 Tactical Sentinel Gate

The completed-iteration audit confirmed that interrupted root work never
replaces the last completed result. Tactical Sentinel v1 then screened
candidate-specific verification requests without live authority. A 10 ms
policy caught the clean suite's 9316 cp tactical miss at 25-250 ms principal
budgets with zero false positives, but found no exact mate through five seconds
and missed a 309 cp endgame error. It advances only to a concurrent,
resource-matched prototype. See `TACTICAL_SENTINEL_V1_2026-06-19.md`.

## Known Methodology Risks

- `suite-fresh-100` leaked into Gen8-v2 training for 47 of 100 positions.
- Historical `all_gen_bench.txt` mixed binaries, models, helper roles, and
  search defaults.
- Several scripts hard-code `F:/tools`.
- Historical results sometimes use dirty commits.
- Short screens are regression detectors, not Elo proof.
- Model files do not consistently embed training commit, dataset hash, rows,
  or registry metadata.

## Promotion Gates

1. Identity and artifact validation.
2. Tests, perft, UCI and serve smoke, model validation.
3. Fixed-depth parity or explained differences.
4. Repeated speed and selectivity matrix.
5. Clean decision suites with no blunder-tail regression.
6. 20-game screen.
7. 100-game confirmation.
8. SPRT when warranted.
9. Bot replay and clock-risk validation.
10. Freeze artifacts, manifest, report, and rollback target.
