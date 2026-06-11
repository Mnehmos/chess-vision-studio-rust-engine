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

## 2026-06-10 afternoon: nine gates, the wall, and the NNUE pivot

Gate ledger for the day (all 10+0.1, conc 12, vs/on frozen gen6 unless noted):

| # | Candidate | Result | Verdict |
|---|---|---|---|
| 1 | eval-conversion code | −13.0 ±28.5, 400 games | REJECTED |
| 2 | Rung-3 MLP (SF labels) | top-1 34.8%→34.3% | REJECTED pre-gate |
| 3 | ks-strong hand weights (bundle) | +6.1 ±31.0 | inconclusive |
| 4 | ks-strong pure A/B (same binary) | +6.1 ±30.3 | inconclusive |
| 5 | Patch 7 pruning, all five | **−188 ±56** | REJECTED hard |
| 6 | Patch 7 minus RFP | −247 fast-fail @36 | RFP exonerated |
| 7 | qsearch-only (delta+SEE) | 50.0% @133 | neutral, stopped |
| 8 | Texel rung2 weights, danger-suite | **50.0% exactly** @300 | no conditional effect |
| 9 | PST/material Texel (56k rows) | holdout flat | data-starved |

Supporting findings: sparse-situation dilution confirmed (only 10.5% of
positions carry live king-danger signal — conditional suite at
`f:/tools/danger-suite.epd`); Texel-on-outcomes independently raises ONLY the
king-safety weights (kingDanger 0→5.1, zone 4.9→6.2, openFile 4.3→5.7) yet
moves no games; search speed is NOT the gap — sustained 1.02M nps vs
Stockfish's measured 911k on the same box (rung2 extraction ≈30% of time).

**Conclusion, proven three independent ways: the 23-scalar hand-crafted eval
is the ceiling.** Learning on it fails (labels), tuning it is invisible
(effect size), and search cannot prune on it (trust). Pivot: **NNUE gen-1**.

## NNUE gen-1 (in flight)

- `selfplay` bin: threaded fixed-depth self-play → JSONL (fen, white-POV cp,
  result); 120k games @ d5 generating into `f:/tools/nnue-data-gen1.jsonl`.
- Trainer `arena/train-nnue.py` (app repo): 768→128 cReLU→1, stm-perspective
  (mirror+colorswap for black), target = 0.6·σ(cp/256) + 0.4·result, CUDA.
- Rust `eval/nnue.rs`: f32 full-recompute forward, **bit-exact parity** with
  the trainer on fixtures; `Searcher::with_nnue` swaps every static/leaf
  eval site; `--nnue <json>` on analyze/uci.
- **Node-speed gate PASSES pre-emptively: 530 knps NNUE vs 426 classical
  (+24%)** — the net is cheaper than rung2 extraction; no incremental
  accumulator needed for gen-1.
- Next: train on the full gen-1 set, SPRT vs gen6; on pass, regenerate data
  with the NNUE engine at deeper labels (gen-2 bootstrap).

## 2026-06-11 overnight: the SF-label saga and the first positive learned eval

Two-track experiment (user-directed): raw NNUE vs CVS-NNUE, same SF-d12
labels, same data/size/budget — isolating input representation.

**Pipeline:** 7.53M unique positions (dedup of the d5 selfplay corpus)
relabeled by 14 SF workers at d12 in ~3.5h, 0 failures, manifest at
f:/tools/labels-sf-d12.manifest.json. Quiet filter via CVS hanging-material
ids: 4.78M rows.

**Three bugs found by gates, in order:**
1. v1 collapse (cvs −527, raw 0.6% — all real mates, zero draws): nets played
   with evals squashed to ±250 while losing by +10 pawns.
2. v2 (quiet filter + stm-relative CVS ids) still collapsed → the fixes were
   real but not root.
3. ROOT CAUSE: **SF UCI scores are side-to-move POV; the relabel conversion
   assumed white-POV** — half of 7.4M labels sign-inverted. The net averaged
   contradictory targets into mush. (Old selfplay corpus had the explicit
   white-POV conversion in selfplay.rs; the new pipeline missed it.)

**v3 (corrected labels + quiet corpus + stm-relative geometry):**
- holdout: raw 0.0239 / cvs 0.0232
- statics: Q-up +1476; preBd6 −404 where classical says −50 and SF −480 —
  the king-danger blindness priced correctly by a learned eval, first time.
- **GATE: cvs-v3 vs gen6 = +9.6 ±30.8 Elo (+169 −158 =73, 400 games,
  LOS 72.9%)** — the first learned eval to reach/exceed the handcrafted
  champion. Not a formal pass; the wall is breached.
