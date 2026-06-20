# Specialist Authority Standard

## Purpose

Specialists may improve attention and tactical coverage, but they must not gain
live move authority merely because they produce a score or reorder candidates.
The Raw principal search owns the move until independently verified evidence
passes a registered gate.

## Authority Classes

### Exact Evidence

- Forced mate with a legal principal variation.
- Syzygy WDL/DTZ result.
- Verified repetition or terminal draw.
- A legal tactical refutation independently confirmed by the principal
  evaluator.

Exact evidence may veto a candidate only after the principal engine confirms
the same result. Replacement selection remains with the principal search.

### Verification Request

- Suspected forcing attack.
- Large Raw/specialist disagreement.
- Quiet defense or king-safety candidate requiring more search.
- Principal variation instability.

A request may buy a bounded Raw re-search. It cannot directly change root
ordering or the returned move.

### Candidate Diversity

- Alternative quiet plan.
- Structural or geometric preference.
- Lower-ranked move for analysis.
- Disagreement sample for training.

Candidate diversity belongs in analysis, teaching, and spare-thread research.
It has no live authority.

## Completed-Iteration Contract

Timed search returns the last fully completed iterative-deepening result.
An interrupted root iteration is diagnostic and provisional only.

Every timed result must report:

- `resultSource`;
- `termination`;
- completed `depth`;
- `attemptedDepth`;
- completed-iteration `rootOrder`;
- optional `partialIteration` with searched candidates, candidate times,
  provisional best move, and aspiration bounds.

`resultSource=completed-iteration` means the returned move is exactly the move
from the final entry in `iterations`. Partial-root state must never overwrite
the completed iteration's root order or move.

## Tactical Sentinel v1

The first sentinel is a non-authoritative evidence experiment:

1. Accept a proposed principal move.
2. Make the move.
3. Search the opponent's child position with RFP, futility, null move, LMR,
   LMP, SEE pruning, delta pruning, and singular extensions disabled.
4. Raise exact evidence only for a positive mate score.
5. Require an independent Raw forced-move search to return the corresponding
   negative mate score.
6. A non-mate major-loss request additionally requires an independent Raw
   search to choose another move and improve the forced score by the registered
   decision margin.
7. Report the proof or verification request; do not select a replacement move.

This experiment runs only through
`benchmarks/scripts/bench_tactical_sentinel.py`. It is not connected to UCI,
the app backend, or the Lichess bot.

## Promotion Gates

1. Forensic capability: detect and confirm the known `g2f3` mate reliably.
2. Clean-suite specificity: no unconfirmed alarms on the frozen holdout.
3. Concurrent same-budget prototype: Raw principal search retains its full
   authority and ordinary throughput.
4. Tail gate: no increase in >=100, >=200, or mate-scale losses.
5. Paired games: same clocks, openings, threads, binary, main net, and profile.
6. Explicit live opt-in and rollback target.

Until all gates pass:

Decision: ANALYSIS MODE ONLY
