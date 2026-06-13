//! Validated tactical-motif enumeration (teaching facts).
//!
//! Analysis-only — never the search hot path. Emits validated FORK opportunities
//! for the side to move: a single legal move whose moved piece attacks two or
//! more winnable enemy targets, where the moved piece is not itself simply lost.
//! Rust validates the geometry and material; the application names the topic.

use crate::attacks::{attackers_of, bishop_attacks, queen_attacks, rook_attacks};
use crate::facts::piece_safety::piece_ref;
use crate::facts::position::square_name;
use crate::facts::types::{FactCollection, MotifOpportunity, PieceRef, PinOpportunity};
use crate::movegen::{generate_legal, gives_check};
use crate::see::see;
use crate::{file_of, rank_of, Color, Move, Piece, Position};

/// Centipawn values for fork bookkeeping. The king is a sentinel 0 — it is never
/// "won", only forced to respond.
const VALUE: [i32; 6] = [100, 300, 300, 500, 900, 0];

/// Validated fork opportunities for the side to move, sorted by forking move.
pub fn motif_opportunities(pos: &Position) -> FactCollection<MotifOpportunity> {
    let mut forks = enumerate_forks(pos);
    forks.sort_by(|a, b| a.move_uci.cmp(&b.move_uci));
    FactCollection::computed(forks)
}

fn enumerate_forks(pos: &Position) -> Vec<MotifOpportunity> {
    let forker_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = fork_after_move(pos, mv, forker_color) {
            out.push(op);
        }
    }
    out
}

fn fork_after_move(pos: &Position, mv: Move, forker_color: Color) -> Option<MotifOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let forker_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // A king "double attack" is not a teaching fork — it cannot win defended
    // material and is never itself capturable. Skip it to avoid false forks.
    if forker_piece == Piece::King {
        return None;
    }
    let forker_value = VALUE[forker_piece.index()];

    // Whether the move gives check (computed on a throwaway clone, pre-move).
    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv);
    let enemy = forker_color.flip();
    let forker_bit = 1u64 << mv.to;

    // Enemy pieces the moved piece now attacks from mv.to.
    let mut targets: Vec<(Piece, u8)> = Vec::new();
    for piece in Piece::ALL {
        let mut bb = after.pieces[enemy.index()][piece.index()];
        while bb != 0 {
            let sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            let attackers = attackers_of(&after.pieces, sq, forker_color, after.all);
            if attackers & forker_bit != 0 {
                targets.push((piece, sq));
            }
        }
    }
    if targets.len() < 2 {
        return None;
    }

    // The forker must not be simply lost: no enemy capture of mv.to wins material.
    // generate_legal respects check, so a check-fork only exposes check-resolving
    // captures — exactly those that could refute the fork by taking the forker.
    if forker_capturable_for_gain(&mut after.clone(), mv.to) {
        return None;
    }

    // Winnable targets: the king (forces a response), a piece worth more than the
    // forker (winning material survives any defender), or an undefended piece.
    let king_target = targets.iter().any(|(p, _)| *p == Piece::King);
    let winnable: Vec<(Piece, u8)> = targets
        .iter()
        .copied()
        .filter(|(p, sq)| {
            *p == Piece::King
                || VALUE[p.index()] > forker_value
                || is_undefended(&after, *sq, enemy)
        })
        .collect();
    let winnable_non_king: Vec<(Piece, u8)> =
        winnable.iter().copied().filter(|(p, _)| *p != Piece::King).collect();

    let valid = if king_target {
        !winnable_non_king.is_empty()
    } else {
        winnable.len() >= 2
    };
    if !valid {
        return None;
    }

    let material_gain = if king_target {
        // The king must step aside; the best winnable piece falls.
        winnable_non_king
            .iter()
            .map(|(p, _)| VALUE[p.index()])
            .max()
            .unwrap_or(0)
    } else {
        // The opponent saves the dearer piece; you collect the next-best.
        let mut vals: Vec<i32> = winnable.iter().map(|(p, _)| VALUE[p.index()]).collect();
        vals.sort_unstable_by(|a, b| b.cmp(a));
        vals.get(1).copied().unwrap_or(0)
    };

    let mut target_refs: Vec<PieceRef> =
        targets.iter().map(|(p, sq)| piece_ref(enemy, *p, *sq)).collect();
    target_refs.sort_by(|a, b| a.id.cmp(&b.id));

    Some(MotifOpportunity {
        kind: "fork".to_string(),
        validator: "fork_validation".to_string(),
        move_uci: mv.to_uci(),
        forking_piece: piece_ref(forker_color, forker_piece, mv.to),
        targets: target_refs,
        gives_check: gives_check_flag,
        king_target,
        material_gain,
    })
}

/// True if the side to move can capture the piece on `sq` for a positive SEE.
fn forker_capturable_for_gain(after: &mut Position, sq: u8) -> bool {
    let legal = generate_legal(after);
    let mut best = 0;
    for mv in legal {
        if mv.to == sq && mv.flag.is_capture() {
            let score = see(after, mv.from, mv.to);
            if score > best {
                best = score;
            }
        }
    }
    best > 0
}

