use crate::facts::pawn_structure::pawn_structure_facts;
use crate::facts::piece_safety::piece_facts;
use crate::facts::types::{FactCollection, PositionFacts, Side};
use crate::{Color, Position};

pub fn position_facts(pos: &Position) -> PositionFacts {
    PositionFacts {
        side_to_move: side(pos.stm),
        pieces: piece_facts(pos),
        pawn_structure: pawn_structure_facts(pos),
        king_safety: FactCollection::uncomputed("not_in_milestone_1"),
        available_captures: FactCollection::uncomputed("not_in_milestone_1"),
        // Motifs are gated by options.includeMotifOpportunities and layered on in
        // move_bundle when requested; the bare position view leaves them unasked.
        available_motifs: FactCollection::uncomputed("not_requested"),
    }
}

pub fn side(color: Color) -> Side {
    match color {
        Color::White => Side::White,
        Color::Black => Side::Black,
    }
}

pub fn square_name(square: u8) -> String {
    let file = (b'a' + square % 8) as char;
    let rank = (b'1' + square / 8) as char;
    format!("{file}{rank}")
}
