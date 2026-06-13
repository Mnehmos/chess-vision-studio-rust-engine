//! Deterministic position hazards and move-to-move deltas.
//!
//! Hazards summarize already-validated board facts. They do not grade moves or
//! claim causation; the application remains responsible for topic attribution.

use crate::facts::position::{position_for_analysis_side, side, square_name};
use crate::facts::types::{FactCollection, HazardFact, PositionFacts, Side};
use crate::movegen::{generate_legal, gives_check};
use crate::{Color, Position};
use std::collections::BTreeMap;

pub fn position_hazards(pos: &Position, facts: &PositionFacts) -> FactCollection<HazardFact> {
    let collections_ready = matches!(facts.available_captures, FactCollection::Computed { .. })
        && matches!(facts.opponent_available_captures, FactCollection::Computed { .. })
        && matches!(facts.available_motifs, FactCollection::Computed { .. })
        && matches!(facts.opponent_available_motifs, FactCollection::Computed { .. })
        && matches!(facts.available_pins, FactCollection::Computed { .. })
        && matches!(facts.opponent_available_pins, FactCollection::Computed { .. });
    if !collections_ready {
        return FactCollection::unavailable("symmetric_hazard_probe_unavailable");
    }

    let mut hazards = BTreeMap::<String, HazardFact>::new();
    add_capture_hazards(&mut hazards, &facts.available_captures);
    add_capture_hazards(&mut hazards, &facts.opponent_available_captures);
    add_fork_hazards(&mut hazards, &facts.available_motifs);
    add_fork_hazards(&mut hazards, &facts.opponent_available_motifs);
    add_pin_hazards(&mut hazards, &facts.available_pins);
    add_pin_hazards(&mut hazards, &facts.opponent_available_pins);
    add_king_pressure(&mut hazards, facts);

    for color in [Color::White, Color::Black] {
        let Ok(probe) = position_for_analysis_side(pos, color) else {
            return FactCollection::unavailable("symmetric_hazard_probe_unavailable");
        };
        for mate in mate_in_one_moves(&probe) {
            let victim = side(color.flip());
            let king_square = square_name(pos.king_sq(color.flip()));
            let id = format!("mate-threat-{}-{mate}", side_name(victim));
            hazards.insert(
                id.clone(),
                HazardFact {
                    id,
                    kind: "mate_threat".into(),
                    side: victim,
                    squares: vec![king_square],
                    magnitude_cp: None,
                    move_uci: Some(mate),
                },
            );
        }
    }

    FactCollection::computed(hazards.into_values().collect())
}

pub fn hazard_deltas(
    before: &FactCollection<HazardFact>,
    after: &FactCollection<HazardFact>,
) -> (
    FactCollection<HazardFact>,
    FactCollection<HazardFact>,
    FactCollection<HazardFact>,
) {
    let (FactCollection::Computed { items: before }, FactCollection::Computed { items: after }) =
        (before, after)
    else {
        let reason = "hazards_unavailable_for_delta";
        return (
            FactCollection::unavailable(reason),
            FactCollection::unavailable(reason),
            FactCollection::unavailable(reason),
        );
    };
    let before_map: BTreeMap<_, _> = before.iter().map(|fact| (&fact.id, fact)).collect();
    let after_map: BTreeMap<_, _> = after.iter().map(|fact| (&fact.id, fact)).collect();
    let created = after
        .iter()
        .filter(|fact| !before_map.contains_key(&fact.id))
        .cloned()
        .collect();
    let removed = before
        .iter()
        .filter(|fact| !after_map.contains_key(&fact.id))
        .cloned()
        .collect();
    let worsened = after
        .iter()
        .filter(|fact| {
            before_map.get(&fact.id).is_some_and(|prior| {
                fact.magnitude_cp.unwrap_or(0) > prior.magnitude_cp.unwrap_or(0)
            })
        })
        .cloned()
        .collect();
    (
        FactCollection::computed(created),
        FactCollection::computed(removed),
        FactCollection::computed(worsened),
    )
}

