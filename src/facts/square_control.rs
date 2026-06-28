use crate::attacks::attackers_of;
use crate::facts::position::{piece_ref_for_square, position_for_analysis_side, square_name};
use crate::facts::types::{FactCollection, PieceRef, SquareFact};
use crate::movegen::generate_legal_list;
use crate::{Color, Position};

/// Deterministic, read-only control facts for every square on the board.
///
/// For each of the 64 squares (ascending index) this reports:
/// - geometric attackers per side (control = at least one attacker), and
/// - the pieces that can *legally* move onto the square, per side.
///
/// Legal movers for the opposite side reuse `position_for_analysis_side`, so the
/// collection is `Unavailable` whenever the actual side to move is in check and
/// excludes en-passant for the non-moving side (its ep square is cleared).
pub fn square_control_facts(pos: &Position) -> FactCollection<SquareFact> {
    let stm = pos.stm;
    let opposite = stm.flip();

    // Legal movers, computed once per side, bucketed by destination square.
    let stm_movers = legal_movers_by_destination(pos, stm);
    let opposite_movers = match position_for_analysis_side(pos, opposite) {
        Ok(probe) => Ok(legal_movers_by_destination(&probe, opposite)),
        Err(reason) => Err(reason),
    };

    let mut facts = Vec::with_capacity(64);
    for sq in 0u8..64 {
        let mut attacked_by_white =
            refs_from_bitboard(pos, attackers_of(&pos.pieces, sq, Color::White, pos.all));
        attacked_by_white.sort_by(|a, b| a.id.cmp(&b.id));
        let mut attacked_by_black =
            refs_from_bitboard(pos, attackers_of(&pos.pieces, sq, Color::Black, pos.all));
        attacked_by_black.sort_by(|a, b| a.id.cmp(&b.id));

        let movers_for = |color: Color| -> FactCollection<PieceRef> {
            if color == stm {
                FactCollection::computed(movers_to(&stm_movers, sq))
            } else {
                match &opposite_movers {
                    Ok(buckets) => FactCollection::computed(movers_to(buckets, sq)),
                    Err(reason) => FactCollection::unavailable(*reason),
                }
            }
        };

        facts.push(SquareFact {
            square: square_name(sq),
            occupied: pos.piece_at(sq).is_some(),
            controlled_by_white: !attacked_by_white.is_empty(),
            controlled_by_black: !attacked_by_black.is_empty(),
            attacked_by_white,
            attacked_by_black,
            legal_movers_white: movers_for(Color::White),
            legal_movers_black: movers_for(Color::Black),
        });
    }

    FactCollection::computed(facts)
}

/// 64-slot table of the `from`-square refs that can legally reach each square.
fn legal_movers_by_destination(pos: &Position, color: Color) -> [Vec<PieceRef>; 64] {
    debug_assert_eq!(
        pos.stm, color,
        "movers must be generated for the side to move"
    );
    let mut clone = pos.clone();
    let list = generate_legal_list(&mut clone);
    let mut buckets: [Vec<PieceRef>; 64] = std::array::from_fn(|_| Vec::new());
    for mv in list.as_slice() {
        if let Some(piece_ref) = piece_ref_for_square(pos, mv.from) {
            buckets[mv.to as usize].push(piece_ref);
        }
    }
    buckets
}

fn movers_to(buckets: &[Vec<PieceRef>; 64], sq: u8) -> Vec<PieceRef> {
    let mut out = buckets[sq as usize].clone();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn refs_from_bitboard(pos: &Position, mut bb: u64) -> Vec<PieceRef> {
    let mut out = Vec::new();
    while bb != 0 {
        let square = bb.trailing_zeros() as u8;
        bb &= bb - 1;
        if let Some(piece_ref) = piece_ref_for_square(pos, square) {
            out.push(piece_ref);
        }
    }
    out
}
