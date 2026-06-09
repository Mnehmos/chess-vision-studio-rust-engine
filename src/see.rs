//! Static Exchange Evaluation (bitboard). Mirrors the legacy TS `see` semantics —
//! occupant value, swap list, x-ray reveal by re-querying sliders through the
//! shrinking occupancy, "a king may not recapture into defense", and minimax of
//! the swap list — so the trained Rung-2 hazard features that depend on SEE stay
//! parity-comparable with the reference engine.
use crate::attacks::attackers_of;
use crate::{Color, Piece, Position};

/// Centipawn piece values. MUST match the TS `PIECE_VALUE` for eval parity.
pub const SEE_VALUE: [i32; 6] = [100, 320, 330, 500, 900, 20000];

fn least_valuable(pieces: &[[u64; 6]; 2], side: Color, attackers: u64) -> Option<(u8, usize)> {
    let s = side.index();
    for p in 0..6 {
        // Piece indices are in ascending value order (pawn..king).
        let bb = pieces[s][p] & attackers;
        if bb != 0 {
            return Some((bb.trailing_zeros() as u8, p));
        }
    }
    None
}

/// SEE for `from -> to`, centipawns from the side-to-move's perspective.
/// Positive ⇒ the mover comes out ahead in the capture sequence on `to`.
pub fn see(pos: &Position, from: u8, to: u8) -> i32 {
    let us = pos.stm;
    let moving = match pos.piece_at(from) {
        Some((c, p)) if c == us => p,
        _ => return 0,
    };
    let mut pieces = pos.pieces; // working copy of the piece sets
    let mut occ = pos.all;
    let mut gain = [0i32; 32];

    // Value captured on `to` (0 if empty — quiet move or en-passant target square).
    gain[0] = match pos.piece_at(to) {
        Some((c, p)) => {
            pieces[c.index()][p.index()] &= !(1u64 << to);
            SEE_VALUE[p.index()]
        }
        None => 0,
    };

    // The mover leaves `from` and now stands on `to`.
    pieces[us.index()][moving.index()] &= !(1u64 << from);
    occ &= !(1u64 << from);
    occ |= 1u64 << to; // a piece always occupies `to` during the exchange
    let mut standing = SEE_VALUE[moving.index()];
    let mut side = us.flip();
    let mut d = 0usize;

    loop {
        let attackers = attackers_of(&pieces, to, side, occ);
        let (lva_sq, lva_p) = match least_valuable(&pieces, side, attackers) {
            Some(x) => x,
            None => break,
        };
        // A king may not recapture if the other side still defends `to`.
        if lva_p == Piece::King.index() && attackers_of(&pieces, to, side.flip(), occ) != 0 {
            break;
        }
        d += 1;
        gain[d] = standing - gain[d - 1];
        standing = SEE_VALUE[lva_p];
        pieces[side.index()][lva_p] &= !(1u64 << lva_sq);
        occ &= !(1u64 << lva_sq);
        side = side.flip();
        if d >= 31 {
            break;
        }
    }

    // Minimax: each side stops capturing once it stops gaining.
    while d > 0 {
        gain[d - 1] = -((-gain[d - 1]).max(gain[d]));
        d -= 1;
    }
    gain[0]
}

/// MVV-LVA ordering score for a capture (heavy on victim value, light on attacker).
#[inline]
pub fn mvv_lva(victim: Piece, attacker: Piece) -> i32 {
    SEE_VALUE[victim.index()] * 16 - SEE_VALUE[attacker.index()]
}
