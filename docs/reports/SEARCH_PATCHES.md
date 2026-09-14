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

## Patches 3–6 — Full Stack ✅ PROMOTED (gated as one generation)

- **Binary:** `uci-gen6-full.exe` (sha256 `59EAD1FF…`) at commit `9c4180d`
- **Baseline:** `patch2b-nullhard` (354fe01)
- **Contents:** fixed 2^21-slot persistent TT with generation aging (Patch 4),
  conservative LMR (Patch 3, `--no-lmr`), PVS + root aspiration windows
  (Patch 5, `--no-pvs`), specialized noisy-only quiescence generation + node
  reorder with TT-probe-before-movegen (Patch 6, semantics-preserving).
- **Tests:** 47/47 at every step; Patch 6 verified node-identical.
- **Throughput:** startpos d6 352ms → 35ms across the day's full stack (~10×);
  kiwipete d6 −35% wall at identical nodes.
- **Gate** (tc 10+0.1, conc 12, 300 games, fixed count):
  **+196 −47 =57 — 75.2%, Elo +192.4 ± 39.3, LOS 100%.**
- **Verdict:** new champion. Native SF-2400 quiet-box anchor next; bot rollout
  follows the anchor.

## Next Search Queue — Gen7 Snapshot Era

The next search work is ordered for one-variable gates against
`snapshot/gen7-acc-futility-2026-06-11`.

1. **Reverse futility pruning:** opt-in `--rfp`; gate with fixed-depth sanity,
   cp-loss suites, telemetry, and 20-game screen.
2. **Pruning/search telemetry:** counters are required before stacking more
   pruning. Use `benchmarks/scripts/bench_telemetry.py`. Columns labeled
   `c/a` are cut/attempt pairs. The `mate_threat_extensions` and
   `hanging_major_extensions` telemetry fields are scaffolded only; their zero
   values do not prove the extension ideas are inactive by policy, just not
   implemented yet.
3. **Late move pruning:** conservative shallow quiet pruning after ordering has
   had a chance; protect quiet defensive resources.
4. **SEE pruning:** skip badly losing captures/tactical garbage; soften around
   checks, promotions, PV nodes, and king attacks.
5. **QSearch cleanup:** SEE-gated captures, delta pruning, stand-pat discipline,
   limited checking moves, promotion handling, and no endless bad capture chains.
6. **Aspiration windows:** previous-score windows with fail-high/fail-low
   re-search accounting.
7. **Move ordering upgrades:** countermove, continuation history, capture
   history, history maluses, aging, hash-move validation, killer hygiene.
8. **LMR retune:** tune by depth, move index, PV, capture/quiet, checks,
   history, killer/countermove, improving flag, and static margin.
9. **Null-move retune / verification:** add endgame and pawn-ending caution;
   consider verification search at deeper depths.
10. **Razoring and extensions:** later-stage, after RFP/LMP/SEE/qsearch are
    stable.

Diagnostic target for every search change:

```text
effective branching factor drops
time-to-depth improves
CP-loss stays sane
catastrophic blunders stay flat
candidate beats the frozen snapshot
```

RFP telemetry gate shape:

```powershell
python benchmarks/scripts/bench_telemetry.py --suite canonical --base-exe target/release/analyze.exe --exe target/release/analyze.exe --extra "--rfp"
python benchmarks/scripts/bench_parity.py --extra "--rfp" --depths 6,7,8
python benchmarks/scripts/bench_cploss.py --suite suite-fresh-100 --extra "--rfp"
python benchmarks/scripts/bench_cploss.py --suite suite-hard-100 --extra "--rfp"
python benchmarks/scripts/bench_screen.py --cand-args "--futility --rfp" --base-args "--futility"
```
