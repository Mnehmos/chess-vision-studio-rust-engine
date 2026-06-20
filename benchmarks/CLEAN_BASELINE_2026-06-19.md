# Clean Holdout Baseline: 2026-06-19

## Method

- Suite: `suite-clean-postmodel-20260619`, 92 positions.
- Source isolation: see `CLEAN_HOLDOUT_2026-06-19.md`.
- Engine budget: fixed depth 8, one thread.
- Search profile: `champion-2026-06-12`.
- Scoring: native Stockfish depth 24 child evaluation.
- Result: `results/20260619-184302-generation-decision.json`.

This is a decision-quality comparison. Fixed depth does not measure throughput
or equal-time playing strength.

## Results

| Engine | Match | Avg CP | P90 | >=100 CP | >=200 CP | Danger Avg |
|---|---:|---:|---:|---:|---:|---:|
| Gen7 raw | 46.7% | 114.5 | 63 | 8.7% | 2.2% | 269.0 |
| Gen8-v2 raw | 45.7% | 117.4 | 109 | 10.9% | 2.2% | 268.0 |
| Gen9 raw | 46.7% | 122.4 | 109 | 12.0% | 3.3% | 273.5 |
| Gen9 Hybrid A | 47.8% | 21.7 | 92 | 9.8% | 1.1% | 17.7 |

## Tail Interpretation

The raw averages are dominated by one shared catastrophic miss. Removing each
engine's single worst result gives:

| Engine | Trimmed Avg CP |
|---|---:|
| Gen7 raw | 19.1 |
| Gen8-v2 raw | 22.1 |
| Gen9 raw | 27.2 |
| Gen9 Hybrid A | 19.2 |

Hybrid A therefore does not broadly dominate ordinary depth-8 decisions. Its
measured gain is primarily catastrophic-blunder prevention.

## Forensic Position

```text
4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32
```

At depth 8, Gen7, Gen8-v2, and Gen9 raw all chose `g2f3`, which allows a forced
mate and loses 8,791 cp relative to the depth-24 oracle child score. Hybrid A
chose `h2h4` and incurred no measured loss.

The depth ladder shows this is a horizon/order problem:

- Gen9 raw continues choosing `g2f3` through depth 9 and finds `h2h3` at depth
  10.
- Hybrid A avoids the move at depth 8, although it also chooses `g2f3` at depth
  7.
- Hybrid A searched 316,411 nodes at depth 8 versus 94,254 for Gen9 raw.

The geometry helper exposes the tactical defense earlier in nominal depth, but
at substantial node cost. The next required gate is equal-time comparison on
an idle machine.

## Decision

The equal-time gate is complete. Hybrid A did not beat the exact matrix-raw
control at any tested budget and produced a catastrophic 100 ms regression on
this same forensic position. See `EQUAL_TIME_HYBRID_A_2026-06-19.md`.

Decision: ANALYSIS MODE ONLY
