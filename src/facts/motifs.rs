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
    DiscoveryOpportunity, FactCollection, MotifOpportunity, PieceRef, PinOpportunity,
    SkewerOpportunity,
};
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
                // rear slider must not itself be capturable for gain in lieu of saving it.
                let undefended = is_undefended(&after, t_sq, enemy);
                let winnable = undefended || VALUE[t_piece.index()] > s_value;
                if !winnable || forker_capturable_for_gain(&mut after.clone(), s_sq) {
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
