# Engine generations

One page for "which gen is which": every engine generation to date, what changed, where its
artifacts live, and what evidence it has. Machine-readable companions:
`benchmarks/engines.json` (benchmarkable identities, naming standard in
`benchmarks/GENERATION_STANDARD.md`) and `benchmarks/search-switches.json` (every search
switch with its gate evidence). The live flag state is in `docs/SEARCH_SWITCHES.md`.

> **Lab mapping.** These are *engine* generations g6–g11: historical production lineages.
> [chess-vision-studio-lab](https://github.com/Mnehmos/chess-vision-studio-lab) generations
> are frozen scientific regimes and start again at G01 (RESEARCH_PROTOCOL.md). The lab
> imports g6–g11 artifacts through its legacy intake catalog (`catalog/` in the lab repo),
> identified by content hash, not by these names.

## Summary

| gen | eval | training signal | status | evidence |
|---|---|---|---|---|
| g6 | classical handcrafted | — | rollback binary | `F:/tools/cvs-baselines/uci-gen6-full.exe` |
| g7 | raw NNUE 768→256 | Stockfish d12 relabel of self-play | **frozen baseline** (`g7.raw-h256.sf-d12.v3`) | gen6→gen7 SPRT +101.9 ±36.5 (`docs/reports/RSI_LOOP_REPORT.md`) |
| g8 | raw NNUE 768→256 | mixed d12 + d20, blend `0.6·σ(cp/256)+0.4·result` | historical champion (`g8.raw-h256.mixed-d12-d20.v2`) | +115 ±63 vs g7, 100-game net swap (README) |
| g9 | raw 768→256 main + core104 residual helper (+ flat/ranker candidates) + rung2 | gen9 corpus, 7.86M rows (`training/gen9/gen9-cvs`, 79 shards) | **N0 frozen reference** (`g9.current-default.raw-plus-residual`, `benchmarks/N0-identity.json`) | ~2520 vs native Stockfish @10+0.1 (`benchmarks/ANCHOR_2026-09-10.md`) |
| g9 + cal | g9 + piecewise output calibration (`nets/eval-cal.json`) | calibration fitted on 1,500 gen9 positions vs SF static eval | **flagship eval** | `nnuecal-gate-20260911`: SUPPORTED, LLR +2.971 @ 1000 games |
| g10 | raw 768→256, many target/data variants | static distillation and search labels (`training/gen10/corpus*`) | experimental, not shippable | `training/gen10/README.md`: best r 0.886 vs incumbent 0.920 |
| g11 | raw 768→256 (`target-cvs/gen11-lich.json`) | 6M Lichess eval-DB d16 search labels, warm-started, sigmoid-mid target, no calibration | candidate, INCONCLUSIVE | `gen11-gate-20260913`: LLR +0.356 @ 2820 games (book exhausted); `gen11-gate2-20260913`: +0.175 @ 820 |

Search work is gated independently of eval generation. "Current" = gen9 nets + calibration
+ every search switch at its source default (see `docs/SEARCH_SWITCHES.md`).

## Artifact locations

| location | tracked | contents |
|---|---|---|
| `nets/` | yes | the shipped flagship set: `matrix-raw.json` (`b23c75b8…`), `matrix-residual.json` (`d2a98888…`), `eval-cal.json` (`89e79a1b…`) |
| `target-cvs/` | no (build dir) | working copies of the flagship set plus every candidate net: `matrix-flat`, `matrix-ranker`, `400k-v1` (768→512), `gen10-*` (13 nets), `gen11-lich`, `gen11-cal`, calibration refits |
| `F:/tools/cvs-baselines/` | no (frozen) | frozen baseline binaries per generation and patch (`uci-gen6-full`, `uci-gen7-*`, `uci-gen8v2-champion`, `analyze-gen9-N0-00ccf79f`) and the gen7 net |
| `training/gen8/` | yes | gen8 plan, manifest, seeds (`docs/GEN8_TRAINING_PLAN.md`) |
| `training/gen9/` | yes (79 shards + `.meta.json` checksums, ~1.2 GB) | gen9 corpus shards and training scripts |
| `training/gen10/` | scripts yes, `corpus*/` ignored | gen10 corpus tooling and trainers; corpora are regenerable |
| `training/funnel/` | yes (runs ignored) | information-gain labeling funnel (#111) |

A net without `trainingCommit` + `datasetManifestHash` metadata is a development artifact,
not a promotion candidate (`benchmarks/GENERATION_STANDARD.md`). None of the g10/g11 nets
carry that metadata yet; the lab intake catalog records this as `provenanceComplete: false`.

## Corpora

| corpus | rows | label contract |
|---|---:|---|
| `training/gen9/gen9-cvs` | 7,861,317 | `cp`, `res`, CVS core feature ids (registry v1) |
| `training/gen10/corpus` | 113,207 | Stockfish d16 `cp` + d12 `cpShallow`, stability-filtered quiet positions |
| `training/gen10/corpus-d20` | 599,363 | CVS NNUE self-play, Stockfish d20 `cp`, `cp_play`, `res` (43,405 unique positions) |
| `training/gen10/corpus-d20-local` | 253,113 | same contract, local run |
| `training/gen10/corpus-static` | 1,500,000 | Stockfish static eval |
| `training/gen10/corpus-static-full` | 4,143,821 | Stockfish static eval over the whole gen9 source |
| `training/gen10/corpus-lich` | 21,168,494 | Lichess eval DB (`fen`, `cp`) |
| `F:/tools/gen8-*.jsonl`, `endgameq-d20.jsonl` | — | gen8-era public/self-play label files |

## Adding a generation

1. Train with provenance metadata (`trainingCommit`, `datasetManifestHash`, rows, epochs).
2. Register an identity in `benchmarks/engines.json` (naming: `GENERATION_STANDARD.md`).
3. Gate it as a net swap against the flagship (`benchmarks/scripts/gate_ladder.py`, `kind: net`).
4. Add its row here with the gate record path, including HOLD and REJECT outcomes.
