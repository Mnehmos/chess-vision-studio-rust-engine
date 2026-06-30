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
    /// Analysis-only legal opportunities for the side that is not to move.
    /// These let the application prove a motif was newly allowed by a move.
    pub opponent_available_motifs: FactCollection<MotifOpportunity>,
    pub opponent_available_pins: FactCollection<PinOpportunity>,
    pub opponent_available_skewers: FactCollection<SkewerOpportunity>,
    pub opponent_available_discoveries: FactCollection<DiscoveryOpportunity>,
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
