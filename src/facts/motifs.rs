//! Validated tactical-motif enumeration (teaching facts).
//!
//! Analysis-only — never the search hot path. Emits validated FORK opportunities
//! for the side to move: a single legal move whose moved piece attacks two or
//! more winnable enemy targets, where the moved piece is not itself simply lost.
//! Rust validates the geometry and material; the application names the topic.

use crate::attacks::{attackers_of, bishop_attacks, queen_attacks, rook_attacks};
use crate::facts::piece_safety::piece_ref;
use crate::facts::position::{position_for_analysis_side, square_name};
use crate::facts::types::{
    AttackDefenderOpportunity, BatteryFact, DeflectionOpportunity, DesperadoOpportunity,
    DiscoveredDefenseOpportunity, DiscoveryOpportunity, DoubleAttackOpportunity, FactCollection,
    InterferenceOpportunity, LureDefenderOpportunity, MotifOpportunity, OverloadOpportunity,
    PieceRef, PinOpportunity, RemoveGuardOpportunity, SkewerOpportunity, TrappedPieceOpportunity,
    WinExchangeOpportunity, XRayDefenseOpportunity, XRayOpportunity,
};
use crate::movegen::{generate_legal, gives_check};
use crate::see::{see, SEE_VALUE};
use crate::{file_of, rank_of, Color, Move, MoveFlag, Piece, Position};

/// Centipawn values for fork bookkeeping. The king is a sentinel 0 — it is never
/// "won", only forced to respond.
const VALUE: [i32; 6] = [100, 300, 300, 500, 900, 0];

/// Validated fork opportunities for the side to move, sorted by forking move.
pub fn motif_opportunities(pos: &Position) -> FactCollection<MotifOpportunity> {
    let mut forks = enumerate_forks(pos);
    forks.sort_by(|a, b| a.move_uci.cmp(&b.move_uci));
    FactCollection::computed(forks)
}

/// Validated forks for a requested side, including the non-moving side. The
/// latter is a counterfactual analysis probe, so en-passant rights are cleared:
/// they belong only to the actual side to move in the source position.
pub fn motif_opportunities_for(pos: &Position, side: Color) -> FactCollection<MotifOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => motif_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
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
    let winnable_non_king: Vec<(Piece, u8)> = winnable
        .iter()
        .copied()
        .filter(|(p, _)| *p != Piece::King)
        .collect();

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

    let mut target_refs: Vec<PieceRef> = targets
        .iter()
        .map(|(p, sq)| piece_ref(enemy, *p, *sq))
        .collect();
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

/// True if a piece on `from` may LEGALLY move to `to` without exposing `us`'s king — i.e. the
/// move is not blocked by an absolute pin. Pure king-ray geometry, so it holds regardless of
/// whose turn it is or any check state (unlike a generate_legal probe, which is unavailable when
/// our move gave check). A pinned piece may still travel ALONG its pin ray, so a capture whose
/// `to` stays collinear with king↔from on the pin axis is permitted.
fn capture_legal_wrt_pin(pos: &Position, from: u8, to: u8, us: Color) -> bool {
    let king = pos.king_sq(us);
    let enemy = us.flip();
    let from_bit = 1u64 << from;
    let occ = pos.all;
    let major = |p: Piece| pos.pieces[enemy.index()][p.index()];
    // Orthogonal (rank/file) pin.
    let orth = rook_attacks(king, occ);
    if orth & from_bit != 0 {
        let exposed = rook_attacks(king, occ & !from_bit) & !orth;
        if exposed & (major(Piece::Rook) | major(Piece::Queen)) != 0 {
            // Pinned orthogonally: legal only if king, from, to share the SAME rank or file.
            let same_file = file_of(king) == file_of(from) && file_of(from) == file_of(to);
            let same_rank = rank_of(king) == rank_of(from) && rank_of(from) == rank_of(to);
            return same_file || same_rank;
        }
    }
    // Diagonal pin.
    let diag = bishop_attacks(king, occ);
    if diag & from_bit != 0 {
        let exposed = bishop_attacks(king, occ & !from_bit) & !diag;
        if exposed & (major(Piece::Bishop) | major(Piece::Queen)) != 0 {
            // Pinned diagonally: legal only if to stays on the king↔from diagonal.
            let kf = file_of(king) as i8 - file_of(from) as i8;
            let kr = rank_of(king) as i8 - rank_of(from) as i8;
            let tf = file_of(to) as i8 - file_of(from) as i8;
            let tr = rank_of(to) as i8 - rank_of(from) as i8;
            // collinear on a diagonal: same slope magnitude and both diagonal steps.
            return kf.abs() == kr.abs()
                && tf.abs() == tr.abs()
                && kf.signum() * kr.signum() == tf.signum() * tr.signum();
        }
    }
    true // `from` is not absolutely pinned
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
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.pinned.id.cmp(&b.pinned.id))
    });
    FactCollection::computed(out)
}

/// Validated pins for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn pin_opportunities_for(pos: &Position, side: Color) -> FactCollection<PinOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => pin_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
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
    slider_attacks(pinner_piece, mv.to, 0)?;

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
        let ray: Vec<String> = squares_between(s, q_sq)
            .into_iter()
            .map(square_name)
            .collect();
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

// ── Skewer opportunities ─────────────────────────────────────────────────────
// A skewer is the mirror of a pin: a slider moves to attack an enemy piece F that
// is forced to step aside — the king (it is in check) or a piece worth more than
// the slider — exposing a strictly less valuable enemy piece B directly behind it
// on the same line. Detected with the same slider-ray probe the pin uses: remove F
// from occupancy and check whether the slider then reaches a winnable enemy piece B
// it could not see before. B's defenders are counted with F removed, since F is
// forced to leave and any defence it provided is illusory.

/// Validated skewer opportunities for the side to move, sorted by move then back id.
pub fn skewer_opportunities(pos: &Position) -> FactCollection<SkewerOpportunity> {
    let skewerer_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(sk) = skewer_after_move(pos, mv, skewerer_color) {
            out.push(sk);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.back.id.cmp(&b.back.id))
    });
    FactCollection::computed(out)
}

/// Validated skewers for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn skewer_opportunities_for(pos: &Position, side: Color) -> FactCollection<SkewerOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => skewer_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn skewer_after_move(pos: &Position, mv: Move, skewerer_color: Color) -> Option<SkewerOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let skewerer_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // Only sliders skewer.
    slider_attacks(skewerer_piece, mv.to, 0)?;
    let skewerer_value = VALUE[skewerer_piece.index()];

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv);
    let enemy = skewerer_color.flip();
    let s = mv.to;

    let atk = slider_attacks(skewerer_piece, s, after.all)?;
    let enemy_occ = enemy_occupancy(&after, enemy);

    // A skewerer that is simply captured is not a real skewer.
    if forker_capturable_for_gain(&mut after.clone(), s) {
        return None;
    }

    let mut front_bb = atk & enemy_occ;
    while front_bb != 0 {
        let f_sq = front_bb.trailing_zeros() as u8;
        front_bb &= front_bb - 1;
        let (_, f_piece) = match after.piece_at(f_sq) {
            Some(x) => x,
            None => continue,
        };
        // The front piece must be forced to move: the king (it is in check), or a
        // piece worth more than the skewerer (it cannot afford to be captured).
        let front_forced = f_piece == Piece::King || VALUE[f_piece.index()] > skewerer_value;
        if !front_forced {
            continue;
        }

        // The first enemy piece newly exposed behind F is the back piece.
        let occ2 = after.all & !(1u64 << f_sq);
        let atk2 = match slider_attacks(skewerer_piece, s, occ2) {
            Some(a) => a,
            None => continue,
        };
        let behind = atk2 & !atk & enemy_occ;
        if behind == 0 {
            continue;
        }
        let b_sq = behind.trailing_zeros() as u8;
        let (_, b_piece) = match after.piece_at(b_sq) {
            Some(x) => x,
            None => continue,
        };
        // A king behind the front piece makes this a pin, not a skewer.
        if b_piece == Piece::King {
            continue;
        }
        // Skewer geometry: the front piece is strictly more valuable than the piece
        // behind it (a king front always qualifies). Otherwise it is a pin.
        if f_piece != Piece::King && VALUE[f_piece.index()] <= VALUE[b_piece.index()] {
            continue;
        }
        // The back piece must be winnable once the front steps aside. Count B's
        // defenders with F removed (F is forced to leave): undefended → win it
        // outright; otherwise it must be worth more than the skewerer to profit
        // through the recapture.
        let back_defenders = attackers_of(&after.pieces, b_sq, enemy, after.all)
            & !(1u64 << b_sq)
            & !(1u64 << f_sq);
        let back_undefended = back_defenders == 0;
        if !back_undefended && VALUE[b_piece.index()] <= skewerer_value {
            continue;
        }

        let ray: Vec<String> = squares_between(s, b_sq).into_iter().map(square_name).collect();
        let material_gain = if back_undefended {
            VALUE[b_piece.index()]
        } else {
            VALUE[b_piece.index()] - skewerer_value
        };

        return Some(SkewerOpportunity {
            kind: "skewer".to_string(),
            validator: "skewer_validation".to_string(),
            move_uci: mv.to_uci(),
            skewerer: piece_ref(skewerer_color, skewerer_piece, s),
            front: piece_ref(enemy, f_piece, f_sq),
            back: piece_ref(enemy, b_piece, b_sq),
            ray,
            gives_check: gives_check_flag,
            material_gain,
        });
    }
    None
}

// ── X-ray attack (attack through an enemy piece) ─────────────────────────────
// Our slider attacks a DEFENDED front enemy piece F, with a second enemy piece B
// directly behind F on the SAME slider line. This is neither a pin (B would be the
// king) nor a skewer (F would be worth more than B and be forced to flee): here
// VALUE[F] <= VALUE[B], B is not the king, and F is not the king, so F is NOT forced
// to move. The tactic is a COUNTING win — take F, get recaptured, our slider
// RE-CAPTURES down the now-cleared line; `see` reveals the rear same-line slider
// through the shrinking occupancy, so a naive one-square count on F
// (`best_see_capture`) MISSES it while the full swap on F wins. That gap IS the motif.
//
// No worst-case-over-all-replies loop is needed (unlike attack_defender/interference):
// the win is a pure exchange on ONE square that `see` already minimaxes over both
// sides' best recaptures. `best_see_capture(pos, f_sq) <= 0` pre-move plus
// `see(after_us, s, f_sq) > 0` post-move is the complete proof.

/// Validated x-ray-attack opportunities for the side to move, sorted by move then front id.
pub fn xray_attack_opportunities(pos: &Position) -> FactCollection<XRayOpportunity> {
    let xrayer_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(x) = xray_attack_after_move(pos, mv, xrayer_color) {
            out.push(x);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.front.id.cmp(&b.front.id))
    });
    FactCollection::computed(out)
}

/// Validated x-ray attacks for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn xray_attack_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<XRayOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => xray_attack_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn xray_attack_after_move(pos: &Position, mv: Move, us: Color) -> Option<XRayOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let xrayer_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // (G0) Only sliders x-ray.
    slider_attacks(xrayer_piece, mv.to, 0)?;

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let enemy = us.flip();
    let s = mv.to;

    // (G1) Our slider must not simply hang on s.
    if forker_capturable_for_gain(&mut after.clone(), s) {
        return None;
    }

    let atk = slider_attacks(xrayer_piece, s, after.all)?;
    let enemy_occ = enemy_occupancy(&after, enemy);

    // (G2) us-to-move probe so see()/best_see_capture score OUR captures. Err bails on
    //      our-king-in-check and on a check-giving mv (documented FN).
    let after_us = position_for_analysis_side(&after, us).ok()?;

    let mut best: Option<XRayOpportunity> = None;
    let mut front_bb = atk & enemy_occ;
    while front_bb != 0 {
        let f_sq = front_bb.trailing_zeros() as u8;
        front_bb &= front_bb - 1;
        let (_, f_piece) = match after.piece_at(f_sq) {
            Some(x) => x,
            None => continue,
        };

        // (G4b) A king FRONT is always a skewer (VALUE[King]=0 satisfies the value
        //       test below), never an x-ray. Drop it here.
        if f_piece == Piece::King {
            continue;
        }

        // (G3) F must be DEFENDED — the whole point is counting THROUGH the defender.
        //      An undefended F is a plain hanging capture, not an x-ray.
        let f_defenders = attackers_of(&after.pieces, f_sq, enemy, after.all) & !(1u64 << f_sq);
        if f_defenders == 0 {
            continue;
        }

        // The rear enemy piece newly seen once F is removed (the pin/skewer ray probe).
        let occ2 = after.all & !(1u64 << f_sq);
        let atk2 = match slider_attacks(xrayer_piece, s, occ2) {
            Some(a) => a,
            None => continue,
        };
        let behind = atk2 & !atk & enemy_occ; // <= 1 relevant bit
        if behind == 0 {
            continue;
        }
        let b_sq = behind.trailing_zeros() as u8;
        let (_, b_piece) = match after.piece_at(b_sq) {
            Some(x) => x,
            None => continue,
        };

        // (G4) PARTITION — disjoint from pin and skewer:
        //   pin    => b_piece == King        (rear is the king)
        //   skewer => VALUE[f] >  VALUE[b]    (front forced to flee a dearer attacker)
        //   x-ray  => b_piece != King AND VALUE[f] <= VALUE[b]  (front NOT forced)
        if b_piece == Piece::King {
            continue; // pin geometry
        }
        if VALUE[f_piece.index()] > VALUE[b_piece.index()] {
            continue; // skewer geometry
        }

        // (G5) CAUSALITY — F must NOT already be winnable by a naive one-square count on
        //      the PRE-MOVE board. The x-ray alignment created by mv is what wins it.
        if best_see_capture(pos, f_sq) > 0 {
            continue;
        }

        // (G6) COUNTING PROOF — the full SEE swap on f_sq from OUR side. see() reveals the
        //      rear same-line slider through the shrinking occupancy, so a defended F that
        //      is not actually winnable through the x-ray returns <= 0 and is dropped.
        let gain = see(&after_us, s, f_sq);
        if gain <= 0 {
            continue;
        }

        // (G7) LEGALITY — see() ignores pins, so a PINNED xrayer on s (its move to s exposed
        //      its own king to an enemy slider through s) passes the counting proof while being
        //      unable to ever capture f_sq. Require a legal capture s -> f_sq on the us-to-move
        //      probe (generate_legal respects the pin). Fixes the pinned-xrayer false positive
        //      found by oracle fuzzing (2 FPs / 1303 fired).
        if !generate_legal(&mut after_us.clone())
            .into_iter()
            .any(|m| m.from == s && m.to == f_sq)
        {
            continue;
        }

        let ray: Vec<String> = squares_between(s, b_sq).into_iter().map(square_name).collect();
        let cand = XRayOpportunity {
            kind: "xray_attack".to_string(),
            validator: "xray_attack_validation".to_string(),
            move_uci: mv.to_uci(),
            xrayer: piece_ref(us, xrayer_piece, s),
            front: piece_ref(enemy, f_piece, f_sq),
            back: piece_ref(enemy, b_piece, b_sq),
            ray,
            gives_check: gives_check_flag,
            material_gain: gain,
        };
        // Highest-gain front for this move; ties -> first seen (deterministic).
        match &best {
            Some(b) if b.material_gain >= cand.material_gain => {}
            _ => best = Some(cand),
        }
    }
    best
}

