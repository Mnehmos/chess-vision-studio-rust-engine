# Eval campaign, slice 1: output calibration (2026-09-11)

**Result: PROMOTE.** `--nnue-cal` crossed the upper SPRT bound at 1000 games
(479-375-146, **LLR +2.971**, ≈+36 Elo) and is now part of the champion identity, the
studio `.env` (live bot), and the gates' `N0_FLAGS`.

## Why this was the next lever

The search-efficiency campaign ended with a measured verdict: the engine is **eval-limited**.
Ported Stockfish pruning at SF's own constants bought +3 nominal plies at 40k nodes yet was
strength-neutral per node and strength-negative where the engine actually plays (12k nodes:
LLR −2.945; equal time 3+0.03: LLR −2.958). Every movecount/SEE/eval cut discards the move a
deep search would pick, because the ordering is weak **and the eval cannot tell which moves
deserve the nodes**. So the campaign moves to eval quality, with a direct instrument.

## Instrument

`benchmarks/scripts/bench_eval_quality.py` measures the static eval against a reference:
- **SF's own static eval** (UCI `eval` → "Final evaluation") — static-vs-static, apples to
  apples; the corpus' shard labels are *search* scores and include tactics a static eval
  cannot see, so they are the (noisier) secondary mode.
- Reports MAE / median / p90 / bias, a phase breakdown, and the linear fit (slope, r).

Baseline on 700 positions (champion net `matrix-raw.json` + `rung2_scalar` fallback):

| evaluator | MAE | median | p90 | slope vs SF | r |
|---|---:|---:|---:|---:|---:|
| NNUE (raw) | 126.4 | 99 | 267 | 0.492 | 0.922 |
| classical/rung2 | 114.5 | 89 | 242 | 0.874 | 0.868 |

Two defects, both structural:
1. **The NNUE was *worse* than the handcrafted eval** on static accuracy (126 vs 114 MAE)
   despite much better ranking (r 0.92 vs 0.87).
2. **Its output scale was 2x compressed** (slope 0.49). Every centipawn margin in the search
   (RFP, futility, aspiration windows, SEE margins, null-move guards) was therefore applied at
   roughly twice its intended severity — and SF-derived constants (`177*depth`,
   `min(45+4d,85)*depth`) were off by the same factor.

## Fix

A monotone piecewise-linear map fitted on oracle labels, applied to `|raw|` with the sign
restored (eval stays exactly antisymmetric), shipped as an artifact:

`target-cvs/eval-cal-20260911.json` (tracked copy: `benchmarks/artifacts/eval-cal-20260911.json`,
because `/target*` is gitignored) — fitted on 1500 positions (gen9 shard FENs vs SF static
eval), holdout-checked by fitting on odd rows and evaluating on even rows:
**holdout MAE 84.7 vs 127.9 raw**.

In-engine (after the fix, same 700 positions):

| evaluator | MAE | median | p90 | slope | r |
|---|---:|---:|---:|---:|---:|
| NNUE + calibration | **85.4** | 65 | 183 | **0.870** | 0.926 |
| classical/rung2 | 112.8 | 86 | 239 | 0.874 | 0.868 |

The calibrated net is 32% more accurate than before **and** now on the same scale as SF's
eval, which is the prerequisite for the SF-derived pruning margins to mean what they say.

## Gate

`benchmarks/results/nnuecal-gate-20260911/` — fixed-node 40k, champion flags, 4910-position
disjoint book, **no adjudication** (the two engines report scores on different scales, so
cutechess' cp-based draw/resign thresholds would fire asymmetrically and bias the verdict;
`match_fixed_nodes.build_cutechess_cmd(adjudicate=False)` and gate_ladder
`spec["no_adjudication"]` exist for exactly this).

1000 games, 479-375-146 (55.2%, ≈+36 Elo), LLR +2.971 → **promote**.

## KPI side effects

| | depth@40k | depth@100ms | nps |
|---|---:|---:|---:|
| champion (pre-cal) | 5.92 | 6.33 | 655k |
| + calibration | 6.33 | 6.92 | 633k |
| + calibration + sfprune + sfnull | 9.17 | 9.50 | 490k |

The calibration is a table lookup: within measurement noise on nps, +0.4 plies at equal nodes.
The `--sfprune` bundle remains unshipped (its own gates reject it at 12k nodes and at equal
time), but the calibrated eval is the fair test bed for selectivity, so
`nnuecal-sfprune` is queued as the next gate.

## Promotion trail

- `benchmarks/scripts/match_fixed_nodes.py` `N0_FLAGS` now carries the calibration (champion
  identity moves forward for all future gates).
- Studio `.env`: `CVS_RUST_NNUE_CAL=...` and `arena/engine-backend/rust-backend.ts` maps it to
  `--nnue-cal` (default-off; unset = raw net output).
- Rebuilt the deployed `target/release` binary and restarted the Lichess bot on it.
- Anchor vs native Stockfish (10+0.1) re-run: `benchmarks/results/anchor-nnuecal-20260911/`.
