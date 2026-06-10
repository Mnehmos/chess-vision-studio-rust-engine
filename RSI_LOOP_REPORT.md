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

## Next experiment — Rung 3: small nonlinear value head
A 2-layer MLP over the full 23-feature Rung-2 vector (23→8→1, ~200 params,
~30 multiplies per eval — negligible in Rust), trained with the proven mixed
objective on the multipv dataset (10/10 shards once labeling completes).
Capacity is the demonstrated blocker; this is the smallest capacity jump that
can represent attacker-count × exposure interactions. Gate stack unchanged:
regression rows + delusion FENs, eval-r4 non-regression, experimental
mini-gauntlet, SF d20 confirm — promote only if green, then re-measure the
time-odds curve.
