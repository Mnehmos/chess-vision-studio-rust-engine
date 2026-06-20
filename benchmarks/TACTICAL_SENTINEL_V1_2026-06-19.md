# Tactical Sentinel v1 Gate: 2026-06-19

## Executive Result

The completed-iteration audit disproved the initial partial-iteration
hypothesis. Timed search already returns the last fully completed depth.
Hybrid A's 100 ms `g2f3` regression came from completed depth 7 while Raw
completed depth 6. An interrupted deeper root did not replace either result.

Tactical Sentinel v1 can produce a precise short-budget verification request
for one mate-scale clean-suite failure, but it cannot prove mate and does not
cover a separate endgame loss. It remains disconnected from live play.

Decision: HOLD FOR MORE DATA

## Search-Retention Audit

The search contract is now explicit:

- `resultSource=completed-iteration` identifies the authoritative move.
- `depth` is the final completed depth.
- `attemptedDepth` reports the interrupted deeper iteration.
- `termination` distinguishes hard time, soft time, external stop, depth cap,
  mate, book, and tablebase exits.
- `partialIteration` reports provisional root work only when
  `--root-diagnostics` is enabled.
- completed root order is no longer overwritten by a later interrupted root.

The production default keeps detailed root timing off, so the audit
instrumentation adds no per-candidate clock overhead to live search.

## Sentinel Contract

Sentinel v1 has no move authority. For a proposed move it runs:

1. A reduced-pruning search from the opponent's child position.
2. An independent Raw forced-move verification.
3. An independent Raw unforced comparison.

Evidence classes:

- `exact-mate`: both sentinel and forced verifier report corresponding mate
  scores.
- `verified-major-loss`: sentinel and verifier exceed 300 cp in opposite
  directions, the comparison chooses another move, and its score improves by
  at least 50 cp.
- `none`: no request.

A major-loss result is a re-search request only. It cannot veto the candidate
or select a replacement.

## Forensic Result

Position:

```text
4r3/2pk2pp/5p2/2P2b2/r7/3n1p2/P2B2PP/R4K1R w - - 0 32
```

Candidate: `g2f3`.

Result:
`results/20260619-213648-tactical-sentinel-forensic.json`.

- No exact mate proof was found at any budget through 5000 ms.
- All three repetitions produced a candidate-specific major-loss request at
  50 ms.
- The request disappeared at 100-250 ms when the independent Raw comparison
  oscillated back to `g2f3`.
- It returned consistently from 500-5000 ms.
- At 250 ms and above, forced Raw valued `g2f3` near -624 cp, still far short
  of the Stockfish mate-scale label.

The sentinel detects serious danger earlier than exact proof, but its own
signal inherits the engine's non-monotonic horizon.

## Clean-Suite Specificity

Source decisions:
`results/20260619-201102-equal-time-paired.json`.

Suite: `suite-clean-postmodel-20260619`, 92 positions. The table uses the
10 ms sentinel/verifier/comparison policy and a 300 cp Stockfish loss threshold.

| Principal budget | Actual >=300 | TP | FP | FN | TN | Result |
|---:|---:|---:|---:|---:|---:|---|
| 25 ms | 1 | 1 | 0 | 0 | 91 | caught 9316 cp miss |
| 50 ms | 1 | 1 | 0 | 0 | 91 | caught 9316 cp miss |
| 100 ms | 1 | 1 | 0 | 0 | 91 | caught 9316 cp miss |
| 250 ms | 1 | 1 | 0 | 0 | 91 | caught 9316 cp miss |
| 500 ms | 1 | 0 | 0 | 1 | 91 | missed 309 cp endgame loss |
| 1000 ms | 0 | 0 | 0 | 0 | 92 | silent |
| 2000 ms | 0 | 0 | 0 | 0 | 92 | silent |

Detailed results:

- `results/20260619-213828-tactical-sentinel-suite.json`
- `results/20260619-213835-tactical-sentinel-suite.json`
- `results/20260619-213755-tactical-sentinel-suite.json`
- `results/20260619-213843-tactical-sentinel-suite.json`
- `results/20260619-213850-tactical-sentinel-suite.json`
- `results/20260619-213857-tactical-sentinel-suite.json`
- `results/20260619-213905-tactical-sentinel-suite.json`

The 5 ms policy produced false requests at the 1000 and 2000 ms principal
budgets. The 10 ms policy removed those false positives. Extending the
500 ms screen to 25-250 ms did not recover the endgame miss and introduced a
false request:
`results/20260619-214128-tactical-sentinel-suite.json`.

## Interpretation

Sentinel v1 is a tactical-danger detector, not a general blunder detector.
It catches the repeated sharp position at index 20 but does not detect the
quiet 309 cp endgame conversion error. Longer sentinel search is not
monotonically better.

The screen is also not an equal-resource playing-strength result. It uses
three independent one-thread processes to establish capability and
specificity. Live promotion requires a concurrent resource-matched design.

## Next Gate

Build a non-authoritative concurrent prototype:

1. Raw publishes its last completed principal move at iteration boundaries.
2. A 10 ms sentinel examines only a changed principal candidate.
3. Forced and unforced Raw verification run on registered spare resources.
4. A verified concern may grant bounded Raw re-search time.
5. Raw still selects the move.
6. Compare against Raw using the same total threads, wall clock, hash, and
   model.

Do not connect Sentinel v1 to UCI, the app backend, ponder, or Lichess before
that gate passes.
