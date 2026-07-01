# Classical Evaluation Experiment for the CVS Rust NNUE Engine

> Canonical experiment spec (provided by the user, 2026-07-01). The post-motifs main path,
> and a standing checklist for busy work. Load-bearing invariants are summarized in the
> agent memory `classical-eval-experiment`. THIS FILE IS THE SOURCE OF TRUTH.

## Purpose

Determine whether handcrafted chess knowledge provides measurable value to the current
Chess Vision Studio Rust engine in one of four roles:

1. A pure classical control arm.
2. A diagnostic instrument for finding NNUE weaknesses.
3. A steer-only system for generating diverse training positions.
4. A shippable residual component added to the frozen NNUE champion.

This is not a classical-only engine rewrite. The current NNUE champion remains the baseline
unless another configuration passes every correctness, reproducibility, runtime, and SPRT
promotion gate.

## Core Experimental Contract

Compare evaluation systems while keeping search constant.

    engine strength = search implementation + evaluation implementation

An evaluation experiment must not silently include unrelated search changes. Search patches
and evaluation patches require separate experiments before any interaction test.

Shippable hybrid contract:

    eval(position) = frozen_champion_nnue(position)
                   + lambda_h * handcrafted_delta(position)
                   + lambda_r * learned_residual(position)

True residual-training target:

    oracle_target - frozen_champion_output - lambda_h * handcrafted_delta

Jointly retraining all branches against the full oracle target does NOT satisfy the residual
contract.

## Non-Goals

- No MCTS work in this experiment.
- No new policy network or root ranker in this experiment.
- No simultaneous search-selectivity changes.
- No claim that a named motif is implemented merely because it exists in the taxonomy.
- No automatic conversion of explanation features into centipawn terms.
- No promotion based on fixed-game win percentage, LOS, one benchmark suite, or Elo point
  estimate alone.
- No claim that zeroability proves improvement. Zeroability proves reversibility only.
- No human, FIDE, USCF, or generally transferable rating claims.

## References

Repo: README.md, CVS_ENGINE_NNUE_INVENTORY.md, RSI_LOOP_REPORT.md, benchmarks/README.md,
benchmarks/GENERATION_STANDARD.md, benchmarks/engines.json; Epic #4; Promotion policy #5;
Fixed-node diagnostics #6; Stabilization and quarantine #7; Differential Evaluator probes #8;
Corpus provenance #9; Residual hybrid #10; Gap routing #11; RSI ledger #12; Gen9 training
safeguards #13; SEE-advisory verification #3.

External: Shannon (static eval + minimax); Knuth & Moore (alpha-beta ordering); Korf
(iterative deepening); Stockfish Fishtest math; Yu Nasu (NNUE); Chessprogramming Wiki
(perft, testing); Cute Chess.

## Experiment Identity — Frozen Baseline

    N0 = frozen Gen8-v2 NNUE champion
       + incremental accumulator
       + declared champion search flags
       + declared hash and thread settings

Record + register in benchmarks/engines.json (all [ ] to do): engine git SHA; release binary
SHA-256; NNUE model SHA-256; feature-registry hash; rustc version + build flags; CPU/OS/ISA;
search flags; hash size; thread count; tablebase + book settings. Prevent promotion tooling
from silently replacing the frozen artifact.

## Experimental Arms

| ID                    | Evaluation path                              | Promotion eligible |
| N0                    | Frozen NNUE champion                         | Yes |
| C0                    | Material + tapered PST + bishop pair + tempo | Yes |
| C1                    | C0 + declared Rung-2 weights                 | Yes |
| H1                    | N0 + handcrafted residual delta              | Yes |
| R1                    | N0 + learned residual                        | Yes |
| HR1                   | N0 + handcrafted delta + learned residual    | Yes |
| S-*                   | Exaggerated steering profile                 | No  |
| LEGACY-CVS-RESIDUAL   | Existing jointly trained CVS residual        | Experimental only |

Add an explicit `EvalMode` registered identity; don't rely on `nnue: None` alone; emit the
effective eval mode through UCI + analysis identity; include every model/registry/weight/
baseline hash in output; refuse unknown/incomplete eval identity in promotion runs.

## Phases (checklist — see full text below)

- Phase 1: Correctness & Reversibility (perft; make/unmake; hash behavior; repetition/rule-50;
  mate-distance; terminal across eval modes; eval perspective/mirror/centipawn convention;
  NNUE incremental-vs-full parity incl. captures/promo/ep/castle/null; reject bad dims/hash/
  models; legacy only under named mode; RESIDUAL ZERO-ABLATION INVARIANT: lambda_h=lambda_r=0
  returns the frozen champion EXACTLY — identical moves/scores/PV/depths/nodes/trajectories/
  termination/fixed-node output. Any failure = hard rejection).
