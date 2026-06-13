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

## 2026-06-11 — NNUE accumulator shipped; patch-7 pruning revival (gen7 changes the premise)

**Accumulator (7208c04):** both-perspective incremental NNUE, slot-reuse stack.
0.71 -> 0.87M nps (+22%), search-IDENTICAL at d8 (same nodes, same moves).
Playout test: ±1cp f32 drift max, >=99% exact. cvs models keep full recompute.

**Patch-7 revival hypothesis:** the -188 rejection ran futility/LMP against the
CLASSICAL eval, which was king-danger-blind (-50 vs SF -480) — pruning trusted
a delusional oracle and cut quiet defenses. gen7 is calibrated. d9 probe
(gen7+accumulator): futility alone = 1.67x time-to-depth with IDENTICAL moves;
full p7 stack = 8.5x with changed moves. Plan: SPRT futility solo vs gen7+acc
baseline at 10+0.1 (then LMP, then SEE-prune, one at a time) once the ponder
gate frees the box. Lesson candidate: prune validity is conditional on eval
calibration — re-test pruning after every eval-generation change.

## 2026-06-11 — PONDER FORMALLY PROMOTED (SPRT H1 accepted)

gen7+ponder vs gen7, 10+0.1, identical binary (76ffb53), cutechess ponder flag:
**+72 -33 =47 (152 games), 62.8%, llr 2.99 > 2.94 — H1 ACCEPTED. ~+91 Elo.**
This with the known illegal-ponder-hint warnings still costing hits (pv[1]
sometimes stale) — fix queued, upside still unclaimed. Frozen:
f:/tools/cvs-baselines/uci-gen7-ponder.exe. Doctrine: enable ponder wherever
an opponent clock exists (cutechess `ponder` flag; bot already has its richer
top-3 cache layer). Box freed -> futility-pruning SPRT launches next
(gen7+accumulator baseline, revival hypothesis: calibrated eval un-breaks
pruning).

## 2026-06-11 — Futility-v1 ACCEPTED WITH NOTE (fixed-N positive, NOT a formal SPRT pass)

Test: futility-v1 on gen7+acc vs gen7+acc base. 10+0.1, conc 1, elo0=0 elo1=20.
Stopped by user decision at +68 -51 =55 / 174 games, 54.9%, ~+34 Elo observed.
LLR ~1.0/2.94 — POSITIVE THROUGHOUT but did NOT cross the formal H1 accept bound.

**Do NOT cite as "SPRT accepted." Cite as "fixed-N positive / accepted with note."**

Hypothesis under test: is futility pruning still toxic after the gen7 NNUE eval
replacement? Prior result on gen6/classical was -188 Elo. Answer: NO — the sign
flipped from strongly negative (king-blind eval) to directionally positive
(+25 to +40 likely, not the early +50-60 spike). Confirms the doctrine:
pruning validity is conditional on eval calibration, not inherent.

Fixed-depth sanity (fresh-100, 82 danger, d9, SF-d12 cp-loss), futility ON vs OFF:
  OFF: avgCP 10.2  danger 11.6  bl100 1%  bl200 0%  match 61%
  ON : avgCP 10.3  danger 11.6  bl100 1%  bl200 0%  match 60%
IDENTICAL decision quality; gen7 blunder collapse (bl200 0%) preserved. Gain is
pure depth, not judgment loss — the correct signature for sound pruning.

Promotion: futility-v1 ACCEPTED PROVISIONALLY into the gen7+acc search stack.
Frozen artifact: f:/tools/cvs-baselines/uci-gen7-acc-futility.exe (d5233c07).
Rollback retained (uci-gen7-ponder cb28a544, gen6 59ead1ff). Default-flip in
SearchOptions deferred until the parallel --helper-nnue source work settles, to
avoid colliding edits; futility stays the documented accepted run flag meanwhile.
Future pruning changes added ONE AT A TIME, gated separately (next: RFP).

