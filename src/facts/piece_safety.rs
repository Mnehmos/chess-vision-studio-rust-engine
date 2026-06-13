use crate::attacks::attackers_of;
use crate::facts::position::{side, square_name};
use crate::facts::types::{FactValue, PieceFact, PieceRef, PieceType, SeeLosingFact};
use crate::movegen::generate_legal;
use crate::see::see;
use crate::{Color, Piece, Position};

pub fn piece_facts(pos: &Position) -> Vec<PieceFact> {
    let mut facts = Vec::new();
    for color in [Color::White, Color::Black] {
        for piece in Piece::ALL {
            let mut bb = pos.pieces[color.index()][piece.index()];
            while bb != 0 {
                let square = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let attackers = refs_from_bitboard(
                    pos,
                    attackers_of(&pos.pieces, square, color.flip(), pos.all),
                );
                let defenders = refs_from_bitboard(
                    pos,
                    attackers_of(&pos.pieces, square, color, pos.all) & !(1u64 << square),
                );
                let see = if color != pos.stm {
                    FactValue::Computed {
                        value: see_losing_for_target(pos, square),
                    }
                } else {
                    FactValue::Unavailable {
                        reason: "piece_not_capturable_by_side_to_move".to_string(),
                    }
                };
                facts.push(PieceFact {
                    piece: piece_ref(color, piece, square),
                    attacker_count: attackers.len() as u32,
                    defender_count: defenders.len() as u32,
                    attacked: !attackers.is_empty(),
                    loose: defenders.is_empty(),
                    attackers,
                    defenders,
                    see,
                    only_defender_of: only_defender_targets(pos, color, square),
                });
            }
        }
    }
    facts.sort_by(|a, b| a.piece.id.cmp(&b.piece.id));
    facts
}

fn see_losing_for_target(pos: &Position, target: u8) -> SeeLosingFact {
    let mut clone = pos.clone();
    let legal = generate_legal(&mut clone);
    let mut best: Option<(String, i32)> = None;
    for mv in legal {
        if mv.to != target || !mv.flag.is_capture() {
            continue;
        }
        let score = see(&clone, mv.from, mv.to);
        if best
            .as_ref()
            .map_or(true, |(_, best_score)| score > *best_score)
        {
            best = Some((mv.to_uci(), score));
        }
    }
    SeeLosingFact {
        losing: best.as_ref().is_some_and(|(_, score)| *score > 0),
        best_capture_uci: best.as_ref().map(|(uci, _)| uci.clone()),
        score_cp: best.map(|(_, score)| score),
    }
}

fn only_defender_targets(pos: &Position, color: Color, defender_square: u8) -> Vec<PieceRef> {
    let mut targets = Vec::new();
    for piece in Piece::ALL {
        let mut bb = pos.pieces[color.index()][piece.index()];
        while bb != 0 {
            let target = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if target == defender_square {
                continue;
            }
            let defenders = attackers_of(&pos.pieces, target, color, pos.all) & !(1u64 << target);
            if defenders == 1u64 << defender_square {
                targets.push(piece_ref(color, piece, target));
            }
        }
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets
}

fn refs_from_bitboard(pos: &Position, mut bb: u64) -> Vec<PieceRef> {
    let mut out = Vec::new();
    while bb != 0 {
        let square = bb.trailing_zeros() as u8;
        bb &= bb - 1;
        if let Some((color, piece)) = pos.piece_at(square) {
            out.push(piece_ref(color, piece, square));
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn piece_ref(color: Color, piece: Piece, square: u8) -> PieceRef {
    let side_name = match color {
        Color::White => "white",
        Color::Black => "black",
    };
    let piece_name = match piece {
        Piece::Pawn => "pawn",
        Piece::Knight => "knight",
        Piece::Bishop => "bishop",
        Piece::Rook => "rook",
        Piece::Queen => "queen",
        Piece::King => "king",
    };
    PieceRef {
        id: format!("{side_name}-{piece_name}-{}", square_name(square)),
        side: side(color),
        piece_type: match piece {
            Piece::Pawn => PieceType::Pawn,
            Piece::Knight => PieceType::Knight,
            Piece::Bishop => PieceType::Bishop,
            Piece::Rook => PieceType::Rook,
            Piece::Queen => PieceType::Queen,
            Piece::King => PieceType::King,
        },
        square: square_name(square),
    }
}