// ── X-ray defense (defend a friendly piece THROUGH an enemy piece) ────────────
// Mirror of xray_attack: xray_attack counts THROUGH an enemy front F to ATTACK a
// rear enemy B; xray_defense places/uses a friendly slider whose defense of a
// friendly piece G passes THROUGH an enemy piece E on the same line, so if the
// enemy takes G our slider recaptures down the now-cleared line even though a naive
// one-square defender count (blocked by E) calls G undefended. No worst-case loop:
// the proof is a pure exchange on ONE square (g_sq) that see() minimaxes; the
// through-defense is revealed by see()'s shrinking occupancy (the attackers_of
// blocker-awareness makes the naive count miss it — that gap is the motif).

/// Validated x-ray-defense opportunities for the side to move, sorted by move then front id.
pub fn xray_defense_opportunities(pos: &Position) -> FactCollection<XRayDefenseOpportunity> {
    let defender_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(x) = xray_defense_after_move(pos, mv, defender_color) {
            out.push(x);
        }
    }
    // Determinism: move, then front-enemy id (mirror xray_attack's sort key).
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.front_enemy.id.cmp(&b.front_enemy.id))
    });
    FactCollection::computed(out)
}

/// Validated x-ray defenses for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn xray_defense_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<XRayDefenseOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => xray_defense_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

/// Clone `pos` with the single bit `sq` cleared from the owning piece set, the owner's
/// per-color occupancy, and `all` together — the sanctioned dual/triple-clear from the
/// overload D-removed probe. stm is left unchanged (no `make()`, so no turn flip), so
/// `see`/`best_see_capture` still score the intended side. `ep` is cleared to keep the
/// three occupancy views consistent for movegen. Returns `None` if `sq` is empty.
fn without_bit(pos: &Position, sq: u8) -> Option<Position> {
    let (color, piece) = pos.piece_at(sq)?;
    let bit = 1u64 << sq;
    let mut probe = pos.clone();
    probe.pieces[color.index()][piece.index()] &= !bit;
    probe.occ[color.index()] &= !bit;
    probe.all &= !bit;
    probe.ep = None;
    Some(probe)
}

fn xray_defense_after_move(
    pos: &Position,
    mv: Move,
    us: Color,
) -> Option<XRayDefenseOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let xrayer_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // (G0) Only sliders x-ray.
    slider_attacks(xrayer_piece, mv.to, 0)?;

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let enemy = us.flip();
    let s = mv.to;

    // (Ga) Our slider must not simply hang on s (mirror xray_attack G1).
    if forker_capturable_for_gain(&mut after.clone(), s) {
        return None;
    }

    let atk = slider_attacks(xrayer_piece, s, after.all)?; // rays with E still present
    let our_occ = enemy_occupancy(&after, us); // color-generic union of `us`'s pieces
    let enemy_occ = enemy_occupancy(&after, enemy);

    // Enemy-to-move probe: SEE on g_sq scores the ENEMY's capture attempt (their turn).
    // Err bails on our-king-in-check and check-giving mv (documented FN, same as G2).
    let enemy_after = position_for_analysis_side(&after, enemy).ok()?;

    let mut best: Option<XRayDefenseOpportunity> = None;
    let mut front_bb = atk & enemy_occ; // front blockers E are ENEMY (Gc)
    while front_bb != 0 {
        let e_sq = front_bb.trailing_zeros() as u8;
        front_bb &= front_bb - 1;
        let (_, e_piece) = match after.piece_at(e_sq) {
            Some(x) => x,
            None => continue,
        };

        // The friendly piece G newly seen once E is removed (pin/skewer/xray ray probe).
        let occ2 = after.all & !(1u64 << e_sq);
        let atk2 = match slider_attacks(xrayer_piece, s, occ2) {
            Some(a) => a,
            None => continue,
        };
        let behind = atk2 & !atk & our_occ; // <= 1 bit, OUR occupancy this time
        if behind == 0 {
            continue;
        }
        let g_sq = behind.trailing_zeros() as u8;
        let (_, g_piece) = match after.piece_at(g_sq) {
            Some(x) => x,
            None => continue,
        };

        // (Gb) King exclusion: a friendly king "behind" is not a defended target.
        if g_piece == Piece::King {
            continue;
        }
        // (Gg) Pawn G excluded (output flooding / promotion subtleties), mirror overload.
        if g_piece == Piece::Pawn {
            continue;
        }

        // (i) TRIGGER — G must actually be under enemy attack (else nothing to defend).
        //     Blocker-aware: with E present this counts the enemy's real pressure on G.
        if attackers_of(&after.pieces, g_sq, enemy, after.all) == 0 {
            continue;
        }

        // (ii) COUNTING PROOF — the full SEE swap on g_sq from the ENEMY side. see()
        //      reveals OUR rear same-line xrayer through the shrinking occupancy, so a
        //      G that IS actually held returns "enemy cannot profit" <= 0.
        let held = best_see_capture(&enemy_after, g_sq);
        if held > 0 {
            continue; // G is already lost anyway; not a save
        }

        // (iii) DISTINCTNESS / CAUSALITY / LOAD-BEARING — remove the xrayer's bit and
        //       re-run the enemy SEE on g_sq. If G now flips to LOST (>0) while the full
        //       board holds it (<=0), the x-ray defense is the sole thing saving G.
        //       Mirrors xray_attack's G5 causality gate (best_see_capture flip).
        let without_xrayer = match without_bit(&enemy_after, s) {
            Some(p) => p,
            None => continue,
        };
        let without = best_see_capture(&without_xrayer, g_sq);
        if without <= 0 {
            continue; // some OTHER defender holds G → plain defense, not the x-ray
        }

        // (Gd/G7) LEGALITY — see() ignores pins; require that our xrayer can legally reach
        //         g_sq once the enemy has taken G. The recapture happens on the resolved
        //         board where E has left its intermediate square and G is gone (E now sits
        //         on g_sq), so probe s -> g_sq on the us-to-move board with BOTH E and G
        //         removed: the line is clear and generate_legal (which respects pins)
        //         yields the move iff the xrayer is not pinned off this ray. Mirror of
        //         xray_attack G7's generate_legal pin guard. Removing E and G along THIS
        //         line cannot change pins on any OTHER line, so the test is sound.
        let recap_board = position_for_analysis_side(&after, us).ok()?;
        let recap_board = match without_bit(&recap_board, e_sq) {
            Some(p) => p,
            None => continue,
        };
        let recap_board = match without_bit(&recap_board, g_sq) {
            Some(p) => p,
            None => continue,
        };
        if !generate_legal(&mut recap_board.clone())
            .into_iter()
            .any(|m| m.from == s && m.to == g_sq)
        {
            continue;
        }

        // material_gain = value of G saved = what the enemy would have won without the
        // xray. `without` is exactly that SEE delta; report it (>0 by construction).
        let ray: Vec<String> = squares_between(s, g_sq).into_iter().map(square_name).collect();
        let cand = XRayDefenseOpportunity {
            kind: "xray_defense".to_string(),
            validator: "xray_defense_validation".to_string(),
            move_uci: mv.to_uci(),
            xrayer: piece_ref(us, xrayer_piece, s),
            front_enemy: piece_ref(enemy, e_piece, e_sq),
            defended: piece_ref(us, g_piece, g_sq),
            ray,
            gives_check: gives_check_flag,
            material_gain: without,
        };
        // Keep highest-saved; ties -> first seen (deterministic), mirror xray_attack.
        match &best {
            Some(b) if b.material_gain >= cand.material_gain => {}
            _ => best = Some(cand),
        }
    }
    best
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

// ── Discovered attacks ───────────────────────────────────────────────────────
// A discovered attack inverts the skewer/pin geometry: the ATTACKING piece is a
// stationary friendly rear slider S, and the MOVING piece merely vacates the only
// blocker on S's ray to an enemy target. After the move S newly attacks the target.
// Sub-types: discovered-check (S now checks the enemy king), double-check (S and the
// moved piece both check). Detected with the proven `after & !before` ray
// recomputation: removing the mover from its square (post-move occupancy) opens
// exactly S's blocked ray, and slider_attacks' blocker-aware resolution finds the
// newly-seen target for free — no slope arithmetic, no hand-rolled "only blocker".
// In a legal position the enemy king is not attacked pre-move, so the only ray that
// can change is the vacated one, and the unveiled enemy set holds at most one bit.

/// Validated discovered-attack opportunities for the side to move, sorted by move
/// then unveiled-target id.
pub fn discovery_opportunities(pos: &Position) -> FactCollection<DiscoveryOpportunity> {
    let discoverer_color = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(d) = discovery_after_move(pos, mv, discoverer_color) {
            out.push(d);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.target.id.cmp(&b.target.id))
    });
    FactCollection::computed(out)
}

/// Validated discoveries for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn discovery_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<DiscoveryOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => discovery_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn discovery_after_move(
    pos: &Position,
    mv: Move,
    discoverer_color: Color,
) -> Option<DiscoveryOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let enemy = discoverer_color.flip();

    // gives_check on its own throwaway clone (make() mutates).
    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv);
    let enemy_occ = enemy_occupancy(&after, enemy);
    let enemy_king = after.king_sq(enemy);
    let king_bit = 1u64 << enemy_king;
    let from_bit = 1u64 << mv.from;
    let to_bit = 1u64 << mv.to;

    // Every friendly rear slider, on the post-move board, excluding the mover itself.
    for s_piece in [Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut bb = after.pieces[discoverer_color.index()][s_piece.index()];
        while bb != 0 {
            let s_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if s_sq == mv.to {
                continue; // the mover (even a slider promotion) is never its own rear slider
            }
            let before_atk = match slider_attacks(s_piece, s_sq, pos.all) {
                Some(a) => a,
                None => continue,
            };
            // S must have had mv.from as its nearest blocker on this ray, so vacating
            // it is what opens the line (slider_attacks stops at the first occupant).
            if before_atk & from_bit == 0 {
                continue;
            }
            let after_atk = match slider_attacks(s_piece, s_sq, after.all) {
                Some(a) => a,
                None => continue,
            };
            // Newly-seen enemy squares. Removing mv.from opens only S's mv.from ray and
            // slider_attacks stops at the first occupant past it, so this is ≤1 bit: the
            // unveiled target. A mover that slid ALONG the ray re-blocks it in after.all,
            // so the target is absent here — this recomputation IS the soundness guard.
            let unveiled = after_atk & !before_atk & enemy_occ;
            if unveiled == 0 {
                continue;
            }
            let t_sq = unveiled.trailing_zeros() as u8;
            let (_, t_piece) = after.piece_at(t_sq)?;
            let s_value = VALUE[s_piece.index()];
            let king_target = t_piece == Piece::King;

            // The unveiled slider checks the king iff it now reaches the king's square
            // (it could not pre-move — legal positions never leave the side-not-to-move
            // in check). The moved piece double-checks iff it also attacks the king.
            let discovered_check = after_atk & king_bit != 0;
            let moved_gives_check = attackers_of(&after.pieces, enemy_king, discoverer_color, after.all)
                & to_bit
                != 0;
            let double_check = discovered_check && moved_gives_check;

            // The moved piece must not be simply hung. For a discovered check the enemy
            // is in check, so generate_legal only surfaces check-resolving captures —
            // capturing the (non-checking) mover is illegal, so this is a no-op there
            // and only bites a plain discovered attack whose mover hangs.
            if forker_capturable_for_gain(&mut after.clone(), mv.to) {
                continue;
            }
            let mover_threat = mover_threat_gain(&after, discoverer_color, enemy, mv.to);

            let material_gain = if king_target || discovered_check {
                // Forcing: the enemy must answer the check; the discovery's value is the
                // moved piece's simultaneous threat (the second prong of the discovery).
                mover_threat
            } else {
                // Plain discovered attack: the unveiled target must be winnable, and the
                // rear slider must not be TRADEABLE. If the opponent can capture the slider
                // with a non-losing exchange (SEE >= 0) they neutralize the whole threat at
                // no cost — e.g. the target counter-captures the slider down the now-open
                // line for an even trade — so the discovery wins nothing even when the
                // slider looks "defended" (the recapture only completes the trade).
                let undefended = is_undefended(&after, t_sq, enemy);
                let winnable = undefended || VALUE[t_piece.index()] > s_value;
                if !winnable || slider_tradeable(&mut after.clone(), s_sq) {
                    continue;
                }
                // LEGALITY: attackers_of / see() are pin-blind — an ABSOLUTELY PINNED unveiled
                // slider attacks t geometrically but can never legally capture it (fuzz-found
                // false positive, 45/19671 discovered_attack ops). capture_legal_wrt_pin is pure
                // king-ray geometry, so it also covers discoverer_checks (whose us-to-move probe
                // is unavailable because the move gave check).
                if !capture_legal_wrt_pin(&after, s_sq, t_sq, discoverer_color) {
                    continue;
                }
                if undefended {
                    VALUE[t_piece.index()]
                } else {
                    VALUE[t_piece.index()] - s_value
                }
            };

            let ray: Vec<String> = squares_between(s_sq, t_sq).into_iter().map(square_name).collect();
            let subtype = if double_check {
                "double_check"
            } else if discovered_check {
                "discovered_check"
            } else if moved_gives_check {
                // The MOVING piece gives check while the unveiled slider wins a non-king
                // target — the "discoverer checks" motif (distinct from discovered_check,
                // where the UNVEILED piece checks). The mover's check is the forcing element:
                // the enemy must answer it, so it cannot freely defend the unveiled target.
                // Material is the unveiled slider's winnable gain (the else branch below), the
                // same standard as a plain discovered attack — the check only makes it harder
                // for the enemy to parry, never easier.
                "discoverer_checks"
            } else {
                "discovered_attack"
            };
            let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);

            return Some(DiscoveryOpportunity {
                kind: subtype.to_string(),
                validator: "discovery_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(discoverer_color, mover_piece, mv.to),
                slider: piece_ref(discoverer_color, s_piece, s_sq),
                target: piece_ref(enemy, t_piece, t_sq),
                ray,
                gives_check: gives_check_flag,
                discovered_check,
                double_check,
                mover_threatens: mover_threat > 0,
                material_gain,
            });
        }
    }
    None
}

