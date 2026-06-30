//! Mate-pattern classification (teaching facts).
//!
//! A POST-mate geometric classifier: for each checkmating move (already proven mate by
//! `mating_moves`), name the pattern (back-rank, smothered, …). The mate itself is
//! never re-verified here — the only obligation is classification CORRECTNESS, so the
//! rule is precision over recall: an unrecognized mate returns None rather than a guess.
//! First batch: back-rank + smothered (rook/queen-along-the-rank vs all-smothered
//! knight — zero overlap). Later batches add more patterns to `classify`.

use crate::attacks::king_attacks;
use crate::facts::hazards::mating_moves;
use crate::facts::piece_safety::piece_ref;
use crate::facts::position::{position_for_analysis_side, square_name};
use crate::facts::types::{FactCollection, MatePatternFact};
use crate::{file_of, rank_of, Color, Move, Piece, Position};

/// Validated named mate patterns for the side to move, sorted by mating move.
pub fn mate_pattern_opportunities(pos: &Position) -> FactCollection<MatePatternFact> {
    let mut out = Vec::new();
    for mv in mating_moves(pos) {
        if let Some(fact) = classify(pos, mv) {
            out.push(fact);
        }
    }
    out.sort_by(|a, b| a.move_uci.cmp(&b.move_uci));
    FactCollection::computed(out)
}

/// Named mate patterns from a requested side's perspective (the side delivering mate).
pub fn mate_pattern_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<MatePatternFact> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => mate_pattern_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

/// All `color`-colored pieces as a bitboard.
fn color_occ(pos: &Position, color: Color) -> u64 {
    let mut bb = 0u64;
    for piece in Piece::ALL {
        bb |= pos.pieces[color.index()][piece.index()];
    }
    bb
}

fn classify(pos: &Position, mv: Move) -> Option<MatePatternFact> {
    let mater = pos.stm;
    let mated = mater.flip();
    let mut after = pos.clone();
    after.make(mv);
    let ksq = after.king_sq(mated);
    let to = mv.to;
    let (_, mating_piece) = after.piece_at(to)?;

    // Most-specific first. The two first-batch patterns do not overlap (knight vs
    // rook/queen), so order is immaterial here; later batches insert ahead of these.
    let (kind, mut key) = if let Some(key) = smothered(&after, mated, ksq, to, mating_piece) {
        ("smothered_mate", key)
    } else if let Some(key) = back_rank(&after, mated, ksq, to, mating_piece) {
        ("back_rank_mate", key)
    } else {
        return None;
    };

    key.sort_unstable();
    key.dedup();
    let key_squares: Vec<String> = key.into_iter().map(square_name).collect();
    Some(MatePatternFact {
        kind: kind.to_string(),
        validator: "mate_pattern_validation".to_string(),
        move_uci: mv.to_uci(),
        mating_piece: piece_ref(mater, mating_piece, to),
        mated_king: piece_ref(mated, Piece::King, ksq),
        key_squares,
        gives_check: true,
    })
}

/// Smothered mate: a knight mates a king every one of whose on-board neighbours is
/// occupied by one of the king's OWN pieces.
fn smothered(after: &Position, mated: Color, ksq: u8, to: u8, mp: Piece) -> Option<Vec<u8>> {
    if mp != Piece::Knight {
        return None;
    }
    let own = color_occ(after, mated);
    let mut nb = king_attacks(ksq);
    let mut key = vec![ksq, to];
    while nb != 0 {
        let s = nb.trailing_zeros() as u8;
        nb &= nb - 1;
        if own & (1u64 << s) == 0 {
            return None; // a neighbour that is not an own piece — not fully smothered
        }
        key.push(s);
    }
    Some(key)
}

/// Back-rank mate: a rook/queen mates ALONG the king's back rank, the king's
/// toward-centre neighbours all blocked by its OWN pieces (the same-rank flanks are
/// occluded by the checking line, so the king cannot flee along the rank).
fn back_rank(after: &Position, mated: Color, ksq: u8, to: u8, mp: Piece) -> Option<Vec<u8>> {
    if mp != Piece::Rook && mp != Piece::Queen {
        return None;
    }
    let kr = rank_of(ksq);
    let back = if mated == Color::White { 0 } else { 7 };
    if kr != back || rank_of(to) != kr {
        return None; // king not on its back rank, or mater not delivering along it
    }
    let kf = file_of(ksq) as i8;
    let r = (kr as i8) + if mated == Color::White { 1 } else { -1 };
    let own = color_occ(after, mated);
    let mut key = vec![ksq, to];
    for df in [-1i8, 0, 1] {
        let f = kf + df;
        if !(0..8).contains(&f) {
            continue;
        }
        let s = (r * 8 + f) as u8;
        if own & (1u64 << s) == 0 {
            return None; // a toward-centre escape is not blocked by an own piece
        }
        key.push(s);
    }
    Some(key)
}
