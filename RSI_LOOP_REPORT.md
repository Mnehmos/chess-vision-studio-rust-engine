# RSI LOOP REPORT — Track B (failure → diagnosis → patch → gate)

Date: 2026-06-09 · Baseline: promoted Rust engine (R0–R5, 26-param Rung-2 mixed weights)
Oracle policy: SF d12 bulk · SF d20+ for every RSI candidate and every gate.

## Cross-run failure table (12 scored runs, 245 games, ~6,800 scored moves)

| Failure tag | Count |
|---|---:|
| value_miseval | **361** |
| search_horizon | 190 |
| quiet_refutation_missed | 147 |
| hanging_piece_missed | 112 |
| tactical_motif_missed | 99 |
| mate_net_tightening | 75 |
| endgame_conversion | 50 |
| repetition_shuffle | 42 |
| king_safety_missed | 26 |
| mate_missed | 24 |
| opening_structure | 20 |
| passed_pawn_missed | 9 |

Family split: **B (king-exposure / initiative / miseval) ≈ 387** vs
**A (conversion / draw-rule) ≈ 92**. Eval is the bottleneck, and family B is
where the losses live; family A only costs draws in already-won positions.

## What does full Stockfish exploit that limited Stockfish did not?

Deep forensics (d20 oracle + Rust d7 re-search) on the three unlimited-SF
losses (g00/g03/g04), the 28-ply Berlin draw (g02), and the SF-2400 loss (g13):

1. **Eval delusion in initiative/king-attack midgames.** In every loss CVS
   held a *positive or near-equal self-eval* while SF d20 already scored the
   position as decisively lost: gaps of +295, +312, +624, +953cp and
   mate-range. Example (g04): CVS +46cp where SF d20 says −578 with a forced
   attack. CVS never triggers defensive consolidation because its eval never
   tells it anything is wrong.
2. **Death by accumulation, not blunder.** Per-move cpLoss in the losing
   games is small (0.7–5.4 per probe) — full SF converts a long series of
   slightly-inaccurate quiet moves into an attack. Limited SF could not chain
   the pressure; full SF can.
3. **Mixed value/search evidence, value-dominant.** Roughly half the probed
   plies persist at Rust d7 (true `value_miseval`); the rest flip
   (`search_horizon`). Deeper search alone cannot fix the family — the eval
   must price king exposure and enemy initiative.
4. **The Berlin draw was clean.** All probed plies in g02 cost ≤0.4 pawns —
   that draw was a fair result, not a conversion failure. (The huge late-game
   cpLoss entries in g00 are lost-position noise: CVS was already being
   mated; every move "loses" a mate-range amount.)

This is the same signature as the first loss (sf2200-g14). User's diagnosis,
kept verbatim: **"CVS was tactically alive, but it overtrusted
attacking/forcing-looking moves while missing defensive consolidation."**

## Patches tested

### Danger-triggered depth extension (search patch) — REJECTED for game mode
- `danger_level(pos)`: enemy queen + king-zone pressure + off-home-rank → +1/+2 root plies, gated OFF by default.
- Targeted A/B (regression rows from g14): ply24 and ply40 recovered to SF-d20
  first moves; ply20 correctly persisted (eval-head evidence). See
  `DANGER_EXTENSION_AB_REPORT.md`.
- Mini-gauntlet (experimental identity, SF-2200, 5 games): regressed → **not
  promoted**. Kept for analysis/RSI rescoring only. This is the gate system
  working: a patch that helps hand-picked positions but not games stays out
  of the baseline.

### 2B King-Exposure Head (eval patch) — implemented inert, training next
- Four new Rung-2 features in Rust (`king_central_exposure`,
  `enemy_queen_near_king`, `open_center_king_penalty`, `king_escape_deficit`),
  serde-default 0 so all existing weight files load unchanged.
- Parity with TS reference still exact (0.000000cp over 628 fixtures);
  38/38 tests green.
- **Decision (from the table above): train 2B first, defer 2A.** The losses
  blocking the resistance band are family B; conversion failures (family A)
  cost only draws and are second in line.

## Regression assets
- `arena/rsi/regressions.jsonl` — named rows incl. sf2200-g14 ply20/24/40.
- Forensic logs: `arena/out/forensic-{g13,g14,unlimited-sf9999-g00/g02/g03/g04}.log`.

## Gate policy (unchanged, mandatory)
A patch is promoted only if: targeted regressions improve · eval-r4 gate does
not regress · experimental-identity mini-gauntlet does not worsen W/D/L or
blunder rate · illegal = 0 · mate-missed = 0 · SF d20+ confirms · runtime
acceptable. Promoted weights ship with versioned metadata.

## 2B linear head: three fits, three pre-gate failures (2026-06-10)

Training data: 9,540 unique gauntlet FENs, SF d12 multipv-8 labels (sibling
candidates + evals), deterministic fen-hash holdout.

| Fit | Objective | Result |
|---|---|---|
| v1 | ridge on static residual (sfEval − staticEval) | holdout MAE worsened (−5.5cp on firing rows) |
| v2 | mixed: residual regression + sibling-ranking hinge | top-1 flat (37.1% → 36.9% holdout), weights collapse to ±2cp |
| v3 | v2 + nonlinear kingDanger (quadratic attack units, x-ray alignment) | kingDanger earns +7cp/unit but top-1 still flat |

