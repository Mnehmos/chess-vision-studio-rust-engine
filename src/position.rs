//! Board state, FEN parsing, and make/unmake. Bitboards are maintained
//! incrementally. make/unmake is exact (an undo stack records the captured piece
//! and the prior castling/ep/halfmove state), which is what perft validates.
//! A Zobrist hash is maintained incrementally (piece-square keys xored in
//! set/clear_piece; stm/castling/ep deltas applied in make; unmake restores the
//! recorded prior hash) — the transposition-table key for the search.
use crate::{castle, Color, Move, MoveFlag, Piece};
use std::sync::OnceLock;

struct Zobrist {
    piece_sq: [[[u64; 64]; 6]; 2],
    stm_black: u64,
    castling: [u64; 16],
    ep_file: [u64; 8],
}

static ZOBRIST: OnceLock<Zobrist> = OnceLock::new();

fn zobrist() -> &'static Zobrist {
    ZOBRIST.get_or_init(|| {
        // splitmix64 from a fixed seed → deterministic keys.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let mut piece_sq = [[[0u64; 64]; 6]; 2];
        for c in piece_sq.iter_mut() {
            for p in c.iter_mut() {
                for s in p.iter_mut() {
                    *s = next();
                }
            }
        }
        let stm_black = next();
        let mut castling = [0u64; 16];
        for v in castling.iter_mut() {
            *v = next();
        }
        let mut ep_file = [0u64; 8];
        for v in ep_file.iter_mut() {
            *v = next();
        }
        Zobrist { piece_sq, stm_black, castling, ep_file }
    })
}

#[inline]
fn ep_key(ep: Option<u8>) -> u64 {
    match ep {
        Some(sq) => zobrist().ep_file[(sq & 7) as usize],
        None => 0,
    }
}

#[derive(Clone)]
struct Undo {
    mv: Move,
    captured: Option<Piece>,
    prev_castling: u8,
    prev_ep: Option<u8>,
    prev_halfmove: u16,
    prev_hash: u64,
}

#[derive(Clone)]
pub struct Position {
    /// pieces[color][piece] bitboards.
    pub pieces: [[u64; 6]; 2],
    /// occupancy per color.
    pub occ: [u64; 2],
    /// all occupied squares.
    pub all: u64,
    pub stm: Color,
    pub castling: u8,
    pub ep: Option<u8>,
    pub halfmove: u16,
    pub fullmove: u16,
    /// Zobrist hash of (pieces, stm, castling, ep-file).
    pub hash: u64,
    history: Vec<Undo>,
}

pub const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

impl Position {
    pub fn startpos() -> Position {
        Position::from_fen(STARTPOS_FEN).unwrap()
    }

