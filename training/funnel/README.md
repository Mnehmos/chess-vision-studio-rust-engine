# Information-gain labeling funnel (#111)

Cheap computation decides where expensive computation is spent. Instead of running Stockfish
d20 on every self-play position (`training/gen10/run_generation.py`), every position gets
exact board facts and a shallow CVS search, a versioned prioritizer ranks them, and only the
selected subset gets deep CVS search, with a smaller sample going to Stockfish.

```
positions ──► tier0 facts (all, ~6 ms) ──► tier1 shallow CVS (all, ~25 ms)
                                                   │
                                            tier2 triage (priority-v1)
                         ┌──────────────┬──────────┼───────────┬───────────┐
                     deep (top k)   uniform (k)  audit (low)  holdout (fixed)
                         └──────────────┴────► tier3 deep CVS (~340 ms) ◄──┘
                                                   │
                              tier4 Stockfish on small samples of each arm
```

Timings are single-thread engine milliseconds per position, measured on the 16-core dev box
with the gen9 champion net (`nnue + nnue-cal + helper-nnue`).

## Running

```powershell
# everything (stages resume: ids already in a stage file are skipped)
python training/funnel/labeling_funnel.py run --config training/funnel/funnel-config.v1.json `
    --run-dir training/funnel/runs/<name> --positions 20000 --workers 6 `
    --artifact-root F:/Github/chess-vision-studio-rust-engine

# or one stage at a time against the same run dir
python training/funnel/labeling_funnel.py tier1 --run-dir training/funnel/runs/<name>
python training/funnel/test_labeling_funnel.py      # stdlib-only tests, no engine needed
```

`--artifact-root` is where relative engine/net/shard paths resolve (a worktree has no
`target/` or `target-cvs/`). The config is copied into the run dir on first use and later
stages read that frozen copy. Engine processes run at below-normal priority so a live bot keeps
its CPU. Stockfish comes from `stockfish.binary` or `CVS_SF_EXE`.

## Tiers and files

| stage | file | provenance key | what it holds |
|---|---|---|---|
| sample | `positions.jsonl` | `gameOutcome` = `game_outcome` | unique positions picked by hash rank (deterministic, independent of shard order); source file/line |
| tier0 | `tier0.jsonl` | `deterministic_geometry`, `bounded_tactical_proof` | legal/capture/check counts, material phase + bucket, piece safety, pawn structure, king safety, square control; validator-backed motif opportunities (who can execute), hazards, SEE captures; taxonomy slugs/families; `uncomputed` kept separate from computed-empty |
| tier1 | `tier1.jsonl` | `search_derived` | cold fixed-node searches at each `nodeBudgets` entry: score, mate, best move, PV, iteration trajectory, stabilization verdict, cutoff/legal-move telemetry; score delta, move change and PV agreement across budgets |
| triage | `triage.jsonl`, `coverage.json` | — | `priority`, `rank`, per-component `{value, weight, contribution}`, top `reasons`, selection flags, `trainEligible` |
| tier3 | `tier3.jsonl` | `search_derived` | deep cold fixed-node label, shallow→deep delta and move change, targets (`scoreCpStm`, `scoreCpWhite`, `expectedScoreStm` logistic-400) |
| tier4 | `tier4.jsonl` | `external_oracle` | Stockfish `go depth D movetime M`: score, mate, best move, PV, reached depth, nodes |
| report | `report.json`, `report.md`, `triage-misses.jsonl` | — | cost per tier, coverage, deep spend by family/phase/material/priority decile/arm, arm comparison, audit miss estimate |
| — | `manifest.json` | — | config sha256, tooling + engine git state, analyze sha256, net sha256s, `identity` (model hashes + every search option), Stockfish sha256, taxonomy version + sha256, prioritizer version, per-stage timing |

Tier 0 asks the facts protocol for a move-level bundle but keeps only the position-level
`before` block, so it passes the lexicographically first legal move. That keeps it search-free.
A Tier-0 record is checked against a whitelist of static keys (`assert_static_input_safe`).
Only those two provenance classes may ever feed a static evaluator input. Search, outcome and
oracle labels live in other files and stay out of Tier 0.

## priority-v1

`priority = Σ weight·value / Σ weight`, each value in [0, 1]. Weights and caps are in the config.

| component | value |
|---|---|
| `scoreInstability` | abs(score change between smallest and largest shallow budget) / `caps.scoreDeltaCp` |
| `bestMoveChange` | 1 if the best move differs between budgets |
| `trajectoryUnstable` | stabilization status at the largest budget (stable 0, unresolved/omission-risk 0.75, unstable/verifier-conflict 1) |
| `pvDisagreement` | 1 − shared PV prefix / shorter PV |
| `tacticalDensity` | (motif opportunities + hazards) / `caps.tacticalItems` |
| `rarity` | max over the position's taxonomy slugs of target/count (plus half-weight material-bucket rarity); target = `targetShare`·pool size, and counts include `priorCountsPath` when set |
| `outcomeDisagreement` | the shallow score points against the game result by more than `outcomeMarginCp` (half weight for draws) |
| `selectivityEdge` | SEE-prune skips / cap, or 1 on a partial-iteration move shift |
| `forcedness` | in check or ≤ `forcedLegalMoves` legal moves |