fn add_capture_hazards(
    out: &mut BTreeMap<String, HazardFact>,
    captures: &FactCollection<crate::facts::types::CaptureOpportunity>,
) {
    let FactCollection::Computed { items } = captures else {
        return;
    };
    for capture in items.iter().filter(|capture| capture.see_cp > 0) {
        let id = format!("losing-material-{}", capture.victim.id);
        let candidate = HazardFact {
            id: id.clone(),
            kind: "losing_material".into(),
            side: capture.victim.side,
            squares: vec![capture.victim_square.clone()],
            magnitude_cp: Some(capture.see_cp),
            move_uci: Some(capture.move_uci.clone()),
        };
        if out
            .get(&id)
            .is_none_or(|existing| candidate.magnitude_cp > existing.magnitude_cp)
        {
            out.insert(id, candidate);
        }
    }
}

fn add_fork_hazards(
    out: &mut BTreeMap<String, HazardFact>,
    motifs: &FactCollection<crate::facts::types::MotifOpportunity>,
) {
    let FactCollection::Computed { items } = motifs else {
        return;
    };
    for fork in items {
        let victim = opposite(fork.forking_piece.side);
        let id = format!("fork-threat-{}-{}", side_name(victim), fork.move_uci);
        let mut squares = vec![fork.forking_piece.square.clone()];
        squares.extend(fork.targets.iter().map(|target| target.square.clone()));
        squares.sort();
        squares.dedup();
        out.insert(
            id.clone(),
            HazardFact {
                id,
                kind: "fork_threat".into(),
                side: victim,
                squares,
                magnitude_cp: Some(fork.material_gain),
                move_uci: Some(fork.move_uci.clone()),
            },
        );
    }
}

fn add_pin_hazards(
    out: &mut BTreeMap<String, HazardFact>,
    pins: &FactCollection<crate::facts::types::PinOpportunity>,
) {
    let FactCollection::Computed { items } = pins else {
        return;
    };
    for pin in items {
        let id = format!("pin-constraint-{}-{}", pin.pinned.id, pin.move_uci);
        let mut squares = vec![
            pin.pinner.square.clone(),
            pin.pinned.square.clone(),
            pin.anchor.square.clone(),
        ];
        squares.extend(pin.ray.iter().cloned());
        squares.sort();
        squares.dedup();
        out.insert(
            id.clone(),
            HazardFact {
                id,
                kind: "pin_constraint".into(),
                side: pin.pinned.side,
                squares,
                magnitude_cp: None,
                move_uci: Some(pin.move_uci.clone()),
            },
        );
    }
}

fn add_king_pressure(out: &mut BTreeMap<String, HazardFact>, facts: &PositionFacts) {
    let FactCollection::Computed { items } = &facts.king_safety else {
        return;
    };
    for king in items {
        if !king.in_check && king.pressured_squares.len() < 2 {
            continue;
        }
        let id = format!("king-pressure-{}-{}", side_name(king.side), king.king_square);
        let mut squares = vec![king.king_square.clone()];
        squares.extend(king.pressured_squares.iter().cloned());
        squares.sort();
        squares.dedup();
        out.insert(
            id.clone(),
            HazardFact {
                id,
                kind: "king_pressure".into(),
                side: king.side,
                squares,
                magnitude_cp: Some((king.pressured_squares.len() as i32) * 10),
                move_uci: None,
            },
        );
    }
}

fn mate_in_one_moves(pos: &Position) -> Vec<String> {
    let mut probe = pos.clone();
    let legal = generate_legal(&mut probe);
    let mut mates = Vec::new();
    for mv in legal {
        let mut check_probe = pos.clone();
        if !gives_check(&mut check_probe, mv) {
            continue;
        }
        let mut after = pos.clone();
        after.make(mv);
        if generate_legal(&mut after).is_empty() {
            mates.push(mv.to_uci());
        }
    }
    mates.sort();
    mates
}

fn opposite(side: Side) -> Side {
    match side {
        Side::White => Side::Black,
        Side::Black => Side::White,
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::White => "white",
        Side::Black => "black",
    }
}
