//! Attack generation. Leaper attacks (pawn/knight/king) are precomputed tables;
//! sliding attacks (bishop/rook/queen) use the classical ray method with a blocker
//! bitscan (no magics yet — correctness first, decent speed).
use crate::{Color, Piece, Position};
use std::sync::OnceLock;

// Direction order: 0=N 1=S 2=E 3=W 4=NE 5=NW 6=SE 7=SW. (file_delta, rank_delta)
const DIRS: [(i8, i8); 8] = [
    (0, 1),
    (0, -1),
    (1, 0),
    (-1, 0),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];
// Whether a direction increases the square index (use lsb for the nearest blocker).
const POSITIVE: [bool; 8] = [true, false, true, false, true, true, false, false];

struct Tables {
    knight: [u64; 64],
    king: [u64; 64],
    pawn: [[u64; 64]; 2], // [color][square]
    rays: [[u64; 64]; 8], // [dir][square]
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn build_tables() -> Tables {
    let mut knight = [0u64; 64];
    let mut king = [0u64; 64];
    let mut pawn = [[0u64; 64]; 2];
    let mut rays = [[0u64; 64]; 8];

    let on = |f: i32, r: i32| f >= 0 && f < 8 && r >= 0 && r < 8;
    let bit = |f: i32, r: i32| 1u64 << (r * 8 + f);

    for sq in 0..64i32 {
        let f = sq % 8;
        let r = sq / 8;

        // Knight.
        for (df, dr) in [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)] {
            if on(f + df, r + dr) {
                knight[sq as usize] |= bit(f + df, r + dr);
            }
        }
        // King.
        for df in -1..=1 {
            for dr in -1..=1 {
                if (df != 0 || dr != 0) && on(f + df, r + dr) {
                    king[sq as usize] |= bit(f + df, r + dr);
                }
            }
        }
        // Pawn captures.
        if on(f - 1, r + 1) {
            pawn[Color::White.index()][sq as usize] |= bit(f - 1, r + 1);
        }
        if on(f + 1, r + 1) {
            pawn[Color::White.index()][sq as usize] |= bit(f + 1, r + 1);
        }
        if on(f - 1, r - 1) {
            pawn[Color::Black.index()][sq as usize] |= bit(f - 1, r - 1);
        }
        if on(f + 1, r - 1) {
            pawn[Color::Black.index()][sq as usize] |= bit(f + 1, r - 1);
        }
        // Rays.
        for (d, (df, dr)) in DIRS.iter().enumerate() {
            let (mut cf, mut cr) = (f + *df as i32, r + *dr as i32);
            while on(cf, cr) {
                rays[d][sq as usize] |= bit(cf, cr);
                cf += *df as i32;
                cr += *dr as i32;
            }
        }
    }

    Tables { knight, king, pawn, rays }
}

#[inline]
pub fn knight_attacks(sq: u8) -> u64 {
    tables().knight[sq as usize]
}
#[inline]
pub fn king_attacks(sq: u8) -> u64 {
    tables().king[sq as usize]
}
#[inline]
pub fn pawn_attacks(color: Color, sq: u8) -> u64 {
    tables().pawn[color.index()][sq as usize]
}

#[inline]
fn ray(dir: usize, sq: u8, occ: u64) -> u64 {
    let t = tables();
    let mut attack = t.rays[dir][sq as usize];
    let blockers = attack & occ;
    if blockers != 0 {
        let blocker_sq = if POSITIVE[dir] {
            blockers.trailing_zeros()
        } else {
            63 - blockers.leading_zeros()
        };
        attack ^= t.rays[dir][blocker_sq as usize];
    }
    attack
}

#[inline]
pub fn bishop_attacks(sq: u8, occ: u64) -> u64 {
    ray(4, sq, occ) | ray(5, sq, occ) | ray(6, sq, occ) | ray(7, sq, occ)
}
#[inline]
pub fn rook_attacks(sq: u8, occ: u64) -> u64 {
    ray(0, sq, occ) | ray(1, sq, occ) | ray(2, sq, occ) | ray(3, sq, occ)
}
#[inline]
pub fn queen_attacks(sq: u8, occ: u64) -> u64 {
    bishop_attacks(sq, occ) | rook_attacks(sq, occ)
}

/// Bitboard of all `by`-colored pieces attacking `sq`, given occupancy `occ` and a
/// piece-set `pieces` (the live board OR a SEE scratch set). Sliders re-resolve
/// through `occ`, so removing a piece from `occ` reveals x-ray attackers behind it.
pub fn attackers_of(pieces: &[[u64; 6]; 2], sq: u8, by: Color, occ: u64) -> u64 {
    let bi = by.index();
    // Pawns: sq is hit by a `by` pawn iff such a pawn sits on a square in the
    // OPPOSITE-color pawn-attack set from sq.
    let mut a = pawn_attacks(by.flip(), sq) & pieces[bi][Piece::Pawn.index()];
    a |= knight_attacks(sq) & pieces[bi][Piece::Knight.index()];
    a |= king_attacks(sq) & pieces[bi][Piece::King.index()];
    a |= bishop_attacks(sq, occ) & (pieces[bi][Piece::Bishop.index()] | pieces[bi][Piece::Queen.index()]);
    a |= rook_attacks(sq, occ) & (pieces[bi][Piece::Rook.index()] | pieces[bi][Piece::Queen.index()]);
    a
}

/// Bitboard of `by`-colored pieces attacking `sq` on the live board.
#[inline]
pub fn attackers_to_color(pos: &Position, sq: u8, by: Color, occ: u64) -> u64 {
    attackers_of(&pos.pieces, sq, by, occ)
}

/// Is `sq` attacked by any piece of color `by`, given occupancy `occ`?
#[inline]
pub fn is_square_attacked(pos: &Position, sq: u8, by: Color, occ: u64) -> bool {
    attackers_of(&pos.pieces, sq, by, occ) != 0
}
