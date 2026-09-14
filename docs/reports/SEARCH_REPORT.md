# Rust R3 — search report (vs the legacy TS reference)

**Goal (R3):** a playable Rust search stack — negamax/αβ + capture quiescence +
forcing quiet-check extensions + move ordering + PV + telemetry + TT — built in
layers, semantically mirroring the legacy TS `Searcher`.

## Headline: ✅ 18/18 EXACT best-move + score parity with TS, ~250–300× faster

### Search parity (the bench battery: forensic #549, startpos, midgame-r1)

Every cell the TS engine was benchmarked on (depths 2–4 × 3 positions × default
and trained-mixed weights) matches **exactly — both the best move and the score**:

| Cell | TS (post-quiescence-fix) | Rust |
|---|---|---|
| default · 549 · d2/d3/d4 | a2a3@315 · a2a3@310 · h7f7@327 | **identical** |
| mixed · 549 · d2/d3/d4 | b3f7@302 · b3f7@314 · h7f7@303 | **identical** |
| default · startpos · d2/d3/d4 | b1c3@10 · b1c3@40 · b1c3@10 | **identical** |
| mixed · startpos · d2/d3/d4 | d2d4@12 · e2e4@42 · e2e4@12 | **identical** |
| default · midgame · d2/d3/d4 | e5e4@25 · e5e4@5 · e5e4@20 | **identical** |
| mixed · midgame · d2/d3/d4 | e5e4@27 · e5e4@4 · e5e4@21 | **identical** |

This includes reproducing the TS engine's **known imperfection**: at d5 the mixed
head still picks the quiet-refuted b3f7 (308cp), recovering to h7f7 at d6 — the
same residual the TS forensic documented. Same chess, including the warts.

### Forensic positions (the d4/d5 lesson)

| Depth | Rust mixed move | Note |
|---:|---|---|
| 4 | **h7f7** ✓ | quiet-check extension avoids the b3f7 blunder (regression-tested) |
| 5 | b3f7 | same residual as TS (refutation beyond the check-extension horizon) |
| 6 | **h7f7** ✓ | recovers, like TS — but Rust reaches d6 in **0.36s** vs TS ~minutes |

The practical fix for the d5 residual class is now simply *depth*: Rust does d6
in less time than TS spent on d2.

### Speed (release, same machine as the TS bench)

| Cell (d4) | TS time | Rust time | Speedup |
|---|---:|---:|---:|
| default · 549 | 6,547 ms | **23 ms** | ~285× |
| mixed · 549 | 4,559 ms | **17 ms** | ~268× |
| default · startpos | 1,187 ms | **5 ms** | ~237× |
| mixed · midgame | 10,764 ms | **36 ms** | ~299× |

Throughput: **~0.6–1.5M nodes/sec** (vs TS ~1–4k) — ~300×. Depth 6 (both weight
sets, all bench positions) runs in 0.18–0.55s.

### What was built (layered, per the R3 plan)

- **R3.1** negamax/αβ + iterative deepening; TS mate scoring (±MATE_SCORE − ply),
  terminal handling (mate/stalemate/insufficient/50-move) at every node.
- **R3.2** capture quiescence: fail-hard stand-pat, SEE ≥ 0 capture/promo filter,
  all-evasions when in check, MAX_QUIESCENCE_PLY=64 — TS rules exactly.
- **R3.3** forcing quiet-check extensions: `gives_check` (exact, incl. discovered),
  SEE ≥ 0, first 2 q-plies, ≤3 per node — the d4 lesson, with telemetry.
- **R3.4** ordering: TT move ≫ captures (MVV-LVA, TS captureOrder formula) ≫
  promotions. (TS's SAN-based check bonus is ordering-only and omitted; values
  are unaffected — confirmed by the exact score parity above.)
- **R3.5** TT: Zobrist-keyed (incremental hash, undo-restored, make/unmake-stable
  by test), TS bound semantics (exact/lower/upper, deeper-entry replacement),
  PV extraction by TT walk. Added last, after layer tests were green; verified
  **TT on/off returns identical move + score** on a fixed battery.
- Telemetry: nodes, qNodes, qCaptureNodes, quietCheckExtensions, maxQDepth,
  ttHits, betaCutoffs, elapsedMs (+ mateThreat/hangingMajor scaffolded at 0).

### Acceptance (R3 gate)

- `cargo test` → **35/35** (6 perft + 10 R1 + 8 eval + 11 search) ✅
- perft still green ✅ · eval parity still **exact 0.000000cp** post-refactor ✅
- SEE/check tests green ✅ · mate-in-1 both colors ✅ · tactic suite ✅
- d4 forensic: blunder avoided, regression-tested ✅ · d5 reported (TS-identical) ✅
- speed report + nodes/qNodes/extensions ✅ · best move vs TS: **18/18 exact** ✅
- No TS engine changes; no app/bot wiring; no weight changes; no Rung-3 PST ✅

### Next (R4)

Gate parity/superiority: Rust searched moves vs TS searched moves vs the
Stockfish scorer, same FEN slices (holdout + independent), depths 2/3/4 (and the
deeper depths only Rust can afford), runtime comparison. The exact search parity
above predicts the R4 quality gates will match TS cell-for-cell — and depth 5–6
should then *beat* the TS engine outright on searched-move quality.
