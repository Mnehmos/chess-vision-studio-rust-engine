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

## Patch 2b — Hardened Null-Move Pruning ✅ PROMOTED

- **Commit:** `354fe01` · binary `uci-patch2b-nullhard.exe` (sha256 `F4583149…`)
- **Baseline:** `76e36a1` (patch1-killers)
- **Change:** make_null/unmake_null (exact hash/ep/halfmove restore); guards:
  never in check, no consecutive nulls, depth ≥ 3, mate-window exclusion,
  static-eval ≥ beta, strong zugzwang filter (major piece or two minors);
  depth-scaled R (2, 3 at depth ≥ 6); fail-hard beta cutoff never stored in
  the TT. `--no-null` kill switch; null_cutoffs telemetry.
- **Tests:** 47/47 incl. zugzwang guard, mate preservation, d4-forensic,
  exact null make/unmake.
- **SPRT** (tc 10+0.1, conc 5): **+142 −85 =66 over 293 games — 59.7%,
  ≈ +68 Elo.** Manually stopped on overwhelming evidence (the sequential
  stop failed to engage; the margin left no ambiguity).
- **Verdict:** new working baseline.

## Patch 3 — Late-Move Reductions (queued)

After ordering quality is proven (it now is): reduce late quiet non-checking
moves at depth ≥ 3, re-search on alpha improvement; no reduction for captures,
promotions, checks, TT move, killers, or the first few moves.
