# Gen8 Training Plan

Gen8's job is narrow: make the hot-path evaluator stronger without giving back
the speed and depth gains from gen7.

Operational scaffold lives in `training/gen8/`. That folder contains the
manifest, seed extraction, sharding, relabel adaptation, and first tracked hard
PGN seed set.

The first candidate must isolate label quality:

```text
gen8-raw-h256-sf-d20
```

Same 768 piece-square input, same h256 hidden layer, same accumulator-compatible
raw NNUE geometry. Better labels first; bigger inputs later.

## Starting Point

Gen7 was trained on:

```text
4.78M quiet positions
filtered from 7.43M Stockfish depth-12 labeled positions
from the larger ~7.7M corpus

architecture:
768 piece-square inputs
-> 256 clipped-ReLU hidden
-> 1 output
~197k parameters
```

That broke the old ceiling because the previous labels were weak. Gen8 should
test whether the next ceiling is label quality, model capacity, or input
geometry, in that order.

## Hard Rule

Do not put full CVS geometry into the main every-node NNUE yet.

The CVS-NNUE path fit training better, but full recompute was too expensive in
play mode. For gen8, the hot path stays raw and incremental. CVS geometry
belongs in side systems until it proves a same-budget play-mode win:

- analysis mode
- candidate explanation
- selector or risk model
- root/PV helper
- training diagnostics

## Dataset Ladder

The next training set should combine stronger relabeling with targeted
coverage:

```text
gen7/futility/ponder self-play
+ existing corpus
+ hard positions from losses
+ endgame/pawn-race positions
+ danger/king-safety positions
-> Stockfish d20 relabel
-> train gen8 raw
```

Relabel in tiers instead of blindly sending the full corpus to d20.

## Tier A: Broad Quiet Corpus

Use the current quiet corpus as the clean base:

```text
4.78M quiet positions
SF d16 or d20
main value target
```

This produces the controlled `gen8-raw-h256-sf-d20` candidate. If d20 is too
expensive for the whole broad set, use d16 for the broad set and reserve d20+ for
the hard booster.

## Tier B: Hard Booster

Add 250k-1M hard positions, oversampled during training, from:

- CVS losses
- eubos-style endgame conversions
- passed-pawn races
- rook trades
- king-danger positions
- positions where gen7 cp-loss was high
- positions where gen7 and Stockfish disagree

Current seed artifact:

- `benchmarks/games/eubos-2453-passed-c-pawn-conversion.pgn`

The hard booster should attack known failure modes without bloating the entire
training distribution.

## Tier C: Fresh Self-Play

Generate fresh positions from the actual current stack:

```text
gen7 + accumulator + futility + ponder
```

This matters because the engine now reaches positions that older gen6/self-play
did not. Deduplicate and balance by:

- position hash
- eval bucket
- phase
- material
- danger/quiet/endgame bucket
- repeat frequency

## Candidate Order

Train and gate candidates in this sequence.

### A. gen8-raw-h256-d20

```text
768 -> 256 -> 1
SF d20 labels
same accumulator-compatible input
```

This is the control. It must exist. If it beats gen7, label quality moved the
needle. If it does not, the h256 raw net may already be label-saturated enough
for this architecture.

### B. gen8-raw-h384-d20

```text
768 -> 384 -> 1
```

This tests whether gen7 is capacity-limited while staying raw and incremental.
It must pass the speed gates; a bigger net that loses too much depth is not a
promotion.

### C. gen8-raw-h512-d20

```text
768 -> 512 -> 1
```

Only train this if h384 looks promising. Treat it as a capacity probe, not the
default assumption.

### D. King-Bucket / HalfKP-Lite

After raw h256/h384/h512 tests, evaluate a king-conditioned input geometry:

```text
piece-square features conditioned by king bucket
```

This lets the net learn that the same piece-square can mean different things
against different king locations, for example a knight on f5 near a castled king
or a rook on e1 against a king on e8. It is more implementation work and should
not precede the controlled raw-net tests.

## Label Schema

Do not save only a centipawn number. Each training row or sidecar record should
preserve:

- FEN
- side to move
- Stockfish d20 eval
- Stockfish best move
- Stockfish PV
- Stockfish WDL, if available
- game phase
- material count
- danger tags
- endgame tags
- gen7 eval
- gen7 chosen move
- gen7 cp-loss
- source bucket

