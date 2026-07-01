use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeachingFactsRequestV1 {
    pub schema_version: u32,
    pub fen_before: String,
    pub played_move_uci: String,
    pub best_move_uci: Option<String>,
    pub refutation_uci: Option<String>,
    pub principal_variation_uci: Option<Vec<String>>,
    pub options: Option<TeachingFactsOptionsV1>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeachingFactsOptionsV1 {
    #[serde(default)]
    pub include_motif_opportunities: bool,
    #[serde(default = "default_true")]
    pub include_counterfactual: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeachingFactBundleV1 {
    pub schema_version: u32,
    pub fen_before: String,
    pub before: PositionFacts,
    pub played: MoveStateFacts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best: Option<MoveStateFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refutation: Option<MoveStateFacts>,
    pub provenance: FactsProvenance,
    pub errors: Vec<FactError>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionFacts {
    pub side_to_move: Side,
    pub pieces: Vec<PieceFact>,
    pub pawn_structure: PawnStructureFacts,
    pub king_safety: FactCollection<KingSafetyFact>,
    pub available_captures: FactCollection<CaptureOpportunity>,
    pub opponent_available_captures: FactCollection<CaptureOpportunity>,
    pub available_motifs: FactCollection<MotifOpportunity>,
    pub available_pins: FactCollection<PinOpportunity>,
    pub available_skewers: FactCollection<SkewerOpportunity>,
    pub available_discoveries: FactCollection<DiscoveryOpportunity>,
    pub available_discovered_defense: FactCollection<DiscoveredDefenseOpportunity>,
    pub available_remove_guard: FactCollection<RemoveGuardOpportunity>,
    pub available_trapped: FactCollection<TrappedPieceOpportunity>,
    pub available_mate_patterns: FactCollection<MatePatternFact>,
    pub available_overload: FactCollection<OverloadOpportunity>,
    pub available_attack_defender: FactCollection<AttackDefenderOpportunity>,
    pub available_deflection: FactCollection<DeflectionOpportunity>,
    pub available_lure_defender: FactCollection<LureDefenderOpportunity>,
    pub available_interference: FactCollection<InterferenceOpportunity>,
    pub available_double_attack: FactCollection<DoubleAttackOpportunity>,
    pub available_xray_attack: FactCollection<XRayOpportunity>,
    pub available_xray_defense: FactCollection<XRayDefenseOpportunity>,
    pub available_win_exchange: FactCollection<WinExchangeOpportunity>,
    /// Analysis-only legal opportunities for the side that is not to move.
    /// These let the application prove a motif was newly allowed by a move.
    pub opponent_available_motifs: FactCollection<MotifOpportunity>,
    pub opponent_available_pins: FactCollection<PinOpportunity>,
    pub opponent_available_skewers: FactCollection<SkewerOpportunity>,
    pub opponent_available_discoveries: FactCollection<DiscoveryOpportunity>,
    pub opponent_available_discovered_defense: FactCollection<DiscoveredDefenseOpportunity>,
    pub opponent_available_remove_guard: FactCollection<RemoveGuardOpportunity>,
    pub opponent_available_trapped: FactCollection<TrappedPieceOpportunity>,
    pub opponent_available_mate_patterns: FactCollection<MatePatternFact>,
    pub opponent_available_overload: FactCollection<OverloadOpportunity>,
    pub opponent_available_attack_defender: FactCollection<AttackDefenderOpportunity>,
    pub opponent_available_deflection: FactCollection<DeflectionOpportunity>,
    pub opponent_available_lure_defender: FactCollection<LureDefenderOpportunity>,
    pub opponent_available_interference: FactCollection<InterferenceOpportunity>,
    pub opponent_available_double_attack: FactCollection<DoubleAttackOpportunity>,
    pub opponent_available_xray_attack: FactCollection<XRayOpportunity>,
    pub opponent_available_xray_defense: FactCollection<XRayDefenseOpportunity>,
    pub opponent_available_win_exchange: FactCollection<WinExchangeOpportunity>,
    pub hazards: FactCollection<HazardFact>,
    pub square_facts: FactCollection<SquareFact>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveStateFacts {
    pub r#move: MoveFact,
    pub fen_after: String,
    pub position: PositionFacts,
    pub deltas: MoveFactDeltas,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFact {
    pub uci: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFactDeltas {
    pub created_hazards: FactCollection<HazardFact>,
    pub removed_hazards: FactCollection<HazardFact>,
    pub worsened_hazards: FactCollection<HazardFact>,
    pub created_structures: FactCollection<StructureDelta>,
    pub removed_structures: FactCollection<StructureDelta>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FactCollection<T> {
    Computed { items: Vec<T> },
    Uncomputed { reason: String },
    Unavailable { reason: String },
}

impl<T> FactCollection<T> {
    pub fn computed(items: Vec<T>) -> Self {
        Self::Computed { items }
    }

    pub fn uncomputed(reason: impl Into<String>) -> Self {
        Self::Uncomputed {
            reason: reason.into(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FactValue<T> {
    Computed { value: T },
    Uncomputed { reason: String },
    Unavailable { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    White,
    Black,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceType {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PieceRef {
    pub id: String,
    pub side: Side,
    pub piece_type: PieceType,
    pub square: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PieceFact {
    #[serde(flatten)]
    pub piece: PieceRef,
    pub attackers: Vec<PieceRef>,
    pub defenders: Vec<PieceRef>,
    pub attacker_count: u32,
    pub defender_count: u32,
    pub attacked: bool,
    pub loose: bool,
    pub see: FactValue<SeeLosingFact>,
    pub only_defender_of: Vec<PieceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeeLosingFact {
    pub losing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_capture_uci: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_cp: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PawnStructureFacts {
    pub doubled: Vec<DoubledPawnFact>,
    pub isolated: Vec<PieceRef>,
    pub passed: Vec<PieceRef>,
    pub islands: Vec<PawnIslandFact>,
    pub backward: FactCollection<PieceRef>,
    pub connected_passed: FactCollection<PieceRef>,
    pub open_files: FactCollection<String>,
    pub semi_open_files: FactCollection<SideFileFact>,
    pub king_shield_missing: FactCollection<KingShieldFact>,
    pub pawn_chains: FactCollection<PawnChainFact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoubledPawnFact {
    pub id: String,
    pub side: Side,
    pub file: String,
    pub squares: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PawnIslandFact {
    pub id: String,
    pub side: Side,
    pub files: Vec<String>,
    pub squares: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureDelta {
    pub fact_id: String,
    pub kind: String,
    pub side: Side,
    pub squares: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideFileFact {
    pub side: Side,
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KingShieldFact {
    pub side: Side,
    pub king_square: String,
    pub missing_squares: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PawnChainFact {
    pub side: Side,
    pub squares: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KingSafetyFact {
    pub side: Side,
    pub king_square: String,
    pub in_check: bool,
    pub attackers: Vec<PieceRef>,
    pub pressured_squares: Vec<String>,
    pub legal_escape_squares: FactCollection<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOpportunity {
    pub move_uci: String,
    pub attacker: PieceRef,
    pub victim: PieceRef,
    pub victim_square: String,
    pub see_cp: i32,
    pub gives_check: bool,
    pub capturing_piece_survives: bool,
    pub highest_value_safe_capture: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotifOpportunity {
    /// Motif family — currently always "fork".
    pub kind: String,
    /// Validator that proved it — "fork_validation".
    pub validator: String,
    /// The single move that creates the motif (long UCI).
    pub move_uci: String,
    /// The piece that delivers the motif, referenced at its post-move square.
    pub forking_piece: PieceRef,
    /// The enemy pieces the motif piece attacks (sorted by id).
    pub targets: Vec<PieceRef>,
    /// Whether the motif move gives check.
    pub gives_check: bool,
    /// Whether one of the targets is the enemy king.
    pub king_target: bool,
    /// Estimated forced/likely material consequence in centipawns.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinOpportunity {
    /// "absolute" (pinned to the king) or "relative" (pinned to a higher-value piece).
    pub kind: String,
    /// Validator that proved it — "pin_validation".
    pub validator: String,
    /// The single move that creates the pin (long UCI).
    pub move_uci: String,
    /// The pinning piece, referenced at its post-move square.
    pub pinner: PieceRef,
    /// The pinned enemy piece.
    pub pinned: PieceRef,
    /// The piece behind the pinned one (the king for an absolute pin).
    pub anchor: PieceRef,
    /// Squares between pinner and anchor along the pin line (includes the pinned square).
    pub ray: Vec<String>,
    /// Whether the pinning move gives check.
    pub gives_check: bool,
    /// Whether the pinned piece is legally immobile (true for an absolute pin).
    pub pinned_immobile: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkewerOpportunity {
    /// Motif family — always "skewer". The granular ChessTempo subtype
    /// (skewer-of-king/-queen/-rook, relative) is derivable from `front.pieceType`.
    pub kind: String,
    /// Validator that proved it — "skewer_validation".
    pub validator: String,
    /// The single move that creates the skewer (long UCI).
    pub move_uci: String,
    /// The skewering slider, referenced at its post-move square.
    pub skewerer: PieceRef,
    /// The attacked, more-valuable enemy piece forced to step aside (the king for a
    /// king skewer).
    pub front: PieceRef,
    /// The lesser enemy piece exposed directly behind the front piece on the line.
    pub back: PieceRef,
    /// Squares between the skewerer and the back piece along the skewer line.
    pub ray: Vec<String>,
    /// Whether the skewering move gives check (true for a king skewer).
    pub gives_check: bool,
    /// Estimated material won when the front piece steps aside, in centipawns.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryOpportunity {
    /// Subtype: "discovered_attack" | "discovered_check" | "double_check".
    pub kind: String,
    /// Validator that proved it — "discovery_validation".
    pub validator: String,
    /// The single move that creates the discovery (long UCI).
    pub move_uci: String,
    /// The piece that moves off the line, at its post-move square (promoted type if a promotion).
    pub mover: PieceRef,
    /// The rear friendly slider unveiled by the move, at its unchanged square.
    pub slider: PieceRef,
    /// The enemy piece the unveiled slider now attacks (the king for a discovered check).
    pub target: PieceRef,
    /// Squares between the slider and the target along the unveiled line.
    pub ray: Vec<String>,
    /// Whether the move gives check (overall).
    pub gives_check: bool,
    /// Whether the unveiled slider checks the enemy king.
    pub discovered_check: bool,
    /// Whether both the unveiled slider AND the moved piece check the king.
    pub double_check: bool,
    /// Whether the moved piece also makes its own winning threat from its new square.
    pub mover_threatens: bool,
    /// Estimated material consequence in centipawns (the unveiled target, or the moved
    /// piece's simultaneous threat for a forcing discovered check).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredDefenseOpportunity {
    /// Motif family — always "discovered_defense".
    pub kind: String,
    /// Validator that proved it — "discovered_defense_validation".
    pub validator: String,
    /// The single move that unveils the defense (long UCI).
    pub move_uci: String,
    /// The piece that moves off the line, at its post-move square (promoted type if a promotion).
    pub mover: PieceRef,
    /// The rear friendly slider unveiled by the move, at its unchanged square.
    pub slider: PieceRef,
    /// The friendly piece (non-king) that was hanging and is now defended by the unveiled slider.
    pub defended_piece: PieceRef,
    /// Squares between the slider and the defended piece along the unveiled line.
    pub ray: Vec<String>,
    /// Whether the move gives check.
    pub gives_check: bool,
    /// Centipawns the discovery saves — the enemy's pre-move winning SEE on the rescued square.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveGuardOpportunity {
    /// Motif family — "capture_the_defender" (removing the guard, capturing variant).
    pub kind: String,
    /// Validator that proved it — "remove_guard_validation".
    pub validator: String,
    /// The single capturing move that removes the guard (long UCI).
    pub move_uci: String,
    /// The capturing piece, at its post-move square (promoted type if it promotes).
    pub mover: PieceRef,
    /// The enemy defender that was captured, at its pre-move square.
    pub captured_defender: PieceRef,
    /// The enemy piece that becomes winnable once the defender is gone.
    pub target: PieceRef,
    /// Whether the capturing move gives check.
    pub gives_check: bool,
    /// SEE centipawns won on the target once the defender is removed.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrappedPieceOpportunity {
    /// Motif family — always "trapped_piece".
    pub kind: String,
    /// Validator that proved it — "trapped_piece_validation".
    pub validator: String,
    /// The trapped enemy piece, at its current square.
    pub piece: PieceRef,
    /// Our pieces currently attacking it (sorted by id).
    pub attackers: Vec<PieceRef>,
    /// The enemy escape destinations we checked (every legal move of the piece still
    /// loses it), for explainability. Sorted, deduped.
    pub escape_squares_tried: Vec<String>,
    /// SEE centipawns we win — the least-bad outcome over staying + every escape.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverloadOpportunity {
    /// Motif family — always "overloading".
    pub kind: String,
    /// Validator that proved it — "overload_validation".
    pub validator: String,
    /// The overloaded enemy defender that cannot guard all its charges at once.
    pub overloaded_defender: PieceRef,
    /// The enemy pieces D critically guards, each winnable once D leaves (sorted by id).
    pub targets: Vec<PieceRef>,
    /// The second-best per-target SEE once the defender is removed (the opponent saves
    /// the dearer charge; we collect the next-best). This is a static teaching figure —
    /// it excludes the deflection sacrifice cost of actually pulling the defender off, so
    /// the realized net can be lower. Mirrors the fork detector's second-best convention.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttackDefenderOpportunity {
    /// Motif family — always "attacking_the_defender".
    pub kind: String,
    /// Validator that proved it — "attack_defender_validation".
    pub validator: String,
    /// The move that attacks the guard (long UCI); never a capture of the defender.
    pub move_uci: String,
    /// Our piece that now attacks the defender, at its post-move square.
    pub mover: PieceRef,
    /// The enemy defender we attack — the sole guard of the target(s), at its square.
    pub attacked_defender: PieceRef,
    /// The enemy charge(s) the defender solely guards, winnable once it is evicted
    /// (sorted by id). Non-king, non-pawn.
    pub targets: Vec<PieceRef>,
    /// Whether the attacking move gives check.
    pub gives_check: bool,
    /// SEE centipawns we win against the enemy's best reply — the least-bad outcome over
    /// every reply of (win the standing/relocated defender, or win an abandoned charge).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeflectionOpportunity {
    /// Motif family — always "deflection".
    pub kind: String,
    /// Validator that proved it — "deflection_validation".
    pub validator: String,
    /// The non-capturing-of-D move that distracts the guard (long UCI); never captures D.
    pub move_uci: String,
    /// Our piece that creates the forcing threat, at its post-move square.
    pub mover: PieceRef,
    /// The enemy defender lured off its post — the sole guard of the target(s), NOT
    /// profitably capturable in place. At its (pre-eviction) square.
    pub distracted_defender: PieceRef,
    /// The enemy charge(s) the defender solely guards, winnable once it is deflected
    /// (sorted by id). Non-king, non-pawn.
    pub targets: Vec<PieceRef>,
    /// Whether the deflecting move gives check.
    pub gives_check: bool,
    /// SEE centipawns we win against the enemy's best reply — the least-bad outcome over
    /// every reply of (win the standing/relocated defender, or a charge the eviction
    /// abandons). SEE_VALUE scale (knight 320, bishop 330, rook 500).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LureDefenderOpportunity {
    /// Motif family — always "luring_the_defender".
    pub kind: String,
    /// Validator that proved it — "lure_defender_validation".
    pub validator: String,
    /// The offered-sacrifice move that decoys the guard (long UCI); the mover lands on a
    /// square where it IS profitably capturable, forcing the recapture.
    pub move_uci: String,
    /// Our SACRIFICED piece (the decoy), at its post-move square s — the piece the enemy
    /// defender is forced to recapture.
    pub mover: PieceRef,
    /// The enemy defender lured off its post by being forced to recapture s — the sole
    /// guard of the target(s), at its (pre-recapture) square.
    pub lured_defender: PieceRef,
    /// The enemy charge(s) the defender solely guards, winnable once it is lured away
    /// (sorted by id). Non-king, non-pawn.
    pub targets: Vec<PieceRef>,
    /// Whether the luring move gives check.
    pub gives_check: bool,
    /// SEE centipawns we net against the enemy's best reply — least-bad over every reply,
    /// already DEBITED for the sacrificed decoy (attack_defender_worst_case subtracts
    /// enemy_take). SEE_VALUE scale.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterferenceOpportunity {
    /// Motif family — always "interference".
    pub kind: String,
    /// Validator that proved it — "interference_validation".
    pub validator: String,
    /// The move that interposes on the line (long UCI); quiet or a capture landing on S.
    pub move_uci: String,
    /// Our interposing piece, at its post-move square S (promoted type if a promotion).
    pub interposer: PieceRef,
    /// The enemy slider whose defense of the target is severed, at its (unchanged) square.
    pub cut_defender: PieceRef,
    /// The enemy piece (non-king, non-pawn) that becomes winnable once the line is cut.
    pub target: PieceRef,
    /// Whether the interposing move gives check.
    pub gives_check: bool,
    /// SEE centipawns we win on the target against the enemy's best reply (least-bad over
    /// every reply; a recapture that re-opens the line and re-defends the target refutes it).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoubleAttackOpportunity {
    /// Motif family — always "double_attack".
    pub kind: String,
    /// Validator that proved it — "double_attack_validation".
    pub validator: String,
    /// The single move that creates both threats (long UCI).
    pub move_uci: String,
    /// The moved piece that delivers threat A, at its post-move square (promoted type if it promotes).
    pub mover: PieceRef,
    /// The distinct friendly piece whose threat on B is newly realized by the move, at its (unchanged) square.
    pub second_attacker: PieceRef,
    /// The enemy piece the moved piece threatens (threat A).
    pub target_a: PieceRef,
    /// The enemy piece the second attacker threatens (threat B), distinct square from A.
    pub target_b: PieceRef,
    /// Whether the double-attack move gives check.
    pub gives_check: bool,
    /// Estimated forced material: the enemy saves the dearer target, we take the lesser — min(threatA, threatB).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XRayOpportunity {
    /// Motif family — always "xray_attack".
    pub kind: String,
    /// Validator that proved it — "xray_attack_validation".
    pub validator: String,
    /// The single move that creates the x-ray alignment (long UCI).
    pub move_uci: String,
    /// Our slider, at its post-move square (promoted type if a slider promotion).
    pub xrayer: PieceRef,
    /// The defended front enemy piece attacked directly.
    pub front: PieceRef,
    /// The rear enemy piece seen THROUGH the front on the same line — the piece whose
    /// presence, revealed by the shrinking occupancy, makes the counting-through win.
    pub back: PieceRef,
    /// Squares between the xrayer and the back piece along the x-ray line.
    pub ray: Vec<String>,
    /// Whether the move gives check.
    pub gives_check: bool,
    /// Full-SEE centipawns won on the front square, counting through the reveal.
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XRayDefenseOpportunity {
    /// Motif family — always "xray_defense".
    pub kind: String,
    /// Validator that proved it — "xray_defense_validation".
    pub validator: String,
    /// The single move that places/uses the defending slider (long UCI).
    pub move_uci: String,
    /// Our slider, at its post-move square (promoted type if a slider promotion).
    pub xrayer: PieceRef,
    /// The ENEMY piece on the line between the xrayer and G, through which the
    /// defense passes; a naive one-square count is blocked by it.
    pub front_enemy: PieceRef,
    /// Our friendly piece defended through the enemy front — held only via the
    /// through-recapture that see() reveals as the occupancy shrinks.
    pub defended: PieceRef,
    /// Squares between the xrayer and the defended piece along the x-ray line.
    pub ray: Vec<String>,
    /// Whether the move gives check.
    pub gives_check: bool,
    /// Centipawns of G saved (the SEE the enemy would win if the xray were absent).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinExchangeOpportunity {
    /// Motif family — always "win_the_exchange".
    pub kind: String,
    /// Validator that proved it — "win_exchange_validation".
    pub validator: String,
    /// The single capturing move that wins the exchange (long UCI).
    pub move_uci: String,
    /// Our capturing minor (bishop/knight), at its post-move square.
    pub mover: PieceRef,
    /// The enemy rook captured for the minor, at its pre-move square.
    pub victim: PieceRef,
    /// Whether the capturing move gives check.
    pub gives_check: bool,
    /// Full-SEE centipawns won (SEE_VALUE scale: ~170 R-for-B, ~180 R-for-N).
    pub material_gain: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatePatternFact {
    /// Pattern slug (snake_case): "back_rank_mate" | "smothered_mate" | ...
    pub kind: String,
    /// Validator that proved it — "mate_pattern_validation".
    pub validator: String,
    /// The mating move (long UCI).
    pub move_uci: String,
    /// The piece that delivers mate, at its post-move square.
    pub mating_piece: PieceRef,
    /// The checkmated king, at its (unchanged) square.
    pub mated_king: PieceRef,
    /// Pattern-defining squares (king, mater, the blocked/covered escapes). Sorted.
    pub key_squares: Vec<String>,
    /// Always true — every mate gives check.
    pub gives_check: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HazardFact {
    pub id: String,
    pub kind: String,
    pub side: Side,
    pub squares: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnitude_cp: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_uci: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SquareFact {
    pub square: String,
    pub occupied: bool,
    pub attacked_by_white: Vec<PieceRef>,
    pub attacked_by_black: Vec<PieceRef>,
    pub controlled_by_white: bool,
    pub controlled_by_black: bool,
    pub legal_movers_white: FactCollection<PieceRef>,
    pub legal_movers_black: FactCollection<PieceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactsProvenance {
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_commit: Option<String>,
    pub facts_registry_version: u32,
    pub validators: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}