// ── Discovered defense ───────────────────────────────────────────────────────
// The exact defensive mirror of `discovery_after_move`: a stationary friendly rear
// slider S and a MOVING piece that merely vacates the only blocker on S's ray toward
// a FRIENDLY target G. After the move S newly DEFENDS G, which was hanging before the
// move. We reuse discovery's proven `after & !before & our_occ` ray recomputation
// (pointed at our own occupancy) and swap the "attack" verdict for a "rescue" verdict,
// proven with SEE from the enemy's perspective: G was losing material before (enemy's
// best capture SEE > 0) and is no longer profitable to take after S is unveiled.

/// Every friendly piece on the board (mirror of `enemy_occupancy`, us-side).
fn friendly_occupancy(pos: &Position, us: Color) -> u64 {
    let mut bb = 0u64;
    for piece in Piece::ALL {
        bb |= pos.pieces[us.index()][piece.index()];
    }
    bb
}

/// Validated discovered-defense opportunities for the side to move, sorted by move
/// then defended-piece id.
pub fn discovered_defense_opportunities(
    pos: &Position,
) -> FactCollection<DiscoveredDefenseOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(d) = discovered_defense_after_move(pos, mv, us) {
            out.push(d);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.defended_piece.id.cmp(&b.defended_piece.id))
    });
    FactCollection::computed(out)
}

/// Validated discovered defenses for a requested side. See `motif_opportunities_for`
/// for the counterfactual side-to-move semantics.
pub fn discovered_defense_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<DiscoveredDefenseOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => discovered_defense_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

/// Material the side to move wins on `sq` with LEGAL play only — a legality-aware SEE. Unlike
/// `best_see_capture`, `generate_legal` respects absolute pins (a pinned "defender" is absent)
/// AND the alternating recursion honours the standard SEE stop rule, so a defender too valuable
/// to be the last recapturer (e.g. a queen behind pawns) is correctly not forced to recapture.
/// Both blind spots produced fuzz-found discovered-defense false positives.
fn legal_capture_gain(pos: &Position, sq: u8, depth: u8) -> i32 {
    if depth == 0 {
        return 0;
    }
    let mut gen = pos.clone();
    let cheapest = generate_legal(&mut gen)
        .into_iter()
        .filter(|m| m.to == sq)
        .min_by_key(|m| pos.piece_at(m.from).map(|(_, p)| VALUE[p.index()]).unwrap_or(0));
    let Some(cap) = cheapest else {
        return 0;
    };
    let victim = pos.piece_at(sq).map(|(_, p)| VALUE[p.index()]).unwrap_or(0);
    let mut after = pos.clone();
    after.make(cap);
    (victim - legal_capture_gain(&after, sq, depth - 1)).max(0)
}

/// Best FULL-BOARD material swing for the side to move via legal captures and promotions —
/// an alternating capture quiescence with stand-pat, built on `generate_legal` (absolute pins
/// respected). Unlike single-square SEE this sees off-square collateral: an in-between grab of
/// a hanging queen, a promotion, a counter-capture elsewhere. Captures/promotions only, so it
/// UNDER-estimates sides with quiet threats — the conservative direction when used to debit a
/// material claim (desperado). Depth bounds the alternation; each level clones + regenerates,
/// so keep call sites rare (gated behind cheap filters).
fn legal_material_quiescence(pos: &Position, depth: u8) -> i32 {
    if depth == 0 {
        return 0;
    }
    let mut gen = pos.clone();
    let mut best = 0; // stand pat
    for m in generate_legal(&mut gen) {
        let mut swing = match pos.piece_at(m.to) {
            Some((_, p)) => SEE_VALUE[p.index()],
            None => 0,
        };
        if let Some(promo) = m.flag.promo_piece() {
            swing += SEE_VALUE[promo.index()] - SEE_VALUE[Piece::Pawn.index()];
        }
        if swing == 0 {
            continue; // quiet non-promotion — outside the quiescence
        }
        let mut after = pos.clone();
        after.make(m);
        let score = swing - legal_material_quiescence(&after, depth - 1);
        if score > best {
            best = score;
        }
    }
    best
}

fn discovered_defense_after_move(
    pos: &Position,
    mv: Move,
    us: Color,
) -> Option<DiscoveredDefenseOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();

    // gives_check on a throwaway pre-move clone (make() mutates) — same as discovery.
    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone(); // after.stm == enemy (make flips the turn)
    after.make(mv);
    let our_occ_after = friendly_occupancy(&after, us);
    let from_bit = 1u64 << mv.from;

    // Enemy-to-move view of the ORIGINAL board, for the causality/hanging probe. Built
    // once, reused per rescued square. In a legal position the enemy cannot be the side
    // that gives check while it is our turn, so this only fails on the routed-parity edge.
    let enemy_pre = position_for_analysis_side(pos, enemy).ok()?;
    // Enemy-to-move view of the AFTER board. after.stm is ALREADY enemy, so this is the
    // routed identity path of position_for_analysis_side (clone, stm already == enemy, no
    // ep wipe, no in-check gate) — a proven no-op probe, not a turn grant.
    let enemy_after = position_for_analysis_side(&after, enemy).ok()?;

    // Every friendly rear slider on the POST-move board, excluding the mover's square.
    for s_piece in [Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut bb = after.pieces[us.index()][s_piece.index()];
        while bb != 0 {
            let s_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if s_sq == mv.to {
                continue; // the mover (even a slider promotion) is never its own rear slider
            }
            let before_atk = match slider_attacks(s_piece, s_sq, pos.all) {
                Some(a) => a,
                None => continue,
            };
            // mv.from must have been S's nearest blocker on this ray, so vacating it is
            // exactly what opens the line (slider_attacks stops at the first occupant).
            if before_atk & from_bit == 0 {
                continue;
            }
            let after_atk = match slider_attacks(s_piece, s_sq, after.all) {
                Some(a) => a,
                None => continue,
            };
            // Newly-DEFENDED friendly square(s). ≤1 relevant bit by discovery's one-bit
            // argument (removing mv.from opens only S's mv.from ray; slider_attacks stops at
            // the first occupant past it). A mover that slid ALONG S's ray re-blocks it in
            // after.all, so G is absent here — this recomputation IS the soundness guard.
            let unveiled_friendly = after_atk & !before_atk & our_occ_after;
            if unveiled_friendly == 0 {
                continue;
            }
            let g_sq = unveiled_friendly.trailing_zeros() as u8;

            // FP guard (b): the rescued square is neither the mover's landing nor the slider.
            if g_sq == mv.to || g_sq == s_sq {
                continue;
            }

            let (_, g_piece) = after.piece_at(g_sq)?;
            // FP guard (d): a king "hanging" is check, a different fact (hazards), not defense.
            if g_piece == Piece::King {
                continue;
            }

            // TRIGGER — G must have NEEDED the defense: hanging BEFORE the discovery, from the
            // enemy's perspective, on the ORIGINAL board. best_see_capture gives the reported
            // material estimate (refined SEE values, consistent with the other detectors)…
            let loss_before = best_see_capture(&enemy_pre, g_sq);
            if loss_before <= 0 {
                continue; // G was NOT losing → nothing to rescue
            }
            // …and it must be a LEGAL loss (best_see_capture ignores pins, so it can over-report
            // a hang whose only attacker is itself pinned — we must not claim to "rescue" an
            // already-safe piece).
            if legal_capture_gain(&enemy_pre, g_sq, 8) <= 0 {
                continue;
            }

            // PROOF the discovery rescues it, LEGALITY-AWARE. best_see_capture ignores absolute
            // pins (a pinned unveiled slider fakes a defender) and can force a too-valuable last
            // recapturer; both produced fuzz-found false positives (e.g. a queen unveiled behind
            // pawns onto a square with more attackers than safe defenders). legal_capture_gain
            // replays the swap with generate_legal (pins respected, standard SEE stop rule), so
            // the enemy's TRUE legal win on G must be ≤ 0 for the rescue to hold.
            if legal_capture_gain(&enemy_after, g_sq, 8) > 0 {
                continue; // still legally winnable → not actually rescued
            }

            // FP guard (a): the mover must not create a NEW hang on mv.to, else the "rescue"
            // is illusory (we traded one hang for another). forker_capturable_for_gain needs
            // stm == enemy, which `after` already is.
            if forker_capturable_for_gain(&mut after.clone(), mv.to) {
                continue;
            }

            let ray: Vec<String> = squares_between(s_sq, g_sq)
                .into_iter()
                .map(square_name)
                .collect();

            return Some(DiscoveredDefenseOpportunity {
                kind: "discovered_defense".to_string(),
                validator: "discovered_defense_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(us, mover_piece, mv.to),
                slider: piece_ref(us, s_piece, s_sq),
                defended_piece: piece_ref(us, g_piece, g_sq),
                ray,
                gives_check: gives_check_flag,
                material_gain: loss_before,
            });
        }
    }
    None
}

/// True if the opponent (the side to move in `after`) can capture the piece on `sq`
/// with a non-losing exchange (SEE >= 0) — i.e. trade it off. A discovered attack whose
/// rear slider can be traded for free is neutralized and wins nothing, even when the
/// slider is "defended" (the recapture only completes the trade).
fn slider_tradeable(after: &mut Position, sq: u8) -> bool {
    let legal = generate_legal(after);
    for mv in legal {
        if mv.to == sq && mv.flag.is_capture() && see(after, mv.from, mv.to) >= 0 {
            return true;
        }
    }
    false
}

/// Best single enemy piece the moved piece now threatens to win from `from_sq`
/// (fork-style: attacked AND winnable). 0 if it lands harmlessly. The enemy king is
/// not counted — it is a check, not a material win.
fn mover_threat_gain(after: &Position, us: Color, enemy: Color, from_sq: u8) -> i32 {
    let mover_piece = match after.piece_at(from_sq) {
        Some((_, p)) => p,
        None => return 0,
    };
    let mover_value = VALUE[mover_piece.index()];
    let bit = 1u64 << from_sq;
    let mut best = 0;
    for piece in Piece::ALL {
        if piece == Piece::King {
            continue;
        }
        let mut bb = after.pieces[enemy.index()][piece.index()];
        while bb != 0 {
            let sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if attackers_of(&after.pieces, sq, us, after.all) & bit == 0 {
                continue;
            }
            if VALUE[piece.index()] > mover_value || is_undefended(after, sq, enemy) {
                best = best.max(VALUE[piece.index()]);
            }
        }
    }
    best
}

// ── Removing the guard (capturing the defender) ──────────────────────────────
// A move that CAPTURES an enemy piece D guarding another enemy piece P, so that with
// D gone P becomes winnable. Unlike the slider motifs, soundness rests entirely on
// SEE: P must NOT be winnable BEFORE the capture (so removing D is the cause) and
// must be winnable AFTER (which automatically accounts for any OTHER remaining
// defender — SEE counts them). Because see() scores for the side to move, the "after"
// win is measured on a counterfactual us-to-move probe (the move just made flips the
// turn to the enemy). Capturing variant only; non-capturing deflection/luring is a
// separate follow-up motif, and captures that give check are skipped (the enemy must
// answer the check first, so the probe is unavailable) — a documented false-negative.

/// Validated capturing-the-defender opportunities for the side to move.
pub fn remove_guard_opportunities(pos: &Position) -> FactCollection<RemoveGuardOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = remove_guard_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.target.id.cmp(&b.target.id))
    });
    FactCollection::computed(out)
}

/// Validated capturing-the-defender for a requested side. See `motif_opportunities_for`
/// for the counterfactual side-to-move semantics.
pub fn remove_guard_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<RemoveGuardOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => remove_guard_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn remove_guard_after_move(pos: &Position, mv: Move, us: Color) -> Option<RemoveGuardOpportunity> {
    if !mv.flag.is_capture() {
        return None; // capturing variant only
    }
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();

    // The captured defender D. For en passant the captured pawn sits behind mv.to.
    let d_sq = if matches!(mv.flag, MoveFlag::EnPassant) {
        if us == Color::White {
            mv.to - 8
        } else {
            mv.to + 8
        }
    } else {
        mv.to
    };
    let (d_color, d_piece) = pos.piece_at(d_sq)?;
    if d_color != enemy || d_piece == Piece::King {
        return None;
    }

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv);

    // Our capturing piece must not be simply lost (after.stm == enemy here).
    if forker_capturable_for_gain(&mut after.clone(), mv.to) {
        return None;
    }

    // Counterfactual "us to move, D gone" probe so see() scores OUR capture of P.
    // Unavailable (Err) when the capture gave check — the enemy must answer first.
    let probe = position_for_analysis_side(&after, us).ok()?;

    let d_bit = 1u64 << d_sq;
    let mut best: Option<RemoveGuardOpportunity> = None;
    for piece in Piece::ALL {
        if piece == Piece::King {
            continue; // the king is a check/mate target, never a material win
        }
        let mut bb = after.pieces[enemy.index()][piece.index()];
        while bb != 0 {
            let p_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if p_sq == d_sq {
                continue;
            }
            // D must have been a defender of P on the BEFORE board.
            let before_defs = attackers_of(&pos.pieces, p_sq, enemy, pos.all) & !(1u64 << p_sq);
            if before_defs & d_bit == 0 {
                continue;
            }
            // The win must be CAUSED by removing D: P must not already be winnable.
            if best_see_capture(pos, p_sq) > 0 {
                continue;
            }
            // P must be winnable once D is gone (SEE accounts for any other defender).
            let gain = best_see_capture(&probe, p_sq);
            if gain <= 0 {
                continue;
            }
            let cand = RemoveGuardOpportunity {
                kind: "capture_the_defender".to_string(),
                validator: "remove_guard_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(us, mover_piece, mv.to),
                captured_defender: piece_ref(enemy, d_piece, d_sq),
                target: piece_ref(enemy, piece, p_sq),
                gives_check: gives_check_flag,
                material_gain: gain,
            };
            // Keep the highest-gain target; ties resolve to the first (deterministic).
            match &best {
                Some(b) if b.material_gain >= cand.material_gain => {}
                _ => best = Some(cand),
            }
        }
    }
    best
}

