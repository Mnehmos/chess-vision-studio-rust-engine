# Snapshot: gen7 search-stack (consolidated) — 2026-06-11

The first engine we deliberately consolidated and chose to keep. Captures the
full post-gen6 stack: learned eval + restored search fundamentals. Use this as
the clean rollback/reference point; all further pruning gates (RFP, LMP, SEE,
null-tune, LMR-retune) build ONE variable at a time on top of THIS.

## What's in it

| layer | value | status |
|---|---|---|
| Eval | gen7 raw NNUE `raw-nnue-h256-sf-d12-v3.json` (sha `4cc9765c`) | promoted champion (SPRT +101.9 vs gen6) |
| Search core | killers/history, hardened null, LMR, PVS+aspiration, shared TT, lazy SMP | gen6 stack |
| Speed | NNUE incremental accumulator (both-perspective, slot-reuse) | +22% nps (0.71→0.87M), search-identical, 68/68 tests |
| Allocation | stack MoveList (no per-node heap) | perf-neutral |
| Selectivity | futility pruning ON | accepted-with-note (fixed-N +34, NOT formal SPRT; blunder collapse preserved) |
| Opponent clock | ponder (UCI worker / bot top-3 cache) | SPRT formally accepted +91 |

## Frozen artifacts

| use | file | sha256[:16] |
|---|---|---|
| match / cutechess (UCI, ponder) | `f:/tools/cvs-baselines/uci-gen7-acc-futility.exe` | `d5233c07` |
| live bot (serve JSON) | `f:/tools/cvs-baselines/analyze-gen7-acc-futility.exe` | `3376c2ac` |
| eval net | `f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json` | `4cc9765c` |
| weights | `arena/out/value-weights-mixed.json` + `rung2-weights-mixed.json` | (gen6 scalars, unchanged) |

Source commit: `d870520`. Local git tag: `snapshot/gen7-acc-futility-2026-06-11`.
NOTE: source tree also carries inert parallel `--helper-nnue` infra (no effect
without `--helper-nnue`); the SearchOptions futility-default flip is deferred
until that work settles — futility is enabled by run-flag here.

## Reproduce / run

Live bot (gen7 + accumulator + futility + 12s clock + ponder cache):
```
CVS_RUST_EXE=.../target-cand/release/analyze.exe \
CVS_RUST_NNUE=f:/tools/cvs-baselines/raw-nnue-h256-sf-d12-v3.json \
CVS_RUST_FUTILITY=1  npm run lichess:bot
```
Match (cutechess, ponder via `ponder` flag, `--futility` for the futility build):
```
uci-gen7-acc-futility.exe --base ... --rung2 ... --nnue raw-nnue-h256-sf-d12-v3.json --futility
```

## Absolute strength (honest)

gen7 anchor: −24.4 ±63.5 vs native SF-2400 (100g) ⇒ ≈2375; +ponder ≈+91,
+futility ≈+34 more (fixed-N). gen6/gen7 anchors are statistically identical at
100 games — anchors are floors, not deltas. Relative SPRT vs gen6 (+101.9) is
the promotion-grade evidence.

## Rollback

Repoint to `uci-gen7-ponder.exe` (`cb28a544`, no futility) or gen6
`uci-gen6-full.exe` (`59ead1ff`). Binaries untouched on disk.
