# Frozen baseline — current promoted engine (updated 2026-06-11)

Freeze created per the CVS Engine+NNUE architecture brief's non-negotiable
preservation rule: **freeze before any new model training.** Nothing here is
overwritten by later CVS-NNUE / gen-2 work; new models get new unique names.

## Promoted champion — GEN-7 (raw NNUE, promoted 2026-06-11)

| Artifact | File | sha256 (first 16) |
|---|---|---|
| UCI engine | `f:\tools\cvs-baselines\uci-gen7-rawnnue.exe` | `780995d1` |
| Net | `f:\tools\cvs-baselines\raw-nnue-h256-sf-d12-v3.json` | `4cc9765c` |
| Base/rung2 weights | same as gen6 (search scalars unchanged) | — |

- **Eval mode:** raw NNUE 768×h256, trained on SF-d12 quiet-filtered
  POV-corrected labels (7.43M → 4.78M rows). SF taught labels only; SF does
  not power runtime eval.
- **Promotion gate (the rule below, satisfied):** SPRT vs gen6 champion,
  10+0.1, F:\tools\openings.epd — **+101.9 ±36.5 Elo, LOS 100%, bound
  crossed at 305 games (+168 −81 =56). Formal pass.**
- **Absolute anchor:** −24.4 ±63.5 vs native SF-2400 (UCI_Elo), 100 games,
  quiet box, conc 1 ⇒ ≈2375. (gen6's old 65%@40 anchor was small-sample;
  100-game gen6 re-anchor pending for the honest delta.)
- **Oracle cp-loss (d5 suite-100, SF-d12 child evals):** avg 23.2cp vs gen6
  29.6cp; blunder≥200cp 1% vs gen6 5%; exact oracle-match parity (51% vs
  52%) — gen7 wins on calibration, not imitation.
- **Rollback path:** repoint default to `uci-gen6-full.exe` (`59ead1ff`),
  section below — binary untouched on disk.

## Previous champion — gen6 classical (rollback target)

| Artifact | File | sha256 (first 16) |
|---|---|---|
| UCI engine | `f:\tools\cvs-baselines\uci-gen6-full.exe` | `59ead1ffde07b1d7` |
| Serve/analyze | `f:\tools\cvs-baselines\analyze-gen6.exe` | `8c2eb81874d4fb19` |
| Base weights | `arena/out/value-weights-mixed.json` | (sha A3435D1A) |
| Rung-2 scalar weights | `arena/out/rung2-weights-mixed.json` | (sha A91268E5) |

- **Eval mode:** `rung2_scalar` (handcrafted material+PST+tapered + 23 Rung-2
  hazard scalars). This is the brief's `rung2-scalar-baseline-frozen`.
- **Search:** gen6 stack (killers/history, hardened null, LMR, PVS+aspiration,
  fixed TT, specialized qsearch). Source: rust master.
- **Source commit:** `c5835c2` (includes lazy SMP, default threads=1).
- **Strength:** +372 self-play Elo over the pre-gen6 baseline; 65% vs native
  SF-2400 at 10+0.1 (+107.5 ±114.7, LOS 97.4%) ⇒ ≈2500 native UCI_Elo.

## Other frozen binaries (study / lineage, not default)

`f:\tools\cvs-baselines\`: uci-7bcb33b.exe (D878AB3E, pre-patch baseline),
uci-patch1-killers.exe (47A9A017), uci-patch2b-nullhard.exe (F4583149),
uci-patch7-pruning.exe (REJECTED −188 Elo, flags default-off).

## Raw-NNUE lineage (UNPROMOTED — the brief's `raw_nnue_gen1`)

768 piece-square stm-perspective net, f32 forward (`src/eval/nnue.rs`),
`--nnue <json>` opt-in. Gen-1 results, all rejected vs gen6:
- h128 / 950k d5 rows: ≈−155 Elo (30%).
- h256 / 7.7M d5 rows: −25.2 ±31.6 Elo (47%, LOS 5.9%).
- gen-2 (d7 labels) datagen in progress at freeze time → `nnue-data-gen2.jsonl`.

These are NOT promoted and do NOT replace the default. They are the
piece-square baseline the planned CVS-geometry NNUE must beat at equal
hidden-size/data to constitute research evidence.

## Restore / verify

The default engine is unchanged on disk; nothing to restore unless a later
promotion is rolled back. To reproduce the champion binary from source:

```
git -C chess-vision-studio-rust-engine checkout c5835c2
CARGO_TARGET_DIR=target-verify cargo build --release --bin uci
# expect sha256 of target-verify/release/uci.exe == 59ead1ff... (LTO-stable)
```

Verify it plays: `uci-gen6-full.exe` + `position startpos` + `go depth 10`
returns a legal bestmove with score near +0.2 (startpos).

## Promotion rule going forward

No CVS-NNUE / gen-2 model is promoted to default without: registry-hash match
(for CVS-NNUE), speed cost reported, and a match-play gate vs THIS frozen
champion that clears its bound. Each promotion records a rollback path back to
`59ead1ff`.
