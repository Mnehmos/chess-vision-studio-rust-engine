# Rust Engine Responsibilities

This repository is the native Chess Vision Studio engine and deterministic facts
authority. It owns legal chess truth, native search, UCI integration, and the
`TeachingFactBundleV1` protocol consumed by the web app.

## Engine Role

| Layer | Responsibility |
|---|---|
| Board core | Bitboard position representation, FEN validation, Zobrist hash, make/unmake, history, repetition input. |
| Move generation | Pseudo-legal/legal moves, noisy moves, check detection, castling, en passant, promotions, and perft correctness. |
| Facts engine | Piece safety, SEE, captures, pawn structure, king safety, motifs, pins, hazards, deltas, provenance. |
| Evaluation | Classical material/PST, trainable value weights, Rung-2 features, NNUE inference, CVS geometry features. |
| Search | Iterative-deepening alpha-beta, quiescence, TT, selectivity flags, SMP, specialist lanes, telemetry. |
| Frontends | `analyze --serve` JSON-line API, UCI binary, perft/eval/search benchmark binaries. |

The Rust engine emits deterministic facts and search results. It does not write
player-facing prose, teaching topics, or causal explanations. The app combines
Rust facts with Stockfish grades for that.

## Runtime Interfaces

### `analyze --serve`

The app's Vite server launches `target/release/analyze --serve`. Each stdin line
returns one JSON stdout line.

Supported plain commands:

| Input | Output |
|---|---|
| `<fen>` | Search result JSON for one FEN. |
| `go <ms> <fen>` | Timed search result JSON. |
| `eval <fen>` | Static eval JSON, plus NNUE score when loaded. |
| `cvs <fen>` | CVS feature registry debug dump. |
| `quit` | Stop server. |

Supported JSON commands:

| `cmd` | Responsibility |
|---|---|
| `facts` | Build `TeachingFactBundleV1` from `schemaVersion`, `fenBefore`, `playedMoveUci`, optional best/refutation/PV, and options. |
| `analyze` or absent | Search a FEN or replayed move history. |
| `go` | Timed search with optional history and forced move. |
| `eval` | Static eval with optional NNUE score. |

### UCI

`target/release/uci` is the external engine frontend for cutechess and other UCI
harnesses. UCI strength claims should use this binary or a clearly named native
binary, not browser/WASM results.

## Teaching Facts Schema

Rust source of truth:

| Schema | File | Responsibility |
|---|---|---|
| `TeachingFactsRequestV1` | `src/facts/types.rs` | Request schema for facts command. |
| `TeachingFactBundleV1` | `src/facts/types.rs` | Versioned facts response with before/played/best/refutation branches. |
| `PositionFacts` | `src/facts/types.rs` | Side to move, pieces, pawn structure, king safety, captures, motifs, pins, opponent probes, and hazards. |
| `MoveStateFacts` | `src/facts/types.rs` | Move fact, FEN after, full position facts, hazard and structure deltas. |
| `FactCollection<T>` / `FactValue<T>` | `src/facts/types.rs` | Tagged computed/uncomputed/unavailable contract. |
| `PieceFact` / `PieceRef` | `src/facts/types.rs` | Stable piece identity, square, attackers, defenders, loose/attacked flags, SEE, and only-defender data. |
| `CaptureOpportunity` | `src/facts/types.rs` | Legal capture candidate with SEE and safety flags. |
| `MotifOpportunity` | `src/facts/types.rs` | Validated fork opportunity. |
| `PinOpportunity` | `src/facts/types.rs` | Validated absolute/relative pin opportunity. |
| `*Opportunity` (skewer, discovery, discovered-defense, remove-guard, trapped, desperado, overload, attack-defender, deflection, lure-defender, interference, double-attack, x-ray attack/defense, win-exchange) + `MatePatternFact` | `src/facts/types.rs` | One struct per validated motif detector — registry v22 inventory in `docs/TEACHING_FACTS_PROTOCOL.md`; soundness rules in `docs/DETECTOR_SOUNDNESS.md`. |
| `HazardFact` | `src/facts/types.rs` | Stable summarized hazard: material, fork, pin, king pressure, or mate threat. |
| `FactsProvenance` | `src/facts/types.rs` | Engine identity, optional commit, facts registry version, and validators. |

Protocol constants:

| Constant | File | Current Responsibility |
|---|---|---|
| `TEACHING_FACTS_SCHEMA_VERSION` | `src/facts/mod.rs` | Breaking wire-schema version. |
| `FACTS_REGISTRY_VERSION` | `src/facts/mod.rs` | Additive or semantic facts/validator registry version. |

## Validation Standards

