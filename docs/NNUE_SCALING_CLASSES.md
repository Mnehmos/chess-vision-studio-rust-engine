# NNUE Scaling Classes

The engine's evaluator is a 768→hidden→1 cReLU net. The `hidden` width is the scaling
knob: more hidden units = more capacity = better position understanding, but more
compute per eval and more data needed to train.

Each class is a point on that curve. Move to the next class only when the current one
is gated, shipped, and its instrument shows it's at ceiling (r and MAE plateau despite
more data). The Lichess eval DB provides ~98M quiet d16+ positions; the Lichess eval DB
plus self-play provides effectively unlimited data.

## The Classes

| class | hidden | params | data at 40:1 | nps (est.) | engine support | status |
|---|---:|---:|---:|---:|---|---|
| **v10-class** | 256 | 197K | 8M | ~700K | ✅ current | ✅ shipped |
| **v11-class** | 512 | 394K | 16M | ~350K | ✅ (hidden ≤ 512) | 🔜 next |
| **v12-class** | 1024 | 788K | 32M | ~175K | ✅ (hidden ≤ 512) | 🔜 |
| **v13-class** | 1536 | 1.18M | 47M | ~120K | needs engine change (hidden > 512) | |
| **v14-class** | 2048 | 1.58M | 63M | ~90K | needs engine change | |
| **v15-class** | 4096 | 3.15M | 126M | ~45K | needs SF-scale training | |

The nps estimates assume the eval is ~30% of search time and scale inversely with
width. The pin-aware movegen (+52% nps) and native SIMD offsets some of this cost.

## Data Requirements

The 40:1 ratio is the measured minimum for the incumbent (7.9M / 197K = 40). Below
that, the net underfits; above it, returns diminish. The Lichess eval DB has ~98M
quiet d16+ positions — enough to properly train up to v14-class (2048 hidden).

Data sources, in order of preference:
1. **Lichess eval DB** (zero labeling CPU, d16+ search labels, 98M quiet available)
2. **Self-play across the inv1 book** (on-policy positions, needs SF relabeling)
3. **Live bot games** (trickle, ~63 quiet positions/hour, but exactly on-policy)

## Scaling Protocol

1. Train the next class warm-started from the current one
2. Score on the eval instrument (MAE + r vs Stockfish's static eval)
3. If the instrument shows improvement, gate with fixed-node SPRT
4. If the gate passes, promote to default and ship

The gate proves the Elo gain; the instrument proves the eval quality. Both must
agree before promotion.

## Jump Points

Each jump needs something the previous class didn't:

- **v10 → v11 (256→512)**: nothing — the engine supports hidden ≤ 512. Just more data.
- **v11 → v12 (512→1024)**: nothing — same.
- **v12 → v13 (1024→1536)**: engine change (`hidden <= 512` debug_assert → raise to
  `≤ 1536`). One-line fix.
- **v13 → v14 (1536→2048)**: same.
- **v14 → v15 (2048→4096)**: needs the two-layer head (the [0,1]-clamped single-layer
  head can't support a linear target at this width) or a different activation.

## The Two-Layer Head Problem

The current architecture is 768 → clamp(acc, 0, 1) → 256 → linear → 1 output. The
[0,1] clamp is load-bearing: it bounds the accumulator so the output layer can learn.
But it also means the net's output is bounded by the weight magnitudes — which is why
a linear centipawn target can't be trained from cold start (the output collapses to
the mean).

SF solves this with a wider accumulator (1024) and multiple output buckets. The path
to v14+ (2048+ hidden) likely needs the same: multiple output buckets by piece count,
or a two-layer head that can support a linear target.

## The Data Pipeline

```
Lichess eval DB ──→ evaldb_extract.py ──→ quiet d16+ positions
                                             │
inv1 book self-play ──→ selfplay_gen ──→ positions
                                             │
live bot games ──→ harvest ──────────→ positions
                                             │
                                             ▼
                                     train_raw_cp.py
                                             │
                                             ▼
                                     genNN-*.json net
                                             │
                                             ▼
                                     bench_eval_quality.py  (instrument)
                                             │
                                             ▼
                                     gate_ladder.py  (SPRT)
                                             │
                                             ▼
                                     promote / reject
```


## Data-to-Parameter Ratio Experiments

The ratio of training samples to parameters is the single most important hyperparameter
for each class. LLM scaling research (Hoffmann et al. "Training Compute-Optimal Large
Language Models", 2022) found that the optimal ratio is ~20 tokens per parameter — not
100:1, not 1:1, but a specific point where the model stops underfitting and starts
overfitting. Chess NNUE may follow a similar curve.

### The experiment: sweep the ratio for each class

For a 512-wide net (394K params), train 8 nets at ratios from 1:1 to 100:1 on the same
data (Lichess eval DB, quiet d16+). Each net is identical except for the amount of data
it sees. Score each on the held-out instrument. Plot MAE and rank correlation vs ratio.

| ratio | positions (394K params) | what we expect |
|---|---:|---|
| 1:1 | 394K | severely undertrained — memorization, no generalization |
| 5:1 | 2M | barely learning the scale of the eval |
| 10:1 | 3.9M | starting to generalize |
| 20:1 | 7.9M | approaching the incumbent's 40:1 at half the width |
| 40:1 | 15.8M | the incumbent's ratio, at half the width |
| 60:1 | 23.6M | past the LLM optimal — possible overfitting onset |
| 80:1 | 31.5M | diminishing returns likely |
| 100:1 | 39.4M | well past optimal for most architectures |

The peak of the MAE-vs-ratio curve is the answer. It tells us:

  * **the minimum data for each class** (where MAE stops improving)
  **whether more data ever helps** (or if we're capacity-limited)
  ***the exact volume needed for the next class** (scale data proportionally to params)

### Why this matters for compute cost

If the optimal ratio is 20:1 (LLM scaling result), then a 1024-wide net (788K params)
needs only 15.8M positions — not 32M. That halves the extraction time and means we can
train a bigger net on less data than the naive 40:1 ratio suggests.

If the optimal is 100:1, we need 79M positions and the Lichess eval DB is barely enough.

The experiment answers this empirically instead of guessing.

### Running the sweep

```
for RATIO in 1 5 10 20 40 60 80 100; do
  CAP=$((394241 * RATIO))
  python arena/evaldb_extract.py F:/tools/lichess_db_eval.jsonl.zst       --out training/gen10/corpus-sweep/ratio-${RATIO}.jsonl       --min-depth 16 --cap $CAP       --exclude training/gen10/corpus-lich/lich-d16-v2.jsonl       --exclude training/gen9/gen9-cvs/shard-*.jsonl
  python training/gen10/train_raw_cp.py       --files training/gen10/corpus-sweep/ratio-${RATIO}.jsonl       --hidden 512 --label-field cp --epochs 16 --lr 0.001       --target-mode sigmoid-mid --init-from target-cvs/matrix-raw.json       --out target-cvs/sweep-ratio-${RATIO}.json
done
```

Then score each on the held-out instrument and plot MAE vs ratio.