/// Best SEE over all legal captures of `target` for `pos`'s side to move. 0 if none.
fn best_see_capture(pos: &Position, target: u8) -> i32 {
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut best = 0;
    for mv in legal {
        if mv.to == target && mv.flag.is_capture() {
            let s = see(&probe, mv.from, mv.to);
            if s > best {
                best = s;
            }
        }
    }
    best
}

// ── Overloaded defender ──────────────────────────────────────────────────────
// A STATE detector (not move-based): an enemy piece D that is the critical defender of
// TWO OR MORE of its own pieces, each winnable for us once D is removed but not while D
// guards it. D cannot guard both, so we win material. Mirrors `trapped_pieces`; the
// D-removed probe clears D's bit and keeps stm == us (no `make()`, so no turn flip), so
// `see`/`best_see_capture` still score OUR captures. King excluded as defender and as
// target; pawns excluded as targets (output flooding / promotion subtleties) but eligible
// as defenders. A shared-attacker guard rejects the case where one of our pieces is the
// sole attacker of both targets — deflecting it onto one leaves nothing to take the other,
// which passes every per-target SEE check yet wins nothing.

/// Validated overloaded enemy defenders for the side to move (the side that exploits).
pub fn overload_opportunities(pos: &Position) -> FactCollection<OverloadOpportunity> {
    let us = pos.stm;
    let enemy = us.flip();
    // We can only begin the combination on our move; if WE are in check we must respond.
    if attackers_of(&pos.pieces, pos.king_sq(us), enemy, pos.all) != 0 {
        return FactCollection::computed(Vec::new());
    }
    let mut out = Vec::new();
    for d_piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        let mut dbb = pos.pieces[enemy.index()][d_piece.index()];
        while dbb != 0 {
            let d_sq = dbb.trailing_zeros() as u8;
            dbb &= dbb - 1;
            if let Some(op) = overload_for_defender(pos, enemy, d_piece, d_sq) {
                out.push(op);
            }
        }
    }
    out.sort_by(|a, b| a.overloaded_defender.id.cmp(&b.overloaded_defender.id));
    FactCollection::computed(out)
}

/// Validated overloaded defenders from a requested side's perspective (the exploiter).
pub fn overload_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<OverloadOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => overload_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn overload_for_defender(
    pos: &Position,
    enemy: Color,
    d_piece: Piece,
    d_sq: u8,
) -> Option<OverloadOpportunity> {
    let d_bit = 1u64 << d_sq;

    // D-removed probe: clear D, keep stm == us (no make(), no turn flip). All THREE
    // occupancy views must stay in sync — `pieces`, per-color `occ`, and `all` — or
    // movegen (which flags captures off `occ[them]`) emits a phantom capture onto D's
    // vacated square and `make` panics "capture on empty".
    let mut probe = pos.clone();
    probe.pieces[enemy.index()][d_piece.index()] &= !d_bit;
    probe.occ[enemy.index()] &= !d_bit;
    probe.all &= !d_bit;
    probe.ep = None;

    // Charges D critically guards: (piece, sq, gain_if_gone, winning-capturer from-sq).
    let mut charges: Vec<(Piece, u8, i32, u8)> = Vec::new();
    for p_piece in [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut pbb = pos.pieces[enemy.index()][p_piece.index()];
        while pbb != 0 {
            let p_sq = pbb.trailing_zeros() as u8;
            pbb &= pbb - 1;
            if p_sq == d_sq {
                continue;
            }
            // (a) D is a direct current defender of P on the live board.
            let before_defs = attackers_of(&pos.pieces, p_sq, enemy, pos.all) & !(1u64 << p_sq);
            if before_defs & d_bit == 0 {
                continue;
            }
            // (b) P is not already winnable — the win must be CAUSED by removing D.
            if best_see_capture(pos, p_sq) > 0 {
                continue;
            }
            // (c) P is winnable once D is gone (SEE counts every other remaining defender).
            let gain = best_see_capture(&probe, p_sq);
            if gain <= 0 {
                continue;
            }
            let from = best_capture_from(&probe, p_sq)?;
            charges.push((p_piece, p_sq, gain, from));
        }
    }
    if charges.len() < 2 {
        return None;
    }
    // Shared-attacker guard: the exploitation deflects one of our pieces onto a target (D
    // recaptures it); a DIFFERENT piece must remain to take the other. Require >=2 charges
    // whose winning captures use distinct pieces — else the single shared attacker is
    // consumed deflecting D and nothing wins the second piece.
    let mut froms: Vec<u8> = charges.iter().map(|c| c.3).collect();
    froms.sort_unstable();
    froms.dedup();
    if froms.len() < 2 {
        return None;
    }

    // material_gain: the opponent saves the dearer; we collect the second-best realized
    // SEE (the fork's `vals.get(1)` convention; for exactly two charges, the lesser gain).
    let mut gains: Vec<i32> = charges.iter().map(|c| c.2).collect();
    gains.sort_unstable_by(|a, b| b.cmp(a));
    let material_gain = gains[1];

    let mut targets: Vec<PieceRef> = charges
        .iter()
        .map(|(p, sq, _, _)| piece_ref(enemy, *p, *sq))
        .collect();
    targets.sort_by(|a, b| a.id.cmp(&b.id));

    Some(OverloadOpportunity {
        kind: "overloading".to_string(),
        validator: "overload_validation".to_string(),
        overloaded_defender: piece_ref(enemy, d_piece, d_sq),
        targets,
        material_gain,
    })
}

/// from-square of the best (max-SEE) legal capture of `target` for `pos`'s side to move;
/// `None` if no capture of `target` exists.
fn best_capture_from(pos: &Position, target: u8) -> Option<u8> {
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    legal
        .into_iter()
        .filter(|mv| mv.to == target && mv.flag.is_capture())
        .max_by_key(|mv| see(&probe, mv.from, mv.to))
        .map(|mv| mv.from)
}

// ── Attacking the defender ────────────────────────────────────────────────────
// A MOVE detector (mirrors remove_guard/fork): a move whose moved piece newly attacks
// an enemy piece D that is the SOLE defender of one or more enemy charges P. D must move
// or be lost, and — against EVERY legal enemy reply — we still win material (win the
// relocated/standing D, or a charge the eviction abandons). Disjoint from remove_guard
// (that CAPTURES D). Every board mutation goes through make() (never a hand-edited probe),
// so the pieces/occ/all views stay coherent by construction — do NOT "optimize" this into
// a hand-removed-D probe (that is the trap that panicked the overload detector).
//
// SOUNDNESS depends on the worst-case ranging over ALL enemy replies, not just D's own
// moves: the enemy may ignore D and instead capture our attacker (often with check),
// interpose a new guard, or move a charge to safety. A D-escape-only loop is a false
// positive (e.g. after our Bc4 attacking a rook-guard, the guarded rook plays Rxd1+).

/// Validated attacking-the-defender moves for the side to move, sorted by move then D.
pub fn attack_defender_opportunities(pos: &Position) -> FactCollection<AttackDefenderOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = attack_defender_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.attacked_defender.id.cmp(&b.attacked_defender.id))
    });
    FactCollection::computed(out)
}

/// Validated attacking-the-defender moves from a requested side's perspective.
pub fn attack_defender_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<AttackDefenderOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => attack_defender_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn attack_defender_after_move(pos: &Position, mv: Move, us: Color) -> Option<AttackDefenderOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let mover_bit = 1u64 << mv.to;

    // (A) Our moved piece must not be simply hung on mv.to.
    if forker_capturable_for_gain(&mut after.clone(), mv.to) {
        return None;
    }
    // (B) us-to-move probe (best_see_capture scores OUR captures). Err bails on
    //     our-king-in-check and on mv-gives-check (documented false-negative).
    let after_us = position_for_analysis_side(&after, us).ok()?;
    // (C) enemy-to-move probe for the reply enumeration (== after here; routed for symmetry).
    let enemy_probe = position_for_analysis_side(&after, enemy).ok()?;
    let mut enemy_gen = enemy_probe.clone();
    let enemy_legal = generate_legal(&mut enemy_gen);
    if enemy_legal.is_empty() {
        return None; // stalemate/mate edge — no reply to exploit
    }

    let mut best: Option<AttackDefenderOpportunity> = None;
    for d_piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        let mut dbb = after.pieces[enemy.index()][d_piece.index()];
        while dbb != 0 {
            let d_sq = dbb.trailing_zeros() as u8;
            dbb &= dbb - 1;
            if d_sq == mv.to {
                continue;
            }
            // mv.to must newly attack D on the post-move board.
            if attackers_of(&after.pieces, d_sq, us, after.all) & mover_bit == 0 {
                continue;
            }
            // CAUSALITY: D not already winnable before mv; winnable in place now; and mv.to
            // itself is the winning attacker (not a discovered rear attacker — that is the
            // discovery motif, not this one).
            if best_see_capture(pos, d_sq) > 0 {
                continue;
            }
            let d_in_place = best_see_capture(&after_us, d_sq);
            if d_in_place <= 0 {
                continue;
            }
            if see(&after_us, mv.to, d_sq) <= 0 {
                continue;
            }
            // Charges D is the SOLE defender of (pre-move geometry), not already winnable.
            let charges = attack_defender_charges(pos, enemy, d_sq);
            if charges.is_empty() {
                continue;
            }
            // Worst-case over ALL enemy replies. None if any reply saves D and every charge.
            let Some(worst) =
                attack_defender_worst_case(&enemy_probe, &enemy_legal, enemy, d_sq, &charges)
            else {
                continue;
            };
            debug_assert!(worst > 0);

            let mut targets: Vec<PieceRef> = charges
                .iter()
                .map(|(p, sq)| piece_ref(enemy, *p, *sq))
                .collect();
            targets.sort_by(|a, b| a.id.cmp(&b.id));

            let cand = AttackDefenderOpportunity {
                kind: "attacking_the_defender".to_string(),
                validator: "attack_defender_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(us, mover_piece, mv.to),
                attacked_defender: piece_ref(enemy, d_piece, d_sq),
                targets,
                gives_check: gives_check_flag,
                material_gain: worst,
            };
            // Keep the highest-gain D for this move; ties -> first (deterministic).
            match &best {
                Some(b) if b.material_gain >= cand.material_gain => {}
                _ => best = Some(cand),
            }
        }
    }
    best
}

/// Enemy non-king, non-pawn charges that `d_sq` is the SOLE defender of on `pos`, and that
/// are not already winnable for us (the eviction, not a pre-existing hang, wins them).
fn attack_defender_charges(pos: &Position, enemy: Color, d_sq: u8) -> Vec<(Piece, u8)> {
    let d_bit = 1u64 << d_sq;
    let mut out = Vec::new();
    for p_piece in [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut bb = pos.pieces[enemy.index()][p_piece.index()];
        while bb != 0 {
            let p_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if p_sq == d_sq {
                continue;
            }
            let defenders = attackers_of(&pos.pieces, p_sq, enemy, pos.all) & !(1u64 << p_sq);
            if defenders == d_bit && best_see_capture(pos, p_sq) <= 0 {
                out.push((p_piece, p_sq));
            }
        }
    }
    out
}

/// The least-bad-for-us material outcome after we attack D, taken over EVERY legal enemy
/// reply. `None` if any reply leaves both D safe and every charge defended (no sound win).
fn attack_defender_worst_case(
    enemy_probe: &Position, // stm == enemy
    enemy_legal: &[Move],
    enemy: Color,
    d_sq: u8,
    charges: &[(Piece, u8)],
) -> Option<i32> {
    let mut worst = i32::MAX;
    for m in enemy_legal {
        // Material the enemy GRABS with this reply, debited from our recovery below. A
        // counter-capturing refutation (e.g. the defender itself, Qc6xd5!, or a shot at our
        // mover) must PAY for what it takes — otherwise our single-square recovery on that
        // square is credited as a free gain, blind both to the piece the reply just captured
        // AND to any collateral of the recapture (the fuzz-found deflection false positive:
        // Qxd5! exd5 recovers the queen on d5 but drops the f5 rook, and never debits our
        // captured queen, so the reply is wrongly dropped as non-minimizing).
        let enemy_take = enemy_probe
            .piece_at(m.to)
            .map(|(_, p)| VALUE[p.index()])
            .unwrap_or(0);
        let mut esc = enemy_probe.clone();
        esc.make(*m); // stm flips back to us
        let enemy_occ = esc.occ[enemy.index()];
        // D's square after this reply (the enemy may have moved D itself).
        let d_now = if m.from == d_sq { m.to } else { d_sq };
        let mut our_best = 0;
        if enemy_occ & (1u64 << d_now) != 0 {
            our_best = our_best.max(best_see_capture(&esc, d_now));
        }
        for (_, p_sq) in charges {
            if m.from == *p_sq {
                continue; // the enemy saved this charge by moving it
            }
            if enemy_occ & (1u64 << *p_sq) != 0 {
                our_best = our_best.max(best_see_capture(&esc, *p_sq));
            }
        }
        let our_net = our_best - enemy_take;
        if our_net <= 0 {
            return None; // this reply saves everything (or counter-wins) — not a sound win
        }
        worst = worst.min(our_net);
    }
    if worst > 0 && worst != i32::MAX {
        Some(worst)
    } else {
        None
    }
}

// ── Interference ───────────────────────────────────────────────────────────────
// A MOVE detector (mirrors attack_defender): a move placing a piece on a square S strictly
// between an enemy SLIDER D (bishop/rook/queen) and an enemy target P (non-king, non-pawn)
// that D defends along that ray. The interposition severs D's defense of P. SOUNDNESS is a
// worst-case over ALL enemy replies: the enemy may recapture our interposer on S (RE-OPENING
// the ray → re-defending P), move P to safety, add a defender, or counter-attack. Reject
// unless every reply still leaves us a realizable SEE win on P. Every board mutation goes
// through make() — never a hand-edited probe (the overload occ-desync panic trap). The
// recapture-reopens edge needs no special case: after esc.make(reply) removes our interposer,
// best_see_capture re-evaluates P with D's ray re-opened (attackers_of is blocker-aware).

/// Validated interference moves for the side to move, sorted by move then target.
pub fn interference_opportunities(pos: &Position) -> FactCollection<InterferenceOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = interference_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.target.id.cmp(&b.target.id))
    });
    FactCollection::computed(out)
}

