# INV-1 gate integrity finding — 2026-09-09

**Verdict:** the 2026-09-09 INV-1 ladder and the bundle confirmation replay the same
games in every batch. Their recorded sample sizes and LLRs are not valid promotion
evidence at the stated n. The harness is fixed, the promoted set has been re-measured on
independent positions, and the result is **HOLD** (not a crossing).

## What was wrong

`F:/tools/openings.epd` contains **12 positions**. The ladder invoked `cutechess-cli`
once per batch with `order=sequential`, so every batch restarted at position 1; with
`-games 120 -repeat` each batch is 12 positions x 2 colors x 5 cycles = the same 120
games. Twenty-five such batches produced the recorded 3000-game gate.

Evidence, reproducible from the committed artifacts:

| check | result |
|---|---|
| `match.jsonl` 120-game segments within one gate | byte-identical (e.g. `singular`: all 25 segments `55-50-15`) |
| PGN movetexts, batch-001 vs batch-002 vs batch-025 (times stripped) | identical |
| distinct positions per gate | 24 (12 openings x 2 colors) |
| distinct positions across the whole ladder | 12 |

The stored per-game rows are real and the records recompute exactly from them
(`benchmarks/scripts/sprt_runner.py` over `match.jsonl` reproduces every recorded
W/L/D, LLR and boundary). What is wrong is the sample-size accounting: the SPRT streams
the repeated rows as if they were independent games.

## Impact

The repetition inflates the LLR roughly linearly with the number of copies:

| record | unique games | LLR on unique games | recorded games | recorded LLR | recorded decision |
|---|---:|---:|---:|---:|---|
| `promo-bundle-confirm-20260909` | 120 (60-45-15) | +0.432 (none) | 801 (6.675 copies) | +2.974 (upper) | **promote** |
| `inv1-ladder-20260909/singular` | 120 (55-50-15) | +0.107 (none) | 3000 (25 copies) | +2.672 (none) | hold |

A HOLD on the unique games became a PROMOTE on the copies. The same defect applies to
every record in `benchmarks/results/inv1-ladder-20260909/` and to
`benchmarks/results/promo-bundle-confirm-20260909/`; `kingact-regate-20260909` has no
stored games at all, so its 3569-game claim cannot be checked.

Promotions that rested on those records: `caphist`, `seeprune`, `improving`, `tt2`,
`kingact` (defaults in `src/search/types.rs`). Rejections (`conthist`, `countermove`,
`delta`, `rule50`) are equally unsupported but change nothing in master.

## The fix

`benchmarks/scripts/gate_ladder.py`:

* `--book` is **required**; the harness fails closed rather than play a 12-position book.
* Each batch gets a **disjoint slice**, written to `openings-batch-NNN.epd`, and
  `positionsUsed` is recorded in the SPRT provenance.
* A seen-set refuses any position a previous batch already played; an exhausted book
  stops the gate with HOLD instead of wrapping around.
* Re-gate specs (`<flag>-regate`, `bundle-inv1`) compare the current champion against the
  champion with the promoted flag(s) disabled.

`benchmarks/scripts/build_inv1_book.py` builds
`benchmarks/suites/openings-inv1-20260909.epd`: **2019 distinct positions** (the 12
historical control openings plus harvested Lichess positions, fullmove 4-16, halfmove
<= 6, material within 2 pawns, 2+ non-pawns per side). Provenance and sha256 in
`openings-inv1-20260909.provenance.json`.

`benchmarks/scripts/test_gate_ladder.py` pins the guard: disjoint slices, repeat
refusal, exhaustion, and the fail-closed CLI.

## Re-validation on independent games

`benchmarks/results/bundle-regate-20260909/` — champion vs champion with the promoted
INV-1 set disabled (`--no-caphist --no-improving --no-king-activity --no-seeprune
--no-tt2`), 40k fixed nodes, 1000 distinct positions, 2000 games:

| games | W-L-D | LLR | boundary | decision |
|---:|---|---:|---|---|
| 2000 | 852-816-332 | +0.269 | none | **hold_for_more_data** |

Score 50.9% -> a point estimate of about +6 Elo for the combined set, not the +44 Elo the
repeated-games record implied. The set is not harmful, but it is **not established** at
the SPRT's declared bounds, and no individual flag in it has valid promotion evidence.

`benchmarks/results/singular-regate-20260909/` — champion vs champion + `--singular`
(the ladder's highest-holding candidate), book offset 1000 (a sample disjoint from the
bundle run), 2000 games:

| games | W-L-D | LLR | boundary | decision |
|---:|---|---:|---|---|
| 2000 | 860-844-296 | -0.401 | none | **hold_for_more_data** |

+2.8 Elo point estimate. The superseded ladder record claimed LLR +2.672 over 3000
repeated games; the 120 unique games it replayed gave LLR +0.107.

Throughput check (`benchmarks/scripts/bench_flag_cost.py`, warmup plus a champion control
at both ends): no promoted flag costs a measurable penalty — champion 0.81 MNPS vs
no-tt2 0.81 MNPS at 1 s/move on the canonical positions, same depth. An earlier sweep
without warmup showed a spurious +15-26% for *every* flag; the control exposed the order
bias, which is why the control is part of the script.

## Individual re-gates and new candidates (continuous chain)

Each gate below is champion vs champion ± one flag, 40k fixed nodes, ~1000 distinct book
positions (no repeats). The W-L-D/LLR are the final `sprt.json` record values (not the
per-batch log lines).

| gate | W-L-D | LLR | decision |
|---|---:|---:|---|
| `loglmr` (new candidate) | 497-392-141 | **+2.965** | **promote** (first valid crossing) |
| `countermove` (new candidate) | 826-855-319 | -1.904 | hold (stays off) |
| `seeprune-regate` | 874-837-289 | +0.291 | hold — not confirmed |
| `caphist-regate` | 846-853-301 | -1.163 | hold — not confirmed |

`loglmr` is promoted (PR #86) and deployed. The re-gates so far say the superseded
promotions are **neutral**, not the +2.9-LLR crossings their records claimed: SEE pruning
and capture history earn their keep at most marginally. The remaining re-gates
(`improving`, `tt2`, `kingact`) and candidates (`iid`, `delta`, `conthist`, `seeverify`,
`rootsafequiet`) run in `benchmarks/scripts/gate_chain.sh` / `gate_chain2.sh`.

> Correction: the commit message on #87 quoted intermediate batch counts for
> countermove/seeprune (743-775-282 / 792-744-264). The final record values are the table
> above; the JSON records were always correct.

## Standing consequences

1. The five promoted defaults stay in place (the point estimate is positive and removing
   them is itself unproven), but their promotion records are superseded by this note and
   the re-gate above. Treat them as unverified until individual re-gates decide.
2. New strength changes are gated with the fixed harness. A gate may only promote on a
   crossing measured over positions the run has not already played.
3. Gate books are now first-class artifacts with provenance; a gate record names the book
   and the number of distinct positions used.
