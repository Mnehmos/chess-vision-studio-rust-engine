//! Piece-square tables — exact port of the legacy TS reference (`src/value/pst.ts`,
//! classical "simplified evaluation" values, Tomasz Michniewski). Each table is 64
//! entries in *visual* order: index 0 = a8 … 7 = h8, 56 = a1 … 63 = h1.
//! `pst_mg`/`pst_eg` map a LERF square + color to the right entry, mirroring
//! vertically for Black — byte-identical to the TS `tableIndex` logic.
//! NOTE (parity): TABLES_EG reuses the MG tables for all non-king pieces, exactly
//! like the TS reference (only the king has a distinct EG table).
use crate::{file_of, rank_of, Color, Piece};

#[rustfmt::skip]
const PAWN_MG: [i32; 64] = [
   0,  0,  0,  0,  0,  0,  0,  0,
  50, 50, 50, 50, 50, 50, 50, 50,
  10, 10, 20, 30, 30, 20, 10, 10,
   5,  5, 10, 25, 25, 10,  5,  5,
   0,  0,  0, 20, 20,  0,  0,  0,
   5, -5,-10,  0,  0,-10, -5,  5,
   5, 10, 10,-20,-20, 10, 10,  5,
   0,  0,  0,  0,  0,  0,  0,  0,
];

#[rustfmt::skip]
const KNIGHT_MG: [i32; 64] = [
  -50,-40,-30,-30,-30,-30,-40,-50,
  -40,-20,  0,  0,  0,  0,-20,-40,
  -30,  0, 10, 15, 15, 10,  0,-30,
  -30,  5, 15, 20, 20, 15,  5,-30,
  -30,  0, 15, 20, 20, 15,  0,-30,
  -30,  5, 10, 15, 15, 10,  5,-30,
  -40,-20,  0,  5,  5,  0,-20,-40,
  -50,-40,-30,-30,-30,-30,-40,-50,
];

#[rustfmt::skip]
const BISHOP_MG: [i32; 64] = [
  -20,-10,-10,-10,-10,-10,-10,-20,
  -10,  0,  0,  0,  0,  0,  0,-10,
  -10,  0,  5, 10, 10,  5,  0,-10,
  -10,  5,  5, 10, 10,  5,  5,-10,
  -10,  0, 10, 10, 10, 10,  0,-10,
  -10, 10, 10, 10, 10, 10, 10,-10,
  -10,  5,  0,  0,  0,  0,  5,-10,
  -20,-10,-10,-10,-10,-10,-10,-20,
];

#[rustfmt::skip]
const ROOK_MG: [i32; 64] = [
   0,  0,  0,  0,  0,  0,  0,  0,
   5, 10, 10, 10, 10, 10, 10,  5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
   0,  0,  0,  5,  5,  0,  0,  0,
];

#[rustfmt::skip]
const QUEEN_MG: [i32; 64] = [
  -20,-10,-10, -5, -5,-10,-10,-20,
  -10,  0,  0,  0,  0,  0,  0,-10,
  -10,  0,  5,  5,  5,  5,  0,-10,
   -5,  0,  5,  5,  5,  5,  0, -5,
    0,  0,  5,  5,  5,  5,  0, -5,
  -10,  5,  5,  5,  5,  5,  0,-10,
  -10,  0,  5,  0,  0,  0,  0,-10,
  -20,-10,-10, -5, -5,-10,-10,-20,
];

#[rustfmt::skip]
const KING_MG: [i32; 64] = [
  -30,-40,-40,-50,-50,-40,-40,-30,
  -30,-40,-40,-50,-50,-40,-40,-30,
  -30,-40,-40,-50,-50,-40,-40,-30,
  -30,-40,-40,-50,-50,-40,-40,-30,
  -20,-30,-30,-40,-40,-30,-30,-20,
  -10,-20,-20,-20,-20,-20,-20,-10,
   20, 20,  0,  0,  0,  0, 20, 20,
   20, 30, 10,  0,  0, 10, 30, 20,
];

#[rustfmt::skip]
const KING_EG: [i32; 64] = [
  -50,-40,-30,-20,-20,-30,-40,-50,
  -30,-20,-10,  0,  0,-10,-20,-30,
  -30,-10, 20, 30, 30, 20,-10,-30,
  -30,-10, 30, 40, 40, 30,-10,-30,
  -30,-10, 30, 40, 40, 30,-10,-30,
  -30,-10, 20, 30, 30, 20,-10,-30,
  -30,-30,  0,  0,  0,  0,-30,-30,
  -50,-30,-30,-30,-30,-30,-30,-50,
];

/// Visual-order index (a8=0 .. h1=63) for a LERF square, mirrored for Black.
#[inline]
fn table_index(color: Color, sq: u8) -> usize {
    let f = file_of(sq) as usize;
    let r = rank_of(sq) as usize; // 0 = rank 1
    let row = if color == Color::White { 7 - r } else { r };
    row * 8 + f
}

#[inline]
fn mg_table(piece: Piece) -> &'static [i32; 64] {
    match piece {
        Piece::Pawn => &PAWN_MG,
        Piece::Knight => &KNIGHT_MG,
        Piece::Bishop => &BISHOP_MG,
        Piece::Rook => &ROOK_MG,
        Piece::Queen => &QUEEN_MG,
        Piece::King => &KING_MG,
    }
}

/// Middlegame PST bonus for a piece of `color` on LERF square `sq` (own perspective).
#[inline]
pub fn pst_mg(piece: Piece, color: Color, sq: u8) -> i32 {
    mg_table(piece)[table_index(color, sq)]
}

/// Endgame PST bonus (MG tables reused for non-kings, exactly like the TS reference).
#[inline]
pub fn pst_eg(piece: Piece, color: Color, sq: u8) -> i32 {
    match piece {
        Piece::King => KING_EG[table_index(color, sq)],
        _ => mg_table(piece)[table_index(color, sq)],
    }
}
