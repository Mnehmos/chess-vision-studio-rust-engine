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
    AttackDefenderOpportunity, DiscoveryOpportunity, FactCollection, InterferenceOpportunity,
    MotifOpportunity, OverloadOpportunity, PieceRef, PinOpportunity, RemoveGuardOpportunity,
    SkewerOpportunity, TrappedPieceOpportunity,
};
use crate::movegen::{generate_legal, gives_check};
use crate::see::see;
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
        if our_best <= 0 {
            return None; // this reply saves everything — not a sound win
        }
        worst = worst.min(our_best);
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
