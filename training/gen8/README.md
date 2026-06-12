# Gen8 Training Buildout

This folder is the operational gen8 pipeline. It turns benchmark games and
self-play into relabelable JSONL, then adapts Stockfish labels into the existing
raw NNUE trainer format.

Gen8 starts with the controlled candidate:

```text
gen8-raw-h256-sf-d20
```

Same raw 768 piece-square input and h256 hidden size as gen7. The first test is
label quality, not a new feature geometry.

## Layout

| path | purpose |
|---|---|
| `manifest.json` | frozen gen8 candidate/data/gate definitions |
| `scripts/extract_pgn_positions.py` | converts PGN games into seed JSONL rows |
| `scripts/shard_jsonl.py` | splits large JSONL files for parallel relabeling |
| `scripts/split_by_source.py` | makes train/validation/test splits by game/source key |
| `scripts/prepare_raw_nnue_rows.py` | converts relabeled rows to `{fen, cp, res}` for `arena/train-nnue.py` |
| `scripts/compare_teacher_depths.py` | compares d12/d20 relabels before spending broad-corpus compute |
| `seeds/hard-pgn-seeds.jsonl` | tracked small seed set from benchmark games |

Large generated files should go under `training/gen8/work/` or an external
drive path, not into git.

## Seed Hard Positions

From the repo root:

```powershell
python training/gen8/scripts/extract_pgn_positions.py `
  --pgn-dir benchmarks/games `
  --out training/gen8/seeds/hard-pgn-seeds.jsonl
```

The seed rows keep full provenance: PGN file, game id, ply, FEN before/after,
played move, result, phase, material, benchmark tags, and `splitKey`.

## Source Splits

Do not random-split positions. Split by game/source so near-duplicates cannot
leak across train/validation/test:

```powershell
python training/gen8/scripts/split_by_source.py `
  --input training/gen8/work/gen8-corpus-candidates.jsonl `
  --out-dir training/gen8/work/splits `
  --prefix gen8-source `
  --train-pct 0.80 `
  --validation-pct 0.10 `
  --test-pct 0.10
```

Freeze final evaluation slices before training:

- gen7 baseline positions
- suite-fresh-100
- suite-hard-100
- eubos endgame seed set
- king-danger set
- passed-pawn set
- quiet set

## Broad Self-Play

The Rust self-play generator already emits trainer-shaped rows:

```powershell
cargo run --release --bin selfplay -- `
  --games 10000 `
  --depth 7 `
  --threads 12 `
  --out training/gen8/work/selfplay-gen7-stack-d7.jsonl `
  --base ../chess-vision-studio/arena/out/value-weights-mixed.json `
  --rung2 ../chess-vision-studio/arena/out/rung2-weights-mixed.json
```

For gen8, this is Tier C distribution data. Relabel it with Stockfish before
training the final candidate.

## Relabel

Shard a seed or self-play file:

```powershell
python training/gen8/scripts/shard_jsonl.py `
  --input training/gen8/work/selfplay-gen7-stack-d7.jsonl `
  --out-dir training/gen8/work/shards `
  --prefix selfplay-gen7-d7 `
  --rows-per-shard 250000
```

Then run the existing app-side Stockfish worker per shard:

```powershell
python ../chess-vision-studio/arena/sf-relabel-worker.py `
  training/gen8/work/shards/selfplay-gen7-d7-0000.jsonl `
  training/gen8/work/relabels/selfplay-gen7-d7-0000.sf-d20.jsonl `
  20
```

The worker writes `sfCp` in side-to-move POV. The trainer expects `cp` in
White-POV, so adapt rows before training. The adapter also defines mate
conversion and cp clipping:

```powershell
python training/gen8/scripts/prepare_raw_nnue_rows.py `
  --input training/gen8/work/relabels/selfplay-gen7-d7-0000.sf-d20.jsonl `
  --out training/gen8/work/train/selfplay-gen7-d7-0000.raw-train.jsonl `
  --mate-cp 32000 `
  --clip-cp 2000
```

For a sample, keep d12 and d20 labels and compare teacher drift:

```powershell
python training/gen8/scripts/compare_teacher_depths.py `
  --shallow training/gen8/work/relabels/sample.sf-d12.jsonl `
  --deep training/gen8/work/relabels/sample.sf-d20.jsonl `
  --out training/gen8/work/relabels/sample.d12-vs-d20.jsonl
```

## Train Raw Candidates

Use the existing trainer from the app repo. It already accepts `--hidden`, so it
can produce h256/h384/h512 raw candidates:

```powershell
python ../chess-vision-studio/arena/train-nnue.py `
  training/gen8/work/train/gen8-raw-d20-train.jsonl `
  --hidden 256 `
  --epochs 12 `
  --out training/gen8/work/models/gen8-raw-h256-sf-d20.json
```

Promote nothing from training loss alone. Every candidate still goes through the
benchmark gates in `benchmarks/README.md`.

Track calibration metrics, not just training loss: avgCP, p90, bl >= 100,
bl >= 200, danger avg, quiet avg, endgame avg, correlation with Stockfish, sign
accuracy, and WDL bucket accuracy when available.

For h384/h512, run speed gates before match testing: fixed-depth NPS, 500ms
depth, 5s depth, same-budget cp-loss, and the 20-game screen.

## Immediate Next Build Steps

1. Add the existing 4.78M quiet corpus path to `manifest.json` once its current
   location is confirmed.
2. Run the PGN seed extraction after every new benchmark-game addition.
3. Freeze the locked eval slices and exclude them from training manifests.
4. Generate a small Tier C smoke self-play file and relabel it at d12 and d20.
5. Compare d12/d20 deltas before broad relabeling.
6. Train `gen8-raw-h256-sf-d20` on a small smoke set to validate the full wire.
7. Scale to the broad quiet corpus and hard booster only after the smoke run
   produces a loadable Rust NNUE JSON.

## 2026-06-12 teacher-depth preview (483-row overlap, sample of d12 corpus)

d12 (gen7's labels) vs d20: avgAbsDelta **65cp**, p50 25, p90 167, p95 276;
**51% of positions move >=25cp, 31% >=50cp, 17% >=100cp.** Gen7 trained on
labels that are off by a quarter-pawn on half the positions — the "label
quality is the wall" thesis holds in preview.

d16 vs d20: avgAbsDelta 43cp, 10% >=100cp. So d16 captures roughly 2/3 of the
d12->d20 correction at ~1/3 the d20 cost. LEANING d16 for Tier-A broad corpus,
d20 reserved for the Tier-B hard booster. Final call after the full 3k d20
sample finishes (~2-3h).

## 2026-06-12 FINAL teacher-depth (full 3k sample)

d12 (gen7) vs d20: avgAbsDelta **67cp**, p50 26, p90 160, p95 243;
52% move >=25cp, 32% >=50cp, 18% >=100cp. d16 vs d20: 44cp avg, 10% >=100cp.
Confirms the preview: d20 carries real label-quality signal over d12.
CAVEAT: gen8-v1 failed on DISTRIBUTION + quiet-filter, NOT label depth, so
the d20 signal is orthogonal to the v1 failure. Staged plan: (1) cheap
recipe-fix (our d12 corpus quiet-filtered as broad base + endgame/booster
supplements) to confirm pipeline >= gen7; (2) only then spend d20 relabel of
our corpus to cash the 67cp signal. d16 is the cost-sweet-spot if d20 too slow.
