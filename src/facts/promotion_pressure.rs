//! Promotion-pressure specialist (#2, v1 — geometric slice).
//!
//! For each passed pawn, emit the deterministic constraints it creates around promotion: its
//! promotion distance + square, whether it can advance now (the stop square is empty), and the
//! DEFENDER's committed resources — the pieces that attack the pawn or control its stop / promotion
//! squares (the "promotion shadow"). CHESS FACTS ONLY — no prose, no "winning". Pure + geometric
//! (read-only, no search): `must_address_now` is a geometric proxy (one square from promotion with
//! an empty, uncontested push). Search-verified must-address, mobility deltas, blockader-exchange
//! status, and tactical-followup sequences are deferred to later slices.

use crate::attacks::attackers_of;
use crate::facts::position::{piece_ref_for_square, side, square_name};
use crate::facts::types::{PieceRef, Side};
use crate::{file_of, rank_of, Color, Piece, Position};

pub const PROMOTION_PRESSURE_SCHEMA_VERSION: u32 = 1;

/// Confidence in the promotion-pressure facts. v1 is purely geometric (no search): the counts are
/// exact, and `must_address_now` is a geometric proxy rather than a search-verified verdict.
///
/// Geometric-v1 limitations (intentional): pins and legality are NOT considered — a pinned defender
/// still counts as a controller/attacker. And `must_address_now` treats any defender that attacks
/// the pawn — including a rook directly behind it on an open file — as "addressable" (the pawn can
/// be captured), so a behind-the-pawn attacker correctly never yields a must-address-true verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromotionConfidence {
    Geometric,
}

/// Deterministic promotion-pressure facts for one passed pawn (#2, v1).
#[derive(Clone, Debug, PartialEq)]
pub struct PromotionPressureFactV1 {
    pub pawn_square: String,
    pub color: Side,
    pub file: u8,
    pub rank: u8,
    /// Ranks from promotion (1 = promotes on the next push).
    pub promotion_distance: u8,
    /// The straight promotion square on the pawn's file.
    pub promotion_square: String,
    /// The square directly in front of the pawn (its blockade / stop square).
    pub stop_square: String,
    /// The stop square is unoccupied (the push is geometrically available).
    pub push_square_empty: bool,
    /// Geometric: equals `push_square_empty` (a search-verified self-check test is deferred).
    pub can_advance_now: bool,
    /// Defender pieces that attack the pawn's square (can capture it).
    pub pawn_attackers: Vec<PieceRef>,
    /// Defender pieces controlling the stop square (blockade resource).
    pub stop_square_controllers: Vec<PieceRef>,
    /// Defender pieces controlling the promotion square.
    pub promotion_square_controllers: Vec<PieceRef>,
    /// Distinct defender pieces committed to any of the above — the promotion shadow (v1).
    pub committed_defenders: Vec<PieceRef>,
    /// Count of distinct committed defenders (a deterministic measure of defensive resources).
    pub committed_defender_count: u16,
    /// Geometric proxy: one square from promoting, the push is empty, and no defender controls the
    /// promotion square or attacks the pawn — so it promotes next push unless the defender acts.
    pub must_address_now: bool,
    pub confidence: PromotionConfidence,
    pub schema_version: u32,
}

/// Emit promotion-pressure facts for every passed pawn of both colors (White's pawns first, then
/// Black's, each ascending by square). Read-only; no search.
pub fn promotion_pressure_facts(pos: &Position) -> Vec<PromotionPressureFactV1> {
    let mut out = Vec::new();
    for color in [Color::White, Color::Black] {
        let defender = color.flip();
        let pawns = bits(pos.pieces[color.index()][Piece::Pawn.index()]);
        let enemy_pawns = bits(pos.pieces[defender.index()][Piece::Pawn.index()]);
        for sq in pawns {
            if !is_passed(color, sq, &enemy_pawns) {
                continue;
            }
            let rank = rank_of(sq);
            let file = file_of(sq);
            let promotion_distance = match color {
                Color::White => 7 - rank,
                Color::Black => rank,
            };
            if promotion_distance == 0 {
                continue; // a pawn cannot legally occupy its own promotion rank
            }
            let stop_sq = match color {
                Color::White => sq + 8,
                Color::Black => sq - 8,
            };
            let promo_rank: u8 = if color == Color::White { 7 } else { 0 };
            let promo_sq = promo_rank * 8 + file;

            let push_square_empty = (pos.all >> stop_sq) & 1 == 0;

            let pawn_attackers = refs(pos, attackers_of(&pos.pieces, sq, defender, pos.all));
            let stop_square_controllers =
                refs(pos, attackers_of(&pos.pieces, stop_sq, defender, pos.all));
            let promotion_square_controllers =
                refs(pos, attackers_of(&pos.pieces, promo_sq, defender, pos.all));
            let committed_defenders = union_by_square(&[
                pawn_attackers.as_slice(),
                stop_square_controllers.as_slice(),
                promotion_square_controllers.as_slice(),
            ]);

            let must_address_now = promotion_distance == 1
                && push_square_empty
                && promotion_square_controllers.is_empty()
                && pawn_attackers.is_empty();

            out.push(PromotionPressureFactV1 {
                pawn_square: square_name(sq),
                color: side(color),
                file,
                rank,
                promotion_distance,
                promotion_square: square_name(promo_sq),
                stop_square: square_name(stop_sq),
                push_square_empty,
                can_advance_now: push_square_empty,
                pawn_attackers,
                stop_square_controllers,
                promotion_square_controllers,
                committed_defender_count: committed_defenders.len() as u16,
                committed_defenders,
                must_address_now,
                confidence: PromotionConfidence::Geometric,
                schema_version: PROMOTION_PRESSURE_SCHEMA_VERSION,
            });
        }
    }
    out
}

fn bits(mut bb: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while bb != 0 {
        out.push(bb.trailing_zeros() as u8);
        bb &= bb - 1;
    }
    out
}

// Identical rule to facts::pawn_structure::is_passed: no enemy pawn on the same or an adjacent file
// on any rank ahead of the pawn (toward its promotion).
fn is_passed(color: Color, square: u8, enemy_pawns: &[u8]) -> bool {
    let file = file_of(square) as i8;
    let rank = rank_of(square);
    !enemy_pawns.iter().any(|enemy| {
        let enemy_file = file_of(*enemy) as i8;
        let enemy_rank = rank_of(*enemy);
        (enemy_file - file).abs() <= 1
            && match color {
                Color::White => enemy_rank > rank,
                Color::Black => enemy_rank < rank,
            }
    })
}

// Pieces occupying the set bits of `bb` (every bit of an `attackers_of` result is a piece square).
fn refs(pos: &Position, mut bb: u64) -> Vec<PieceRef> {
    let mut out = Vec::new();
    while bb != 0 {
        let sq = bb.trailing_zeros() as u8;
        bb &= bb - 1;
        if let Some(r) = piece_ref_for_square(pos, sq) {
            out.push(r);
        }
    }
    out
}

// Union by board square (a piece can defend more than one of the stop/promotion/pawn squares).
fn union_by_square(groups: &[&[PieceRef]]) -> Vec<PieceRef> {
    let mut out: Vec<PieceRef> = Vec::new();
    for group in groups {
        for r in *group {
            if !out.iter().any(|e| e.square == r.square) {
                out.push(r.clone());
            }
        }
    }
    out.sort_by(|a, b| a.square.cmp(&b.square));
    out
}
