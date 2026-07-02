# Teaching Facts Protocol V1

`TeachingFactBundleV1` is the versioned boundary between the Rust chess core and
Chess Vision Studio's teaching compiler. Rust emits deterministic chess facts.
The application combines those facts with Stockfish grades, classifies teaching
topics, and renders committed explanation plans.

The current additive contract is schema version 1, facts registry version **22**
(`FACTS_REGISTRY_VERSION` in `src/facts/mod.rs` is the source of truth; the version
history at the bottom of this document mirrors the comment block there).

## Request

Send one JSON line to `analyze --serve`:

```json
{
  "cmd": "facts",
  "schemaVersion": 1,
  "fenBefore": "...",
  "playedMoveUci": "e2e4",
  "bestMoveUci": "g1f3",
  "refutationUci": "d8h4",
  "principalVariationUci": ["g1f3", "d7d5"],
  "options": {
    "includeMotifOpportunities": true,
    "includeCounterfactual": true
  }
}
```

Required fields are `schemaVersion`, `fenBefore`, and `playedMoveUci`. The played
move must be legal from `fenBefore`. `bestMoveUci`, when present, is validated from
the same position. `refutationUci` is validated after the played move. A principal
variation is replayed from `fenBefore` and stops at the first illegal move.

## Response Shape

```ts
interface TeachingFactBundleV1 {
  schemaVersion: 1;
  fenBefore: string;
  before: PositionFacts;
  played: MoveStateFacts;
  best?: MoveStateFacts;
  refutation?: MoveStateFacts;
  provenance: {
    engine: 'cvs-bitboard-core';
    engineCommit?: string;
    factsRegistryVersion: 5;
    validators: string[];
  };
  errors: FactError[];
}
```

Optional branch failures are reported in `errors` and omit that branch. An invalid
required played move or unsupported schema returns a top-level protocol error.

## Position Facts

```ts
interface PositionFacts {
  sideToMove: 'white' | 'black';
  pieces: PieceFact[];
  pawnStructure: PawnStructureFacts;
  kingSafety: FactCollection<KingSafetyFact>;
  availableCaptures: FactCollection<CaptureOpportunity>;
  opponentAvailableCaptures: FactCollection<CaptureOpportunity>;
  // Motif-opportunity collections (gated by options.includeMotifOpportunities).
  // Every collection below also has an opponentAvailable* twin — the symmetric
  // analysis probe for the side not to move.
  availableMotifs: FactCollection<MotifOpportunity>;              // forks (v2)
  availablePins: FactCollection<PinOpportunity>;                  // v3
  availableSkewers: FactCollection<SkewerOpportunity>;            // v7
  availableDiscoveries: FactCollection<DiscoveryOpportunity>;     // v8
  availableDiscoveredDefense: FactCollection<DiscoveredDefenseOpportunity>; // v18
  availableRemoveGuard: FactCollection<RemoveGuardOpportunity>;   // v9
  availableTrapped: FactCollection<TrappedPieceOpportunity>;      // v10
  availableDesperado: FactCollection<DesperadoOpportunity>;       // v22
  availableMatePatterns: FactCollection<MatePatternFact>;         // v11
  availableOverload: FactCollection<OverloadOpportunity>;         // v12
  availableAttackDefender: FactCollection<AttackDefenderOpportunity>; // v13
  availableDeflection: FactCollection<DeflectionOpportunity>;     // v19
  availableLureDefender: FactCollection<LureDefenderOpportunity>; // v20
  availableInterference: FactCollection<InterferenceOpportunity>; // v14
  availableDoubleAttack: FactCollection<DoubleAttackOpportunity>; // v15
  availableXrayAttack: FactCollection<XRayOpportunity>;           // v16
  availableXrayDefense: FactCollection<XRayDefenseOpportunity>;   // v17
  availableWinExchange: FactCollection<WinExchangeOpportunity>;   // v21
  // ...opponentAvailable* twins of all of the above...
  hazards: FactCollection<HazardFact>;
  squareFacts: FactCollection<SquareFact>;                        // v6
}
```

`available*` collections describe legal opportunities for `sideToMove`.
`opponentAvailable*` collections are symmetric analysis probes for the other side.
An opposite-side probe is `unavailable` when the real side to move is in check,
because granting the checking side another turn would create an illegal state.
En-passant is cleared for opposite-side probes.

Each piece includes its stable reference, square, geometric attackers and
defenders, counts, loose/attacked flags, only-defender relationships, and SEE
status. SEE is computed only for a legal capture; unavailable states are never
guessed by flipping the turn.

Pawn structure includes doubled, isolated, passed, island, open-file,
semi-open-file, king-shield, and pawn-chain facts. Move branches include computed
created and removed structure deltas.

## Captures and King Safety

Structured legal captures and king-safety facts (since registry v5):