- `Position::from_fen` rejects structurally illegal FENs, missing kings, extra
  kings, bad side-to-move fields, bad castling/en-passant fields, and positions
  where the idle side is already in check.
- Required played moves in facts requests must be legal from `fenBefore`.
- Optional best/refutation branches report `FactError` and omit that branch when
  illegal. They must not invalidate the required played branch.
- Principal variation validation replays from `fenBefore` and records the first
  illegal PV move as an error.
- Opposite-side fact probes are unavailable while the actual side to move is in
  check; Rust must not fake legal states by simply flipping the turn.
- En passant is cleared for opposite-side probes because it belongs to the real
  turn only.
- Fact arrays with identity significance should be stable and deterministic.
- `computed([])` means a validator ran and found nothing; it is not the same as
  `uncomputed` or `unavailable`.
- NNUE/CVS models must match registry conventions and hashes. The engine refuses
  incompatible CVS-NNUE registry hashes rather than silently mis-mapping inputs.
- Search feature flags should be measured one variable at a time and documented
  before promotion to default behavior.

## File Map

### Core Library

| File | Responsibility |
|---|---|
| `src/lib.rs` | Public crate modules, `Color`, `Piece`, `MoveFlag`, `Move`, square helpers, and re-exports. |
| `src/position.rs` | Bitboard state, FEN parsing/serialization, validation, Zobrist hash, make/unmake, and history replay. |
| `src/attacks.rs` | Attack tables and attack queries for leapers, sliders, pawns, kings, and attackers. |
| `src/movegen.rs` | Pseudo/legal/noisy move generation, checks, castling, promotions, en passant, and move lists. |
| `src/see.rs` | Static exchange evaluation and MVV-LVA values. |
| `src/perft.rs` | Perft and divide functions for movegen validation. |
| `src/tt.rs` | Shared transposition table packing, probing, replacement, and flags. |
| `src/search.rs` | Search options, lane profiles, telemetry, alpha-beta, quiescence, pruning/selectivity, SMP, PV, and root scope. |

### Evaluation

| File | Responsibility |
|---|---|
| `src/eval/mod.rs` | Eval exports, phase constants, insufficient material, terminal handling, White-relative and side-relative eval. |
| `src/eval/pst.rs` | Piece-square table lookup. |
| `src/eval/weights.rs` | `ValueWeights`, `MaterialWeights`, `Rung2Weights`, defaults, and serde-compatible trained weight schema. |
| `src/eval/rung2.rs` | Rung-2 feature extraction and weighted contribution. |
| `src/eval/nnue.rs` | NNUE model loading, registry compatibility checks, full/incremental inference, and accumulator updates. |
| `src/eval/cvs_features.rs` | CVS geometry feature registry, active feature IDs/names, and registry hash. |

### Facts Engine

| File | Responsibility |
|---|---|
| `src/facts/mod.rs` | Facts modules, public exports, schema and registry constants. |
| `src/facts/types.rs` | Serializable request/response/data schemas for the teaching facts protocol. |
| `src/facts/position.rs` | Builds `PositionFacts`, opposite-side probes, side conversion, and square naming. |
| `src/facts/piece_safety.rs` | Piece refs/facts, attackers/defenders, only-defender relationships, SEE-losing facts, capture opportunities, and king safety. |
| `src/facts/pawn_structure.rs` | Doubled/isolated/passed/island/open/semi-open/shield/chain facts and structure deltas. |
| `src/facts/motifs.rs` | All validated motif detectors (18 at registry v22: fork, pin, skewer, discovery family, discovered defense, remove-guard/attack/deflect/lure-the-defender, overload, interference, trapped, desperado, double attack, x-ray attack/defense, win-exchange) plus the shared soundness helpers (`capture_legal_wrt_pin`, `legal_capture_gain`, `legal_material_quiescence`, `attack_defender_worst_case`). See `docs/DETECTOR_SOUNDNESS.md`. |
| `src/facts/mate_patterns.rs` | Named post-mate pattern classification (back-rank, smothered, epaulette, Damiano, Boden). |
| `src/facts/square_control.rs` | Deterministic 64-square control and legal-mover facts. |
| `src/facts/hazards.rs` | Derived hazards and hazard deltas from lower-level validated facts. |
| `src/facts/move_bundle.rs` | `TeachingFactBundleV1` builder, branch application, legal UCI validation, PV validation, provenance, and optional branch errors. |

### Binaries

