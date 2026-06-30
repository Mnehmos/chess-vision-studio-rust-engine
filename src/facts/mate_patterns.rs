//! Mate-pattern classification (teaching facts).
//!
//! A POST-mate geometric classifier: for each checkmating move (already proven mate by
//! `mating_moves`), name the pattern (back-rank, smothered, …). The mate itself is
//! never re-verified here — the only obligation is classification CORRECTNESS, so the
//! rule is precision over recall: an unrecognized mate returns None rather than a guess.
//! First batch: back-rank + smothered (rook/queen-along-the-rank vs all-smothered
//! knight — zero overlap). Later batches add more patterns to `classify`.

use crate::attacks::{bishop_attacks, king_attacks, pawn_attacks};
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

    // Most-specific first. Smothered (knight) is disjoint from the major-piece patterns;
    // epaulette (both flanks own) and Damiano (pawn-defended adjacent queen) are stricter
    // than the general back-rank fallback, so they precede it.
    let (kind, mut key) = if let Some(key) = smothered(&after, mated, ksq, to, mating_piece) {
        ("smothered_mate", key)
    } else if let Some(key) = epaulette(&after, mated, ksq, to, mating_piece) {
        ("epaulette_mate", key)
    } else if let Some(key) = damiano(&after, mater, ksq, to, mating_piece) {
        ("damiano_mate", key)
    } else if let Some(key) = boden(&after, mater, mated, ksq, to, mating_piece) {
        ("boden_mate", key)
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

/// Epaulette mate: a rook/queen checks along the king's file or rank, and the king's
/// two flanks PERPENDICULAR to the check (the "epaulettes") are BOTH its own pieces,
/// sandwiching it. Both flanks must be on-board (king not on the relevant edge).
fn epaulette(after: &Position, mated: Color, ksq: u8, to: u8, mp: Piece) -> Option<Vec<u8>> {
    if mp != Piece::Rook && mp != Piece::Queen {
        return None;
    }
    let kf = file_of(ksq) as i8;
    let kr = rank_of(ksq) as i8;
    let flanks: [(i8, i8); 2] = if file_of(to) == file_of(ksq) {
        [(kf - 1, kr), (kf + 1, kr)] // file check -> same-rank flanks
    } else if rank_of(to) == rank_of(ksq) {
        [(kf, kr - 1), (kf, kr + 1)] // rank check -> same-file flanks
    } else {
        return None; // a diagonal queen check is not an epaulette
    };
    let own = color_occ(after, mated);
    let mut key = vec![ksq, to];
    for (f, r) in flanks {
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            return None; // king on the edge — not sandwiched between two epaulettes
        }
        let s = (r * 8 + f) as u8;
        if own & (1u64 << s) == 0 {
            return None; // a flank is not an own piece
        }
        key.push(s);
    }
    Some(key)
}

/// Damiano's mate: a queen mates from a square ADJACENT to the king, defended by a
/// friendly PAWN (the pawn-supported adjacent queen).
fn damiano(after: &Position, mater: Color, ksq: u8, to: u8, mp: Piece) -> Option<Vec<u8>> {
    if mp != Piece::Queen {
        return None;
    }
    if king_attacks(ksq) & (1u64 << to) == 0 {
        return None; // queen not adjacent to the king
    }
    let to_bit = 1u64 << to;
    let mut pawns = after.pieces[mater.index()][Piece::Pawn.index()];
    while pawns != 0 {
        let psq = pawns.trailing_zeros() as u8;
        pawns &= pawns - 1;
        if pawn_attacks(mater, psq) & to_bit != 0 {
            return Some(vec![ksq, to, psq]); // a friendly pawn defends the mating queen
        }
    }
    None
}

/// Boden's mate: two bishops on intersecting diagonals mate the king, every empty
/// flight square covered between them and every occupied neighbour an own self-block.
/// The mating bishop checks along one diagonal; a SECOND friendly bishop covers — on the
/// CROSSING diagonal — at least one empty escape the mater cannot reach, and the two
/// bishops together must cover EVERY empty escape (precision over recall: a bishop mate
/// merely accompanied by an idle second bishop is not Boden's).
fn boden(after: &Position, mater: Color, mated: Color, ksq: u8, to: u8, mp: Piece) -> Option<Vec<u8>> {
    if mp != Piece::Bishop {
        return None;
    }
    let neighbours = king_attacks(ksq);
    let own = color_occ(after, mated);
    // Every occupied king-neighbour must be one of the king's OWN pieces; an enemy piece
    // beside the king is a different mating picture, not the self-boxed Boden king.
    if neighbours & after.all & !own != 0 {
        return None;
    }
    let empty_escapes = neighbours & !after.all;
    // The mating bishop's own coverage (it delivers the check along one diagonal).
    let mating_cover = bishop_attacks(to, after.all);
    // A SECOND friendly bishop on the crossing diagonal that (1) together with the mater
    // covers every empty flight, and (2) covers an empty flight the mater itself cannot
    // — the essential criss-cross that defines Boden's.
    let mut bishops = after.pieces[mater.index()][Piece::Bishop.index()] & !(1u64 << to);
    while bishops != 0 {
        let b = bishops.trailing_zeros() as u8;
        bishops &= bishops - 1;
        let second_cover = bishop_attacks(b, after.all);
        if empty_escapes & !(mating_cover | second_cover) != 0 {
            continue; // a flight square neither bishop covers — not jointly boxed
        }
        if second_cover & empty_escapes & !mating_cover == 0 {
            continue; // the second bishop adds no new flight coverage — not the criss-cross
        }
        return Some(vec![ksq, to, b]);
    }
    None
}
