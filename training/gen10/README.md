# Gen10 evaluator slice — what was tried, what was found (2026-09-11)

Goal: retrain the evaluator on **clean data** now that the game-generation repeat bug is
fixed, and fix the output compression at its source instead of patching it with the
shipped calibration curve.

**Outcome: not yet shippable.** Everything below is evidence for the next attempt: the
recipe is now understood exactly, three real bugs were found and fixed, and the tooling
builds million-position corpora in ~20 minutes. The shipped calibration remains the best
evaluator we have (gate +2.971 LLR ≈ +36 Elo, anchor ≈ +74 Elo).

## What the shipped pipeline actually does (decoded)

`training/gen9/scripts/train_matrix.py` puts `sigmoid(net(x)/256)` in the loss against

    target = 0.6 * sigmoid(cp/256) + 0.4 * game_result        (export: outputScaleCp = 400)

Because the engine consumes the net **linearly** (`eval = 400 * net(x)`) and the sigmoid
in the loss runs in its linear region, the effective target in engine-eval space is

    eval_target(cp) = 1024 * (sigmoid(cp/256) - 0.5)

i.e. a smooth, saturating compression of centipawns: +300cp maps to ~+162, +1000cp to
~+295, and everything beyond ~±1200 saturates near ±512. That is the compression the
shipped `--nnue-cal` curve inverts (measured slope 0.49 raw → 0.87 calibrated, static MAE
vs Stockfish 126 → 85).

So: the compression is **structural to this architecture** (hidden layer clamped to [0,1]),
not a bug. Training a *linear* cp target with the same architecture was tried — targets
±1500cp ⇒ net outputs ±3.75 — and it cannot be trained from a cold start: the clamped
hidden layer saturates (position-independent units) and the net collapses to predicting
the mean (holdout MAE ~280cp, engine slope 0.06).

## Bugs found in the new training path (all fixed)

1. **Zero-initialised embedding.** `nn.init.zeros_(embed.weight)` makes every input feature
   identical, so the net can only ever learn a constant. The proven recipe uses
   `normal_(std=0.05)`.
2. **Label POV.** Corpus labels are White-POV; the input encoding is side-to-move relative.
   The loader must flip the target for black-to-move rows (as the proven loader does).
   Without it half the targets are sign-flipped: rank correlation collapsed to r 0.12-0.56
   against the incumbent's 0.92.
3. **Batch offsets.** `EmbeddingBag` needs strictly increasing offsets, so a permuted batch
   must be sorted before slicing the flat index array (CUDA illegal-access otherwise).

## Corpus tooling (kept)

`build_corpus.py` — quiet-filtered (no check, no capture), deduped positions; per-worker
incremental part files (`--resume` skips what is on disk); `movetime`-capped searches so a
starved engine can never wedge a worker; live rows/sec + ETA. Modes:

* `--depth 0` — **static distillation**: SF's own `eval` (~1 ms/position). Built
  **1.5M positions in 20.7 min** with 14 workers.
* `--depth 16 --shallow-depth 12` — search labels with a stability filter (drops positions
  where shallow and deep disagree by more than `--stable-max`). Built 113k positions in 39
  min (the deep search is ~1 s/position under load).

Measured label quality of the *old* corpora (120 sampled rows vs a fresh d24):
**median |delta| 48cp, p90 1156cp** — the same order as the documented "gen7 d12 vs d20 =
65cp" teacher noise, i.e. stale/noisy labels.

## Measured results (held-out positions, vs Stockfish's static eval)

| evaluator | slope | r | MAE (raw → calibrated) |
|---|---:|---:|---:|
| shipped net + shipped curve (reference) | 0.87 | **0.920** | 93.1 → **94.9**\* |
| gen10 mid-target, **4.1M** static labels | 0.481 | **0.872** | 133.7 → 178.4 |
| gen10 mid-target, 1.5M static labels | 0.467 | 0.848 | 138.3 → 170.9 |
| gen10 sigmoid target, 1.5M static labels | 0.150 | 0.815 | 254.7 → 283.6 |
| gen10 sigmoid target, 113k clean search labels | 0.131 | 0.748 | 266.6 → 295.1 |
| gen10 sigmoid target, 113k old labels | 0.084 | 0.659 | 276.4 → 298.9 |