Selection (`select`), all from `seed`:

* **holdout**: `unit_hash(holdoutSeed, id) < holdoutFraction`. Membership depends only on the
  position, so it doesn't change with the pool or prioritizer version. It never enters a
  training arm (`trainEligible: false`), but it is deep-labeled and sent to Stockfish, which
  makes it the untouched external-oracle validation set.
* **deep**: the top `deepFraction·N` non-holdout positions by priority (ties broken by hash).
* **uniform**: a random `deepFraction·N` non-holdout sample. Same size and node budget as
  deep, so it is the equal-compute control arm.
* **audit**: a random `auditFraction·N` sample from the low-priority remainder. It measures
  what triage misses.
* **stockfish**: the top `priorityFraction·N` of deep, the first `uniformFraction·N` of
  uniform, `auditFraction·N` of audit, and all of holdout.

Change a weight, cap or component, and bump `PRIORITIZER_VERSION`.

## Reading the report

* **informative**: the deep label moved the score by ≥ `report.informativeDeltaCp` versus the
  largest shallow budget. It is score-only on purpose: between 16k and 400k nodes the best move
  changes in most positions, often between near-equal quiet moves, so move churn is reported
  separately.
* **informative yield ratio** (priority/uniform) is partly circular, because priority reads
  shallow instability. The non-circular check is **oracle disagreement** on equal-size
  Stockfish samples. CVS scores are calibrated-NNUE centipawns and Stockfish's are Stockfish
  centipawns, so part of every absolute difference is scale mismatch; compare arms against
  each other, not against zero.
* **audit miss estimate**: the low-priority informative rate times the size of the
  low-priority pool. `estimatedRecall` is capped by the deep budget (`maxRecallAtBudget`).
  `triageLift` is the budget-free measure of triage quality.
* Neither proxy is the research question itself, which is whether a net trained on the
  priority corpus beats one trained on a uniform corpus at equal compute. That needs a
  training + gate run on `tier3.jsonl` (priority arm vs uniform arm).

## First measured run (2026-09-14)

`runs/gen10-d20-20k` (not committed; regenerate with the command above): 20,000 unique
positions from `training/gen10/corpus-d20/shards` (43,405 unique of 599,363 rows), default
config, 6 workers, gen9 champion net + calibration + residual helper, analyze sha256
`36e9b159…`, facts registry v23.

| tier | positions | engine-sec | ms/position |
|---|---:|---:|---:|
| tier0 facts | 20,000 | 121 | 6.1 |
| tier1 shallow CVS (2k + 16k nodes) | 20,000 | 524 | 26.2 |
| tier3 deep CVS (400k nodes) | 4,351 | 1,687 | 388 |
| tier4 Stockfish (d18, 3 s cap) | 1,135 | 512 | 451 |
| **total** | | **2,845** | |

For scale: Stockfish at the same ~451 ms/position on all 20,000 positions would be ~9,000
engine-seconds, and it would produce none of the semantic corpus.

| arm (2,000 / 2,000 / 400 / 212) | informative rate | move change | CVS-vs-SF disagreement (n) |
|---|---:|---:|---:|
| priority (deep) | 0.411 | 0.631 | 0.556 (383) |
| uniform | 0.306 | 0.570 | 0.504 (371) |
| low-priority audit | 0.273 | 0.498 | 0.451 (193) |
| holdout | 0.311 | 0.552 | 0.498 (205) |

* Equal budget: 790.6M vs 795.4M deep nodes; 216 positions are in both arms.
* Triage lift 1.51. Estimated recall 0.145 against a maximum of 0.353 at a 10% deep budget.
  About 27% of low-priority positions still move ≥ 60 cp under deep search.
* **Result: INCONCLUSIVE on the research question.**
  * The priority arm concentrates shallow→deep score change (1.34× uniform), but that ratio
    is partly circular.
  * The non-circular signal points the right way (priority 55.6% vs uniform 50.4% Stockfish
    disagreement), but the gap is about 1.4 standard errors, which is not significant.
  * CVS-vs-SF absolute gaps (holdout p50 79 cp, mean 170 cp) include scale mismatch between
    calibrated CVS centipawns and Stockfish centipawns.
* Coverage: underrepresented taxonomy slugs in this source include epaulette/back-rank/
  damiano mates, luring, desperado, double check, distraction, overloading and mate threat.
  These are the classes a coverage-aware prior should chase.
* **Next:** (1) train an evaluator on the priority arm vs the uniform arm at equal rows, and
  gate both. (2) Grow the Stockfish samples to ~1,500 per arm, so a 5-point disagreement gap
  resolves at 2σ. (3) Score move quality (deep score of the shallow move via `forcedMoveUci`)
  instead of raw move churn.

## Active-learning loop

1. Run the funnel on new positions (self-play/live games/search distribution).
2. Train on `tier3` targets from `trainEligible` positions, joined with `tier0` facts as
   auxiliary labels.
3. Pass the previous run's `coverage.json` as `tiers.tier2.coverage.priorCountsPath`, so rarity
   is measured against everything already labeled.
4. Re-run with the new net in `engine.args`, and watch whether oracle disagreement on the
   holdout falls and deep-label demand (priority mass) shrinks.