Static NNUE value training should remain quiet-position first. Noisy tactical
positions are often better suited for move ordering, selector, policy, or helper
training.

## Training Hygiene

Gen8 can fail by lying to the validation loop. These guardrails are mandatory.

### Split By Source

Split by game/source, never by random position. Near-duplicate positions from
the same game must not leak across train, validation, and test.

Use source-level splits:

- train games/sources
- validation games/sources
- test games/sources
- hard-suite holdout

Seed rows carry `splitKey`/`sourceKey` for this. Use
`training/gen8/scripts/split_by_source.py` before training.

### Locked Evaluation Slices

Before training the first real gen8 candidate, freeze the final eval gate and do
not train on those slices:

- gen7 baseline positions
- suite-fresh-100
- suite-hard-100
- eubos endgame seed set
- king-danger set
- passed-pawn set
- quiet set

These are comparison instruments, not training material.

### Label Normalization

For Stockfish labels, record and keep stable:

- mate score handling
- centipawn clipping range
- Stockfish `sfCp` POV: side-to-move
- trainer `cp` POV: White
- result/WDL convention
- static eval vs child-search label format

The adapter script converts side-to-move `sfCp` to White-POV `cp` and clips
labels explicitly. Gen7 only worked after POV correction; Gen8 must not regress
there.

### Teacher Delta

For a sample of positions, keep both d12 and d20 labels:

- SF d12 eval
- SF d20 eval
- signed delta
- absolute delta

Use `training/gen8/scripts/compare_teacher_depths.py`. If d20 barely changes
quiet positions, spend compute on the hard booster before relabeling all 4.78M
rows.

### Hard Booster Weight

Oversample the hard booster, but do not let it dominate:

```text
80-90% broad quiet corpus
10-20% hard booster
```

Test hard-booster weights separately:

```text
hard weight 1x
hard weight 2x
hard weight 4x
```

The failure mode is a net that improves on eubos-style failures while getting
worse in ordinary positions.

### Calibration Metrics

Do not judge by holdout loss alone. Track:

- holdout loss
- avgCP
- p90
- bl >= 100
- bl >= 200
- danger avg
- quiet avg
- endgame avg
- correlation with Stockfish
- sign accuracy
- WDL bucket accuracy, if available

Gen7 won by calibration and blunder collapse, not imitation rate. Judge gen8 the
same way.

### Immediate Speed Gates For Larger Nets

For h384 and h512, run speed gates immediately:

- fixed-depth NPS
- 500ms depth
- 5s depth
- same-budget cp-loss
- 20-game screen

A bigger net promotes only if eval quality beats the depth it costs.

## CVS Side Models

The most likely CVS contribution to gen8 is not full every-node eval. Prefer
side outputs:

```text
gen8 raw NNUE:
  hot-path value

CVS selector:
  predicts when gen8 is likely wrong
  predicts candidate cp-loss
  predicts whether a helper move deserves verification

CVS explanation head:
  tags king danger, defender removal, passed-pawn danger, SEE ambiguity
```

This keeps raw NNUE deep while letting CVS guide analysis and selective help.

## Gate Sequence

For each candidate:

1. Freeze gen7 snapshot as the baseline.
2. Build the relabel set from Tier A, Tier B, and Tier C.
3. Relabel broad positions at SF d16/d20 and hard positions at SF d20/d24.
4. Train `gen8-raw-h256-d20`, then h384, then h512 only if justified.
5. Run fixed-depth sanity.
6. Run Gate 3 cp-loss suites.
7. Run Gate 5 20-game live-dev screen.
8. Run 80+ games only if the candidate is promising.
9. Use SF ladder and promotion gates only after the screen/confirm path.
10. Test king-bucket / HalfKP-lite only after raw-net candidates are resolved.

## Main Bet

The next hot-path strength probably comes from:

```text
better labels + slightly larger raw NNUE + better search
```

The next CVS strength contribution probably comes from:

```text
selector / explanation / root helper / analysis layer
```

Gen8 motto:

```text
Raw NNUE owns the hot path. CVS learns when the hot path needs help.
```
