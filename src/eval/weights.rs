//! Trainable value-head weights — Rust port of the TS `ValueWeights` (8 base
//! scalars) and `Rung2Weights` (18 hazard-feature weights). The serde field names
//! match the trained JSON snapshots (`value-weights-mixed.json`,
//! `rung2-weights-mixed.json`) byte-for-byte, so the Rust engine loads the exact
//! weights the TS engine was gated with. Defaults reproduce the handcrafted eval.
use serde::Deserialize;

/// Multipliers on the base material values + the three scalar bonuses.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ValueWeights {
    pub material: MaterialWeights,
    #[serde(rename = "pstScale")]
    pub pst_scale: f64,
    #[serde(rename = "bishopPair")]
    pub bishop_pair: f64,
    pub tempo: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct MaterialWeights {
    pub p: f64,
    pub n: f64,
    pub b: f64,
    pub r: f64,
    pub q: f64,
}

impl Default for ValueWeights {
    fn default() -> Self {
        ValueWeights {
            material: MaterialWeights { p: 1.0, n: 1.0, b: 1.0, r: 1.0, q: 1.0 },
            pst_scale: 1.0,
            bishop_pair: 30.0,
            tempo: 10.0,
        }
    }
}

/// Rung-2 hazard-feature weights (cp per feature unit). All-zero = inert
/// (byte-identical handcrafted eval), exactly like the TS DEFAULT_RUNG2_WEIGHTS.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rung2Weights {
    pub mobility_knight: f64,
    pub mobility_bishop: f64,
    pub mobility_rook: f64,
    pub mobility_queen: f64,
    pub king_shield: f64,
    pub king_zone_pressure: f64,
    pub king_open_file: f64,
    pub passed_pawn_mg: f64,
    pub passed_pawn_eg: f64,
    pub connected_passed_pawn: f64,
    pub rook_open_file: f64,
    pub rook_semi_open_file: f64,
    pub rook_seventh: f64,
    pub doubled_pawn: f64,
    pub isolated_pawn: f64,
    pub bishop_pair_mg: f64,
    pub bishop_pair_eg: f64,
    pub hanging_piece: f64,
}

impl Rung2Weights {
    /// True when every weight is exactly zero (the inert default).
    pub fn is_zero(&self) -> bool {
        let w = self;
        w.mobility_knight == 0.0
            && w.mobility_bishop == 0.0
            && w.mobility_rook == 0.0
            && w.mobility_queen == 0.0
            && w.king_shield == 0.0
            && w.king_zone_pressure == 0.0
            && w.king_open_file == 0.0
            && w.passed_pawn_mg == 0.0
            && w.passed_pawn_eg == 0.0
            && w.connected_passed_pawn == 0.0
            && w.rook_open_file == 0.0
            && w.rook_semi_open_file == 0.0
            && w.rook_seventh == 0.0
            && w.doubled_pawn == 0.0
            && w.isolated_pawn == 0.0
            && w.bishop_pair_mg == 0.0
            && w.bishop_pair_eg == 0.0
            && w.hanging_piece == 0.0
    }
}
