use crate::facts::pawn_structure::structure_deltas;
use crate::facts::position::{position_facts, square_name};
use crate::facts::types::*;
use crate::facts::{FACTS_REGISTRY_VERSION, TEACHING_FACTS_SCHEMA_VERSION};
use crate::movegen::generate_legal;
use crate::{Move, Position};

pub fn build_teaching_fact_bundle(
    request: &TeachingFactsRequestV1,
) -> Result<TeachingFactBundleV1, String> {
    if request.schema_version != TEACHING_FACTS_SCHEMA_VERSION {
        return Err(format!(
            "unsupported teaching facts schemaVersion {}; expected {}",
            request.schema_version, TEACHING_FACTS_SCHEMA_VERSION
        ));
    }
    let before_pos = Position::from_fen(&request.fen_before)?;
    let before = position_facts(&before_pos);
    let (played, played_pos) = apply_branch(&before_pos, &request.played_move_uci, &before)?;
    let mut errors = Vec::new();

    let include_counterfactual = request
        .options
        .as_ref()
        .map_or(true, |options| options.include_counterfactual);
    let best = if include_counterfactual {
        optional_branch(
            &before_pos,
            request.best_move_uci.as_deref(),
            &before,
            "bestMoveUci",
            &mut errors,
        )
    } else {
        None
    };
    let refutation_before = position_facts(&played_pos);
    let refutation = optional_branch(
        &played_pos,
        request.refutation_uci.as_deref(),
        &refutation_before,
        "refutationUci",
        &mut errors,
    );

    if let Some(line) = &request.principal_variation_uci {
        validate_line(&before_pos, line, &mut errors);
    }
    if request
        .options
        .as_ref()
        .is_some_and(|options| options.include_motif_opportunities)
    {
        errors.push(FactError {
            code: "fact_uncomputed".into(),
            message: "motif opportunities are not implemented in milestone 1".into(),
            field: Some("options.includeMotifOpportunities".into()),
        });
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
            validators: vec![
                "legal_move_generation".into(),
                "attack_map".into(),
                "see".into(),
                "pawn_structure".into(),
            ],
        },
        errors,
    })
}

fn optional_branch(
    start: &Position,
    uci: Option<&str>,
    before: &PositionFacts,
    field: &str,
    errors: &mut Vec<FactError>,
) -> Option<MoveStateFacts> {
    let uci = uci?;
    match apply_branch(start, uci, before) {
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
) -> Result<(MoveStateFacts, Position), String> {
    let mut pos = start.clone();
    let mv = legal_uci(&mut pos, uci)?;
    pos.make(mv);
    let after = position_facts(&pos);
    let (created_structures, removed_structures) =
        structure_deltas(&before.pawn_structure, &after.pawn_structure);
    let facts = MoveStateFacts {
        r#move: move_fact(mv),
        fen_after: pos.to_fen(),
        position: after,
        deltas: MoveFactDeltas {
            created_hazards: FactCollection::uncomputed("not_in_milestone_1"),
            removed_hazards: FactCollection::uncomputed("not_in_milestone_1"),
            worsened_hazards: FactCollection::uncomputed("not_in_milestone_1"),
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