/// Validated interference moves from a requested side's perspective.
pub fn interference_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<InterferenceOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => interference_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn interference_after_move(pos: &Position, mv: Move, us: Color) -> Option<InterferenceOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let interposer_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();
    let s = mv.to;

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy

    // (A) Our interposer must not simply hang on S (to a non-reopening capture).
    if forker_capturable_for_gain(&mut after.clone(), s) {
        return None;
    }
    // (B) us-to-move probe; Err bails on our-king-in-check and on mv-gives-check.
    let after_us = position_for_analysis_side(&after, us).ok()?;
    // (C) enemy-to-move probe + reply list for the worst-case.
    let enemy_probe = position_for_analysis_side(&after, enemy).ok()?;
    let mut enemy_gen = enemy_probe.clone();
    let enemy_legal = generate_legal(&mut enemy_gen);
    if enemy_legal.is_empty() {
        return None;
    }

    let mut best: Option<InterferenceOpportunity> = None;
    for d_piece in [Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut dbb = pos.pieces[enemy.index()][d_piece.index()];
        while dbb != 0 {
            let d_sq = dbb.trailing_zeros() as u8;
            dbb &= dbb - 1;
            let Some(d_ray_before) = slider_attacks(d_piece, d_sq, pos.all) else {
                continue;
            };
            let d_bit = 1u64 << d_sq;
            for p_piece in [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
                let mut pbb = pos.pieces[enemy.index()][p_piece.index()] & d_ray_before;
                while pbb != 0 {
                    let p_sq = pbb.trailing_zeros() as u8;
                    pbb &= pbb - 1;
                    if p_sq == d_sq {
                        continue;
                    }
                    // S strictly between D and P on the (colinear) ray.
                    if !squares_between(d_sq, p_sq).contains(&s) {
                        continue;
                    }
                    // D must be a live defender of P on the pre-move board.
                    let defenders =
                        attackers_of(&pos.pieces, p_sq, enemy, pos.all) & !(1u64 << p_sq);
                    if defenders & d_bit == 0 {
                        continue;
                    }
                    // Causality: P not already winnable before the interposition.
                    if best_see_capture(pos, p_sq) > 0 {
                        continue;
                    }
                    // Severance proof: after the move, D no longer sees P through S.
                    let Some(d_ray_after) = slider_attacks(d_piece, d_sq, after.all) else {
                        continue;
                    };
                    if d_ray_after & (1u64 << p_sq) != 0 {
                        continue;
                    }
                    // P winnable now the ray is cut (SEE counts every other remaining defender).
                    if best_see_capture(&after_us, p_sq) <= 0 {
                        continue;
                    }
                    // Worst-case over ALL enemy replies (recapture-on-S reopen handled for free).
                    let Some(worst) =
                        interference_worst_case(&enemy_probe, &enemy_legal, enemy, p_sq)
                    else {
                        continue;
                    };
                    debug_assert!(worst > 0);

                    let cand = InterferenceOpportunity {
                        kind: "interference".to_string(),
                        validator: "interference_validation".to_string(),
                        move_uci: mv.to_uci(),
                        interposer: piece_ref(us, interposer_piece, s),
                        cut_defender: piece_ref(enemy, d_piece, d_sq),
                        target: piece_ref(enemy, p_piece, p_sq),
                        gives_check: gives_check_flag,
                        material_gain: worst,
                    };
                    match &best {
                        Some(b) if b.material_gain >= cand.material_gain => {}
                        _ => best = Some(cand),
                    }
                }
            }
        }
    }
    best
}

/// Least-bad-for-us material outcome on P after we interpose, over EVERY legal enemy reply.
/// `None` if any reply saves P — including the enemy recapturing our interposer on S, which
/// re-opens D's ray so `best_see_capture` sees P re-defended (blocker-aware) and yields <= 0.
fn interference_worst_case(
    enemy_probe: &Position, // stm == enemy
    enemy_legal: &[Move],
    enemy: Color,
    p_sq: u8,
) -> Option<i32> {
    let mut worst = i32::MAX;
    for m in enemy_legal {
        let mut esc = enemy_probe.clone();
        esc.make(*m); // stm flips back to us
        let p_now = if m.from == p_sq { m.to } else { p_sq };
        let mut our_best = 0;
        if esc.occ[enemy.index()] & (1u64 << p_now) != 0 {
            our_best = best_see_capture(&esc, p_now);
        }
        if our_best <= 0 {
            return None; // this reply saves the target
        }
        worst = worst.min(our_best);
    }
    if worst > 0 && worst != i32::MAX {
        Some(worst)
    } else {
        None
    }
}

// ── Deflection / Distraction (non-capturing removal of the guard) ──────────────
// A MOVE detector (mirrors attack_defender_after_move's skeleton, occupies its DISJOINT
// slice — the gap named at the remove_guard header): a move that does NOT capture the
// enemy defender D yet creates a FORCING threat evicting D from its post, where D is NOT
// profitably capturable in place (best_see_capture(after_us, d_sq) <= 0 — the exact
// COMPLEMENT of attack_defender's see(after_us, mv.to, d_sq) > 0 gate, so the two
// detectors are provably disjoint over the same square and never both emit). D is the
// SOLE guard of >= 1 non-king/non-pawn charge P (attack_defender_charges). Against EVERY
// legal enemy reply we still win material — win the relocated/standing D, or a charge the
// eviction abandons. Every board mutation goes through make() (never a hand-edited probe —
// the overload occ-desync panic trap). SOUNDNESS is attack_defender_worst_case ranging
// over ALL enemy replies (rejects the "enemy ignores D and plays Rxd1+" refutation). The
// eviction is PROVED by the worst-case rather than enumerated: it returns Some only if no
// reply saves D AND every charge — precisely the forcing property deflection needs.

/// Validated deflection/distraction moves for the side to move, sorted by move then defender.
pub fn deflection_opportunities(pos: &Position) -> FactCollection<DeflectionOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = deflection_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.distracted_defender.id.cmp(&b.distracted_defender.id))
    });
    FactCollection::computed(out)
}

/// Validated deflection/distraction moves from a requested side's perspective.
pub fn deflection_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<DeflectionOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => deflection_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn deflection_after_move(pos: &Position, mv: Move, us: Color) -> Option<DeflectionOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy

    // (A) Our moved piece must not be simply hung on mv.to.
    if forker_capturable_for_gain(&mut after.clone(), mv.to) {
        return None;
    }
    // (B) us-to-move probe (best_see_capture scores OUR captures). Err bails on
    //     our-king-in-check and on mv-gives-OUR-check (documented false-negative).
    let after_us = position_for_analysis_side(&after, us).ok()?;
    // (C) enemy-to-move probe + reply list for the worst-case.
    let enemy_probe = position_for_analysis_side(&after, enemy).ok()?;
    let mut enemy_gen = enemy_probe.clone();
    let enemy_legal = generate_legal(&mut enemy_gen);
    if enemy_legal.is_empty() {
        return None; // stalemate/mate edge — no reply to exploit
    }

    let mut best: Option<DeflectionOpportunity> = None;
    for d_piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        let mut dbb = after.pieces[enemy.index()][d_piece.index()];
        while dbb != 0 {
            let d_sq = dbb.trailing_zeros() as u8;
            dbb &= dbb - 1;
            if d_sq == mv.to {
                continue; // D is not the square we moved to (that would be a capture of D).
            }
            // CAUSALITY: D not already winnable before mv (attack_defender_charges enforces
            // the charge half; this enforces the D half).
            if best_see_capture(pos, d_sq) > 0 {
                continue;
            }
            // DEFINING DEFLECTION GATE: D is NOT profitably capturable in place — the exact
            // COMPLEMENT of attack_defender's d_in_place > 0 gate, so this detector is
            // provably DISJOINT from attacking_the_defender (no double-emission).
            if best_see_capture(&after_us, d_sq) > 0 {
                continue;
            }
            // Charges D is the SOLE defender of (pre-move geometry), not already winnable —
            // the shipped helper: non-king/non-pawn, sole-defended, causal.
            let charges = attack_defender_charges(pos, enemy, d_sq);
            if charges.is_empty() {
                continue;
            }
            // FORCING PROOF: mv must actually EVICT D. Rather than enumerate eviction TYPES,
            // PROVE it with the shipped worst-case — None if any enemy reply saves D AND
            // every charge (rejects the counter-capture "Rxd1+" refutation).
            let Some(worst) =
                attack_defender_worst_case(&enemy_probe, &enemy_legal, enemy, d_sq, &charges)
            else {
                continue;
            };
            debug_assert!(worst > 0);

            let mut targets: Vec<PieceRef> = charges
                .iter()
                .map(|(p, sq)| piece_ref(enemy, *p, *sq))
                .collect();
            targets.sort_by(|a, b| a.id.cmp(&b.id));

            let cand = DeflectionOpportunity {
                kind: "deflection".to_string(),
                validator: "deflection_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(us, mover_piece, mv.to),
                distracted_defender: piece_ref(enemy, d_piece, d_sq),
                targets,
                gives_check: gives_check_flag,
                material_gain: worst,
            };
            // Keep the highest-gain D for this move; ties -> first (deterministic).
            match &best {
                Some(b) if b.material_gain >= cand.material_gain => {}
                _ => best = Some(cand),
            }
        }
    }
    best
}

// ── Luring the Defender (Decoy) ────────────────────────────────────────────────
// A MOVE detector (mirrors deflection_after_move's skeleton, occupies the DISJOINT
// sac-to-decoy slice deflection/attack_defender/fork/double_attack all REJECT): the mover
// lands on a square s where it IS profitably capturable (an offered SACRIFICE), and the
// enemy's forced recapture is by a piece D that is the SOLE guard of a charge P. The
// defining gate is forker_capturable_for_gain(after, s) == TRUE — the exact case those four
// detectors bail on (they `return None` there) — so this detector is provably disjoint from
// them and never double-emits. D is additionally required to BE the forced recapturer of s
// (cheapest_legal_capturer_from), otherwise the enemy takes s with a throwaway and keeps
// guarding P. Against EVERY legal enemy reply we still net material AFTER paying the sac:
// attack_defender_worst_case debits enemy_take (our decoy on s) exactly once and, for the
// forced Dxs reply, re-evaluates D on s (d_now == s). Every board mutation goes through
// make() (never a hand-edited probe — the overload occ-desync panic trap).

/// Validated luring-the-defender (decoy) moves for the side to move, sorted by move then
/// lured defender.
pub fn lure_defender_opportunities(pos: &Position) -> FactCollection<LureDefenderOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = lure_defender_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.lured_defender.id.cmp(&b.lured_defender.id))
    });
    FactCollection::computed(out)
}

/// Validated luring-the-defender moves from a requested side's perspective.
pub fn lure_defender_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<LureDefenderOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => lure_defender_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

/// from-square of the CHEAPEST (min piece-value) legal capture of `sq` for `pos`'s side to
/// move; `None` if `sq` has no legal capturer. This is the piece the enemy is FORCED to
/// recapture with — the defender the sac decoys. Ties → lowest from-square (deterministic).
fn cheapest_legal_capturer_from(pos: &Position, sq: u8) -> Option<u8> {
    let mut probe = pos.clone();
    generate_legal(&mut probe)
        .into_iter()
        .filter(|mv| mv.to == sq && mv.flag.is_capture())
        .min_by_key(|mv| {
            (
                pos.piece_at(mv.from)
                    .map(|(_, p)| VALUE[p.index()])
                    .unwrap_or(0),
                mv.from,
            )
        })
        .map(|mv| mv.from)
}

fn lure_defender_after_move(
    pos: &Position,
    mv: Move,
    us: Color,
) -> Option<LureDefenderOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    let enemy = us.flip();

    // King mover exclusion: a king can never be the offered/sacrificed decoy — it is never
    // capturable-for-gain and cannot "land" on a square the enemy wins. Bail early.
    if mover_piece == Piece::King {
        return None;
    }

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let s = mv.to;

    // DEFINING LURE GATE: the mover lands on a square where it IS profitably capturable — an
    // offered sac. This is the EXACT case deflection/attack_defender/fork/double_attack REJECT
    // (they `return None` here), so this detector is provably DISJOINT from all of them.
    if !forker_capturable_for_gain(&mut after.clone(), s) {
        return None; // not a sacrifice → not a lure
    }

    // us-to-move probe (built for symmetry; Err bails on our-king-in-check and on
    // mv-gives-OUR-check — documented false-negative, same as every move detector).
    let _after_us = position_for_analysis_side(&after, us).ok()?;
    // enemy-to-move probe + reply list drive the shipped worst-case AND the forced-recapturer
    // identity. (== after here; routed for symmetry with deflection.)
    let enemy_probe = position_for_analysis_side(&after, enemy).ok()?;
    let mut enemy_gen = enemy_probe.clone();
    let enemy_legal = generate_legal(&mut enemy_gen);
    if enemy_legal.is_empty() {
        return None; // stalemate/mate edge — no reply to exploit
    }

    // Identity of the FORCED recapturer of s: the least-cost enemy piece that legally captures
    // s. If a cheaper NON-defender can take s, the lure of a specific D is not forced — we
    // later require D to BE this piece.
    let recapturer_from = cheapest_legal_capturer_from(&enemy_probe, s)?; // no capturer → bail

    let mut best: Option<LureDefenderOpportunity> = None;
    for d_piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ] {
        let mut dbb = after.pieces[enemy.index()][d_piece.index()];
        while dbb != 0 {
            let d_sq = dbb.trailing_zeros() as u8;
            dbb &= dbb - 1;
            if d_sq == s {
                continue; // D is not the square we sac'd onto.
            }
            // LURE FORCE: D must be THE forced recapturer of s. If a different (cheaper) enemy
            // piece can recapture s, the lure of D is not forced — skip.
            if recapturer_from != d_sq {
                continue;
            }
            // CAUSALITY on D: D not already winnable before mv (the D half; attack_defender_charges
            // enforces the charge half). Mirror deflection.
            if best_see_capture(pos, d_sq) > 0 {
                continue;
            }
            // Charges D is the SOLE defender of (pre-move geometry), not already winnable — the
            // shipped helper: non-king/non-pawn, sole-defended, causal.
            let charges = attack_defender_charges(pos, enemy, d_sq);
            if charges.is_empty() {
                continue;
            }
            // FORCING + NET-MATERIAL PROOF: the shipped all-replies worst-case. It ranges over
            // EVERY enemy reply and DEBITS enemy_take (the piece the reply grabs — here our
            // sacrificed mover on s), returning Some(worst) only if NO reply saves both
            // D-post-recapture AND every charge. That is EXACTLY "D is lured off P and we net
            // material even after paying the sac". None → this line refutes the lure.
            let Some(worst) =
                attack_defender_worst_case(&enemy_probe, &enemy_legal, enemy, d_sq, &charges)
            else {
                continue;
            };
            debug_assert!(worst > 0);

            let mut targets: Vec<PieceRef> = charges
                .iter()
                .map(|(p, sq)| piece_ref(enemy, *p, *sq))
                .collect();
            targets.sort_by(|a, b| a.id.cmp(&b.id));

            let cand = LureDefenderOpportunity {
                kind: "luring_the_defender".to_string(),
                validator: "lure_defender_validation".to_string(),
                move_uci: mv.to_uci(),
                mover: piece_ref(us, mover_piece, s), // the sacrificed decoy, on s
                lured_defender: piece_ref(enemy, d_piece, d_sq),
                targets,
                gives_check: gives_check_flag,
                material_gain: worst,
            };
            // Keep the highest-gain D for this move; ties -> first (deterministic).
            match &best {
                Some(b) if b.material_gain >= cand.material_gain => {}
                _ => best = Some(cand),
            }
        }
    }
    best
}