Targeted delusion check (the only test that matters for a targeted head):
the five forensic FENs move 10–50cp toward SF where 300–1000cp is needed;
g03 doesn't fire; g13 moves the wrong way (the danger index sees an attack
SF refutes). **Conclusion: hand-crafted scalars with linear weights lack the
capacity to express king-attack delusion** — the third independent
confirmation of the capacity lesson (after Rung-1 and Phase-B ranking).

All three weight sets remain UNPROMOTED; baseline untouched. The kingDanger
feature itself is kept (inert) — it is a useful INPUT for a higher-capacity
head. Bug lesson recorded: weight JSONs are camelCase (`kingDanger`);
snake_case fields are silently ignored by serde and the head stays inert.

## Lichess halcyonbot loss added to regressions (2026-06-10)

Game: halcyonbot (2249) 1-0 ChessVisionStudioEng (2209), Lichess `4fxkLVBb`,
300+3. CVS as Black left the king uncastled, walked e8/f8/f7/f6/g5/h6/h5,
and was mated by `Qg5#`.

New rows in `arena/rsi/regressions.jsonl`: `Na7`, `Ng8`, `Nc6`, `f5`, `Kf7`,
and `Bd6`. Current candidate `go 500` repeats four of the six bad decisions
(`Na7`, `Nc6`, `f5`, `Bd6`) and avoids two (`Ng8`, `Kf7`). The pre-`Bd6`
position is the important gate row: `danger_level=2`, `kingDanger=5.0625`,
`kingCentralExposure=2`, `kingEscapeDeficit=-1`, yet candidate still plays
`e7d6` into `Qe8+`. Verdict: the signal exists; the value head underprices it.
This is now a first-class Rung-3 king-danger regression.

## Lichess halcyonbot endgame loss added to regressions (2026-06-10)

Game: ChessVisionStudioEng (2209) 0-1 halcyonbot (2240), Lichess `RWvWpJZ1`,
300+3. CVS as White reached a long queen/bishop/pawn endgame and was mated by
`Qb3#` on move 62.

New rows in `arena/rsi/regressions.jsonl`: `Kb2`, `Kc3`, `Bf3`, and `Ka3`.
Current candidate `go 500` repeats all four live moves. This is not the same
Rung-3 center-king family: the endgame FENs do not fire current `kingDanger`
(`0.0` on the key `Ka3` row). Treat it as a queen-endgame/mate-defense track:
passed-pawn race valuation, check-chain survival, and short-mate avoidance.
The critical row is `61.Ka3`, where candidate still chooses `b2a3` with a mate
score (`scoreCp=-999996`, `mate=-4`) into the `Qc2/Qb3#` net.

## Rung 3 MLP head: trained, REJECTED pre-gate (2026-06-10)

28→6→1 MLP (`arena/train-rung3-net.ts`), mixed objective, 9,304 parents /
81,916 FENs, 150 epochs. Regression MAE improved (147→134 holdout) but
**move-ranking did not**: holdout top-1 34.78% → 34.34%, train top-1 fell.
The net predicts SF's eval better without changing which move wins — the
only thing that affects play. Fourth consecutive eval-head failure (Rung-1,
2B v1–v3, Phase-B ranking, Rung-3 MLP). Revised lesson: **the blocker is not
capacity, it is the label source** — generic gauntlet multipv labels do not
teach king-safety move discrimination. `arena/out/rung3-net.json` UNPROMOTED.

## Eval-conversion patch (fifty-move pressure + simplify-when-ahead): gate result

SPRT vs gen6, 10+0.1, conc 12, 400 games (`f:/tools/sprt-evalconv.pgn`):
**REJECTED — −13.0 ±28.5 Elo, LOS 18.5%** (early 54.9% @61 was noise). Code
kept in tree (family-A draw hygiene), but it earns no Elo claim and ships
only inside a bundle that wins its own gate.

## FEN-validation hardening (2026-06-10)

While testing, a hand-written "quiet endgame" FEN crashed BOTH the candidate
and the frozen gen6 (`attacks.rs` index 64 panic via `in_check` on an empty
king bitboard). Root cause: the FEN was itself illegal — the side NOT to move
was already in check, so pseudo movegen legally "captured the king". Search
is sound on legal positions; the gap was input validation. Fix:
`Position::from_fen` now rejects positions with missing/duplicate kings or
the idle side in check (`tests/fen_validation.rs`, 3 tests). All analysis
entry points (serve, uci, faucet, RSI rescoring) inherit the guard.

## Hand-tuned king-safety weights (CPW-guided) — current experiment

Research (chessprogramming.org King Safety): attack-units → nonlinear
S-curve, suppress lone attackers, gate on enemy queen, scale by phase —
**our Rust 2B features already implement all of this**; they were inert only
because trained fits kept collapsing the weights to ~0. So the patch is
hand-set weights, no code change:

| weights file | kingDanger | centralExp | openCenter | escDef | qNear |
|---|---:|---:|---:|---:|---:|
| `rung2-weights-kingsafety.json` | 9 | 12 | 25 | 8 | 4 |
| `rung2-weights-ks-strong.json` | 15 | 20 | 45 | 12 | 6 |

Targeted A/B on the six 4fxkLVBb loss rows (`go 500`, candidate binary):
mixed = 4/6 BAD with delusion (+47cp while losing). ks-strong = **flips the
game-losing row `e7d6`→`c8d7` (SF's best) and `f8f7`-row→`b5c4` (SF's best)**;
self-eval honest (−304 vs −155 pre-mate). Remaining 3 BAD are earlier subtle
drift moves. Next: SPRT ks-strong vs gen6 (same binary pair as evalconv gate,
so attribution is clean). Promote only on SPRT pass.