## 2026-06-11 — Gate 6 SF escalation ladder: full snapshot stack ~2530 blitz

Full deployed stack (gen7 + accumulator + futility + PONDER — first anchor with
the complete stack), 10+0.1, 20 games per rung, escalate at >=60%:

  rung 1 vs SF-2400: +13 -6 =1  67.5%  (~+127 => ~2527)   ESCALATED
  rung 2 vs SF-2500: +10 -8 =2  55.0%  (~+35  => ~2535)   STOP (<60%)

Two independent rungs cross-validate at ~2525-2535 blitz vs native SF. Bare
gen7 on the same SF-2400, same TC, same book: 44.9% (~2375). The day's stack
work (+91 ponder, +34 futility, +22% nps) converted EXTERNALLY at near-full
value: ~+150-160 Elo measured against Stockfish in one day.

Caveats: 20-game rungs (each ~±150), but two consistent rungs tighten the
estimate; 10+0.1 is blitz — slow-TC confirmation still recommended before
citing as durable absolute strength. PGNs: gate6-sf2400.pgn / gate6-sf2500.pgn.

## 2026-06-11 — RFP-v2 FORMAL SPRT PASS (third formal acceptance)

rfp-v2 (depth<=4 cap) vs snapshot (gen7+acc+futility), 10+0.1, elo0=0 elo1=20:
**+101 -58 =61 (220 games), 59.8%, Elo +68.8 +/-39.6, LOS 100%,
llr 2.97 > 2.94 — H1 ACCEPTED.**

Pipeline story: rfp-v1 (depth<=6) PASSED fresh-100 but FAILED hard-100 on a
single mate-scale outlier (+9259cp deep-cut miss) — the mined hard suite
caught what the standard suite couldn't. v2 tightened the depth cap 6->4,
kept +0.7-0.8 ply at equal movetime, passed Gates 1-3, screened neutral
(50%, =10/20 minimum), then formally crossed the SPRT bound.

NEW CHAMPION STACK: gen7 + accumulator + futility + RFP (+ ponder for
opponent-clock settings). Frozen: uci-gen7-acc-fut-rfp.exe (452c12cc),
analyze-gen7-acc-fut-rfp.exe (a9cf216e). Rollback: snapshot f07caae binaries.
Selectivity tally on gen7: futility +34 (fixed-N), RFP +69 (formal).
Next per roadmap: external rung screen -> smarttime gate -> telemetry -> LMP.

## 2026-06-11 — RFP HOLD reconciliation (suspicious screen resolved as noise)

User-corrected Gate-5 screen (current target/release/uci.exe both sides,
--futility --rfp vs --futility, 5+0.05) scored 45.0% (+6 -8 =6) — SUSPICIOUS,
HOLD declared. Investigation per gate doctrine ("inspect PGNs"):

1. PGNs clean: 20/20 normal terminations, zero time forfeits/crashes/illegal
   moves; losses were long fully-played games (6 as white, 2 as black).
2. Build exonerated: true parity, SPRT-era champion artifact (452c12cc-era
   analyze a9cf216e) vs current build (cf8d1c11-era analyze), identical flags,
   d8, all 10 canonical positions: 0 differences — moves, scores, and node
   counts identical. Telemetry + parallel edits confirmed observation-only.
3. Screen extended to 100 games, same config, same TC:
   **+50 -23 =27, 63.5% — GREEN** (cutechess +96.2 +/-60.2, LOS 99.9%,
   PGN screen-rfp-ext100-20260611.pgn). The 45% start was a cold 20-game
   sample of the same effect (P(<=45% | true +69) ~ 6-8%).

