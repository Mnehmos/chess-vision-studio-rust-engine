//! Value evaluation — Rust port of the legacy TS reference (`src/value/valueEngine.ts`),
//! parameterized by the trainable `ValueWeights` (+ optional `Rung2Weights`).
//! Composition is identical to the TS `evaluateWhiteFloat`:
//!   terminal short-circuits → tapered material+PST per piece → bishop pair →
//!   tempo → optional Rung-2 contribution. With default weights this reproduces
//!   the handcrafted eval; with the trained mixed weights it reproduces the gated
//!   Rung-2 head. Parity target: TS float within tolerance on a curated FEN suite.
pub mod cvs_features;
pub mod nnue;
pub mod pst;
pub mod rung2;
pub mod weights;

pub use cvs_features::{extract_cvs_features, registry_hash, CvsActiveFeatures, CVS_INPUT_DIM};
pub use nnue::{Accumulator, Nnue};
pub use rung2::{extract_rung2, rung2_contribution, Rung2Features};
pub use weights::{Rung2Weights, ValueWeights};

use crate::movegen::{generate_legal, in_check};
use crate::see::SEE_VALUE;
use crate::{Color, Piece, Position};

/// Phase weight per piece type (pawn..king) — matches TS PHASE_VALUE.
pub const PHASE_VALUE: [i32; 6] = [0, 1, 1, 2, 4, 0];
/// Full non-pawn army both sides = 24 — matches TS MAX_PHASE.
pub const MAX_PHASE: i32 = 24;
/// Mate score — matches TS MATE_SCORE.
pub const MATE_SCORE: f64 = 1_000_000.0;

/// Non-pawn material on the board (0..24), used to taper mg/eg.
pub fn phase_units(pos: &Position) -> i32 {
    let mut units = 0;
    for ci in 0..2 {
        for p in 0..6 {
            units += pos.pieces[ci][p].count_ones() as i32 * PHASE_VALUE[p];
        }
    }
    units.min(MAX_PHASE)
}

/// chess.js `isInsufficientMaterial` semantics: K vs K; K+minor vs K; or kings +
/// bishops only with every bishop on the same square color.
pub fn insufficient_material(pos: &Position) -> bool {
    let total = pos.all.count_ones();
    if total == 2 {
        return true;
    }
    let knights = pos.pieces[0][Piece::Knight.index()] | pos.pieces[1][Piece::Knight.index()];
    let bishops = pos.pieces[0][Piece::Bishop.index()] | pos.pieces[1][Piece::Bishop.index()];
    if total == 3 && (knights.count_ones() == 1 || bishops.count_ones() == 1) {
        return true;
    }
    if total == bishops.count_ones() + 2 {
        // Kings + bishops only: insufficient iff all bishops share a square color.
        const LIGHT: u64 = 0x55AA55AA55AA55AA; // a1 dark; light squares mask
        let on_light = (bishops & LIGHT).count_ones();
        return on_light == 0 || on_light == bishops.count_ones();
    }
    false
}

/// Pre-round White-POV float — the parity target against the TS `evaluateWhiteFloat`.
pub fn evaluate_white_float(
    pos: &mut Position,
    w: &ValueWeights,
    r2: Option<&Rung2Weights>,
) -> f64 {
    // Terminal short-circuits, in the same order as the TS reference.
    let no_legal = generate_legal(pos).is_empty();
    if no_legal {
        if in_check(pos) {
            // Side to move is mated => bad for them.
            return if pos.stm == Color::White {
                -MATE_SCORE
            } else {
                MATE_SCORE
            };
        }
        return 0.0; // stalemate
    }
    if pos.halfmove >= 100 || insufficient_material(pos) {
        return 0.0;
    }
    evaluate_white_float_nonterminal(pos, w, r2)
}

/// The weighted-term sum WITHOUT terminal short-circuits — for search leaves that
/// already know the position has legal moves (avoids a second movegen per leaf).
/// Callers must handle mate/stalemate/draw themselves (the search does, mirroring
/// the TS searcher's own terminal handling).
pub fn evaluate_white_float_nonterminal(
    pos: &Position,
    w: &ValueWeights,
    r2: Option<&Rung2Weights>,
) -> f64 {
    let units = phase_units(pos);
    let mg_w = units as f64 / MAX_PHASE as f64;
    let eg_w = 1.0 - mg_w;

    let mut score = 0.0f64;
    for (ci, color, sign) in [
        (0usize, Color::White, 1.0f64),
        (1usize, Color::Black, -1.0f64),
    ] {
        for p in Piece::ALL {
            let mat_mul = match p {
                Piece::Pawn => w.material.p,
                Piece::Knight => w.material.n,
                Piece::Bishop => w.material.b,
                Piece::Rook => w.material.r,
                Piece::Queen => w.material.q,
                Piece::King => 1.0, // king material fixed, like the TS reference
            };
            let mut bb = pos.pieces[ci][p.index()];
            while bb != 0 {
                let sq = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let material = mat_mul * SEE_VALUE[p.index()] as f64;
                let mg = pst::pst_mg(p, color, sq) as f64;
                let eg = pst::pst_eg(p, color, sq) as f64;
                let positional = w.pst_scale * (mg * mg_w + eg * eg_w);
                score += sign * (material + positional);
            }
        }
    }

    let wb = pos.pieces[0][Piece::Bishop.index()].count_ones();
    let bb = pos.pieces[1][Piece::Bishop.index()].count_ones();
    if wb >= 2 {
        score += w.bishop_pair;
    }
    if bb >= 2 {
        score -= w.bishop_pair;
    }

    score += if pos.stm == Color::White {
        w.tempo
    } else {
        -w.tempo
    };

    if let Some(r2w) = r2 {
        score += rung2_contribution(pos, r2w);
    }

    score
}

/// JS `Math.round` (half toward +infinity), for byte-parity with the TS `evaluateWhite`.
#[inline]
pub fn js_round(x: f64) -> i32 {
    (x + 0.5).floor() as i32
}

/// Static evaluation in centipawns from White's perspective.
pub fn evaluate_white(pos: &mut Position, w: &ValueWeights, r2: Option<&Rung2Weights>) -> i32 {
    js_round(evaluate_white_float(pos, w, r2))
}

/// Static evaluation from the side-to-move's perspective (negamax convention).
pub fn evaluate(pos: &mut Position, w: &ValueWeights, r2: Option<&Rung2Weights>) -> i32 {
    let white = evaluate_white(pos, w, r2);
    if pos.stm == Color::White {
        white
    } else {
        -white
    }
}
