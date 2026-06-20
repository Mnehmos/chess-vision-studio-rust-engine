# Hybrid A Equal-Time Gate: 2026-06-19

## Decision

Hybrid A does not beat its exact raw control at equal time. No tested budget
has a bootstrap interval excluding zero, median paired delta is zero
throughout, and Hybrid A adds a catastrophic 100 ms regression on the clean
holdout's known forced-mate position.

The helper remains useful as an analysis and candidate-diversity tool. It is
not approved for live play.

Decision: ANALYSIS MODE ONLY

## Exact Pair

| Property | Raw control | Hybrid A |
|---|---|---|
| Registry ID | `g9.raw-control.matrix-raw` | `g9.hybrid-a.raw-plus-residual` |
| Binary SHA | `ca18ad5ca44554b5` | `ca18ad5ca44554b5` |
| Main net SHA | `b23c75b8e25d7480` | `b23c75b8e25d7480` |
| Helper net SHA | none | `d2a98888ad83a356` |
| Search profile | `champion-2026-06-12` | `champion-2026-06-12` |
| Profile SHA | `2f152be22ac3893b` | `2f152be22ac3893b` |
| Threads | 1 | 1 |
| Book / Syzygy | off / off | off / off |

Hybrid A is `quiet-root-residual-ordering-v1`: the residual helper affects only
quiet root move ordering. Its child score is multiplied by 10 and clamped to
`[-4000, 4000]`. It does not replace the main evaluation and does not affect
the TT move, captures, or promotions.

## Method

- Suite: `suite-clean-postmodel-20260619`.
- Suite hash: `a52d68d4bf55d99e`.
- Positions: 92.
- Budgets: 25, 50, 100, 250, 500, 1000, and 2000 ms.
- Shared randomized order: `equal-time-clean-20260619-v1`.
- Engine order alternated by position and budget.
- Both processes warmed before measurement.
- Stockfish depth-24 child scoring.
- Paired delta: `Hybrid cp loss - Raw cp loss`; negative favors Hybrid.
- Bootstrap: 20,000 deterministic resamples of the paired mean.
- Search result:
  `results/20260619-201102-equal-time-paired.json`.
- Forensic ladder:
  `results/20260619-201338-forensic-time-ladder.json`.

The Lichess bot, Vite server, API engine workers, and resident Stockfish worker
were stopped for the run. One inert Windows process-table entry with no
handles, path, command line, or CPU data remained and was excluded by the idle
guard.

## Results

| Budget | Mean delta CP | Median | Bootstrap 95% CI | H / R / tie | Same move |
|---:|---:|---:|---:|---:|---:|
| 25 ms | +5.79 | 0 | [-2.82, +17.42] | 5 / 9 / 78 | 78 |
| 50 ms | +3.28 | 0 | [-2.97, +11.61] | 7 / 7 / 78 | 78 |
| 100 ms | +93.13 | 0 | [-5.99, +286.24] | 12 / 6 / 74 | 72 |
| 250 ms | +2.64 | 0 | [-1.62, +7.62] | 6 / 10 / 76 | 75 |
| 500 ms | -2.17 | 0 | [-13.75, +8.32] | 8 / 7 / 77 | 75 |
| 1000 ms | +0.72 | 0 | [-6.68, +7.75] | 8 / 11 / 73 | 71 |
| 2000 ms | +5.86 | 0 | [-0.32, +13.26] | 11 / 11 / 70 | 68 |

`H / R / tie` counts positions where Hybrid wins, Raw wins, or both have the
same Stockfish loss. The helper changed root order on 88-89 of 92 positions,
so this is not an inert-feature result. It changed the final move on 14-24
positions depending on budget.

Median completed depth was identical at every budget. At 2000 ms, Raw averaged
about 967k NPS and Hybrid about 947k NPS. Hybrid therefore paid measurable
ordering overhead without converting it into a reliable depth or quality gain.

## Tail Analysis

The 100 ms mean is dominated by:

```text
4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32
```

Raw returned `h2h4` from completed depth 6 for 135 cp loss. Hybrid returned
`g2f3` from completed depth 7 for 8,880 cp loss. The paired regression is
8,745 cp. A code audit confirmed that the engine already rejects interrupted
iterations and returns the last completed depth; the failure is horizon
oscillation at a different completed depth, not partial-iteration authority.
Removing four observations from each tail changes the 100 ms mean from
`+93.13` to `-1.07` cp, confirming that the central distribution is tied while
the tail risk is not.

At 2000 ms, Raw averaged 13.5 cp loss versus Hybrid's 19.4. Raw's p95/max were
74/125 cp versus Hybrid's 97/206. This is not statistically decisive, but it
does not support spending more live clock on the current helper policy.

## Forensic Ladder

Across three repetitions per budget:

- Raw first avoided `g2f3` in every repetition at 25 ms.
- Hybrid first avoided it in every repetition at 50 ms.
- Both policies regressed to `g2f3` at 100-250 ms as later partial iterations
  completed.
- Both avoided it in every repetition at 500, 1000, and 2000 ms.

The sustained safe threshold is therefore 500 ms for both policies. Hybrid A
changes when the horizon is encountered; it does not remove the horizon.

## Deployment

The exact matrix-raw control becomes the live play candidate. Hybrid A remains
available to the analysis UI. The Lichess bot no longer inherits the analysis
helper implicitly; live helper experiments require the explicit
`CVS_LICHESS_RUST_HELPER_NNUE` opt-in.

Any successor helper must use a new policy ID and independently gate its
multiplier, clamp, activation confidence, throughput, clean-suite tail, and
paired game strength.

The next experiment is the proof-only tactical sentinel defined in
`SPECIALIST_AUTHORITY_STANDARD.md`. It has no live move authority.