// ── Trapped pieces ───────────────────────────────────────────────────────────
// A STATE detector (not move-based): an enemy piece that is attacked and has no safe
// escape. The piece belongs to the enemy while it is our move, so the escape scan
// needs the enemy-to-move probe; after the enemy hypothetically moves the piece,
// `make` flips the turn back to us, so SEE on its new square is scored from our side
// (the same double-flip discipline the discovery detector's first cut got wrong).
// King and pawns are excluded (the king is never won; trapped pawns flood output and
// raise promotion/en-passant value subtleties — a documented false-negative).

/// Validated trapped enemy pieces for the side to move (the side that traps).
pub fn trapped_pieces(pos: &Position) -> FactCollection<TrappedPieceOpportunity> {
    let us = pos.stm;
    let enemy = us.flip();
    let mut out = Vec::new();
    for piece in [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut bb = pos.pieces[enemy.index()][piece.index()];
        while bb != 0 {
            let p_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if let Some(op) = trapped_for_piece(pos, us, enemy, piece, p_sq) {
                out.push(op);
            }
        }
    }
    out.sort_by(|a, b| a.piece.id.cmp(&b.piece.id));
    FactCollection::computed(out)
}

/// Validated trapped pieces from a requested side's perspective (the side that traps).
pub fn trapped_pieces_for(pos: &Position, side: Color) -> FactCollection<TrappedPieceOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => trapped_pieces(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn trapped_for_piece(
    pos: &Position,
    us: Color,
    enemy: Color,
    piece: Piece,
    p_sq: u8,
) -> Option<TrappedPieceOpportunity> {
    // (1) The piece must be in danger NOW: attacked, and winnable where it stands.
    if attackers_of(&pos.pieces, p_sq, us, pos.all) == 0 {
        return None;
    }
    let in_place_gain = best_see_capture(pos, p_sq);
    if in_place_gain <= 0 {
        return None; // adequately defended — it can simply sit; not trapped
    }

    // (2) No safe escape. Enumerate the enemy's legal moves of THIS piece. Reasoning
    // about enemy replies is only sound when our own king is not in check.
    let enemy_probe = position_for_analysis_side(pos, enemy).ok()?;
    let mut gen = enemy_probe.clone();
    let enemy_legal = generate_legal(&mut gen);

    let mut worst = in_place_gain;
    let mut escapes = Vec::new();
    for mv in enemy_legal.iter().filter(|m| m.from == p_sq) {
        // After the enemy moves the piece, `make` flips the turn to us, so SEE on the
        // piece's new square is scored from our side (capturing its attacker counts as
        // an escape iff the piece is then safe on the attacker's square).
        let mut after = enemy_probe.clone();
        after.make(*mv);
        let gain = best_see_capture(&after, mv.to);
        if gain <= 0 {
            return None; // a safe destination exists — the piece escapes
        }
        worst = worst.min(gain);
        escapes.push(square_name(mv.to));
    }

    escapes.sort();
    escapes.dedup();
    let mut attackers: Vec<PieceRef> = Vec::new();
    let mut ab = attackers_of(&pos.pieces, p_sq, us, pos.all);
    while ab != 0 {
        let sq = ab.trailing_zeros() as u8;
        ab &= ab - 1;
        if let Some((_, pc)) = pos.piece_at(sq) {
            attackers.push(piece_ref(us, pc, sq));
        }
    }
    attackers.sort_by(|a, b| a.id.cmp(&b.id));

    Some(TrappedPieceOpportunity {
        kind: "trapped_piece".to_string(),
        validator: "trapped_piece_validation".to_string(),
        piece: piece_ref(enemy, piece, p_sq),
        attackers,
        escape_squares_tried: escapes,
        material_gain: worst,
    })
}

// ── Desperado ────────────────────────────────────────────────────────────────
// A STATE detector (mirror of `trapped_pieces`, applied to OUR side): one of OUR
// non-king/non-pawn pieces q is already doomed — attacked now and with no escape
// that saves it (the trapped_pieces predicate, applied to our piece while it is
// our move) — yet q has a legal CAPTURE that snatches enemy material on the way
// out. Since q is lost anyway (best passive salvage = 0), any positive-SEE grab
// strictly beats dying for nothing, so the fact fires with material_gain = the
// best such grab.
//
// Turn discipline: pos.stm == us, so see(pos, q_sq, to) scores OUR grab directly
// (see.rs scores from pos.stm). The doomed proof reasons about ENEMY replies, so
// it uses an enemy-to-move probe (position_for_analysis_side(pos, enemy)); the
// escape scan then double-flips exactly like trapped_for_piece (after.make(m)
// flips stm to enemy, so best_see_capture(&after, m.to) scores the enemy's grab
// of q on its new square). King excluded (never won for material) and pawns
// excluded (promotion/EP value subtleties + output flooding — the documented
// false-negative trapped_pieces also takes).

/// Validated desperado opportunities for the side to move (the side that owns q).
pub fn desperado_opportunities(pos: &Position) -> FactCollection<DesperadoOpportunity> {
    let us = pos.stm;
    let enemy = us.flip();
    let mut out = Vec::new();
    for piece in [Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let mut bb = pos.pieces[us.index()][piece.index()]; // OUR pieces
        while bb != 0 {
            let q_sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if let Some(op) = desperado_for_piece(pos, us, enemy, piece, q_sq) {
                out.push(op);
            }
        }
    }
    // Determinism: piece id (== our-side-<type>-<square>, unique per q) then move.
    out.sort_by(|a, b| a.piece.id.cmp(&b.piece.id).then_with(|| a.move_uci.cmp(&b.move_uci)));
    FactCollection::computed(out)
}

/// Validated desperados from a requested side's perspective. Same counterfactual
/// side-to-move semantics as `trapped_pieces_for`.
pub fn desperado_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<DesperadoOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => desperado_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn desperado_for_piece(
    pos: &Position,
    us: Color,
    enemy: Color,
    piece: Piece,
    q_sq: u8,
) -> Option<DesperadoOpportunity> {
    // IN-CHECK GATE. All enemy-reply reasoning is only sound when our own king is
    // not in check. If we ARE in check, the enemy-to-move probe is Err and we bail
    // (documented false-negative, same as trapped/attack_defender).
    let enemy_probe = position_for_analysis_side(pos, enemy).ok()?;

    // ── STEP 1: DOOMED PROOF (the trapped_pieces predicate, inverted to our q). ──
    // (1a) q must be attacked NOW, and winnable by the enemy where it stands. The
    //      staying-salvage is 0 because the enemy simply takes it: score the enemy's
    //      best capture of q_sq on the ENEMY-to-move probe.
    if attackers_of(&pos.pieces, q_sq, enemy, pos.all) == 0 {
        return None; // not attacked → not doomed
    }
    let in_place_loss = best_see_capture(&enemy_probe, q_sq);
    if in_place_loss <= 0 {
        return None; // adequately defended → q can just sit; not doomed
    }

    // (1b) NO SAFE ESCAPE. Enumerate OUR legal moves of q on the real board (stm ==
    //      us; drops pinned-q moves). For each destination, does q survive there? A
    //      destination is safe iff, after we move q there, the ENEMY cannot profitably
    //      take it. after.make(m) flips stm to enemy, so best_see_capture(&after,
    //      m.to) scores the enemy's grab of q on its new square. gain <= 0 ⇒ q is safe
    //      there ⇒ NOT doomed. (This loop also visits q's captures; a capture that
    //      lands q on a safe square legitimately means q escaped by capturing to
    //      safety — correct, matches trapped's "capture its attacker to safety".)
    let mut gen = pos.clone();
    let our_legal = generate_legal(&mut gen);
    for m in our_legal.iter().filter(|m| m.from == q_sq) {
        let mut after = pos.clone();
        after.make(*m); // after.stm == enemy
        let enemy_grab = best_see_capture(&after, m.to);
        if enemy_grab <= 0 {
            return None; // a SAFE destination exists → q is not doomed
        }
    }

    // ── STEP 2: DESPERADO GAIN, WORST-CASE. A bare see(pos, q_sq, m.to) resolves only
    //    the exchange on the grab square and was blind to any reply that wins bigger
    //    material ELSEWHERE — an in-between grab of our hanging queen, a promotion, an
    //    off-square counter-capture (fuzz: 69/640 false positives, every one engine-
    //    confirmed). And a 1-reply-with-SEE-recovery model still overcredits (the
    //    recovery's own collateral is invisible, one ply deeper). So the claim is fully
    //    conservative: bank the victim, then subtract the enemy's best LEGAL capture/
    //    promotion quiescence from the post-grab board (alternating, stand-pat, pins
    //    respected via generate_legal). Captures-only can under-claim quiet threats —
    //    the safe direction for a zero-false-positive material fact.
    let mut best_gain = 0i32;
    let mut best_uci: Option<String> = None;
    let mut best_victim: Option<(Piece, u8)> = None;
    for m in our_legal.iter().filter(|m| m.from == q_sq && m.flag.is_capture()) {
        // Emit only standard captures with a clean victim PieceRef; skip en-passant
        // (pawn victim behind m.to — q is never a pawn, so this only drops EP).
        let victim_sq = m.to;
        let Some((vc, vp)) = pos.piece_at(victim_sq) else {
            continue;
        };
        if vc != enemy || vp == Piece::King {
            continue; // never "win" the king
        }
        let banked = SEE_VALUE[vp.index()];
        let mut after = pos.clone();
        after.make(*m); // after.stm == enemy
        let gain = banked - legal_material_quiescence(&after, 5);
        if gain > best_gain {
            best_gain = gain;
            best_uci = Some(m.to_uci());
            best_victim = Some((vp, victim_sq));
        }
    }

    // ── STEP 3: FIRE iff the grab RECOVERS more than passively dying. q is doomed, so
    //    the best non-capturing salvage is 0 (STEP 1b proved every relocation still
    //    loses q). Capturing beats dying for nothing ⇔ best_gain > 0.
    if best_gain <= 0 {
        return None;
    }
    let (v_piece, v_sq) = best_victim?;
    let move_uci = best_uci?;

    Some(DesperadoOpportunity {
        kind: "desperado".to_string(),
        validator: "desperado_validation".to_string(),
        move_uci,
        piece: piece_ref(us, piece, q_sq), // OUR doomed piece
        captured_victim: piece_ref(enemy, v_piece, v_sq),
        material_gain: best_gain, // SEE_VALUE scale (N320,B330,R500,Q900)
    })
}

// ── Double attack (two distinct pieces, one move) ────────────────────────────
// One legal move where the MOVED piece threatens target A AND that same move makes a
// SECOND, DIFFERENT friendly piece's threat on a distinct target B newly realizable,
// and the enemy cannot parry both. This is the two-DIFFERENT-pieces multiple-attack
// that the fork detector structurally cannot see (a fork is one moved piece hitting
// ≥2 winnable targets). Disjointness we ENFORCE:
//   • vs fork:      B is threatened by a distinct piece q_sq != mv.to (G5).
//   • vs discovery: q must not be a rear slider that gained sight of B only because
//                   mv.from was vacated (G6, `double_attack_is_discovery`).
// A and B are on distinct squares attacked by distinct pieces, each SEE-proven
// winnable after the move with correct causality, so one enemy tempo can rescue at
// most one prong — material_gain = min(A, B). G7 hardening (require one prong
// undefended) rules out the single edge case where one enemy move re-guards both.

/// Validated double-attack opportunities for the side to move, sorted by move then
/// target ids.
pub fn double_attack_opportunities(pos: &Position) -> FactCollection<DoubleAttackOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(op) = double_attack_after_move(pos, mv, us) {
            out.push(op);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.target_a.id.cmp(&b.target_a.id))
            .then_with(|| a.target_b.id.cmp(&b.target_b.id))
    });
    FactCollection::computed(out)
}

