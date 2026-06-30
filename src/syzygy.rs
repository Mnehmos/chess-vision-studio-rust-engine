use crate::{Color, Piece, Position};
use pyrrhic_rs::{Color as PyrrhicColor, Piece as PyrrhicPiece, TableBases, WdlProbeResult};

#[derive(Clone, Copy)]
pub struct CvsEngineAdapter;

impl pyrrhic_rs::EngineAdapter for CvsEngineAdapter {
    fn pawn_attacks(color: PyrrhicColor, square: u64) -> u64 {
        let cvs_color = match color {
            PyrrhicColor::White => Color::White,
            PyrrhicColor::Black => Color::Black,
        };
        crate::attacks::pawn_attacks(cvs_color, square as u8)
    }

    fn knight_attacks(square: u64) -> u64 {
        crate::attacks::knight_attacks(square as u8)
    }

    fn bishop_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::bishop_attacks(square as u8, occupied)
    }

    fn rook_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::rook_attacks(square as u8, occupied)
    }

    fn queen_attacks(square: u64, occupied: u64) -> u64 {
        crate::attacks::queen_attacks(square as u8, occupied)
    }

    fn king_attacks(square: u64) -> u64 {
        crate::attacks::king_attacks(square as u8)
    }
}

pub struct Syzygy {
    pub tb: TableBases<CvsEngineAdapter>,
}

impl Syzygy {
    pub fn new(path: &str) -> Result<Self, String> {
        TableBases::new(path)
            .map(|tb| Syzygy { tb })
            .map_err(|e| format!("Failed to initialize tablebases: {:?}", e))
    }

    pub fn max_pieces(&self) -> u32 {
        self.tb.max_pieces()
    }

    pub fn probe_wdl(&self, pos: &Position) -> Option<WdlProbeResult> {
        if pos.castling != 0 {
            return None;
        }
        let white = pos.occ[Color::White.index()];
        let black = pos.occ[Color::Black.index()];
        let kings = pos.pieces[0][Piece::King.index()] | pos.pieces[1][Piece::King.index()];
        let queens = pos.pieces[0][Piece::Queen.index()] | pos.pieces[1][Piece::Queen.index()];
        let rooks = pos.pieces[0][Piece::Rook.index()] | pos.pieces[1][Piece::Rook.index()];
        let bishops = pos.pieces[0][Piece::Bishop.index()] | pos.pieces[1][Piece::Bishop.index()];
        let knights = pos.pieces[0][Piece::Knight.index()] | pos.pieces[1][Piece::Knight.index()];
        let pawns = pos.pieces[0][Piece::Pawn.index()] | pos.pieces[1][Piece::Pawn.index()];
        let ep = pos.ep.map(|s| s as u32).unwrap_or(0);
        let turn = pos.stm == Color::White;

        self.tb.probe_wdl(white, black, kings, queens, rooks, bishops, knights, pawns, ep, turn).ok()
    }

    pub fn probe_root(&self, pos: &Position) -> Option<(crate::Move, WdlProbeResult)> {
        if pos.castling != 0 {
            return None;
        }
        let white = pos.occ[Color::White.index()];
        let black = pos.occ[Color::Black.index()];
        let kings = pos.pieces[0][Piece::King.index()] | pos.pieces[1][Piece::King.index()];
        let queens = pos.pieces[0][Piece::Queen.index()] | pos.pieces[1][Piece::Queen.index()];
        let rooks = pos.pieces[0][Piece::Rook.index()] | pos.pieces[1][Piece::Rook.index()];
        let bishops = pos.pieces[0][Piece::Bishop.index()] | pos.pieces[1][Piece::Bishop.index()];
        let knights = pos.pieces[0][Piece::Knight.index()] | pos.pieces[1][Piece::Knight.index()];
        let pawns = pos.pieces[0][Piece::Pawn.index()] | pos.pieces[1][Piece::Pawn.index()];
        let ep = pos.ep.map(|s| s as u32).unwrap_or(0);
        let turn = pos.stm == Color::White;
        let rule50 = pos.halfmove as u32;

        let res = self.tb.probe_root(white, black, kings, queens, rooks, bishops, knights, pawns, rule50, ep, turn).ok()?;
        match res.root {
            pyrrhic_rs::DtzProbeValue::DtzResult(dtz_res) => {
                let mut p_clone = pos.clone();
                let legal = crate::movegen::generate_legal_list(&mut p_clone);
                for i in 0..legal.len() {
                    let mv = legal.get(i);
                    if mv.from == dtz_res.from_square && mv.to == dtz_res.to_square {
                        let promo = mv.flag.promo_piece();
                        let matches_promo = match (promo, dtz_res.promotion) {
                            (None, PyrrhicPiece::Pawn) => true,
                            (Some(Piece::Queen), PyrrhicPiece::Queen) => true,
                            (Some(Piece::Rook), PyrrhicPiece::Rook) => true,
                            (Some(Piece::Bishop), PyrrhicPiece::Bishop) => true,
                            (Some(Piece::Knight), PyrrhicPiece::Knight) => true,
                            _ => false,
                        };
                        if matches_promo {
                            return Some((mv, dtz_res.wdl));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