- raw-v3 control vs gen6 running (the representation question).

Lessons banked: gates catch what holdout cannot (v1 had the best holdout and
the worst play); verify label POV conventions at the boundary; quiet-filter
training corpora; stm-mirror ALL input families together.

## 2026-06-11 — GEN-7 promoted; oracle CP-LOSS standing benchmark added

**Anchor (final):** gen7 vs native SF-2400, 100 games, quiet box: **−24.4 ±63.5
(LOS 22.4%)** ⇒ ≈2375 absolute. gen6 100-game re-anchor running for honest delta.

**Promotion:** gen7 (uci-gen7-rawnnue.exe 780995d1 + raw-nnue-h256-sf-d12-v3
4cc9765c) promoted to champion per gate rule — SPRT vs gen6 formally passed
(+101.9 ±36.5, bound crossed @305). Manifest updated, rollback = 59ead1ff.

**New standing benchmark** `arena/bench-oracle-cploss.py` (replaces exact-match-only):
engine move at d5, SF-d12 evals child of chosen vs child of oracle move; loss =
max(0, best−chosen). Suite-100 results:

| engine | match% | avgCP | p90 | bl≥100 | bl≥200 | dangerAvg | quietAvg |
|---|---|---|---|---|---|---|---|
| **gen7** | 51 | **23.2** | **87** | **8%** | **1%** | 23.9 | 20.2 |
| gen6 fast | 52 | 29.6 | 105 | 10% | **5%** | 30.2 | 26.6 |
| cvs-v3 | 43 | 24.6 | 105 | 11% | 0% | 23.6 | 29.1 |
| see / defender | 50/51 | ~124 | ~116 | ~11% | 6% | **~146** | ~24 |
| pawn | **55** | 26.6 | 112 | 11% | 3% | 25.9 | 29.8 |

Verdict: gen7 wins on **calibration** (lowest avg loss, p90, and 5× fewer
≥200cp blunders than gen6) at equal exact-match — exactly why move-match alone
was blind to the +102. see/defender lanes are high-variance: worst authorities
(146cp danger avg — their hanging×4 profiles blunder unsupervised) yet best
candidate generators (24–25% unique-and-better-than-gen7). Arbiter fodder, not
mains.

**Lane table rebuilt around gen7 baseline:** ensemble oracle coverage 74% vs
gen7 alone 51%; some lane beats gen7 on cp-loss in 28% of positions. The lane
layer is NOT obsolete post-gen7 — next frontier: rebuild roster with gen7 as
main/verifier (gen6 + cvs-v3 + scalar profiles as diversity lanes), keep only
lanes that add unique low-loss candidates, re-gate arbiter v2/v3 by cp-loss.

## 2026-06-11 — Arbiter v3 (gen7-centered) PASSES the cp-loss milestone

`arena/bench-arbiter-v3.py`: gen7 main/verifier, 9 lanes as provocateurs,
gen7 child-verification d7 (d8 danger), support-scaled margins swept post-hoc.
Same-run SF-d12 cp-loss scoring (gen7-alone re-scored in-run for fairness):

| config | match% | avgCP | p90 | bl100 | bl200 | danger | quiet | giveback | harvest |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| gen7-alone | 51 | 22.7 | 79 | 8% | 2% | 23.6 | 18.4 | — | — |
| arb 5/15 | 55 | **15.4** | **58** | 5% | 1% | 15.7 | 14.0 | 19% of 31 sw | **60%** |
| arb 10/25 | 56 | 16.0 | 69 | 5% | 1% | 16.4 | 14.4 | 13% of 23 sw | 47% |
| arb 25/50 | 57 | 15.8 | 69 | 5% | 1% | **15.2** | 18.4 | 6% of 16 sw | 37% |

Milestone targets (avg<23.2, p90<87, bl200<=1%, danger<23.9, match>=51) met at
EVERY margin. Danger avg cut by a third (23.6 -> ~16). bl200 1% preserved.
see/defender harvested under verification without their 146cp blunders leaking.
Caveats: same suite-100 the lanes were developed on (fresh-suite validation
queued); benchmark-level decision quality, in-engine arbiter mode not yet built.
SF-d12 rescore variance: gen7-alone reads 22.7/2% this run vs 23.2/1% prior.

## 2026-06-11 — Arbiter v3 FRESH-SUITE VALIDATION: PASSED

100 never-before-used positions (build-fresh-suite.py, seed 20260611, 82/18
danger/quiet via CVS detector, original suite excluded). Margins frozen before
results, no tuning after. gen7-alone replicates almost exactly (avg 22.9 vs
22.7, bl200 2%) — and the arbiter generalizes BETTER than on the dev suite:

arb 5/15:  avg 8.9 (−61%), p90 36, bl200 0%, danger 9.6, harvest 70%, giveback 11%
arb 10/25: avg 9.5,        p90 36, bl200 0%, danger 10.4, harvest 63%, giveback 8%

All five pass criteria met at every margin. Suite-overfit hypothesis REJECTED.
Freeze manifest updated (frozen-evals/arbiter-v3-gen7-suite100-cploss-pass).
Next: in-engine/bot-layer arbiter mode on opponent clock (ponder-cache shape);
margin recommendation 5/15 or 10/25.

## 2026-06-11 — Bot-layer gate: PLAIN PONDER PASSES, ARBITER-CACHE FAILS

`arena/bench-ponder-cache.py`: 80 real transitions from the gen7-vs-gen6 SPRT
games, top-5 predictor (gen7 d4 ranking), tiered opponent-clock effort, d6
quick verify (reject >25cp). Cache hit rate 88.8% on real game replies.

| config | avgCP | hit | miss | bl100 | bl200 | verify-rej |
|---|---:|---:|---:|---:|---:|---:|
| gen7-alone (d5) | 28.2 | 27.8 | 31.9 | 12.5% | 1.2% | — |
| **gen7+plain-ponder (d7)** | **24.9** | **24.0** | 31.9 | 10.0% | **0.0%** | 1/71 |
| gen7+arb-cache (5/15) | 29.0 | 28.7 | 31.9 | 13.8% | 1.2% | 3/71 |

GATE VERDICT: plain ponder PROMOTES (beats gen7-alone, blunders eliminated).
Arbiter-cache REJECTED at the bot layer — worse than doing nothing.

Why the suite win didn't transfer (two compounding causes):
1. WRONG BASELINE ON THE SUITES: arbiter was only ever compared to gen7-d5.
   Given the same opponent-time budget, a plain gen7-d7 search of the
   predicted child beats the arbiter's d5-main + candidate-verify, because
   full search considers every move at depth while the arbiter only
   deep-checks the d5 move + lane proposals. "Search deeper" beat "consult
   the panel" at equal (actually lower) cost.
2. POPULATION SHIFT: suites were 82% danger-classified; real game transitions
   are mostly ordinary positions where lane switches add noise, not rescue.

Hypothesis under test now (same seed, 4th config): DANGER-GATED HYBRID — CVS
geometry decides WHEN to convene the lane panel (danger child -> arbiter,
quiet child -> plain d7). This is the doctrine's "CVS = attention/search-
control layer" made literal.

## 2026-06-11 — Bot-layer gate FINAL: hybrid also fails; PLAIN PONDER is the design

Danger-gated hybrid (CVS decides when to convene the panel): avgCP 29.2 /
bl100 13.8% / bl200 1.2% — indistinguishable from always-on arbiter (29.0).
Cause: the CVS danger detector fires on most real middlegame positions
(hanging facts are ubiquitous), so the gate barely gated.

FINAL bot-layer verdict (80 real transitions, 88.8% hit rate):
  PROMOTE: gen7 + plain ponder d7 (24.9 vs 28.2 avgCP, bl200 1.2%->0%,
           verify-reject 1/71). Cheaper than the lane fan-out AND better.
  REJECT:  arbiter cache (29.0) and danger-gated hybrid (29.2) at the bot
           layer. The arbiter's suite wins were real but only vs a d5
           baseline; given the same extra budget, full-width depth beats
           candidate-verify on the real game distribution.

Role correction to the doctrine: specialist lanes + arbiter are a fixed-depth
ANALYSIS instrument (decision quality at capped depth, explanation, Control
Lens teaching) — not a runtime play improver when extra clock can simply buy
depth. Possible future regime where it could still pay: depth-saturated
settings (TT-full long thinks) or as candidate generator INSIDE search
(root-move ordering hints) — untested, not claimed.

## 2026-06-11 — gen6 100-game re-anchor: anchors cannot resolve the gen6/gen7 gap

gen6 vs native SF-2400, 100 games: +40 -47 =13, 46.5%, -24.4 +/-64.3 — the
IDENTICAL point estimate to gen7's -24.4 +/-63.5. A 100-game anchor (+/-64)
cannot resolve a +102 head-to-head gap; both gens read ~2375+/-65 absolute.
The 305-game SPRT bound-cross remains the promotion-grade relative evidence.
Confirms the standing lesson: external fixed-strength anchors compress
learned-eval gaps; use anchors as floors, not deltas, below ~400 games.
