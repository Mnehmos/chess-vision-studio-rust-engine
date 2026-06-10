# Search Patch Reports

Promotion discipline: every patch ships with code summary, test output,
node-count sanity, SPRT self-play result, and (when the box is quiet) a native
SF-2400 anchor reading. Relative gates run at high concurrency (fair — both
sides share the load); absolute anchors run at concurrency 1 on a silent box.

## Patch 1 — Killer + History Move Ordering ✅ PROMOTED

- **Commit:** `76e36a1` · binary `uci-patch1-killers.exe` (sha256 `47A9A017…`)
- **Baseline:** `7bcb33b` (repetition fix + UCI, pre-ordering)
- **Change:** two killer slots per ply; depth²-weighted side/from/to history
  table; ordering `TT > winning captures/promotions (SEE-split) > killers >
  history quiets > rest > losing captures`; tables update only on quiet beta
  cutoffs; reset per search call (determinism); `killer_cutoffs` /
  `history_cutoffs` telemetry.
- **Tests:** 42/42 including the 18-cell exact best-move/score parity battery.
- **Node sanity:** startpos d6 238,403 → 91,263 nodes (−62%) at identical
  eval; midgame d5 neutral (+5%).
- **SPRT** (tc 10+0.1, concurrency 8, elo0=0 elo1=10, α=β=0.05):
  **H1 ACCEPTED** — 153W–68L–51D over 272 games, **Elo +112.3 ± 38.8**,
  LOS 100%, LLR 2.95/2.94.
- **Native SF-2400 anchor:** queued for the next quiet-box window.
- **Verdict:** new working baseline. All later patches gate against `76e36a1`.

## Patch 2 — Null-Move Pruning (in progress)

Planned per the mission brief: `make_null`/`unmake_null` in position.rs;
enabled only when not in check, depth ≥ 3, side to move has non-pawn material,
and no consecutive nulls; R=2 first; null results never stored unsafely over
repetition-dependent scores. Gates: full battery + explicit zugzwang
regression positions + SPRT vs `76e36a1`.

## Patch 3 — Late-Move Reductions (queued)

After ordering quality is proven (it now is): reduce late quiet non-checking
moves at depth ≥ 3, re-search on alpha improvement; no reduction for captures,
promotions, checks, TT move, killers, or the first few moves.