| File | Responsibility |
|---|---|
| `src/bin/analyze.rs` | JSON-line app/arena server, batch FEN analysis, facts command, feature dumps, timed search, forced-move root scope, and telemetry JSON. |
| `src/bin/uci.rs` | UCI engine frontend. |
| `src/bin/perft.rs` | Perft runner and divide validation. |
| `src/bin/eval_parity.rs` | Eval parity checks against fixtures. |
| `src/bin/search_bench.rs` | Local search benchmark runner. |
| `src/bin/cvs_bench.rs` | CVS feature benchmark runner. |
| `src/bin/selfplay.rs` | Self-play data generation. |

### Tests

| File | Responsibility |
|---|---|
| `tests/perft.rs` | Move generation correctness across standard perft positions. |
| `tests/fen_validation.rs` | FEN validation and illegal-position rejection. |
| `tests/see_and_checks.rs` | SEE, attack, check, and gives-check behavior. |
| `tests/eval.rs` | Eval symmetry, material, terminal, and Rung-2 behavior. |
| `tests/search.rs` | Search mates, PV legality, TT, null move, repetition, forced root move, and forensic positions. |
| `tests/smp.rs`, `tests/specialist_lanes.rs` | SMP and lane behavior. |
| `tests/nnue_accumulator.rs` | Incremental NNUE accumulator parity. |
| `tests/facts_protocol.rs` | Facts schema, fixtures, provenance, optional branch errors, serve mode, and registry behavior. |
| `tests/facts_position.rs` | Piece identity, relationships, SEE availability, and FEN rejection in facts. |
| `tests/facts_piece_safety.rs` | Capture opportunities and king safety facts. |
| `tests/facts_pawn_structure.rs` | Pawn structure facts. |
| `tests/facts_fork.rs`, `tests/facts_pin.rs`, and one `tests/facts_<detector>.rs` battery per motif detector (skewer, discovered, discovered_defense, remove_guard, trapped, desperado, overload, attack_defender, deflection, lure_defender, interference, double_attack, xray_attack, xray_defense, win_exchange, mate_patterns) | Detector positives, hard negatives (including the refuting reply and every fuzz-found false-positive regression), stability, and non-mutation. |
| `tests/serve_diagnostic.rs` | Fixed-node diagnostic contract: cold determinism, exact budget, prior-search-cannot-alter-cold, warm TT carry. |
| `tests/facts_hazards.rs` | Hazard creation/removal and unavailable states. |
| `tests/danger.rs` | Danger extension and king-danger behavior. |

### Benchmarks, Training, and Reports

| Path | Responsibility |
|---|---|
| `benchmarks/` | Gate ladder, benchmark instructions, result templates. |
| `training/gen8/` | Gen8 training documentation and artifacts. |
| `frozen-evals/` | Frozen evaluation snapshots/manifests for parity and regression anchors. |
| `*_REPORT.md`, `SEARCH_*.md`, `GEN8_TRAINING_PLAN.md`, `CVS_*` | Engineering reports, accepted/rejected patches, claim discipline, and training/search notes. |

## Change Standards

When adding a new teaching fact:

1. Add or extend schema structs in `src/facts/types.rs`.
2. Add extraction/validation in the narrowest `src/facts/*` module, following the
   guard set and counting-proof rules in `docs/DETECTOR_SOUNDNESS.md`.
3. Wire it through `position_facts` or `move_bundle` as appropriate.
4. Add provenance validator names when the fact is active.
5. Add tests with positives, hard negatives, and unavailable/uncomputed behavior,
   then run the adversarial FP fuzz + engine-arbiter verification protocol from
   `docs/DETECTOR_SOUNDNESS.md` before merge.
6. Update the TypeScript mirror in `../chess-vision-studio/engine/teaching/types.ts`.
7. Update golden fixtures and increment `FACTS_REGISTRY_VERSION` for semantic or
   validator changes (one bump + one history line per detector).
8. Update `benchmarks/data/motif-taxonomy.json` (`detectedBy`) and the mapping in
   `benchmarks/scripts/check_detector_coverage.py`, and run that guard — CI fails
   on taxonomy claims the registry does not back.

When changing search:

1. Keep flags gated until measured.
2. Add a direct regression test for the position or family that motivated the
   change.
3. Record opponent, binary, time control, game count, weights, harness, and flags
   for any strength claim.
4. Avoid claiming transfer to human ratings from browser/WASM or bot games.

When changing eval/NNUE:

1. Preserve serde compatibility or add defaults for old weight files.
2. Refuse incompatible model registry hashes or conventions.
3. Run parity and accumulator tests.
4. Benchmark with fixed weights and fixed search flags.

## Local Verification

```bash
cargo fmt
cargo test
cargo test --release
cargo run --release --bin perft
cargo run --release --bin search_bench
```

App-side smoke tests:

```bash
cd ../chess-vision-studio
npm run build
npx vitest run arena/__tests__/engine-backend.test.ts
```