Verdict: the formal SPRT (+68.8, llr 2.97, 220 games @10+0.1) stands and now
has an independent 100-game GREEN confirmation at 5+0.05 hyperblitz on the
current build. RECOMMENDATION: lift HOLD, promote RFP into the champion
stack (user's call per gate discipline). Bot restart with --rfp unblocks on
that decision.

## 2026-06-11 — RFP ruling: ACCEPTED WITH NOTE (user), promoted to live bot

User ruling on RFP-v2: **ACCEPTED WITH NOTE** — standard engine practice,
not degrading; treated as a solid but not headline gain. Supporting record:
formal SPRT bound crossing (+68.8, llr 2.97, 220 games @10+0.1) and the
100-game GREEN screen (63.5% @5+0.05); operative label is the user's.

Live bot restarted on the consolidated stack: frozen
analyze-gen7-acc-fut-rfp.exe (a9cf216e) + gen7 net, CVS_RUST_FUTILITY=1 +
CVS_RUST_RFP=1 (backend flag wired in rust-backend.ts), ponder picker k3.
Previous bot build (target-cand, futility-only) retired.

Smarttime SPRT continues in parallel (interim ~185 games, llr ~0.4,
+19 +/-43 — trending fixed-N positive / inconclusive).

## 2026-06-11 — Smarttime SPRT stopped by user at 215 games

--smarttime (soft/hard split) vs flat budget, both sides current build with
--futility --rfp, 10+0.1: **+83 -70 =62 (215 games), 53.0%,
Elo +19.9 +/-39.6, LOS 83.8%, llr 0.484 — bound NOT crossed.**
Label: **fixed-N positive / inconclusive.** Do NOT cite as SPRT pass.
20-game screen @5+0.05 was GREEN (60%). Status: live-dev only pending a
future gate; flag stays off by default. PGN: sprt-smarttime.pgn.

## 2026-06-11 — Countermove (roadmap item 7): implemented, weak standalone

--countermove (refutation of opponent's previous move, ordered below killers;
per-ply prev-move stack, null subtrees excluded). Flag OFF byte-identical to
champion (node-exact on canonical). Telemetry @d8: first-move cutoff
34.4%->36.6%, cutIdx 1.48->1.44, ttHit 12.2%->14.1% — but EBF 2.68->2.82
(two openings grew) and equal-movetime depth flat (93 vs 92).
Gate-5 screen: +6 -8 =6 (45.0%) SUSPICIOUS with no positive prior.
Decision: NOT promoted; flag stays off; revisit paired with continuation
history. Next: continuation history (item 8) as its own gate.

## 2026-06-11 — Conthist neutral; TT leak found; tt-prune-store patch

Continuation history (--conthist, 1-ply pair table): flag-off byte-identical,
telemetry flat (1st-cut 34.2% vs 34.4%), nodes -0.6%, screen +6 -6 =8 (50.0%)
NEUTRAL. Not promoted; table too sparse to reorder the quiet tier as built.

TT investigation (the audit's 12.2% "hit rate"): raw counters show only ~20%
of probed nodes even FIND an entry at d8 — yet the 2M-slot table sits at ~2%
load. Cause: RFP cuts ~60% of shallow non-PV nodes and returned WITHOUT
storing, so every ID iteration re-probes those nodes empty and recomputes the
NNUE static eval. (Also found: bench_telemetry hashMoveCutoffPct denominator
bug — prints >100%.)

Patch: --tt-prune-store stores Lower(beta) at depth on RFP cutoff (position-
only bound, unlike null — safe). Measured: any-entry 21.9%->43.0%, fixed-depth
nodes +0.2% (same tree), equal-movetime depth +3 summed (92->95; two suites
+2 ply each). First patch tonight with a real same-budget depth gain.
Screen running.

## 2026-06-11 — tt-prune-store screen GREEN, SPRT launched

Gate-5 screen (--tt-prune-store vs base, both --futility --rfp, 5+0.05):
**+12 -5 =3 (67.5%) GREEN** — strongest screen of the night, consistent with
the +3 summed equal-movetime depth. SPRT running: 10+0.1, elo0=0 elo1=20,
max 400 (sprt-ttprunestore.pgn). One variable.

## 2026-06-11 — rule50 eval scaling REJECTED (v1)

--rule50 (eval × (256−hm)/256 at static/leaf eval): flag-off byte-identical;
screen +5 -7 =8 (45.0%) SUSPICIOUS; PGN inspection shows the TARGET metric
regressed — cand drew from peaks +9.31 (50-move), +3.91 (rep @56 plies!),
+3.49, +2.07. Mechanism: Zobrist excludes the halfmove counter, so scaled
scores contaminate the TT across hm contexts (same trap as storing null
cutoffs), and high-hm scaling flattens move gradients exactly when technique
needs them. Decision: REJECT; conversion pressure must be POSITION-ONLY.
Next: king-activity-when-ahead (TT-safe), its own gate.

## 2026-06-11 — king-activity v1: NEUTRAL at 100, conversion leak persists

20-game screen GREEN (60%) did not hold: 100 games +38 -32 =30 (53.0%)
NEUTRAL, and cand still drew from +11.57 / +6.50 / +6.27 (13/30 draws peaked
>=+1.5). Diagnosis: proximity+centralization gives no gradient in trivially
won endings — the winner shuffles between eval-equal moves. v2 adds the
classic mop-up term (drive the LOSING king to the edge), same flag.

## 2026-06-11 — king-activity v2 NEUTRAL; conversion lane shelved

v2 (mop-up: drive losing king to edge, 8x weight + proximity): 100 games
+37 -31 =32 (53.0%) NEUTRAL, draw-peak profile unchanged (22/32 >= +1.5,
still drew from +7.3/+5.8). SF-d22 ground truth on six peak positions:
4 REAL wins unconverted (incl. +8.2, up a bishop in a pawn ending) and
2 eval fictions (+3.6 claimed in a +0.2 KRB-vs-KR fortress).

Verdict: conversion failures are real but NOT eval-gradient-limited — two
independent formulations moved nothing at 5+0.05. Hypothesis: depth-limited
(zugzwang/pawn-race lines). The pending tt-prune-store depth gain is the
likelier cure; revisit conversion AFTER it lands, measured at 10+0.1.
Both terms stay flag-gated off (king-activity, rule50). Eval-fiction note
filed for gen8 training (endgame labels thin in gen7 data).

## 2026-06-11 — tt-prune-store SPRT: fixed-N positive / inconclusive @400

--tt-prune-store vs base (both --futility --rfp), 10+0.1, elo0=0 elo1=20:
**+151 -133 =116 (400 games), 52.2%, Elo +15.6 +/-28.7, LOS 85.7%,
llr 0.52 — NO bound crossed.** Label: fixed-N positive / inconclusive.
Supporting record: screen 67.5% GREEN, ttEntry 22%->43%, fixed-depth tree
identical (+0.2% nodes), equal-movetime depth +3 summed. Mechanically sound
and cheap; promotion label is the user's call (futility precedent:
accepted-with-note). Flag stays off pending ruling.

## 2026-06-11 — LMP shelved (v1 RED, v2 suspicious)

LMP v1 (4+d², depth<=4): screen +4 -9 =7 (37.5%) RED — telemetry showed 83%
of attempted quiets skipped, -55% nodes; budgets assume 85-95% first-move
cutoff ordering, ours is ~35%. v2 (8+2d²): +6 -8 =6 (45.0%) SUSPICIOUS.
Shelved pending ordering improvements — third data point (after countermove,
conthist) that ORDERING QUALITY is the binding constraint on the pruning
roadmap. Next: qsearch TT (55-62% of nodes, currently table-blind).

## 2026-06-11 evening session synthesis (post-RFP)

Gates run (one variable each, all flag-gated off by default, all flag-off
byte-identical): smarttime (fixed-N inconclusive @215, user-dropped),
countermove (45% susp), conthist (50% neutral), tt-prune-store (67.5% GREEN
screen -> fixed-N +15.6 @400, NO bound), rule50 (REJECTED, TT contamination),
king-activity v1/v2 (53% neutral x2), LMP v1/v2 (RED 37.5% / susp 45%),
qtt (-7.4% nodes, 0 move changes; 45% @20, 100-game ext running).

The evening's three structural findings:
1. ORDERING QUALITY BINDS THE PRUNING ROADMAP. ~35% first-move cutoff means
   every quiet-tail trick (countermove/conthist/LMP) underperforms its
   textbook value. The fix order is hash-move coverage (TT work) before more
   ordering heuristics, before any more pruning.
2. RFP-CUT NODES WERE TT-INVISIBLE (fixed by tt-prune-store, pending label).
   Same class of finding as the q-node table-blindness (qtt, pending).
3. GEN7 ENDGAME EVAL FICTION measured: fortress +0.2 scored +3.6; deep
   endgame labels thin in SF-d12-quiet training diet. Gen8 training note.
   Real unconverted wins are depth-limited at hyperblitz, not gradient-
   limited (two eval-term formulations moved nothing).

## 2026-06-11 — qtt: NEUTRAL at 100 (fixed-N positive-leaning)

--qtt 100 games: +39 -34 =27 (52.5%), +17.4 +/-58.7. Same band and same
shape as tt-prune-store: mechanically clean (-7.4% nodes, zero move changes
at fixed depth), too small to prove at this sample. Both TT patches pending
user label; complementary but must be accepted individually (never bundle).

## 2026-06-12 — USER RULINGS: tt-prune-store + qtt both ACCEPTED WITH NOTE

Both TT patches accepted with note (fixed-N positive, no SPRT bound; cite
accordingly). Pair-confirmation screen launched (--tt-prune-store --qtt
together vs champion base — each was gated solo; this checks interaction
only). On clean screen: freeze new champion stack
gen7+acc+futility+rfp+ttps+qtt and restart bot on it.

## 2026-06-12 — TT pair CONFIRMED; new champion frozen; bot on full stack

Pair confirmation (--tt-prune-store --qtt vs champion base, 100 games):
**+41 -29 =30 (56.0%)** — no interaction; pair estimate ~= sum of parts.
NEW CHAMPION STACK: gen7 + acc + futility + RFP + tt-prune-store + qtt.
Frozen: uci-gen7-acc-fut-rfp-tt.exe (c6fe4d1e),
analyze-gen7-acc-fut-rfp-tt.exe (4b27f67e). Rollback: the -rfp pair.
Bot restarted on it (CVS_RUST_TTPS/CVS_RUST_QTT wired in rust-backend.ts;
orphan-kill discipline applied, 0 stragglers verified).

histmalus v1 screen: +5 -7 =8 (45.0%) suspicious standalone — consistent
with research: history pays when CONSUMED (history-aware LMR / continuation
pruning), not as pure reordering. Label: FOUNDATION, neutral standalone.
Next gate: history-informed LMR (one mechanism = malus substrate + LMR
consumption; explicitly noted as a dependent chain, not a bundle).

## 2026-06-12 — histmalus+histlmr screen GREEN; SPRT launched vs new champion

History mechanism complete (maluses+gravity substrate, LMR consumption):
d8 nodes -23.1%, +3 summed depth @500ms, screen +11 -6 =3 (62.5%) GREEN.
SPRT running: 10+0.1, elo0=0 elo1=20, max 400, base = NEW champion stack
(futility+rfp+ttps+qtt), cand adds --histmalus --histlmr. One mechanism,
dependent chain (substrate + consumer), explicitly not a bundle of
independents. PGN: sprt-histlmr.pgn.

## 2026-06-12 — rule50-v2 REJECTED; conversion lane CLOSED (eval-side)

v2 (damp + SF adjust_key50 bucketing + hm>=96 cutoff guard): 100 games
+29 -41 =30 (44.0%), draw-peak profile UNCHANGED (21/30 draws peaked >=+1.5;
still drew from +9.37/+8.01/+6.42). The bucketing fixed v1's TT poisoning,
but at our depths the damp's horizon gradient is a few cp while it distorts
all hm-20..40 middlegame evals by 10-15%. Conclusion: eval-side conversion
pressure REJECTED twice with mechanisms; the lane closes. Conversion path =
depth (TT patches, histlmr) + gen8 endgame training data + (later) Syzygy.
Both rule50 and king-activity remain flag-gated negatives in the source.

## 2026-06-12 — Two-bucket TT (--tt2): bench-NEUTRAL, important negative

Built depth-preferred + always-replace buckets (flag-off byte-identical,
verified). Fixed-depth d8: nodes -0.1%, ttHit 13.8%->13.7%, movetime depth
-2 summed. CONCLUSION: eviction is NOT the cause of our low (~14%) TT-hit
rate — the research's #1 TT hypothesis (replacement policy) is falsified for
our engine. The low hit rate is intrinsic: either we genuinely revisit few
transpositions at our pruning widths, or probes miss for a different reason
(qsearch store coverage, generation aging too aggressive, or a key issue).
tt2 SHELVED (no bench signal; do not spend SPRT cores). NEXT TT step is
DIAGNOSTIC not corrective: instrument probe misses by cause (never-stored vs
evicted vs fresh-position) before any more TT code. Search-track lesson:
measure the actual failure before applying the textbook fix.

## 2026-06-12 — TT probe-miss DIAGNOSTIC: low hit rate is intrinsic, not fixable via TT

Instrumented probe misses (d11, 3 middlegames, 2.08M probes):
  any-entry 14% | depth-hit 8% | **miss-COLD 82%** | miss-contended 3% |
  entry-but-shallow 5%.
82% of misses are COLD (index never written) vs 3% contention. The table is
UNDER-filled, not churning. Low hit rate = heavily-pruned search over mostly-
UNIQUE positions, not eviction (confirms tt2 neutral). VERDICT: TT is NOT a
search-leverage point; the audit's "pathological 12% hit" framing was wrong.
The only lever that raises hit rate is MOVE ORDERING (better ordering -> PV
re-convergence across ID iterations -> transposition hits). Redirects the
search track fully back to ordering/history (the live SPRT) + capture history
+ cheap ordering wins. Drop TT from the roadmap. Diagnostic counters kept
(observation-only, search-identical).

## 2026-06-12 — "improving" flag (--improving): positive bench, screen-queued

Eval-trajectory primitive: per-ply static-eval stack; a node is "improving"
if its static eval exceeds the same side's eval two plies back. Currently
consumed by LMR (one extra reduction ply when NOT improving). Flag-off
byte-identical. Bench: d8 nodes -8.4%, movetime depth +2 summed. Clean
positive (contrast tt2 neutral). Independent of histlmr. SCREEN-QUEUED for
when SPRT frees cores. Next consumers if it lands: improving-gated futility
margin + the viable-LMP budget split (4+d² vs (3+d²)/2).

## 2026-06-12 — gen8-raw-h256-d20 v1: REJECTED (regressed on fresh-100)

Eval-only A/B (same frozen gen7 exe, swap net), fresh-100, d9, SF-d12:
  gen7  match 60.0% avgCP 11.6 p90 50  bl200 0.0% danger 13.5
  gen8  match 52.0% avgCP 23.4 p90 94  bl200 2.0% danger 23.3   GATE 3 FAIL (all 4).

Diagnosis (two compounding causes, both my process errors):
1. NO QUIET FILTER. gen7 trained on 4.78M QUIET positions (filtered from
   7.43M). I trained gen8 on RAW public Lichess positions incl. mid-tactic /
   in-check / unstable nodes -> the net learned noise. Quiet filtering is a
   core NNUE training principle; I skipped it for the public data.
2. DISTRIBUTION SHIFT. Public-as-BROAD replaced our-engine's distribution with
   Lichess user-analysis positions (opening/middlegame heavy, different from
   what our search reaches). gen7's edge was OUR-distribution calibration; gen8
   threw that away. The danger (middlegame) regression is the fingerprint.

Lesson: public data should SUPPLEMENT (fill endgame gaps), not SUPPLANT the
broad base. The real gen8 control is OUR 4.78M quiet corpus relabeled d12->d20
(distribution held constant, label depth isolated) -- the expensive relabel I
dodged by using public data. Corrected recipe options below.

## 2026-06-12 — gen8-v2 (corrected recipe): recovers, but cp-loss gate LEAKED

v2 corpus = our quiet d12 (83%, distribution restored) + quiet endgame (14%)
+ booster (3%); maxlen 32 (v1 was 64 -> v1 also had malformed public FENs
doubling features, a 3rd v1 bug). v2 holdout 0.012 (v1 0.025).
cp-loss gate fresh-100: gen8v2 match 66% avgCP 6.7 danger 7.5 vs gen7 60%/
11.6/13.5 -- PASS all 4, danger ~halved. BUT leakage check: 47/100 fresh-100
positions are IN v2's training corpus -> cp-loss INFLATED by memorization.
fresh-100 is NOT held-out from our-corpus-trained nets (benchmark hygiene
finding). Clean verdict = head-to-head GAME screen (swap net only, immune to
cp-loss leakage). Running now.

## 2026-06-12 — gen8-v2 head-to-head: +115 Elo over gen7 (clean, leakage-immune)

Game screen (swap NNUE only, same champion search, 5+0.05, 100g):
**gen8v2 vs gen7 +55 -23 =22, 66.0%, Elo +115.2 +/-63.3, LOS 100.0%.**
A game screen is immune to the cp-loss training leakage, so this is a REAL
eval gain ~ the gen6->gen7 magnitude. Recipe = our quiet d12 distribution
(83%) + public-endgame d20 (14%) + our-weakpoint d20 booster (3%), lambda 1.0.
Achieved on d12 BROAD labels -> the 67cp d20 signal is still UNSPENT (d20
relabel of our corpus now running, 12 cores).

STATUS: strong new-champion EVAL candidate, screen-positive @100, LOS 100%.
Promotion path = full gate ladder (cp-loss on a NON-leaked suite + blunder/
danger sanity) -> SPRT vs gen7 stack -> user ruling. Not yet promoted.

## 2026-06-12 — improving-into-LMR: REJECTED on games (bench-positive trap)

--improving (extra LMR reduction ply when not improving) vs gen8v2 champion:
14-17-9 (40g) 46.3%, -26 Elo. Bench was +ve (-8.4% nodes, +2 depth) but did
NOT convert -- the extra reduction costs more accuracy than the depth buys.
Third reduction-based patch to disappoint (histlmr marginal, improving neg):
our search is at diminishing returns on REDUCTIONS. Eval-stack infra kept
(could feed futility margin later). PIVOT: pure ORDERING wins (capture
history, safe-check bonus, threat-escape) sharpen hit quality WITHOUT the
accuracy/depth tradeoff that reductions pay.

## 2026-06-12 — caphist REJECTED; SEARCH track at diminishing returns vs strong eval

--caphist vs gen8v2 champion: 16-23-21 (60g) 44.2%, -41 Elo (LOS 13%).
Telemetry was +ve (1st-cut 40.2->40.9%, cutIdx down) but did NOT convert —
captures were already near-optimally ordered by MVV-LVA+SEE; the learned
table added variance.

PATTERN (4 patches on the gen8-v2 champion): histlmr marginal (accepted as
substrate only), improving -26, caphist -41, [countermove/conthist/LMP earlier
on gen7 also failed]. CONCLUSION: with a much STRONGER eval (gen8-v2 +115),
the search already finds good moves, so ordering/selectivity tweaks have less
room and are not converting. The measured Elo is now coming from EVAL, not
search. STRATEGIC PIVOT: focus compute on the eval track — gen8-v3 (d20
relabel, running) + self-play corpus growth — not more search grinding.
Remaining cheap search items (safe-check bonus, threat-escape) are low-
priority; shelve caphist + improving behind flags.
