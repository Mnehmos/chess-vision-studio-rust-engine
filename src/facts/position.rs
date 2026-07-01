use crate::attacks::attackers_of;
use crate::facts::pawn_structure::pawn_structure_facts;
use crate::facts::piece_safety::{capture_opportunities, piece_facts};
use crate::facts::types::{FactCollection, PositionFacts, Side};
use crate::{Color, Position};

pub fn position_facts(pos: &Position) -> PositionFacts {
    PositionFacts {
        side_to_move: side(pos.stm),
        pieces: piece_facts(pos),
        pawn_structure: pawn_structure_facts(pos),
        king_safety: crate::facts::piece_safety::king_safety_facts(pos),
        available_captures: capture_opportunities(pos),
        opponent_available_captures: match position_for_analysis_side(pos, pos.stm.flip()) {
            Ok(probe) => capture_opportunities(&probe),
            Err(reason) => FactCollection::unavailable(reason),
        },
        // Motifs and pins are gated by options.includeMotifOpportunities and layered
        // on in move_bundle when requested; the bare view leaves them unasked.
        available_motifs: FactCollection::uncomputed("not_requested"),
        available_pins: FactCollection::uncomputed("not_requested"),
        available_skewers: FactCollection::uncomputed("not_requested"),
        available_discoveries: FactCollection::uncomputed("not_requested"),
        available_remove_guard: FactCollection::uncomputed("not_requested"),
        available_trapped: FactCollection::uncomputed("not_requested"),
        available_mate_patterns: FactCollection::uncomputed("not_requested"),
        available_overload: FactCollection::uncomputed("not_requested"),
        opponent_available_motifs: FactCollection::uncomputed("not_requested"),
        opponent_available_pins: FactCollection::uncomputed("not_requested"),
        opponent_available_skewers: FactCollection::uncomputed("not_requested"),
        opponent_available_discoveries: FactCollection::uncomputed("not_requested"),
        opponent_available_remove_guard: FactCollection::uncomputed("not_requested"),
        opponent_available_trapped: FactCollection::uncomputed("not_requested"),
        opponent_available_mate_patterns: FactCollection::uncomputed("not_requested"),
        opponent_available_overload: FactCollection::uncomputed("not_requested"),
        hazards: FactCollection::uncomputed("motifs_not_requested"),
        square_facts: crate::facts::square_control::square_control_facts(pos),
    }
}

/// Build a legal-move probe for either color without changing the source board.
/// Opposite-side probes are unavailable when the actual side to move is in check:
/// granting the checking side another turn would create an illegal king-capture
/// state. En-passant is also cleared because it belongs only to the actual turn.
pub fn position_for_analysis_side(
    pos: &Position,
    requested: Color,
) -> Result<Position, &'static str> {
    let mut probe = pos.clone();
    if probe.stm != requested {
        let checked_king = probe.king_sq(probe.stm);
        if attackers_of(&probe.pieces, checked_king, requested, probe.all) != 0 {
            return Err("opposite_side_probe_while_in_check");
        }
        probe.stm = requested;
        probe.ep = None;
    }
    Ok(probe)
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

/// Build a [`crate::facts::types::PieceRef`] for whatever piece occupies `square`,
/// or `None` when the square is empty. Shared by the attack/control fact builders.
pub fn piece_ref_for_square(pos: &Position, square: u8) -> Option<crate::facts::types::PieceRef> {
    pos.piece_at(square)
        .map(|(color, piece)| crate::facts::piece_safety::piece_ref(color, piece, square))
}
