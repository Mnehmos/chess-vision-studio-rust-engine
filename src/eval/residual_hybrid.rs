//! Residual-hybrid evaluation scaffold (#10, slice 1).
//!
//! The shippable residual contract (INV-7): the engine score is the FROZEN champion baseline plus
//! two ADDITIVE, INDEPENDENTLY ZEROABLE components — a declared handcrafted delta and an optional
//! learned residual:
//!
//! ```text
//! eval(pos) = champion_baseline(pos)
//!           + lambda_h * handcrafted_delta(pos)
//!           + lambda_r * learned_residual(pos)
//! ```
//!
//! Zeroability is REVERSIBILITY, not a quality claim: when both lambdas are zero the added code paths
//! are BYPASSED and the engine recovers the champion's exact score (byte-identical). Promotion still
//! requires #5's SPRT pass on the non-zero configuration.
//!
//! This module is purely ADDITIVE: it wraps a baseline score the caller already computed and does NOT
//! change the live search/eval path, so the champion's play is unchanged until a profile is wired in
//! and SPRT-promoted. Steering profiles (diversity, not strength) are marked `SteerOnly` and refused
//! by promotion tooling. The learned-residual branch is a stub here (returns 0) pending a trained
//! model; this slice lands the contract, the zero-weight fast path, the handcrafted feature registry,
//! and the steer-only separation. No reuse of Rung2 weights — the baseline is supplied by the caller.

use crate::{Color, Piece, Position};

pub const RESIDUAL_HYBRID_MODEL_VERSION: u32 = 1;
/// Fixed-point denominator for the lambda multipliers, so the eval math stays integer/deterministic.
pub const LAMBDA_SCALE: i32 = 256;

/// Whether a configuration is shippable or exists only to steer position generation (diversity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    /// Eligible for promotion (subject to #5 SPRT).
    Shippable,
    /// Diversity-only: exaggerated weights to generate varied positions; NEVER promoted/packaged.
    SteerOnly,
}

/// A declared handcrafted feature: a deterministic, white-POV integer signal the residual may weight.
/// The registry is versioned via RESIDUAL_HYBRID_MODEL_VERSION; weight vectors index it by position.
pub struct HandcraftedFeature {
    pub name: &'static str,
    pub eval_white: fn(&Position) -> i32,
}

/// White minus black bishop-pair indicator — one declared, obviously-correct handcrafted signal.
fn bishop_pair_balance(pos: &Position) -> i32 {
    let has_pair = |c: Color| (pos.pieces[c.index()][Piece::Bishop.index()].count_ones() >= 2) as i32;
    has_pair(Color::White) - has_pair(Color::Black)
}

/// The declared handcrafted feature registry (extensible). Weight vectors align to this order.
pub const HANDCRAFTED_FEATURES: &[HandcraftedFeature] = &[HandcraftedFeature {
    name: "bishop_pair_balance",
    eval_white: bishop_pair_balance,
}];

/// White-POV handcrafted delta = sum(feature_i(pos) * weights[i]). Missing weights count as 0.
pub fn handcrafted_delta_white(pos: &Position, weights: &[i32]) -> i32 {
    HANDCRAFTED_FEATURES
        .iter()
        .enumerate()
        .map(|(i, f)| (f.eval_white)(pos) * weights.get(i).copied().unwrap_or(0))
        .sum()
}

/// The optional learned residual (stub for slice 1: no trained model yet -> 0, white POV).
fn learned_residual_white(_pos: &Position) -> i32 {
    0
}

/// A residual-hybrid configuration: the frozen baseline it sits on, the two independent lambdas, the
/// handcrafted weight vector, and an optional learned-residual model hash (a minimal manifest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidualHybridConfig {
    pub model_version: u32,
    pub baseline_hash: String,
    pub kind: ProfileKind,
    pub lambda_h: i32,
    pub lambda_r: i32,
    pub handcrafted_weights: Vec<i32>,
    pub residual_hash: Option<String>,
}

impl ResidualHybridConfig {
    /// A zeroed shippable config over a named frozen baseline: both lambdas 0 -> recovers baseline.
    pub fn zeroed(baseline_hash: impl Into<String>) -> Self {
        Self {
            model_version: RESIDUAL_HYBRID_MODEL_VERSION,
            baseline_hash: baseline_hash.into(),
            kind: ProfileKind::Shippable,
            lambda_h: 0,
            lambda_r: 0,
            handcrafted_weights: vec![0; HANDCRAFTED_FEATURES.len()],
            residual_hash: None,
        }
    }

    /// True when both components are off (the zero-weight fast path applies).
    pub fn is_zeroed(&self) -> bool {
        self.lambda_h == 0 && self.lambda_r == 0
    }
}

/// Telemetry for one residual-hybrid evaluation (feature cost + per-branch contributions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResidualTelemetry {
    /// Whether the handcrafted/residual features were extracted (false on the zero-weight fast path).
    pub features_extracted: bool,
    pub handcrafted_delta_cp: i32,
    pub residual_delta_cp: i32,
}

pub struct ResidualEval {
    pub score_cp: i32,
    pub telemetry: ResidualTelemetry,
}

/// Combine a FROZEN baseline score with the additive residual components for `pos` (stm POV, matching
/// `eval::evaluate`). ZERO-WEIGHT FAST PATH: when both lambdas are zero the handcrafted/residual
/// features are NOT extracted and `baseline_score` is returned EXACTLY (byte-identical recovery).
pub fn residual_hybrid_eval(
    baseline_score: i32,
    pos: &Position,
    cfg: &ResidualHybridConfig,
) -> ResidualEval {
    if cfg.is_zeroed() {
        return ResidualEval {
            score_cp: baseline_score,
            telemetry: ResidualTelemetry::default(), // features_extracted: false
        };
    }
    // White-POV deltas -> stm POV (negate when black to move) so they add to the stm-POV baseline.
    let to_stm = |white: i32| if pos.stm == Color::White { white } else { -white };
    let h_white = if cfg.lambda_h != 0 {
        handcrafted_delta_white(pos, &cfg.handcrafted_weights)
    } else {
        0
    };
    let r_white = if cfg.lambda_r != 0 {
        learned_residual_white(pos)
    } else {
        0
    };
    // i64 intermediate so a future multi-feature registry with large weights cannot overflow i32;
    // identical to plain i32 math in the normal centipawn range (same truncation toward zero).
    let h_cp = (cfg.lambda_h as i64 * to_stm(h_white) as i64 / LAMBDA_SCALE as i64) as i32;
    let r_cp = (cfg.lambda_r as i64 * to_stm(r_white) as i64 / LAMBDA_SCALE as i64) as i32;
    ResidualEval {
        score_cp: baseline_score + h_cp + r_cp,
        telemetry: ResidualTelemetry {
            features_extracted: true,
            handcrafted_delta_cp: h_cp,
            residual_delta_cp: r_cp,
        },
    }
}

/// Promotion guard: a steer-only profile may NEVER be promoted or packaged as a champion. Returns
/// Err with a reason for a steer-only config (mirrors #5's promotion tooling refusing it).
pub fn refuse_steer_only_for_promotion(cfg: &ResidualHybridConfig) -> Result<(), String> {
    match cfg.kind {
        ProfileKind::Shippable => Ok(()),
        ProfileKind::SteerOnly => Err(format!(
            "steer-only profile (baseline {}) is diversity-only and refused by promotion/packaging",
            cfg.baseline_hash
        )),
    }
}