    pub fn from_fen(fen: &str) -> Result<Position, String> {
        let mut pos = Position {
            pieces: [[0; 6]; 2],
            occ: [0; 2],
            all: 0,
            stm: Color::White,
            castling: 0,
            ep: None,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
            history: Vec::with_capacity(64),
        };
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(format!("FEN needs ≥4 fields: {fen}"));
        }
        // Board: FEN lists rank 8 first, files a→h.
        let mut rank: i32 = 7;
        for row in parts[0].split('/') {
            let mut file: i32 = 0;
            for ch in row.chars() {
                if let Some(d) = ch.to_digit(10) {
                    file += d as i32;
                } else {
                    let (color, piece) = parse_piece(ch).ok_or(format!("bad piece '{ch}'"))?;
                    if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                        return Err(format!("FEN out of range at '{ch}'"));
                    }
                    pos.set_piece(color, piece, (rank * 8 + file) as u8);
                    file += 1;
                }
            }
            rank -= 1;
        }
        pos.stm = match parts[1] {
            "w" => Color::White,
            "b" => Color::Black,
            s => return Err(format!("bad stm '{s}'")),
        };
        for ch in parts[2].chars() {
            match ch {
                'K' => pos.castling |= castle::WK,
                'Q' => pos.castling |= castle::WQ,
                'k' => pos.castling |= castle::BK,
                'q' => pos.castling |= castle::BQ,
                '-' => {}
                _ => return Err(format!("bad castling '{ch}'")),
            }
        }
        pos.ep = if parts[3] == "-" {
            None
        } else {
            Some(parse_square(parts[3]).ok_or(format!("bad ep '{}'", parts[3]))?)
        };
        if let Some(h) = parts.get(4) {
            pos.halfmove = h.parse().unwrap_or(0);
        }
        if let Some(f) = parts.get(5) {
            pos.fullmove = f.parse().unwrap_or(1);
        }
        // Finalize the Zobrist hash (piece keys were xored by set_piece during parse).
        if pos.stm == Color::Black {
            pos.hash ^= zobrist().stm_black;
        }
        pos.hash ^= zobrist().castling[pos.castling as usize];
        pos.hash ^= ep_key(pos.ep);
        Ok(pos)
    }

    #[inline]
    pub fn king_sq(&self, color: Color) -> u8 {
        self.pieces[color.index()][Piece::King.index()].trailing_zeros() as u8
    }

    #[inline]
    fn set_piece(&mut self, color: Color, piece: Piece, sq: u8) {
        let b = 1u64 << sq;
        self.pieces[color.index()][piece.index()] |= b;
        self.occ[color.index()] |= b;
        self.all |= b;
        self.hash ^= zobrist().piece_sq[color.index()][piece.index()][sq as usize];
    }
    #[inline]
    fn clear_piece(&mut self, color: Color, piece: Piece, sq: u8) {
        let b = !(1u64 << sq);
        self.pieces[color.index()][piece.index()] &= b;
        self.occ[color.index()] &= b;
        self.all &= b;
        self.hash ^= zobrist().piece_sq[color.index()][piece.index()][sq as usize];
    }
    #[inline]
    fn move_piece(&mut self, color: Color, piece: Piece, from: u8, to: u8) {
        self.clear_piece(color, piece, from);
        self.set_piece(color, piece, to);
    }
    #[inline]
    fn piece_at_color(&self, color: Color, sq: u8) -> Option<Piece> {
        let b = 1u64 << sq;
        for p in Piece::ALL {
            if self.pieces[color.index()][p.index()] & b != 0 {
                return Some(p);
            }
        }
        None
    }

    /// The (color, piece) on `sq`, or None if empty.
    #[inline]
    pub fn piece_at(&self, sq: u8) -> Option<(Color, Piece)> {
        let b = 1u64 << sq;
        if self.occ[Color::White.index()] & b != 0 {
            self.piece_at_color(Color::White, sq).map(|p| (Color::White, p))
        } else if self.occ[Color::Black.index()] & b != 0 {
            self.piece_at_color(Color::Black, sq).map(|p| (Color::Black, p))
        } else {
            None
        }
    }
    #[inline]
    fn clear_castle_for_square(&mut self, sq: u8) {
        // Any move FROM or TO a corner clears that corner's castling right
        // (covers a rook leaving home AND a rook being captured at home).
        match sq {
            0 => self.castling &= !castle::WQ,
            7 => self.castling &= !castle::WK,
            56 => self.castling &= !castle::BQ,
            63 => self.castling &= !castle::BK,
            _ => {}
        }
    }

    /// True when the current position already occurred earlier in the
    /// make/unmake history (same Zobrist hash, which includes side-to-move,
    /// castling and ep-file). Only the reversible window matters: nothing
    /// before the last pawn move/capture (halfmove reset) can repeat. One
    /// prior occurrence is enough for the search to treat the node as a draw
    /// — the IUBKTvjF lesson: an engine with no repetition knowledge checked
    /// forever in a two-piece-up position and drew by threefold.
    pub fn is_repetition(&self) -> bool {
        let window = self.halfmove as usize;
        self.history
            .iter()
            .rev()
            .take(window)
            .any(|u| u.prev_hash == self.hash)
    }

    pub fn make(&mut self, mv: Move) {
        let us = self.stm;
        let them = us.flip();
        let from = mv.from;
        let to = mv.to;
        let moving = self.piece_at_color(us, from).expect("make: no piece on from");
        let mut captured: Option<Piece> = None;
        let prev_castling = self.castling;
        let prev_ep = self.ep;
        let prev_halfmove = self.halfmove;
        let prev_hash = self.hash;
        self.ep = None;

        match mv.flag {
            MoveFlag::Quiet => self.move_piece(us, moving, from, to),
            MoveFlag::DoublePush => {
                self.move_piece(us, Piece::Pawn, from, to);
                self.ep = Some(if us == Color::White { to - 8 } else { to + 8 });
            }
            MoveFlag::Capture => {
                let cap = self.piece_at_color(them, to).expect("make: capture on empty");
                self.clear_piece(them, cap, to);
                captured = Some(cap);
                self.move_piece(us, moving, from, to);
            }
            MoveFlag::EnPassant => {
                self.move_piece(us, Piece::Pawn, from, to);
                let capsq = if us == Color::White { to - 8 } else { to + 8 };
                self.clear_piece(them, Piece::Pawn, capsq);
                captured = Some(Piece::Pawn);
            }
            MoveFlag::KingCastle => {
                self.move_piece(us, Piece::King, from, to);
                let (rf, rt) = if us == Color::White { (7, 5) } else { (63, 61) };
                self.move_piece(us, Piece::Rook, rf, rt);
            }
            MoveFlag::QueenCastle => {
                self.move_piece(us, Piece::King, from, to);
                let (rf, rt) = if us == Color::White { (0, 3) } else { (56, 59) };
                self.move_piece(us, Piece::Rook, rf, rt);
            }
            _ => {
                // Promotion (with or without capture).
                self.clear_piece(us, Piece::Pawn, from);
                if mv.flag.is_capture() {
                    let cap = self.piece_at_color(them, to).expect("make: promo-cap on empty");
                    self.clear_piece(them, cap, to);
                    captured = Some(cap);
                }
                self.set_piece(us, mv.flag.promo_piece().unwrap(), to);
            }
        }

        // Castling rights.
        if moving == Piece::King {
            let mask = if us == Color::White { castle::WK | castle::WQ } else { castle::BK | castle::BQ };
            self.castling &= !mask;
        }
        self.clear_castle_for_square(from);
        self.clear_castle_for_square(to);

        self.halfmove = if moving == Piece::Pawn || captured.is_some() || mv.flag.promo_piece().is_some() {
            0
        } else {
            self.halfmove + 1
        };
        if us == Color::Black {
            self.fullmove += 1;
        }
        self.stm = them;
        // Hash deltas for the non-piece state (piece keys already xored by
        // set/clear/move_piece during the mutations above).
        let z = zobrist();
        self.hash ^= z.stm_black;
        self.hash ^= z.castling[prev_castling as usize] ^ z.castling[self.castling as usize];
        self.hash ^= ep_key(prev_ep) ^ ep_key(self.ep);
        self.history.push(Undo { mv, captured, prev_castling, prev_ep, prev_halfmove, prev_hash });
    }

    pub fn unmake(&mut self) {
        let undo = self.history.pop().expect("unmake: empty history");
        let mv = undo.mv;
        let us = self.stm.flip(); // the side that moved
        let them = us.flip();
        let from = mv.from;
        let to = mv.to;

        match mv.flag {
            MoveFlag::Quiet | MoveFlag::DoublePush => {
                let moving = self.piece_at_color(us, to).expect("unmake: nothing on to");
                self.move_piece(us, moving, to, from);
            }
            MoveFlag::Capture => {
                let moving = self.piece_at_color(us, to).expect("unmake: nothing on to");
                self.move_piece(us, moving, to, from);
                self.set_piece(them, undo.captured.expect("unmake: missing captured"), to);
            }
            MoveFlag::EnPassant => {
                self.move_piece(us, Piece::Pawn, to, from);
                let capsq = if us == Color::White { to - 8 } else { to + 8 };
                self.set_piece(them, Piece::Pawn, capsq);
            }
            MoveFlag::KingCastle => {
                self.move_piece(us, Piece::King, to, from);
                let (rf, rt) = if us == Color::White { (7, 5) } else { (63, 61) };
                self.move_piece(us, Piece::Rook, rt, rf);
            }
            MoveFlag::QueenCastle => {
                self.move_piece(us, Piece::King, to, from);
                let (rf, rt) = if us == Color::White { (0, 3) } else { (56, 59) };
                self.move_piece(us, Piece::Rook, rt, rf);
            }
            _ => {
                // Promotion: remove the promoted piece, restore the pawn.
                self.clear_piece(us, mv.flag.promo_piece().unwrap(), to);
                self.set_piece(us, Piece::Pawn, from);
                if mv.flag.is_capture() {
                    self.set_piece(them, undo.captured.expect("unmake: missing promo-cap"), to);
                }
            }
        }

        if us == Color::Black {
            self.fullmove -= 1;
        }
        self.stm = us;
        self.castling = undo.prev_castling;
        self.ep = undo.prev_ep;
        self.halfmove = undo.prev_halfmove;
        // Restoring the recorded hash wipes out the piece-key xors applied during
        // the restore moves above — exact by construction.
        self.hash = undo.prev_hash;
    }
}

fn parse_piece(ch: char) -> Option<(Color, Piece)> {
    let color = if ch.is_ascii_uppercase() { Color::White } else { Color::Black };
    let piece = match ch.to_ascii_lowercase() {
        'p' => Piece::Pawn,
        'n' => Piece::Knight,
        'b' => Piece::Bishop,
        'r' => Piece::Rook,
        'q' => Piece::Queen,
        'k' => Piece::King,
        _ => return None,
    };
    Some((color, piece))
}

fn parse_square(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let file = b[0].checked_sub(b'a')?;
    let rank = b[1].checked_sub(b'1')?;
    if file > 7 || rank > 7 {
        return None;
    }
    Some(rank * 8 + file)
}