/// True if `owner` has no defender (other than the piece itself) for `sq`.
fn is_undefended(pos: &Position, sq: u8, owner: Color) -> bool {
    let defenders = attackers_of(&pos.pieces, sq, owner, pos.all) & !(1u64 << sq);
    defenders == 0
}

// ── Pin opportunities ───────────────────────────────────────────────────────
// A pin is created when a slider moves so it attacks an enemy piece P with a more
// valuable enemy piece (or the king) directly behind P on the same line. Detected
// by removing P from occupancy and checking whether the slider then reaches an
// enemy anchor it could not see before.

/// Validated pin opportunities for the side to move, sorted by move then pinned id.
pub fn pin_opportunities(pos: &Position) -> FactCollection<PinOpportunity> {
    let pinner_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(pin) = pin_after_move(pos, mv, pinner_color) {
            out.push(pin);
        }
    }
    out.sort_by(|a, b| a.move_uci.cmp(&b.move_uci).then_with(|| a.pinned.id.cmp(&b.pinned.id)));
    FactCollection::computed(out)
}

fn slider_attacks(piece: Piece, sq: u8, occ: u64) -> Option<u64> {
    match piece {
        Piece::Bishop => Some(bishop_attacks(sq, occ)),
        Piece::Rook => Some(rook_attacks(sq, occ)),
        Piece::Queen => Some(queen_attacks(sq, occ)),
        _ => None,
    }
}

fn pin_after_move(pos: &Position, mv: Move, pinner_color: Color) -> Option<PinOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let pinner_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // Only sliders pin.
    if slider_attacks(pinner_piece, mv.to, 0).is_none() {
        return None;
    }

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv);
    let enemy = pinner_color.flip();
    let s = mv.to;

    let atk = slider_attacks(pinner_piece, s, after.all)?;
    let enemy_occ = enemy_occupancy(&after, enemy);

    // A pinner that is simply captured is not a real pin.
    if forker_capturable_for_gain(&mut after.clone(), s) {
        return None;
    }

    let mut relative_pin: Option<PinOpportunity> = None;
    let mut pinned_bb = atk & enemy_occ;
    while pinned_bb != 0 {
        let p_sq = pinned_bb.trailing_zeros() as u8;
        pinned_bb &= pinned_bb - 1;
        let occ2 = after.all & !(1u64 << p_sq);
        let atk2 = match slider_attacks(pinner_piece, s, occ2) {
            Some(a) => a,
            None => continue,
        };
        // The first enemy piece newly exposed past P is the anchor.
        let behind = atk2 & !atk & enemy_occ;
        if behind == 0 {
            continue;
        }
        let q_sq = behind.trailing_zeros() as u8;
        let (_, p_piece) = match after.piece_at(p_sq) {
            Some(x) => x,
            None => continue,
        };
        let (_, q_piece) = match after.piece_at(q_sq) {
            Some(x) => x,
            None => continue,
        };
        let absolute = q_piece == Piece::King;
        let relative = VALUE[q_piece.index()] > VALUE[p_piece.index()];
        if !absolute && !relative {
            continue;
        }
        let ray: Vec<String> = squares_between(s, q_sq).into_iter().map(square_name).collect();
        let pin = PinOpportunity {
            kind: if absolute { "absolute" } else { "relative" }.to_string(),
            validator: "pin_validation".to_string(),
            move_uci: mv.to_uci(),
            pinner: piece_ref(pinner_color, pinner_piece, s),
            pinned: piece_ref(enemy, p_piece, p_sq),
            anchor: piece_ref(enemy, q_piece, q_sq),
            ray,
            gives_check: gives_check_flag,
            pinned_immobile: absolute,
        };
        // Absolute pins are strongest — return the first one immediately.
        if absolute {
            return Some(pin);
        }
        if relative_pin.is_none() {
            relative_pin = Some(pin);
        }
    }
    relative_pin
}

fn enemy_occupancy(pos: &Position, enemy: Color) -> u64 {
    let mut bb = 0u64;
    for piece in Piece::ALL {
        bb |= pos.pieces[enemy.index()][piece.index()];
    }
    bb
}

/// Squares strictly between two colinear squares (exclusive of both endpoints).
fn squares_between(a: u8, b: u8) -> Vec<u8> {
    let (fa, ra) = (file_of(a) as i8, rank_of(a) as i8);
    let (fb, rb) = (file_of(b) as i8, rank_of(b) as i8);
    let df = (fb - fa).signum();
    let dr = (rb - ra).signum();
    let mut out = Vec::new();
    let (mut f, mut r) = (fa + df, ra + dr);
    while (f, r) != (fb, rb) {
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            break;
        }
        out.push((r * 8 + f) as u8);
        f += df;
        r += dr;
    }
    out
}
