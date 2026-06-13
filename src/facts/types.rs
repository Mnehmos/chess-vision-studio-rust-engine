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
    pub available_motifs: FactCollection<MotifOpportunity>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KingSafetyFact {
    pub side: Side,
    pub king_square: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOpportunity {
    pub move_uci: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotifOpportunity {
    pub validator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HazardFact {
    pub id: String,
    pub kind: String,
    pub side: Side,
    pub squares: Vec<String>,
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
