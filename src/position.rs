//! Board state, FEN parsing, and make/unmake. Bitboards are maintained
//! incrementally. make/unmake is exact (an undo stack records the captured piece
//! and the prior castling/ep/halfmove state), which is what perft validates.
use crate::{castle, Color, Move, MoveFlag, Piece};

#[derive(Clone)]
struct Undo {
    mv: Move,
    captured: Option<Piece>,
    prev_castling: u8,
    prev_ep: Option<u8>,
    prev_halfmove: u16,
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
    }
    #[inline]
    fn clear_piece(&mut self, color: Color, piece: Piece, sq: u8) {
        let b = !(1u64 << sq);
        self.pieces[color.index()][piece.index()] &= b;
        self.occ[color.index()] &= b;
        self.all &= b;
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
        self.history.push(Undo { mv, captured, prev_castling, prev_ep, prev_halfmove });
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