```ts
interface CaptureOpportunity {
  moveUci: string;
  attacker: PieceRef;
  victim: PieceRef;
  victimSquare: string;
  seeCp: number;
  givesCheck: boolean;
  capturingPieceSurvives: boolean;
  highestValueSafeCapture: boolean;
}

interface KingSafetyFact {
  side: 'white' | 'black';
  kingSquare: string;
  inCheck: boolean;
  attackers: PieceRef[];
  pressuredSquares: string[];
  legalEscapeSquares: FactCollection<string>;
}
```

Capture ordering and flags are deterministic. Legal escape squares are computed
symmetrically when the requested side can be probed legally.

## Motif Detectors

When `options.includeMotifOpportunities` is true, each position view includes every
validated motif-opportunity collection for both sides. When false or absent, those
collections are `uncomputed` with reason `not_requested`. Two representative shapes
(the rest follow the same conventions — camelCase serde, `kind` + `validator`
strings, `PieceRef` participants, deterministic ordering, and a `materialGain` that
is the engine's proven worst-case, never an optimistic count):

```ts
interface MotifOpportunity {
  kind: 'fork';
  validator: 'fork_validation';
  moveUci: string;
  forkingPiece: PieceRef;
  targets: PieceRef[];
  givesCheck: boolean;
  kingTarget: boolean;
  materialGain: number;
}

interface PinOpportunity {
  kind: 'absolute' | 'relative';
  validator: 'pin_validation';
  moveUci: string;
  pinner: PieceRef;
  pinned: PieceRef;
  anchor: PieceRef;
  ray: string[];
  givesCheck: boolean;
  pinnedImmobile: boolean;
}
```

### Detector inventory (registry v22)

| Detector | `kind` value(s) | Validator | Claim |
|---|---|---|---|
| Fork | `fork` | `fork_validation` | One moved piece attacks ≥2 winnable targets; forker not simply lost. |
| Pin | `absolute` / `relative` | `pin_validation` | A move creates a pin against the king or a dearer piece. |
| Skewer | `skewer` | `skewer_validation` | A dearer front piece is forced off a line, exposing the piece behind. |
| Discovery | `discovered_attack` / `discovered_check` / `double_check` / `discoverer_checks` | `discovery_validation` | Moving unveils a rear slider's attack (or check); `discoverer_checks` = the MOVING piece checks while the unveiled slider wins material. |
| Discovered defense | `discovered_defense` | `discovered_defense_validation` | Moving unveils a friendly slider that rescues a legally hanging piece. |
| Capturing the defender | `capture_the_defender` | `remove_guard_validation` | Capturing a defender makes its charge winnable. |
| Trapped piece | `trapped_piece` | `trapped_piece_validation` | An enemy piece is attacked and every escape still loses it. |
| Desperado | `desperado` | `desperado_validation` | OUR doomed piece has a capture that recovers material before dying (worst-case gain over a full legal capture quiescence). |
| Mate patterns | `back_rank_mate` / `smothered_mate` / `epaulette_mate` / `damiano_mate` / `boden_mate` | `mate_pattern_validation` | Post-mate classification of the delivered pattern. |
| Overloaded defender | `overloading` | `overload_validation` | A defender guards more duties than it can meet. |
| Attacking the defender | `attacking_the_defender` | `attack_defender_validation` | Attack a sole defender so it must move or be lost (worst-case over all enemy replies, replies debited for what they capture). |
| Deflection | `deflection` | `deflection_validation` | A non-capturing move forces a sole defender off its charge (same debited worst-case). |
| Luring the defender | `luring_the_defender` | `lure_defender_validation` | An offered sacrifice decoys the sole defender onto the capture square. |
| Interference | `interference` | `interference_validation` | An interposition severs a slider's defense of a target. |
| Double attack | `double_attack` | `double_attack_validation` | One move creates two winning threats from two DIFFERENT pieces. |
| X-ray attack | `xray_attack` | `xray_attack_validation` | A slider wins a defended front piece by counting through it to the piece behind. |
| X-ray defense | `xray_defense` | `xray_defense_validation` | A slider defends a friendly piece through an enemy blocker. |
| Win the exchange | `win_the_exchange` | `win_exchange_validation` | A capture whose full SEE swap wins a rook for a minor. |

The taxonomy mapping (which ChessTempo motif each detector covers) lives in
`benchmarks/data/motif-taxonomy.json`; `benchmarks/scripts/check_detector_coverage.py`
fails CI when the taxonomy claims a detector the registry does not register.
Soundness rules and the false-positive classes these detectors are hardened
against are documented in [DETECTOR_SOUNDNESS.md](DETECTOR_SOUNDNESS.md).

## Hazards and Move Deltas

When motif opportunities are requested and symmetric probes are available, the
engine derives stable hazards from validated lower-level facts (since registry v5):

```ts
interface HazardFact {
  id: string;
  kind:
    | 'losing_material'
    | 'fork_threat'
    | 'pin_constraint'
    | 'king_pressure'
    | 'mate_threat';
  side: 'white' | 'black';
  squares: string[];
  magnitudeCp?: number;
  moveUci?: string;
}

interface MoveFactDeltas {
  createdHazards: FactCollection<HazardFact>;
  removedHazards: FactCollection<HazardFact>;
  worsenedHazards: FactCollection<HazardFact>;
  createdStructures: FactCollection<StructureDelta>;
  removedStructures: FactCollection<StructureDelta>;
}
```

Hazards summarize facts; they do not grade a move or claim causation. If the
symmetric position probe cannot be performed, hazards and hazard deltas are
explicitly unavailable.

## Computed, Uncomputed, and Unavailable

Every optional fact collection is a tagged union:

```ts
type FactCollection<T> =
  | { status: 'computed'; items: T[] }
  | { status: 'uncomputed'; reason: string }
  | { status: 'unavailable'; reason: string };
```

`computed` with an empty list means the validator ran and found none. `uncomputed`
means the validator was not requested or is outside the current registry.
`unavailable` means the validator exists but cannot answer for this state. These
states must not be collapsed into zero, `false`, or an empty list.

A scalar fact uses the equivalent `FactValue<T>` union. A boolean inside a
`computed` value is a deterministic result.

## Identity and Conventions

- Squares use lowercase algebraic coordinates: `a1` through `h8`.
- Moves use legal long UCI: `e2e4`, `e7e8q`.
- Sides are `white` and `black`.
- Piece types are lowercase names.
- Piece IDs use `<side>-<piece>-<square>`, such as `white-knight-f3`.
- Fact IDs describe their complete current identity. They are not array indexes.
- Arrays are serialized in stable lexical order where identity matters.

## Proof and Validators

Registry v22 provenance lists, in order (the exact list is asserted by
`tests/facts_protocol.rs::registry_provenance_lists_every_active_validator`):

- `legal_move_generation`, `attack_map`, `see`, `capture_opportunities`,
  `king_safety`, `pawn_structure`, `square_control` — the base fact layers.
- `fork_validation`, `pin_validation`, `skewer_validation`, `discovery_validation`,
  `remove_guard_validation`, `trapped_piece_validation`, `mate_pattern_validation`,
  `overload_validation`, `attack_defender_validation`, `interference_validation`,
  `double_attack_validation`, `xray_attack_validation`, `xray_defense_validation`,
  `discovered_defense_validation`, `deflection_validation`,
  `lure_defender_validation`, `win_exchange_validation`, `desperado_validation`
  — the motif detectors (present only when motif opportunities were requested).

The engine returns validator provenance but no causal attribution. Application
teaching events must cite the validators that support their evidence and must fail
closed when a required collection is missing, uncomputed, or unavailable.

## Responsibility Boundary

Rust owns legal board truth, relationships, SEE, captures, motifs, pins, king
safety, structural facts, hazards, deltas, and provenance. Stockfish grades moves
and supplies best lines. The application owns topic classification, causal gates,
teaching vocabulary, UI, aggregation, and optional narration. An LLM teaching
narrator receives only a committed `ExplanationPlan`.

## Golden Fixtures

`fixtures/teaching-facts/v1` contains five byte-stable response fixtures:

- `allowed-fork.json`
- `allowed-pin.json`
- `missed-hanging-piece.json`
- `failed-defense.json`
- `pawn-structure-damage.json`

Rust regenerates and byte-compares them in `tests/facts_protocol.rs`. The
application consumes mirrored copies in contract and compiler tests.

## Versioning Policy

- Additive fields may retain `schemaVersion` and increment `factsRegistryVersion`.
- Renaming fields, changing conventions or tagged-union semantics, or changing
  required fields requires a new schema version.
- Changes to fact meaning or validator behavior increment `factsRegistryVersion`.
- Consumers reject unknown schema versions and incomplete current-registry shapes.
- Cached teaching output is valid only for its schema, facts registry, compiler,
  and engine settings provenance.

## Registry Version History

Mirrors the comment block above `FACTS_REGISTRY_VERSION` in `src/facts/mod.rs`:

| v | Added |
|---|---|
| 2 | fork enumeration (`availableMotifs`) |
| 3 | pin enumeration |
| 4 | non-moving-side motif probes (`opponentAvailable*`) |
| 5 | structured captures, symmetric capture probes, king safety, king shields |
| 6 | 64-square control + legal movers (`squareFacts`) |
| 7 | skewers |
| 8 | discovered attacks |
| 9 | capturing-the-defender |
| 10 | trapped pieces |
| 11 | named mate patterns |
| 12 | overloaded defenders |
| 13 | attacking-the-defender |
| 14 | interference |
| 15 | double attack |
| 16 | x-ray attack |
| 17 | x-ray defense |
| 18 | discovered defense |
| 19 | deflection / distraction |
| 20 | luring-the-defender (decoy) |
| 21 | win-the-exchange |
| 22 | desperado |
