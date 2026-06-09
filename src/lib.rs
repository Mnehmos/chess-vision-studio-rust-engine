//! CVS Bitboard Core v0.
//!
//! A from-scratch bitboard chess core focused on **correct legal move generation**
//! and **perft**. This is the Rust engine-core seed for Chess Vision Studio; it does
//! NOT replace the current chess.js-based engine — it exists to prove a fast, correct
//! movegen foundation (later: search, SEE, eval).
//!
//! Board convention: Little-Endian Rank-File (LERF). Square index `s = rank*8 + file`,
//! so a1=0, b1=1, …, h1=7, a2=8, …, h8=63. Bit `s` of a `u64` bitboard = that square.

pub mod attacks;
pub mod eval;
pub mod movegen;
pub mod perft;
pub mod position;
pub mod see;

pub use position::Position;

/// Side to move / piece color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    #[inline]
    pub fn flip(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Piece type (color-agnostic).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

impl Piece {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
    pub const ALL: [Piece; 6] = [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ];
}

/// Castling-rights bit flags.
pub mod castle {
    pub const WK: u8 = 1;
    pub const WQ: u8 = 2;
    pub const BK: u8 = 4;
    pub const BQ: u8 = 8;
}

/// What kind of move this is — drives make/unmake (the 16-bit-style flag taxonomy).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveFlag {
    Quiet,
    DoublePush,
    KingCastle,
    QueenCastle,
    Capture,
    EnPassant,
    PromoN,
    PromoB,
    PromoR,
    PromoQ,
    PromoNCap,
    PromoBCap,
    PromoRCap,
    PromoQCap,
}

impl MoveFlag {
    #[inline]
    pub fn is_capture(self) -> bool {
        matches!(
            self,
            MoveFlag::Capture
                | MoveFlag::EnPassant
                | MoveFlag::PromoNCap
                | MoveFlag::PromoBCap
                | MoveFlag::PromoRCap
                | MoveFlag::PromoQCap
        )
    }
    /// The piece a pawn promotes to, if this is a promotion flag.
    #[inline]
    pub fn promo_piece(self) -> Option<Piece> {
        match self {
            MoveFlag::PromoN | MoveFlag::PromoNCap => Some(Piece::Knight),
            MoveFlag::PromoB | MoveFlag::PromoBCap => Some(Piece::Bishop),
            MoveFlag::PromoR | MoveFlag::PromoRCap => Some(Piece::Rook),
            MoveFlag::PromoQ | MoveFlag::PromoQCap => Some(Piece::Queen),
            _ => None,
        }
    }
}

/// A move: from-square, to-square, and a flag describing its kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub flag: MoveFlag,
}

impl Move {
    #[inline]
    pub fn new(from: u8, to: u8, flag: MoveFlag) -> Move {
        Move { from, to, flag }
    }
    /// Long-algebraic (UCI) string, e.g. "e2e4", "e7e8q".
    pub fn to_uci(self) -> String {
        let sq = |s: u8| -> String {
            let file = (b'a' + (s % 8)) as char;
            let rank = (b'1' + (s / 8)) as char;
            format!("{file}{rank}")
        };
        let promo = match self.flag.promo_piece() {
            Some(Piece::Knight) => "n",
            Some(Piece::Bishop) => "b",
            Some(Piece::Rook) => "r",
            Some(Piece::Queen) => "q",
            _ => "",
        };
        format!("{}{}{}", sq(self.from), sq(self.to), promo)
    }
}

/// Square-index helpers (LERF).
#[inline]
pub fn rank_of(sq: u8) -> u8 {
    sq >> 3
}
#[inline]
pub fn file_of(sq: u8) -> u8 {
    sq & 7
}
#[inline]
pub fn square(file: u8, rank: u8) -> u8 {
    rank * 8 + file
}
