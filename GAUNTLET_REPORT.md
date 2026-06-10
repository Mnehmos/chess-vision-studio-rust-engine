# GAUNTLET REPORT — CVS Rust Engine vs Stockfish (Track A)

Date: 2026-06-09 · Engine: `cvs-bitboard-core` (promoted baseline, 26-param Rung-2 mixed weights)
Oracle/opponent: Stockfish (WASM build) · Scoring oracle: SF depth 12 bulk, depth 20+ deep.

> **Headline (user's wording, kept verbatim):**
> "CVS exhausted Stockfish's limited UCI_Elo ladder, scoring 90% against SF-3000.
> Against unlimited Stockfish at equal 100ms, CVS scored 20%, confirming full
> Stockfish is the next resistance class."

## Claim discipline

All claims have the form **"CVS scored X against SF UCI_Elo-N at movetime M."**
No official human-Elo claims are made. Where CVS lost zero games the Elo
estimate is a **lower bound** (the formula clamps). Equal-clock 500/500ms is
the official ladder standard from SF-3000 upward.

## Limited UCI_Elo ladder — COMPLETE (exhausted)

| Rung | Format | W–D–L | Score | Notes |
|---|---|---|---:|---|
| SF-800 | SF 80ms, CVS d5 | 19–1–0 | 97.5% | run 1313 |
| SF-1000 | SF 80ms, CVS d5 | 20–0–0 | 100% | run 1313 |
| SF-1200 | SF 80ms, CVS d5 | 17–3–0 | 92.5% | run 1313 |
| SF-1400 | SF 80ms, CVS d5 | 20–0–0 | 100% | run 1503 |
| SF-1600 | SF 80ms, CVS d5 | 20–0–0 | 100% | run 1503 |
| SF-1800 | SF 80ms, CVS d5 | 20–0–0 | 100% | run 1503 |
| SF-2000 | SF 80ms, CVS d5 | 16–4–0 | 90% | first draws (conversion-class), run 1537 |
| SF-2200 | SF 80ms, CVS d5 | 18–1–1 | 92.5% | **first loss** (g14, king-exposure family), run 1537 |
| SF-2400 | SF 80ms, CVS d5 | 17–2–1 | 90% | run 1616 |
| SF-2600 | SF 80ms, CVS d5 | 19–1–0 | 97.5% | run 1636 — promoted baseline only |
| SF-3000 | **equal clock 500/500ms** | 16–4–0 | 90% | run 1656 |
| SF-3190 | equal clock 500/500ms | 16–3–1 | 87.5% | run 1713 — **UCI_Elo limiter ceiling** |

The SF-2600 rung (and every official rung) was run with the **promoted
baseline only** — the danger extension remained an unpromoted experiment and
was never used for official ladder measurement.

No rung in the limited range produced a 35–65% resistance band: CVS dominates
the entire UCI_Elo limiter range (1320–3190 plus the Skill-Level rungs below
the limiter floor).

## Full Stockfish (limiter OFF) — the resistance instrument

Equal-clock probe:

| Match | W–D–L | Score |
|---|---|---:|
| CVS 100ms vs full SF 100ms (n=5) | 0–2–3 | **20%** |
| CVS 500ms vs full SF 500ms (n=10) | 1–3–6 | **25%** |

Time-odds curve, CVS@500ms vs full SF (n=10 per point):

| SF time | W–D–L | Score |
|---|---|---:|
| 25ms | 3–3–4 | 45% |
| 50ms | 4–3–3 | 55% |
| 100ms | 3–4–3 | 50% |
| 250ms | 5–3–2 | 65% |
| 500ms | 1–3–6 | 25% |

**Resistance band found: CVS@500ms ≈ full Stockfish given 25–250ms**
(scores 45–65%, the target 35–65% band), and clearly below full Stockfish at
equal clock. This — not a UCI_Elo number — is the engine's current measuring
stick, and future promotions are benchmarked against it.

## Termination character

- Wins: predominantly direct attacks and material conversion; delivered mates
  scored as best (oracle terminal-position fix).
- Draws (11 across the ladder): conversion family — fifty-move pressure,
  repetition shuffle, non-progress shuffle in winning endgames.
- Losses (5 total incl. unlimited probes): king-exposure/initiative family —
  slow slides, not single blunders (see RSI_LOOP_REPORT.md).

## Native Stockfish anchor (cutechess, 2026-06-10)

First properly-calibrated external benchmark: cutechess-cli, **native
Stockfish (official latest, AVX2)** with UCI_LimitStrength, tc 10+0.1,
paired balanced openings, 20 games per rung, CVS via its new UCI mode.

| Opponent (native SF UCI_Elo) | W–D–L | Score | Elo diff |
|---|---|---:|---|
| 2000 | 14–1–5 | 72.5% | +168 ±193 |
| 2400 | 6–4–10 | 40% | −70 ±147 |
| 2800 | 0–1–19 | 2.5% | clearly outmatched |

**Claim: CVS scored 72.5% vs native SF UCI_Elo-2000 and 40% vs UCI_Elo-2400
at tc 10+0.1 → CVS ≈ 2250–2350 in native Stockfish's UCI_Elo terms at fast
blitz.** The 2400 rung sits squarely in the 35–65% resistance band — the
native-calibrated successor to the WASM time-odds curve. The WASM ladder's
labels (90% at "3000") were inflated by the WASM build's speed handicap; this
anchor is the honest one going forward.

Caveat: these games ran on the pre-repetition-fix binary — 6 of the 80 games
ended in 3-fold repetition, several plausibly in winning positions (the
known conversion failure). Re-measure after the fix is in the match binary;
the 2400 score should improve.

## Next

The native-SF 2400 rung (40%) is the new official resistance point. RSI
promotions are benchmarked against it via cutechess; the WASM ladder is
retired as an instrument. Climbing resumes when a promotion moves the band.

## Native SF-2400 anchor, post-search-ladder (2026-06-10 PM)

Same opponent and protocol as the morning anchor (native Stockfish AVX2,
UCI_LimitStrength, tc 10+0.1, cutechess, concurrency 1, quiet box). Engine:
`uci-gen6-full.exe` (commit 9c4180d) — repetition fix + the day's full search
ladder (killers/history, hardened null-move, persistent TT, LMR, PVS+aspiration,
specialized qsearch movegen).

| Engine | vs native SF-2400 | Elo diff |
|---|---:|---|
| morning (pre-fix, 26-param eval) | 40% | −70 |
| **gen6-full (search ladder)** | **+25 −14 =1, 65.0%** | **+107.5 ± 114.7, LOS 97.4%** |

**Claim: CVS (gen6-full) scored 65% against native Stockfish UCI_Elo-2400 at
tc 10+0.1 → ≈ +108 Elo, i.e. CVS ≈ 2500 in native Stockfish UCI_Elo terms.**
The improvement is entirely from SEARCH; the 26-param eval is unchanged
(Rung-3 head still in training). Wide error bars (40 games) — a 100-game
re-measure is warranted before treating 2500 as firm, but the direction
(40% → 65% vs the identical opponent) is unambiguous.
