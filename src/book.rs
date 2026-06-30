use crate::{Move, Position};
use polyglot_book_rs::PolyglotBook;
use std::time::SystemTime;

pub struct Book {
    pub book: PolyglotBook,
    seed: u64,
}

impl Book {
    pub fn new(path: &str) -> Result<Self, String> {
        let book = PolyglotBook::load(path)
            .map_err(|e| format!("Failed to load Polyglot book: {:?}", e))?;
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42);
        Ok(Book { book, seed })
    }

    fn next_rand(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed
    }

    pub fn query(&mut self, pos: &Position) -> Option<Move> {
        let fen = pos.to_fen();
        let entries = self.book.get_all_moves_from_fen(&fen);
        if entries.is_empty() {
            return None;
        }

        let mut p_clone = pos.clone();
        let legal_moves = crate::movegen::generate_legal_list(&mut p_clone);
        let mut matches = Vec::new();
        let mut total_weight = 0;

        for entry in entries {
            let mut found_move = None;
            for i in 0..legal_moves.len() {
                let m = legal_moves.get(i);
                if matches_polyglot(&m, &entry.chess_move) {
                    found_move = Some(m);
                    break;
                }
            }
            if let Some(mv) = found_move {
                total_weight += entry.weight as u32;
                matches.push((mv, entry.weight as u32));
            }
        }

        if total_weight == 0 {
            return None;
        }

        let target = self.next_rand() % total_weight as u64;
        let mut current = 0;
        for (mv, w) in matches {
            current += w as u64;
            if target < current {
                return Some(mv);
            }
        }

        None
    }
}

fn matches_polyglot(mv: &Move, pg_move: &polyglot_book_rs::PolyglotMove) -> bool {
    if mv.from != pg_move.from || mv.to != pg_move.to {
        return false;
    }
    match (mv.flag.promo_piece(), pg_move.promotion) {
        (None, None) => true,
        (Some(p), Some(pg_p)) => {
            matches!(
                (p, pg_p),
                (crate::Piece::Queen, polyglot_book_rs::types::Piece::WQueen)
                    | (crate::Piece::Queen, polyglot_book_rs::types::Piece::BQueen)
                    | (crate::Piece::Rook, polyglot_book_rs::types::Piece::WRook)
                    | (crate::Piece::Rook, polyglot_book_rs::types::Piece::BRook)
                    | (crate::Piece::Bishop, polyglot_book_rs::types::Piece::WBishop)
                    | (crate::Piece::Bishop, polyglot_book_rs::types::Piece::BBishop)
                    | (crate::Piece::Knight, polyglot_book_rs::types::Piece::WKnight)
                    | (crate::Piece::Knight, polyglot_book_rs::types::Piece::BKnight)
            )
        }
        _ => false,
    }
}
