use crate::facts::hazards::{hazard_deltas, position_hazards};
use crate::facts::motifs::{
    discovery_opportunities, discovery_opportunities_for, double_attack_opportunities,
    double_attack_opportunities_for, motif_opportunities, attack_defender_opportunities,
    attack_defender_opportunities_for, interference_opportunities, interference_opportunities_for,
    motif_opportunities_for, overload_opportunities, overload_opportunities_for, pin_opportunities,
    pin_opportunities_for, remove_guard_opportunities, remove_guard_opportunities_for,
    skewer_opportunities, skewer_opportunities_for, trapped_pieces, trapped_pieces_for,
};
use crate::facts::mate_patterns::{mate_pattern_opportunities, mate_pattern_opportunities_for};
use crate::facts::pawn_structure::structure_deltas;
use crate::facts::position::{position_facts, square_name};
use crate::facts::types::*;
use crate::facts::{FACTS_REGISTRY_VERSION, TEACHING_FACTS_SCHEMA_VERSION};
use crate::movegen::generate_legal;
use crate::{Move, Position};

/// Position facts, with validated fork + pin motifs layered on only when requested.
fn facts_for(pos: &Position, include_motifs: bool) -> PositionFacts {
    let mut facts = position_facts(pos);
    if include_motifs {
        facts.available_motifs = motif_opportunities(pos);
        facts.available_pins = pin_opportunities(pos);
        facts.available_skewers = skewer_opportunities(pos);
        facts.available_discoveries = discovery_opportunities(pos);
        facts.available_remove_guard = remove_guard_opportunities(pos);
        facts.available_trapped = trapped_pieces(pos);
        facts.available_mate_patterns = mate_pattern_opportunities(pos);
        facts.available_overload = overload_opportunities(pos);
        facts.available_attack_defender = attack_defender_opportunities(pos);
        facts.available_interference = interference_opportunities(pos);
        facts.available_double_attack = double_attack_opportunities(pos);
        facts.opponent_available_motifs = motif_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_pins = pin_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_skewers = skewer_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_discoveries = discovery_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_remove_guard = remove_guard_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_trapped = trapped_pieces_for(pos, pos.stm.flip());
        facts.opponent_available_mate_patterns =
            mate_pattern_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_overload = overload_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_attack_defender =
            attack_defender_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_interference =
            interference_opportunities_for(pos, pos.stm.flip());
        facts.opponent_available_double_attack =
            double_attack_opportunities_for(pos, pos.stm.flip());
        facts.hazards = position_hazards(pos, &facts);
    }
    facts
}

pub fn build_teaching_fact_bundle(
    request: &TeachingFactsRequestV1,
) -> Result<TeachingFactBundleV1, String> {
    if request.schema_version != TEACHING_FACTS_SCHEMA_VERSION {
        return Err(format!(
            "unsupported teaching facts schemaVersion {}; expected {}",
            request.schema_version, TEACHING_FACTS_SCHEMA_VERSION
        ));
    }
    let include_motifs = request
        .options
        .as_ref()
        .is_some_and(|options| options.include_motif_opportunities);

    let before_pos = Position::from_fen(&request.fen_before)?;
    let before = facts_for(&before_pos, include_motifs);
    let (played, played_pos) = apply_branch(
        &before_pos,
        &request.played_move_uci,
        &before,
        include_motifs,
    )?;
    let mut errors = Vec::new();

    let include_counterfactual = request
        .options
        .as_ref()
        .is_none_or(|options| options.include_counterfactual);
    let best = if include_counterfactual {
        optional_branch(
            &before_pos,
            request.best_move_uci.as_deref(),
            &before,
            "bestMoveUci",
            include_motifs,
            &mut errors,
        )
    } else {
        None
    };
    let refutation_before = facts_for(&played_pos, include_motifs);
    let refutation = optional_branch(
        &played_pos,
        request.refutation_uci.as_deref(),
        &refutation_before,
        "refutationUci",
        include_motifs,
        &mut errors,
    );

    if let Some(line) = &request.principal_variation_uci {
        validate_line(&before_pos, line, &mut errors);
    }

    let mut validators = vec![
        "legal_move_generation".to_string(),
        "attack_map".to_string(),
        "see".to_string(),
        "capture_opportunities".to_string(),
        "king_safety".to_string(),
        "pawn_structure".to_string(),
        "square_control".to_string(),
    ];
    if include_motifs {
        validators.push("fork_validation".to_string());
        validators.push("pin_validation".to_string());
        validators.push("skewer_validation".to_string());
        validators.push("discovery_validation".to_string());
        validators.push("remove_guard_validation".to_string());
        validators.push("trapped_piece_validation".to_string());
        validators.push("mate_pattern_validation".to_string());
        validators.push("overload_validation".to_string());
        validators.push("attack_defender_validation".to_string());
        validators.push("interference_validation".to_string());
        validators.push("double_attack_validation".to_string());
    }

    Ok(TeachingFactBundleV1 {
        schema_version: TEACHING_FACTS_SCHEMA_VERSION,
        fen_before: request.fen_before.clone(),
        before,
        played,
        best,
        refutation,
        provenance: FactsProvenance {
            engine: "cvs-bitboard-core".into(),
            engine_commit: option_env!("CVS_ENGINE_COMMIT").map(str::to_string),
            facts_registry_version: FACTS_REGISTRY_VERSION,
            validators,
        },
        errors,
    })
}