/// Validated double attacks for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn double_attack_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<DoubleAttackOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => double_attack_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn double_attack_after_move(
    pos: &Position,
    mv: Move,
    us: Color,
) -> Option<DoubleAttackOpportunity> {
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let mover_piece = mv.flag.promo_piece().unwrap_or(moving_piece);
    // A king mover cannot "win" defended material and is never itself capturable — the
    // same exclusion the fork detector makes. (King as a TARGET is excluded via
    // mover_threat_gain and the Piece::King guards in the threat-B scan.)
    if mover_piece == Piece::King {
        return None;
    }
    let enemy = us.flip();

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let from_bit = 1u64 << mv.from;

    // (G1) Our moved piece must not be simply hung on mv.to.
    if forker_capturable_for_gain(&mut after.clone(), mv.to) {
        return None;
    }
    // us-to-move probe so best_see_capture / see score OUR captures. Err bails on our
    // king-in-check AND on mv-gives-check (documented false-negative, same as
    // attack_defender / remove_guard).
    let after_us = position_for_analysis_side(&after, us).ok()?;

    // THREAT A: the discovery helper's rule — best winnable enemy piece the moved piece
    // attacks from mv.to (king excluded, SEE-style winnable). > 0 required (G2).
    let threat_a = mover_threat_gain(&after, us, enemy, mv.to);
    if threat_a <= 0 {
        return None;
    }
    // Recover A's square by the same rule mover_threat_gain uses so the PieceRef is exact.
    let (a_piece, a_sq) = double_attack_best_mover_target(&after, us, enemy, mv.to, threat_a)?;
    // Threat A must be LEGALLY executable by the mover. mover_threat_gain is attack-geometry
    // only, so a PINNED mover (e.g. mv itself interposed against a check) passes it while
    // being unable to ever capture a_sq — the pinned-mover false positive. Require a legal,
    // winning capture of a_sq FROM mv.to on the us-to-move probe (generate_legal respects the
    // pin), mirroring threat B's best_see_capture/see legality.
    let mut a_legal_probe = after_us.clone();
    if !generate_legal(&mut a_legal_probe)
        .into_iter()
        .any(|m| m.from == mv.to && m.to == a_sq && see(&after_us, m.from, m.to) > 0)
    {
        return None;
    }
    let a_undefended = is_undefended(&after, a_sq, enemy);

    // THREAT B: every OTHER friendly piece q (q_sq != mv.to) that attacks some enemy t,
    // where t is newly winnable BECAUSE of mv and q is NOT a discovery rear slider.
    let mut best: Option<DoubleAttackOpportunity> = None;
    for q_piece in Piece::ALL {
        if q_piece == Piece::King {
            continue; // the king never "wins" material as an attacker
        }
        let mut qbb = after.pieces[us.index()][q_piece.index()];
        while qbb != 0 {
            let q_sq = qbb.trailing_zeros() as u8;
            qbb &= qbb - 1;
            if q_sq == mv.to {
                continue; // that is the mover → fork territory (G5)
            }
            let q_bit = 1u64 << q_sq;

            for t_piece in Piece::ALL {
                if t_piece == Piece::King {
                    continue; // king is a check, not a material win (G8)
                }
                let mut tbb = after.pieces[enemy.index()][t_piece.index()];
                while tbb != 0 {
                    let t_sq = tbb.trailing_zeros() as u8;
                    tbb &= tbb - 1;
                    if t_sq == a_sq {
                        continue; // DISTINCT targets — one enemy move can't save both (G7)
                    }
                    // q must actually attack t on the post-move board.
                    if attackers_of(&after.pieces, t_sq, us, after.all) & q_bit == 0 {
                        continue;
                    }
                    // (G3) CAUSALITY: t NOT already winnable before mv; winnable now.
                    if best_see_capture(pos, t_sq) > 0 {
                        continue;
                    }
                    let gain_b = best_see_capture(&after_us, t_sq);
                    if gain_b <= 0 {
                        continue;
                    }
                    // (G4) q ITSELF must be the winning attacker of t (not a rear discovered
                    // slider that only fires down q's line).
                    if see(&after_us, q_sq, t_sq) <= 0 {
                        continue;
                    }
                    // (G6) NON-DISCOVERY: reject a slider q that saw t only via the mv.from
                    // vacancy — that is the discovery detector's fact, not ours.
                    if double_attack_is_discovery(q_piece, q_sq, t_sq, pos.all, after.all, from_bit)
                    {
                        continue;
                    }
                    // (G7) hardening: at least one prong must be undefended so a single enemy
                    // tempo cannot re-guard both lines. An undefended target must be answered
                    // on its own square, leaving the other prong standing.
                    if !a_undefended && !is_undefended(&after, t_sq, enemy) {
                        continue;
                    }

                    // material_gain = min(A, B): enemy saves the dearer, we take the lesser.
                    let gain = threat_a.min(gain_b);
                    debug_assert!(gain > 0);

                    let cand = DoubleAttackOpportunity {
                        kind: "double_attack".to_string(),
                        validator: "double_attack_validation".to_string(),
                        move_uci: mv.to_uci(),
                        mover: piece_ref(us, mover_piece, mv.to),
                        second_attacker: piece_ref(us, q_piece, q_sq),
                        target_a: piece_ref(enemy, a_piece, a_sq),
                        target_b: piece_ref(enemy, t_piece, t_sq),
                        gives_check: gives_check_flag,
                        material_gain: gain,
                    };
                    // Keep the highest-gain pair; ties resolve to the first-seen (lowest ids
                    // via the ordered Piece::ALL + trailing_zeros scan) for determinism.
                    match &best {
                        Some(b) if b.material_gain >= cand.material_gain => {}
                        _ => best = Some(cand),
                    }
                }
            }
        }
    }
    best
}

/// The (piece, sq) of the single best winnable enemy target the moved piece hits from
/// `from_sq`, recomputed by mover_threat_gain's exact rule so the emitted PieceRef
/// matches the value used for threat A. King excluded.
fn double_attack_best_mover_target(
    after: &Position,
    us: Color,
    enemy: Color,
    from_sq: u8,
    want: i32,
) -> Option<(Piece, u8)> {
    let mover_value = VALUE[after.piece_at(from_sq)?.1.index()];
    let bit = 1u64 << from_sq;
    let mut best: Option<(Piece, u8)> = None;
    for piece in Piece::ALL {
        if piece == Piece::King {
            continue;
        }
        let mut bb = after.pieces[enemy.index()][piece.index()];
        while bb != 0 {
            let sq = bb.trailing_zeros() as u8;
            bb &= bb - 1;
            if attackers_of(&after.pieces, sq, us, after.all) & bit == 0 {
                continue;
            }
            let winnable = VALUE[piece.index()] > mover_value || is_undefended(after, sq, enemy);
            if winnable && VALUE[piece.index()] == want {
                // tie-break: lowest square, deterministic.
                match best {
                    Some((_, s)) if s <= sq => {}
                    _ => best = Some((piece, sq)),
                }
            }
        }
    }
    best
}

/// True iff `q` is a slider that gained sight of `t_sq` ONLY because `mv.from` was
/// vacated — i.e. this threat belongs to the discovery detector. Non-sliders are never
/// discoveries here.
fn double_attack_is_discovery(
    q_piece: Piece,
    q_sq: u8,
    t_sq: u8,
    occ_before: u64,
    occ_after: u64,
    from_bit: u64,
) -> bool {
    let (Some(before), Some(after_ray)) = (
        slider_attacks(q_piece, q_sq, occ_before),
        slider_attacks(q_piece, q_sq, occ_after),
    ) else {
        return false; // knight / pawn second attacker — never a discovery
    };
    let t_bit = 1u64 << t_sq;
    // Already saw t pre-move → not newly unblocked → keep (not a discovery).
    if before & t_bit != 0 {
        return false;
    }
    // Newly sees t. It IS a discovery iff q had mv.from as a blocker on its ray pre-move
    // (from_bit was on q's ray) AND now sees t — vacating mv.from opened the line. This
    // mirrors discovery's `before_atk & from_bit != 0` gate.
    (after_ray & t_bit != 0) && (before & from_bit != 0)
}

// ── Win the exchange (win a rook for a minor) ────────────────────────────────
// A MOVE detector, disjoint material-PROFILE refinement. A single legal capture whose
// SEE swap on the captured square nets specifically a ROOK for a MINOR (bishop or knight):
// our minor takes an enemy rook and the exchange resolves to a gain in the rook-minus-minor
// band. see() is the fast PROFILE pre-filter, but it IGNORES ABSOLUTE PINS and is blind to
// non-capturing counter-resources (e.g. a lurking pawn queening), so soundness is confirmed
// by a legality-aware make()-based WORST-CASE over EVERY legal enemy reply — debiting what
// each reply captures AND any promotion swing (both fuzz-found FP classes). Distinct from
// fork/skewer/remove_guard/xray: none of those classify the exchange-VALUE profile;
// kind="win_the_exchange" is its own teaching fact and MAY legitimately co-occur with them.

// Band derived from SEE_VALUE (rook - bishop = 170, rook - knight = 180). Tolerance
// window brackets both while excluding a plain rook grab (~500) and equal/near-equal
// trades. Kept as consts so it tracks SEE_VALUE, never a hard-coded literal.
// SEE_VALUE is indexed pawn..king (0..5). Piece::index() is not const, so index the
// array with the fixed enum ordinals directly: Knight=1, Bishop=2, Rook=3.
const SEE_VALUE_KNIGHT: i32 = SEE_VALUE[1]; // 320
const SEE_VALUE_BISHOP: i32 = SEE_VALUE[2]; // 330
const SEE_VALUE_ROOK: i32 = SEE_VALUE[3]; // 500
const WIN_EXCHANGE_MIN: i32 = SEE_VALUE_ROOK - SEE_VALUE_BISHOP - 20; // 150
const WIN_EXCHANGE_MAX: i32 = SEE_VALUE_ROOK - SEE_VALUE_KNIGHT + 5; // 185

/// Validated win-the-exchange opportunities for the side to move, sorted by move then
/// victim id.
pub fn win_exchange_opportunities(pos: &Position) -> FactCollection<WinExchangeOpportunity> {
    let us = pos.stm;
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut out = Vec::new();
    for mv in legal {
        if let Some(w) = win_exchange_after_move(pos, mv, us) {
            out.push(w);
        }
    }
    out.sort_by(|a, b| {
        a.move_uci
            .cmp(&b.move_uci)
            .then_with(|| a.victim.id.cmp(&b.victim.id))
    });
    FactCollection::computed(out)
}

/// Validated win-the-exchange for a requested side. See `motif_opportunities_for` for the
/// counterfactual side-to-move semantics.
pub fn win_exchange_opportunities_for(
    pos: &Position,
    side: Color,
) -> FactCollection<WinExchangeOpportunity> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => win_exchange_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

fn win_exchange_after_move(pos: &Position, mv: Move, us: Color) -> Option<WinExchangeOpportunity> {
    // (G0) Capturing moves only. A quiet move wins no material on a square.
    if !mv.flag.is_capture() {
        return None;
    }
    let (_, moving_piece) = pos.piece_at(mv.from)?;
    let attacker_piece = mv.flag.promo_piece().unwrap_or(moving_piece);

    // (G3a) Attacker profile: only a MINOR can "win the exchange". This also excludes a
    //       King attacker (a plain grab) and rook/queen/pawn attackers (R-for-R, queen
    //       sac, pawn-takes are not this motif). A promotion capture yields a non-minor
    //       promo_piece except PromoN/PromoB — still gated by the band below.
    if attacker_piece != Piece::Bishop && attacker_piece != Piece::Knight {
        return None;
    }

    let enemy = us.flip();

    // Victim square: EP handled like remove_guard (captured pawn sits behind mv.to). A
    // pawn victim can never yield the exchange, so EP is excluded by (G3b) below, but we
    // still resolve d_sq correctly for the piece_at lookup / determinism id.
    let d_sq = if matches!(mv.flag, MoveFlag::EnPassant) {
        if us == Color::White {
            mv.to - 8
        } else {
            mv.to + 8
        }
    } else {
        mv.to
    };
    let (v_color, victim_piece) = pos.piece_at(d_sq)?;

    // (G3b) Victim profile: only an enemy ROOK yields the exchange. Excludes pawn victim
    //       (EP + plain, no exchange), minor victim (equal trade), queen victim (winning
    //       the queen, not the exchange), and — since v_color must be enemy — self-capture.
    if v_color != enemy || victim_piece != Piece::Rook {
        return None;
    }

    let mut check_probe = pos.clone();
    let gives_check_flag = gives_check(&mut check_probe, mv);

    // (G6) COUNTING PROOF — full SEE swap on the captured square from OUR (pre-move) side.
    //      see() minimaxes the recapture sequence (our minor is given up, we take their
    //      rook, x-ray reveals any rear same-line recapturer through the shrinking occ).
    //      pos.stm == us and mv.from holds our minor, so see(pos, mv.from, mv.to) scores
    //      exactly this exchange. EP is unreachable here (pawn victim rejected by G3b), so
    //      the rook always sits on mv.to == d_sq for the plain-capture path.
    let score = see(pos, mv.from, mv.to);
    if score <= 0 {
        return None; // must actually be a net win
    }
    // (G4) EXCHANGE BAND — the net must be specifically rook-for-minor, not a hanging-rook
    //      grab (~500, already covered by counting/piece_safety) nor an equal/near trade.
    if !(WIN_EXCHANGE_MIN..=WIN_EXCHANGE_MAX).contains(&score) {
        return None;
    }

    // (G7) LEGALITY (SEE ignores absolute pins) — mv came from generate_legal(pos)
    //      (pins dropped), so a pinned minor that see() would wrongly let capture the
    //      rook is never a candidate here. Structurally guaranteed by the enumeration.

    // (G8) LEGALITY-AWARE WORST-CASE CONFIRM — see() ignores absolute pins in the recapture
    //      sequence and can therefore credit an ILLEGAL recapture (inflating the net) or a
    //      too-valuable last recapturer. A plain forker_capturable_for_gain on mv.to is WRONG
    //      here: the enemy recapturing our minor is EXPECTED and already priced into `score`
    //      (that is WHY the net is ~170 not ~500), so it would reject every real exchange.
    //      Instead we recompute the realized net over EVERY LEGAL enemy reply (make()-based,
    //      pins dropped by generate_legal), debiting whatever each reply itself captures and
    //      crediting only our own LEGAL recovery. The rook we already banked is `+victim`.
    let mut after = pos.clone();
    after.make(mv); // after.stm == enemy
    let banked = SEE_VALUE[victim_piece.index()];
    let realized = win_exchange_worst_case(&after, enemy, mv.to, banked)?;
    // (G4b) The legality-aware realized net must ALSO land in the rook-for-minor band; a pin
    //       that inflated see() collapses here, and a plain rook grab never reaches this path.
    if !(WIN_EXCHANGE_MIN..=WIN_EXCHANGE_MAX).contains(&realized) {
        return None;
    }

    Some(WinExchangeOpportunity {
        kind: "win_the_exchange".to_string(),
        validator: "win_exchange_validation".to_string(),
        move_uci: mv.to_uci(),
        mover: piece_ref(us, attacker_piece, mv.to),
        victim: piece_ref(enemy, victim_piece, d_sq),
        gives_check: gives_check_flag,
        material_gain: realized,
    })
}

/// Realized material for us after we have PLAYED the winning capture (our minor now stands
/// on `sq`, having banked `banked` centipawns for the captured rook; `after.stm == enemy`).
/// Minimax over EVERY legal enemy reply: the enemy grabs `enemy_gain` — debited — which is
/// what the reply CAPTURES (our minor on `sq` OR collateral elsewhere) PLUS any PROMOTION
/// swing (a non-capturing counter-resource like h1=Q must still be paid for, or we falsely
/// bank the exchange while the enemy queens). After the reply WE recover our best LEGAL
/// single-square SEE on `sq`. The worst-case (minimum) net = `banked + our_recovery -
/// enemy_gain`. `None` if any legal reply drives the net non-positive (no sound win) or the
/// enemy has no reply (mate/stalemate edge — no exchange to bank). Using generate_legal on
/// each make()-produced board makes this pin-aware, unlike raw see().
fn win_exchange_worst_case(after: &Position, enemy: Color, sq: u8, banked: i32) -> Option<i32> {
    let mut enemy_probe = after.clone();
    let enemy_legal = generate_legal(&mut enemy_probe);
    if enemy_legal.is_empty() {
        return None; // mate/stalemate: nothing recaptures, no exchange resolved
    }
    let mut worst = i32::MAX;
    for m in &enemy_legal {
        // Debit what this reply itself captures (collateral or our minor).
        let mut enemy_gain = enemy_probe
            .piece_at(m.to)
            .map(|(_, p)| SEE_VALUE[p.index()])
            .unwrap_or(0);
        // Debit a promotion swing: the reply may ignore our minor and QUEEN instead — a
        // material resource that captures nothing on `m.to`, so it is invisible to the
        // capture debit above. Without this, a lurking passed pawn (e.g. h2h1=Q) lets us
        // wrongly bank the exchange while the enemy nets a queen. (fuzz-found FP)
        if let Some(promo) = m.flag.promo_piece() {
            enemy_gain += SEE_VALUE[promo.index()] - SEE_VALUE[Piece::Pawn.index()];
        }
        let mut esc = enemy_probe.clone();
        esc.make(*m); // stm flips back to us
        // Our best LEGAL recovery on the contested square (0 if we cannot recapture there).
        let our_recovery = if esc.occ[enemy.index()] & (1u64 << sq) != 0 {
            best_see_capture(&esc, sq).max(0)
        } else {
            0
        };
        let net = banked + our_recovery - enemy_gain;
        if net <= 0 {
            return None; // some legal reply refutes the exchange — not a sound win
        }
        worst = worst.min(net);
    }
    if worst > 0 && worst != i32::MAX {
        Some(worst)
    } else {
        None
    }
}