- Phase 2: Classical control arms C0/C1 (freeze ValueWeights + Rung2Weights; manifests +
  checksums; regression fixtures; eval/s + NPS; feature firing rates + contribution dists;
  dead/sign-contradicting features; no search-selectivity compensation).
- Phase 3: Handcrafted residual features (deterministic, versioned, bounded/normalized,
  perspective-correct, independently zeroable, fixture-covered, cost/frequency-measured,
  excluded from zero-weight fast path; stable IDs; registry hash in artifacts; raw vs
  weighted values; no hidden Rung-2 reuse; H-only ablation auto). Candidates: pawn structure,
  king safety, passed-pawn pressure, mobility, rook activity, hanging-material, defender-
  removal, endgame scaling — CANDIDATES not accepted terms.
- Phase 4: True learned residual (per-row provenance: source kind/id/game/ply/split-key/
  finder/labeler/frozen-baseline/stabilization/label status, traceable through shard→relabel→
  prepare→train; no game crossing train/val/test; exclude unparented synthetic; quarantine
  unstable/conflicting; residual target from EXACT frozen N0; store hashes/seed/budget/
  manifests; ablations N0/H1/R1/HR1 + multi-seed + >=2 capacities + >=2 corpus gens; baseline
  branch stays frozen).
- Phase 5: Deterministic diagnostic interface (#6): nodeBudget in analysis JSON; requested/
  consumed nodes; diagnosticIsolation cold + warm-declared; cold resets TT/history/killers/
  counters/caches; refuse MT in diagnostic; book off unless declared; fixed-node forced-root
  through analysis; benchlib.search_nodes; deterministic multi-process fan-out preserving
  input order; test prior-search-cannot-alter-cold.
- Phase 6: Differential probes. P-SEARCH (registered-engine integration; SPRT between node
  budgets; larger frozen fixtures; failure-family changes; stability/quarantine counts;
  marginal value by phase; don't over-interpret small node gains). P-EVAL-K (identical arch,
  const recipe, vary only corpus/labels; source-identical locked holdouts; multi-seed; fixed-
  node + equal-time; provenance/hashes). P-EVAL-C (capacity h256 vs h512; const corpus/labels/
  optimizer/seed; fixed-node + equal-time + inference cost + accumulator cost + size + load;
  per-node vs per-second separate). P-GRID (eval/model gen × search config × fixed-node
  budget over C0/C1/N0/H1/R1/HR1 × champion + minimal profiles; reproduce pruning-vs-eval
  interaction; report interactions).
- Phase 7: Benchmark suites — correctness; fixed-position decision quality (top-1/top-k,
  cp-loss, blunder rate, mate correctness/distance, quiet-defense, hanging-conversion, SEE-
  trap avoidance, sacrifice verification, passed-pawn/promotion, endgame conversion, draw
  defense); runtime (eval/s, NPS, nodes, depth, NTS/TTS, accumulator/handcrafted/residual
  cost, load time, memory, firing rates, zero-path extraction stays 0); per-bucket by
  opening/middlegame/endgame/tactical/positional/quiet/king-safety/pawn/promotion/conversion/
  draw-defense/label-stability/corpus-source/motif-tag.
- Phase 8: Motif & teaching-fact discipline. Taxonomy is a ROADMAP not proof. STILL REQUIRED:
  generate detector coverage counts directly from the registry; FAIL CI when docs claim a
  detector the registry marks missing; require positive + adversarial-negative fixtures;
  proof-bearing output for tactical claims; keep taxonomy-only labels OUT of eval features;
  keep uncertain motif claims out of oracle labels; measure PRECISION before optimizing
  recall; admit a motif into evaluation only through a separate measured experiment.
- Phase 9: Strength testing. Screens decide only continue/stop (HOLD_FOR_MORE_DATA / REJECT /
  ANALYSIS_MODE_ONLY / LIVE_DEV_ONLY; a positive screen is not a promotion). Formal promotion
  match: vs frozen champion; exact hashes; identical search except the declared variable;
  fixed threads/hash; declared TC/nodes; paired color-reversed openings + suite hash; PGN+UCI
  preserved; SPRT hypotheses declared beforehand; run to a boundary or cap; PROMOTE requires
  UPPER boundary; lower-bound crossing REJECTS; no boundary = HOLD_FOR_MORE_DATA. External
  anchors only after beating the internal frozen champion (pinned SF rungs; a structurally
  different engine; multi-opponent gauntlet; mirrored openings/exact settings).
- Phase 10: Promotion gates IN ORDER — (1) Correctness (perft/make-unmake/NNUE parity/eval
  perspective/terminal). (2) Identity & reversibility (full provenance; frozen hash match;
  registry match; EXACT zero-ablation; steer-only guard; no undeclared search change). (3)
  Deterministic decision value (fixed-node; cold-isolation; cp-loss not regressed beyond
  tolerance; critical tactical/conversion buckets not regressed). (4) Runtime (NPS/depth/
  equal-time/memory/size recorded; cost justified by decision value). (5) Strength (formal
  SPRT record; UPPER boundary crossed; artifacts match record; PGN checksum; no open
  correctness exception). (6) Reporting (machine-readable manifest; raw artifacts; appended
  to RSI ledger; supported conclusions separated from interpretation; README names opponent/
  control/games/settings/uncertainty). Only then: PROMOTE.

## Decision Rules

- Classical arm wins SPRT: valid candidate; don't conclude NNUE generally inferior;
  investigate corpus/capacity/search via P-GRID; preserve as registered artifact.
- Classical loses but reveals gaps: route positions into the calibration-hole pipeline;
  relabel with the independent oracle; keep classical as diagnostic/steer tool; don't ship.
- Handcrafted helps at fixed nodes, loses at equal time: keep unpromoted; optimize extraction
  separately; consider explanation-only / steer-only; rerun equal-time after the cost patch.
- Handcrafted fails strength but improves diversity: mark steer-only; refuse in champion
  packaging; preserve full source provenance.
- Learned residual wins: verify the baseline branch stayed frozen; verify the exact residual
  target; verify zero-ablation identity; promote ONLY the exact residual artifact that crossed
  SPRT.
- Result unstable: quarantine; increase deterministic node budget; invoke independent
  verification; preserve disagreement (don't silently pick one answer).

## Remaining Repository Work Directly Blocking This Experiment

- [x] Complete the analysis-side fixed-node and isolation interface (#6). `analyze --serve`
      accepts `nodeBudget` + `diagnosticIsolation` (cold|warm) on go/analyze JSON requests;
      reports `diagnostic.{requestedNodes,consumedNodes,singleThread,multithreadRefused,book}`;
      cold = fresh searcher per request (empty TT, prior search cannot alter it), warm =
      persisted searcher (aged TT carries forward); forced-root supported. Single-thread is
      forced (INV-2). Tests: tests/serve_diagnostic.rs (determinism, cold isolation, warm carry).
- [x] Complete the canonical SPRT match runner (#5). benchmarks/scripts/sprt_runner.py: pure
      BayesElo trinomial LLR + sequential decision (stop at the first boundary crossing or a game
      cap), streaming candidate-POV results; emits a record valid against sprt-result.schema.json
      and passing lint_promotion.py (INV-1: promote requires the upper boundary). CLI consumes a
      cutechess/JSONL result stream; tc="nodes:<N>" ties it to the #6 fixed-node control. Tests:
      benchmarks/scripts/test_sprt_runner.py (bounds, LLR sign, promote/reject/hold, malformed-
      hypothesis guard, and a closed-loop check that produced records pass the promotion linter).
- [ ] Complete independent verification and calibration (#7).
- [ ] Complete P-EVAL-K, P-EVAL-C, P-GRID (#8).
- [ ] Add complete self-play provenance through the pipeline (#9).
- [ ] Wire the residual-hybrid configuration into the live evaluation adapter (#10).
- [ ] Replace the learned-residual stub with a versioned model loader.
- [ ] Train against the true frozen-baseline residual target.
- [ ] Add automatic N0, H1, R1, HR1 ablations.
- [ ] Finish the remaining tactical-volatility triggers and verdict system (#3).
- [ ] Build one canonical experiment command.

## Completion Criteria

One command can: (1) resolve+validate all artifact identities; (2) run correctness +
zero-ablation gates; (3) run cold one-thread fixed-node comparisons; (4) run equal-time
comparisons; (5) produce P-SEARCH/P-EVAL-K/P-EVAL-C/P-GRID reports; (6) quarantine unstable/
provenance-invalid samples; (7) generate all required ablations; (8) launch/consume the
declared SPRT match; (9) produce an append-only experiment record; (10) permit PROMOTE only
after a valid upper-bound crossing; (11) refuse steer-only/legacy-experimental configs; (12)
preserve every command/manifest/model/PGN/log/checksum needed to reproduce the conclusion.
