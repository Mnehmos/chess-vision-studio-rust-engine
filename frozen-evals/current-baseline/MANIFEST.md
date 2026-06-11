# Frozen baseline — current promoted engine (2026-06-10)

Freeze created per the CVS Engine+NNUE architecture brief's non-negotiable
preservation rule: **freeze before any new model training.** Nothing here is
overwritten by later CVS-NNUE / gen-2 work; new models get new unique names.

## Promoted champion (default engine)

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