Rank correlation tracks data volume cleanly (113k → 0.748, 1.5M → 0.848, 4.1M → 0.872) but
does not reach the incumbent's 0.920: the shipped net was trained on 7.9M positions with
**search** labels blended with game results, which carry more information than a static
distillation. 4.1M is also the ceiling of this *source*: the shard corpus contains only
that many quiet, deduped positions, so scaling further requires **generating new
positions** (self-play across the 4,910 distinct inv1 book openings — repeat-free by
construction — plus the live bot's games), not re-sampling the same shards.

\* the reference's own curve was fitted on a larger sample than this run's 288 positions,
hence "calibrated" not improving on raw here.

## Conclusion / next attempt

1.5M clean static-distillation labels do **not** beat the shipped 7.9M-row net. Ranking
(r 0.85 vs 0.92) is the gap, and it is a data/architecture gap, not a target-encoding one
(now that the encoding is decoded and matched). Next attempt should either

* **scale the corpus** to the full ~7.9M positions (static distillation: ~2 hours at the
  measured 1000 labels/s) so only the *label quality* differs from the incumbent, or
* **change the architecture** so a linear cp target is trainable (two hidden layers, or an
  unscaled accumulator), which would remove the need for any calibration curve,

and then gate with `benchmarks/scripts/gate_ladder.py` (per-gate `net` override is wired, so
a candidate net can be swapped in while the baseline keeps the champion's).

Measured status of the two paths (2026-09-11):

* **Data-only retrain: does not beat the shipped net.** 4.1M clean static labels reach
  r 0.872 / MAE 134 against the incumbent's r 0.920 / MAE 93. Scaling the *existing*
  source is exhausted (4.1M quiet positions is all the shard corpus has).
* **Architecture: linear centipawn target needs a two-layer head.** Not attempted in
  code. The shipped one-layer [0,1]-clamped head can only learn a bounded, compressed
  score (which `--nnue-cal` inverts); a linear target collapses to the mean under every
  init/LR/embedding-scale variant tried.

## Linear centipawn target: solved the *training*, not yet the *accuracy* (2026-09-12)

The collapse was a **cold-start** problem, and it is now understood and worked around
without any engine change:

* warm-starting from an already-trained net and fitting **only a linear output head**
  trains immediately (`--init-from ... --freeze-features`): slope 0.65, MAE 114 raw --
  usable centipawns, **no calibration curve needed**;
* joint fine-tuning with **discriminative LRs** (head 2e-3, features 1e-4) improves it
  further: r 0.871 -> 0.881, MAE 114 -> 110, slope 0.69;
* a second round on the full 4.1M corpus is flat (r 0.881, MAE 110): converged.

Mechanism: with a linear target the initial output already equals the target mean, so the
output layer gets no coherent gradient while the hidden layer is still position-independent;
the shared-LR joint run destabilises (114 -> 262) because the head must move ~10x faster
than the features it reads.

So a **linear-centipawn evaluator is now trainable** -- `target-cvs/gen10-linear-r2.json`
is such a net (`outputScaleCp=400`, linear semantics, no calibration) -- but it is still
*less accurate* than the shipped compressed net + curve (r 0.881 / MAE 110 vs 0.920 / 93).
The remaining gap is **feature quality**, not the target encoding: the features were trained
for the compressed objective, and closing the gap needs new features (wider/deeper) and new
data, i.e. the project described above.

So the next evaluator attempt needs *new* position generation (self-play over the
distinct-opening book + live games) labelled with search scores, not another pass over
the existing corpus.