// ── Battery (two-rooks / queen-bishop / Alekhine's Gun) ──────────────────────
// A STATE detector (the overload/trapped shape): OUR sliders doubled on one line
// with an empty corridor between them, the rear projecting force through the front.
// No material claim => NO see/best_see_capture, NO legal_material_quiescence, NO
// make()-based worst-case-over-all-enemy-replies loop — nothing is claimed that an
// enemy reply could refute, which is why the FP surface is ~zero. Do NOT "add SEE
// for rigor": that would import the pin/last-recapturer/collateral pitfalls for a
// claim this fact never makes. The only occupancy edit is pure u64 math on the
// attacks call (the pin/xray reveal probe — `xray_attack_after_move` pattern),
// never a hand-edited Position (the overload occ-desync trap) and never make().
// Pins are irrelevant and deliberately ignored: a pinned front or rear piece still
// forms a real STANDING battery because nothing is claimed to move (a non-bug).
//
// Non-redundant with the shipped line motifs: pin/skewer/xray require an ENEMY
// piece on the line and discovery requires the front piece to MOVE; a battery is
// standing friendly-friendly alignment — the structural precursor those exploit.
// A queen behind a friendly bishop on a FILE is a discovery precursor, NOT a
// battery (the front must bear on the line-class; the pass structure encodes it).
//
// Guards:
//   G1 pairing     — rear AND front drawn only from the pass's line-class set
//                    ({R,Q} orth / {B,Q} diag); kings/pawns/knights can never
//                    appear in a fact, by construction.
//   G2 alignment   — front ∈ rook_attacks/bishop_attacks(rear, pos.all): blocker-
//                    aware, so f ∈ atk ⇔ the corridor is empty. NEVER hand-rolled.
//   G3 projection  — the causality analogue for a state claim: the doubled line
//                    must actually project past the front (G3a: reveal non-empty,
//                    so a muzzle at the board edge emits nothing) and must not
//                    fire point-blank into an own pawn wall with no enemy in the
//                    extension (G3b — the flood control for real-game closed files).
//   G4 gun overlay — Q←R←R on one file emits ONE alekhines_gun; every pair with
//                    BOTH endpoints inside a fired gun's 3-square set is suppressed
//                    (including the reversed inner pair that G3b's pawn-only test
//                    admits, since the queen is not a friendly pawn).
//   G5 determinism — fixed type order + LSB scans construct deterministically;
//                    canonical (rear.id, front.id) sort + ordered-pair dedupe.
//
// Documented deliberate false negatives: (a) doubled pieces whose muzzles sit at
// board edges in both directions emit nothing; (b) doubling behind an own pawn you
// intend to push is a legitimate plan G3b rejects; (c) tripled formations other
// than the Q-R-R gun emit as component pairs, not a named triple.

/// Line class of a battery pair.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BatteryLine {
    File,
    Rank,
    Diagonal,
}

impl BatteryLine {
    fn as_str(self) -> &'static str {
        match self {
            BatteryLine::File => "file",
            BatteryLine::Rank => "rank",
            BatteryLine::Diagonal => "diagonal",
        }
    }
}

/// Intermediate aligned pair, kept as raw squares until emission so the gun
/// overlay can chain and suppress on square identity.
struct BatteryRawPair {
    rear_pc: Piece,
    rear_sq: u8,
    front_pc: Piece,
    front_sq: u8,
    line: BatteryLine,
}

/// Validated standing battery formations for the side to move (facts about OUR
/// pieces). NO in-check gate: purely structural, with no move or material claim —
/// a battery exists even while its owner is in check (unlike overload, whose
/// exploitation begins with a move); the `_for` probe still yields
/// `Unavailable("opposite_side_probe_while_in_check")` via
/// `position_for_analysis_side`.
pub fn battery_opportunities(pos: &Position) -> FactCollection<BatteryFact> {
    let us = pos.stm;
    let enemy = us.flip();

    let mut pairs: Vec<BatteryRawPair> = Vec::new();

    // ORTH pass: rear ∈ {Rook, Queen}, front ∈ {Rook, Queen}, on rook lines.
    for rear_pc in [Piece::Rook, Piece::Queen] {
        let mut rbb = pos.pieces[us.index()][rear_pc.index()];
        while rbb != 0 {
            let r_sq = rbb.trailing_zeros() as u8;
            rbb &= rbb - 1;
            let ratk = rook_attacks(r_sq, pos.all);
            for front_pc in [Piece::Rook, Piece::Queen] {
                let mut fbb = pos.pieces[us.index()][front_pc.index()] & ratk;
                while fbb != 0 {
                    let f_sq = fbb.trailing_zeros() as u8;
                    fbb &= fbb - 1;
                    if f_sq == r_sq {
                        continue; // same-type self-pair guard
                    }
                    // (G2) aligned by construction: rook_attacks stops at the first
                    // blocker, so f_sq ∈ ratk ⇔ the corridor between them is empty.
                    let line = if file_of(f_sq) == file_of(r_sq) {
                        BatteryLine::File
                    } else {
                        BatteryLine::Rank
                    };
                    if let Some(p) =
                        battery_pair(pos, us, enemy, rear_pc, r_sq, front_pc, f_sq, line)
                    {
                        pairs.push(p);
                    }
                }
            }
        }
    }

    // DIAG pass: rear ∈ {Bishop, Queen}, front ∈ {Bishop, Queen}, on bishop lines.
    // B+B pairs are possible via promotion and emit as generic "battery".
    for rear_pc in [Piece::Bishop, Piece::Queen] {
        let mut rbb = pos.pieces[us.index()][rear_pc.index()];
        while rbb != 0 {
            let r_sq = rbb.trailing_zeros() as u8;
            rbb &= rbb - 1;
            let datk = bishop_attacks(r_sq, pos.all);
            for front_pc in [Piece::Bishop, Piece::Queen] {
                let mut fbb = pos.pieces[us.index()][front_pc.index()] & datk;
                while fbb != 0 {
                    let f_sq = fbb.trailing_zeros() as u8;
                    fbb &= fbb - 1;
                    if f_sq == r_sq {
                        continue; // same-type self-pair guard
                    }
                    if let Some(p) = battery_pair(
                        pos,
                        us,
                        enemy,
                        rear_pc,
                        r_sq,
                        front_pc,
                        f_sq,
                        BatteryLine::Diagonal,
                    ) {
                        pairs.push(p);
                    }
                }
            }
        }
    }

    // (G4) Alekhine's Gun overlay (file-only, queen rearmost), chained on the
    // POST-GUARD pair list: P1 = (Q at q, R at m, File) and P2 = (R at m, R at f,
    // File) sharing the middle square m on the queen's file. Same file + shared
    // middle + both blocker-aware-aligned ⇒ monotone q→m→f order is implied (a
    // third piece between any two would have broken alignment).
    let mut guns: Vec<(u8, u8, u8)> = Vec::new(); // (q_sq, m_sq, f_sq)
    for p1 in pairs.iter().filter(|p| {
        p.rear_pc == Piece::Queen && p.front_pc == Piece::Rook && p.line == BatteryLine::File
    }) {
        for p2 in pairs.iter().filter(|p| {
            p.rear_pc == Piece::Rook
                && p.front_pc == Piece::Rook
                && p.line == BatteryLine::File
                && p.rear_sq == p1.front_sq
                && file_of(p.front_sq) == file_of(p1.rear_sq)
        }) {
            debug_assert_eq!(
                (rank_of(p2.front_sq) as i8 - rank_of(p2.rear_sq) as i8).signum(),
                (rank_of(p1.front_sq) as i8 - rank_of(p1.rear_sq) as i8).signum()
            );
            guns.push((p1.rear_sq, p1.front_sq, p2.front_sq));
        }
    }
    // Suppress EVERY pair whose BOTH endpoints lie inside a gun's square set — not
    // just the two chained components. The reversed inner pair (e.g. the front rook
    // behind the middle rook pointing back at the queen) passes G3b because the
    // queen is "not a friendly pawn"; without whole-set suppression it would leak a
    // spurious two_rooks_battery in every gun position.
    pairs.retain(|p| {
        !guns.iter().any(|g| {
            let s = [g.0, g.1, g.2];
            s.contains(&p.rear_sq) && s.contains(&p.front_sq)
        })
    });

    let mut out: Vec<BatteryFact> = Vec::new();
    for p in &pairs {
        out.push(battery_emit_pair(us, p));
    }
    for (q_sq, m_sq, f_sq) in guns {
        out.push(BatteryFact {
            kind: "battery".to_string(),
            validator: "battery_validation".to_string(),
            subtype: "alekhines_gun".to_string(),
            rear: piece_ref(us, Piece::Queen, q_sq),
            front: piece_ref(us, Piece::Rook, f_sq), // the muzzle
            middle: Some(piece_ref(us, Piece::Rook, m_sq)),
            ray: squares_between(q_sq, f_sq)
                .into_iter()
                .map(square_name)
                .collect(), // contains m
            line: BatteryLine::File.as_str().to_string(),
        });
    }
    // (G5) determinism: canonical order + ordered-pair dedupe (belt-and-suspenders —
    // construction visits each ordered square pair once, and a pair aligns in
    // exactly one line class).
    out.sort_by(|a, b| {
        a.rear
            .id
            .cmp(&b.rear.id)
            .then_with(|| a.front.id.cmp(&b.front.id))
    });
    out.dedup_by(|a, b| a.rear.id == b.rear.id && a.front.id == b.front.id);
    FactCollection::computed(out)
}

/// Validated batteries from a requested side's perspective. See
/// `motif_opportunities_for` for the counterfactual side-to-move semantics.
pub fn battery_opportunities_for(pos: &Position, side: Color) -> FactCollection<BatteryFact> {
    match position_for_analysis_side(pos, side) {
        Ok(probe) => battery_opportunities(&probe),
        Err(reason) => FactCollection::unavailable(reason),
    }
}

/// Per-pair guard: the PROJECTION test (G3), the causality analogue for a state
/// claim — the doubled line only teaches if the multiplied force points at
/// something or through open space. The reveal beyond F is the proven after&!before
/// x-ray probe: pure u64 occupancy math on the attacks call, NO Position mutation.
/// `slider_attacks(Queen, ..)` (= queen_attacks) is safe here: removing the single
/// blocker F only extends the one ray through F, so the diff is confined to that ray.
#[allow(clippy::too_many_arguments)]
fn battery_pair(
    pos: &Position,
    us: Color,
    enemy: Color,
    rear_pc: Piece,
    r_sq: u8,
    front_pc: Piece,
    f_sq: u8,
    line: BatteryLine,
) -> Option<BatteryRawPair> {
    let before = slider_attacks(rear_pc, r_sq, pos.all)?; // Some for R/B/Q
    let after = slider_attacks(rear_pc, r_sq, pos.all & !(1u64 << f_sq))?;
    let ext = after & !before; // squares strictly beyond F, up to & incl. the next blocker
    if ext == 0 {
        return None; // (G3a) muzzle at the board edge — the pair projects nothing
    }
    // First square beyond F along the ray (squares_between signum-walk style);
    // ext != 0 guarantees it is on-board and ∈ ext (attack sets include their
    // first blocker).
    let df = (file_of(f_sq) as i8 - file_of(r_sq) as i8).signum();
    let dr = (rank_of(f_sq) as i8 - rank_of(r_sq) as i8).signum();
    let first_beyond = ((rank_of(f_sq) as i8 + dr) * 8 + (file_of(f_sq) as i8 + df)) as u8;
    debug_assert!(ext & (1u64 << first_beyond) != 0);
    let hits_enemy = ext & enemy_occupancy(pos, enemy) != 0; // first piece down-line is enemy
    let own_pawn_muzzle =
        pos.pieces[us.index()][Piece::Pawn.index()] & (1u64 << first_beyond) != 0;
    if !hits_enemy && own_pawn_muzzle {
        return None; // (G3b) fires point-blank into its own pawn wall
    }
    Some(BatteryRawPair {
        rear_pc,
        rear_sq: r_sq,
        front_pc,
        front_sq: f_sq,
        line,
    })
}

/// Subtype rules at emission:
///   ORTH: R+R                -> "two_rooks_battery"   (file or rank)
///         Q+R / R+Q / Q+Q    -> "battery"
///   DIAG: {Q,B} either order -> "queen_bishop_battery"
///         Q+Q / B+B          -> "battery"
///   gun overlay              -> "alekhines_gun" (emitted directly, not here)
fn battery_emit_pair(us: Color, p: &BatteryRawPair) -> BatteryFact {
    let subtype = match p.line {
        BatteryLine::File | BatteryLine::Rank => {
            if p.rear_pc == Piece::Rook && p.front_pc == Piece::Rook {
                "two_rooks_battery"
            } else {
                "battery"
            }
        }
        BatteryLine::Diagonal => {
            if p.rear_pc != p.front_pc {
                "queen_bishop_battery"
            } else {
                "battery"
            }
        }
    };
    BatteryFact {
        kind: "battery".to_string(),
        validator: "battery_validation".to_string(),
        subtype: subtype.to_string(),
        rear: piece_ref(us, p.rear_pc, p.rear_sq),
        front: piece_ref(us, p.front_pc, p.front_sq),
        middle: None,
        ray: squares_between(p.rear_sq, p.front_sq)
            .into_iter()
            .map(square_name)
            .collect(),
        line: p.line.as_str().to_string(),
    }
}
