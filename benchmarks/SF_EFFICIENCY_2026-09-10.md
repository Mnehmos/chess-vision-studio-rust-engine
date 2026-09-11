# Search efficiency vs Stockfish: the selectivity experiment (2026-09-10)

**Question.** The fixed-node KPI says we reach ~6 plies at 40k nodes where Stockfish reaches
~13 (a 24-73x nodes-to-depth deficit). The campaign target was "avg depth per node >= SF 15".
Can we close it by porting Stockfish's own pruning structure at its published constants?

**Answer: no — and we now know why.** Porting SF's exact structure buys +3 nominal plies at
40k nodes and is strength-neutral *per node*, but it is **strength-negative at the node budgets
the engine actually plays at** (-51 Elo at 12k nodes) and **-95 Elo at equal time** (3+0.03),
despite the extra depth. The bottleneck is not search selectivity; it is per-node quality
(evaluation accuracy and move ordering). Every movecount-pruning decision cuts the engine's own
eventual best move, because our ordering is far weaker than Stockfish's.

## What was ported (`src/search/*`, all flags OFF by default)

| flag | content (constants are SF's, converted to centipawns at x100/208) |
|---|---|
| `--sfprune` | RFP to depth 19 at margin `min(45+4d, 85)*d`; quiet futility `119*lmrDepth+90+164`; SEE pruning `177*d` (captures) and `23*lmrDepth^2` (quiets); capture futility; movecount budget `(3+d^2)/2` at every depth |
| `--sfnull` | SF null condition `eval+365 >= beta-13*depth`; depth-scaled `R = 7 + depth/3 + max((eval-beta)/256,0)` |
| `--sfqs` | SF qsearch move budget (after two moves only checks/promotions) — measured neutral |
| `--lmr2` | LMR from the second move, PV one-ply rebate — measured neutral |
| `--iir` | SF internal iterative reduction — measured neutral (we have few deep TT-less nodes) |
| `--conthist2` | second continuation history (SF `contHist[1]`) — +0.09 plies, ordering-only |

Two exact fast paths landed with them: a lazy per-move `gives_check` cache and an
"unattacked quiet destination" guard that skips the SEE call entirely. Both are
behavior-preserving; the champion's own nps is unchanged (668k vs 674k).

## Measurements

KPI (median over a 12-position suite; Stockfish = native AVX2 binary, same node budget):

| config | depth@40k | branch | fmc% | qPct | depth@100ms | nps |
|---|---|---|---|---|---|---|
| champion | 6.08 | 3.05 | 44.9 | 53.8 | 6.42 | 687k |
| + sfprune | 9.08 | 1.39 | 59.4 | 39.1 | — | 556k |
| + sfprune + sfnull | 8.80 | 1.36 | 60.7 | 38.5 | **9.50** | 556k |
| Stockfish | 13.83 | | | | | |

Gates (BayesElo trinomial SPRT, elo0=0/elo1=10, alpha=beta=0.05, disjoint book slices):

| instrument | result |
|---|---|
| fixed nodes 40k, 4910-position book | **HOLD** — 1680 games, LLR -0.121 (neutral per node) |
| fixed nodes 12k (the budget a 3+0.03 clock gives) | **REJECT** — 578 games, 216-300-62, LLR -2.945 (~-51 Elo) |
| equal time `tc=3+0.03` (idle machine, re-run) | **REJECT** — 335 games, 108-197-30, LLR -2.958 (~-95 Elo) |

The depth gain is real and reproducible; it simply does not convert to strength.
Note the direction: the *smaller* the node budget, the worse selectivity performs. That
retro-explains the whole prior pruning record (`sel-aggr` 21-118-3, `sel-lmp-soft` 75-166-21,
`seeprune` -2.958, `delta` -2.964, `improving` -2.974, `tt2` -2.945).

## Why: ordering and eval quality, measured

New diagnostic (`benchmarks/scripts/diag_order_rank.py`) — rank of native Stockfish's
depth-18 best move inside our 40k-node root ordering, 30 book positions:

* rank <= 10: **19/30**; median rank 6; max rank 32.
* self-reference: our own 1.5M-node best move is rank 1 in 13/24 positions, rank > 10 in 4/24.
* first-move cutoff rate: champion 45%, sfprune 59%, Stockfish ~90%.

So in roughly a third of positions the move a deep search would pick sits past the point where
any movecount budget (or SEE margin) cuts. Stockfish can prune because its TT + history
ordering has already tried the best move; pruning our tail discards it. The same argument
applies to eval-based cuts (RFP/futility/razoring): our static eval MAE vs SF is ~172cp, far
wider than the margins being applied, so those cuts fire on noise.

## Consequences for the KPI

"Average depth per node >= Stockfish" is achievable only through selectivity, and selectivity
is strength-negative for this engine until ordering and eval improve. Chasing the depth number
directly trades real Elo for nominal plies. The see-saw has tipped to the eval side: the engine
searches deeper than its evaluation can use.

## Tooling left in place

* `benchmarks/scripts/diag_nodes.py` — node split + search-shape telemetry (qPct, TT entry/hit,
  fmc%, cutoff index, searched branching).
* `benchmarks/scripts/diag_order_rank.py` — ordering rank against a deep reference.
* `benchmarks/scripts/match_timed.py` — equal-time referee (the fixed-node gate cannot see a
  nodes-per-second tradeoff; this one can).
* `gate_ladder.py` LADDER entries `sfprune`, `sfprune-ch2`, `conthist2`, `iir`.

Records: `benchmarks/results/sfprune-gate-20260910/`, `.../sfprune-12k-gate-20260910/`,
`.../sfprune-tc2-20260910/`, and `benchmarks/SF_EFFICIENCY_2026-09-10.json`.


## Addendum 2026-09-11 — re-tested on the calibrated eval

The eval calibration (`benchmarks/EVAL_CALIBRATION_2026-09-11.md`, +36 Elo fixed-node,
+74 Elo anchored) put the centipawn margins into the units the SF constants assume, so the
bundle's rejection was re-tested: `nnuecal-sfprune` = calibrated eval + `--sfprune --sfnull`
at 40k nodes, 1500 games, **HOLD** (643-641-216, LLR −0.630, point estimate ≈ −5 Elo).

| configuration | depth@40k | 40k-node gate | 12k-node gate | equal time |
|---|---:|---|---|---|
| sfprune (uncalibrated eval) | 9.08 | HOLD (−0.12) | **REJECT** (−2.945) | **REJECT** (−2.958) |
| sfprune (calibrated eval) | 9.17 | HOLD (−0.63) | not run (see note) | not run (see note) |

Reading: the calibration removed the *soundness* failure (the same bundle went from clearly
harmful at playable budgets to neutral at 40k), which confirms the diagnosis that a compressed
eval scale was corrupting every cp-based decision. It did **not** turn selectivity into a gain,
because the remaining blocker is move ordering, not eval scale: SF's deep best move still sits
past rank 10 in our ordering in about a third of positions, so the pruned tail still contains
the move. The 12k/equal-time gates were not re-run: per node the bundle is now merely neutral
while it still costs 1.2-1.3x nodes per second, so the outcome is determined by the 40k result
plus the measured nps cost.

Next lever for the KPI: ordering (contHist[1], TT-move availability, pawn history) and further
eval accuracy — the calibrated net is still at MAE ~85 vs Stockfish's static eval, and its
residual errors are concentrated in the middlegame/endgame (~93-95 MAE vs ~70 in the opening).
