use crate::attacks::attackers_of;
use crate::facts::position::{side, square_name};
use crate::facts::types::{
    CaptureOpportunity, FactCollection, FactValue, KingSafetyFact, PieceFact, PieceRef, PieceType,
    SeeLosingFact,
};
use crate::movegen::{generate_legal, gives_check};
use crate::see::see;
use crate::{Color, MoveFlag, Piece, Position};

const PIECE_VALUE: [i32; 6] = [100, 300, 300, 500, 900, 0];

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

pub fn capture_opportunities(pos: &Position) -> FactCollection<CaptureOpportunity> {
    let mut legal_probe = pos.clone();
    let captures: Vec<_> = generate_legal(&mut legal_probe)
        .into_iter()
        .filter(|mv| mv.flag.is_capture())
        .collect();
    let mut out = Vec::with_capacity(captures.len());

    for mv in captures {
        let Some((attacker_color, attacker_piece)) = pos.piece_at(mv.from) else {
            continue;
        };
        let victim_square = if mv.flag == MoveFlag::EnPassant {
            if attacker_color == Color::White {
                mv.to - 8
            } else {
                mv.to + 8
            }
        } else {
            mv.to
        };
        let Some((victim_color, victim_piece)) = pos.piece_at(victim_square) else {
            continue;
        };
        let mut check_probe = pos.clone();
        let gives_check_flag = gives_check(&mut check_probe, mv);
        let see_cp = see(pos, mv.from, mv.to);
        let mut after = pos.clone();
        after.make(mv);
        let survives = !capturable_for_gain(&mut after, mv.to);
        out.push(CaptureOpportunity {
            move_uci: mv.to_uci(),
            attacker: piece_ref(attacker_color, attacker_piece, mv.from),
            victim: piece_ref(victim_color, victim_piece, victim_square),
            victim_square: square_name(victim_square),
            see_cp,
            gives_check: gives_check_flag,
            capturing_piece_survives: survives,
            highest_value_safe_capture: false,
        });
    }

    let best_safe = out
        .iter()
        .filter(|capture| capture.see_cp > 0 && capture.capturing_piece_survives)
        .max_by_key(|capture| {
            (
                PIECE_VALUE[piece_type_index(capture.victim.piece_type)],
                capture.see_cp,
            )
        })
        .map(|capture| capture.move_uci.clone());
    for capture in &mut out {
        capture.highest_value_safe_capture = best_safe.as_deref() == Some(&capture.move_uci);
    }
    out.sort_by(|a, b| a.move_uci.cmp(&b.move_uci));
    FactCollection::computed(out)
}

pub fn king_safety_facts(pos: &Position) -> FactCollection<KingSafetyFact> {
    let mut facts = Vec::new();
    for color in [Color::White, Color::Black] {
        let king_square = pos.king_sq(color);
        let attackers = refs_from_bitboard(
            pos,
            attackers_of(&pos.pieces, king_square, color.flip(), pos.all),
        );
        let mut pressured_squares = Vec::new();
        for square in king_zone(king_square) {
            if attackers_of(&pos.pieces, square, color.flip(), pos.all) != 0 {
                pressured_squares.push(square_name(square));
            }
        }
        pressured_squares.sort();
        let legal_escape_squares =
            match crate::facts::position::position_for_analysis_side(pos, color) {
                Ok(mut probe) => {
                    let mut squares = generate_legal(&mut probe)
                        .iter()
                        .filter(|mv| mv.from == king_square)
                        .map(|mv| square_name(mv.to))
                        .collect::<Vec<_>>();
                    squares.sort();
                    squares.dedup();
                    FactCollection::computed(squares)
                }
                Err(reason) => FactCollection::unavailable(reason),
            };
        facts.push(KingSafetyFact {
            side: crate::facts::position::side(color),
            king_square: square_name(king_square),
            in_check: !attackers.is_empty(),
            attackers,
            pressured_squares,
            legal_escape_squares,
        });
    }
    FactCollection::computed(facts)
}

fn capturable_for_gain(pos: &mut Position, square: u8) -> bool {
    generate_legal(pos)
        .into_iter()
        .filter(|mv| mv.to == square && mv.flag.is_capture())
        .any(|mv| see(pos, mv.from, mv.to) > 0)
}

fn king_zone(square: u8) -> Vec<u8> {
    let file = (square % 8) as i8;
    let rank = (square / 8) as i8;
    let mut zone = Vec::new();
    for df in -1..=1 {
        for dr in -1..=1 {
            let f = file + df;
            let r = rank + dr;
            if (0..8).contains(&f) && (0..8).contains(&r) {
                zone.push((r * 8 + f) as u8);
            }
        }
    }
    zone
}

fn piece_type_index(piece: PieceType) -> usize {
    match piece {
        PieceType::Pawn => Piece::Pawn.index(),
        PieceType::Knight => Piece::Knight.index(),
        PieceType::Bishop => Piece::Bishop.index(),
        PieceType::Rook => Piece::Rook.index(),
        PieceType::Queen => Piece::Queen.index(),
        PieceType::King => Piece::King.index(),
    }
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
