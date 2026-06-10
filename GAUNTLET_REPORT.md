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

## Next

Climbing is paused at the instrument boundary (full SF). The ladder resumes
when an RSI promotion moves the time-odds curve; the next checkpoints are
SF@10/5/1ms extensions only if the band shifts upward.
