# R4 — Stockfish-scored gate: Rust vs TS vs the SF oracle

**Question:** does the Rust engine, at practical depths, beat the legacy TS/chess.js
engine when Stockfish (depth 10) scores the actual chosen moves?

## Verdict: ✅ GREEN — parity at d2–d4, strict superiority at d6, "huge win" met

**Recommendation: Rust is eligible to become the active CVS engine backend, at
operating depth 6** (strictly dominates TS d4 on quality AND runtime).

## Setup

- Slices: holdout = multipv dataset [543..638) (95 positions, 74 unique FENs);
  unseen = [603..638) (35 positions — the gate-independent slice no keep-gate
  ever touched). Weights: the trained mixed base+Rung-2 (same files both engines).
- Scorer: Stockfish depth 10 via the shared persisted eval cache; cpLoss in pawns;
  blunder = cpLoss ≥ 2; identical scoring path for both engines.
- TS picks measured once and frozen in `arena/out/ts-picks-cache.json` (the TS
  engine is a fixed reference; future runs skip it entirely). TS think-time totals
  are from the measured run; per-move times are uniform-averaged from those totals.

## Move agreement (same depth, same weights)

73/74 unique positions identical at d2, d3 AND d4 (98.6%). The single divergence
(`r4rk1/1p1bbpp1/…`) is an equal-score tie-break (different movegen order), as
predicted by R3's exact-parity bench.

## Quality table (Stockfish-scored searched moves)

| Engine | Depth | Slice | Top-1 | Avg cpLoss | Median | Blunder % | Mate missed | Illegal | Think time(s) |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|
| TS | 2 | holdout | 30.5% | 0.291 | 0.160 | 1.1% | 0 | 0 | 70.4 |
| TS | 3 | holdout | 38.9% | 0.239 | 0.070 | 1.1% | 0 | 0 | 278.9 |
| TS | 4 | holdout | 46.3% | 0.217 | 0.100 | 0.0% | 0 | 0 | 2166.8 |
| Rust | 2 | holdout | 30.5% | 0.293 | 0.180 | 1.1% | 0 | 0 | 0.1 |
| Rust | 3 | holdout | 40.0% | 0.239 | 0.070 | 1.1% | 0 | 0 | 0.5 |
| Rust | 4 | holdout | 47.4% | 0.217 | 0.100 | 0.0% | 0 | 0 | 3.5 |
| Rust | 5 | holdout | 38.9% | 0.192 | 0.040 | 1.1% | 0 | 0 | 14.6 |
| **Rust** | **6** | holdout | 40.0% | **0.159** | **0.040** | **0.0%** | 0 | 0 | **68.0** |
| TS | 2 | unseen | 34.3% | 0.290 | 0.070 | 0.0% | 0 | 0 | 25.9 |
| TS | 3 | unseen | 45.7% | 0.202 | 0.040 | 0.0% | 0 | 0 | 102.8 |
| TS | 4 | unseen | 71.4% | 0.159 | 0.000 | 0.0% | 0 | 0 | 798.3 |
| Rust | 3 | unseen | 45.7% | 0.202 | 0.040 | 0.0% | 0 | 0 | 0.1 |
| **Rust** | **5** | unseen | 54.3% | **0.124** | **0.000** | **0.0%** | 0 | 0 | 3.8 |
| Rust | 6 | unseen | 48.6% | 0.128 | 0.000 | 0.0% | 0 | 0 | 15.8 |

## Findings

1. **Parity (d2–d4):** Rust quality is identical to TS at every shared depth
   (avg/median/blunder match; the tiny top-1 deltas are the one tie-break
   position). The scorer integration is validated.
2. **Superiority (d5/d6):** Rust d6 vs TS d4 on the holdout: avg cpLoss
   0.217 → **0.159 (−27%)**, median 0.100 → 0.040, blunders 0%, **32× faster**
   (68s vs 2167s). On the unseen slice Rust d5 reaches avg **0.124** with median
   0.000. The "huge win" criterion — *Rust d6 faster than TS d4 AND better* — is met.
3. **Safety:** illegal = 0 and mate-missed = 0 in every cell of the matrix.
4. **Forensic #549:** d4 plays h7f7 (the quiescence fix, regression-tested);
   d5 still picks the quiet-refuted b3f7 — the known residual, visible as the
   single d5 blunder (1.1%); **d6 resolves it** (h7f7) and the d6 row's blunder
   rate is 0.0%. Depth is the cure for this failure class, and Rust makes the
   depth affordable.
5. **Runtime:** Rust does the entire d2–d6 pick sweep in ~87s; TS needed ~42 min
   for d2–d4 alone. Rust d4 ties TS d4 at **~620×** less think time.

## Caveats

- One equal-score tie-break divergence per depth (documented above) — value-
  neutral, inherent to movegen ordering.
- Top-1 (move == SF's #1) is not monotonic in depth (deeper, better-scoring moves
  sometimes differ from SF-d10's first line); cpLoss/blunder are the gate metrics.
- TS per-move times in the table are uniform-averaged from measured totals.

## Next

R5: expose the Rust engine as the app's engine backend (CLI subprocess bridge,
backend selector, integration tests). Default flips to Rust per this verdict.