fn optional_branch(
    start: &Position,
    uci: Option<&str>,
    before: &PositionFacts,
    field: &str,
    include_motifs: bool,
    errors: &mut Vec<FactError>,
) -> Option<MoveStateFacts> {
    let uci = uci?;
    match apply_branch(start, uci, before, include_motifs) {
        Ok((facts, _)) => Some(facts),
        Err(message) => {
            errors.push(FactError {
                code: "illegal_move".into(),
                message,
                field: Some(field.into()),
            });
            None
        }
    }
}

fn apply_branch(
    start: &Position,
    uci: &str,
    before: &PositionFacts,
    include_motifs: bool,
) -> Result<(MoveStateFacts, Position), String> {
    let mut pos = start.clone();
    let mv = legal_uci(&mut pos, uci)?;
    pos.make(mv);
    let after = facts_for(&pos, include_motifs);
    let (created_structures, removed_structures) =
        structure_deltas(&before.pawn_structure, &after.pawn_structure);
    let (created_hazards, removed_hazards, worsened_hazards) =
        hazard_deltas(&before.hazards, &after.hazards);
    let facts = MoveStateFacts {
        r#move: move_fact(mv),
        fen_after: pos.to_fen(),
        position: after,
        deltas: MoveFactDeltas {
            created_hazards,
            removed_hazards,
            worsened_hazards,
            created_structures: FactCollection::computed(created_structures),
            removed_structures: FactCollection::computed(removed_structures),
        },
    };
    Ok((facts, pos))
}

fn legal_uci(pos: &mut Position, uci: &str) -> Result<Move, String> {
    generate_legal(pos)
        .into_iter()
        .find(|mv| mv.to_uci() == uci)
        .ok_or_else(|| format!("illegal UCI move '{uci}' for {}", pos.to_fen()))
}

fn validate_line(start: &Position, line: &[String], errors: &mut Vec<FactError>) {
    let mut pos = start.clone();
    for (index, uci) in line.iter().enumerate() {
        match legal_uci(&mut pos, uci) {
            Ok(mv) => pos.make(mv),
            Err(message) => {
                errors.push(FactError {
                    code: "illegal_pv_move".into(),
                    message,
                    field: Some(format!("principalVariationUci[{index}]")),
                });
                break;
            }
        }
    }
}

fn move_fact(mv: Move) -> MoveFact {
    let uci = mv.to_uci();
    MoveFact {
        from: square_name(mv.from),
        to: square_name(mv.to),
        promotion: (uci.len() == 5).then(|| uci[4..5].to_string()),
        uci,
    }
}
