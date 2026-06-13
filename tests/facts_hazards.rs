use cvs_bitboard_core::facts::move_bundle::build_teaching_fact_bundle;
use cvs_bitboard_core::facts::{
    FactCollection, TeachingFactsOptionsV1, TeachingFactsRequestV1,
};

fn request(fen: &str, played: &str) -> TeachingFactsRequestV1 {
    TeachingFactsRequestV1 {
        schema_version: 1,
        fen_before: fen.into(),
        played_move_uci: played.into(),
        best_move_uci: None,
        refutation_uci: None,
        principal_variation_uci: None,
        options: Some(TeachingFactsOptionsV1 {
            include_motif_opportunities: true,
            include_counterfactual: true,
        }),
    }
}

#[test]
fn emits_material_and_fork_hazards_with_stable_evidence() {
    let bundle = build_teaching_fact_bundle(&request(
        "6k1/8/8/6n1/8/8/8/R5K1 w - - 0 1",
        "a1e1",
    ))
    .unwrap();
    let FactCollection::Computed { items } = bundle.played.position.hazards else {
        panic!("hazards should be computed");
    };
    let fork = items.iter().find(|hazard| hazard.kind == "fork_threat").unwrap();
    assert_eq!(fork.side, cvs_bitboard_core::facts::Side::White);
    assert_eq!(fork.move_uci.as_deref(), Some("g5f3"));
    assert_eq!(fork.magnitude_cp, Some(500));
}

#[test]
fn move_bundle_reports_created_and_removed_hazard_deltas() {
    let bundle = build_teaching_fact_bundle(&request(
        "6k1/8/8/6n1/8/8/8/R5K1 w - - 0 1",
        "a1e1",
    ))
    .unwrap();
    let FactCollection::Computed { items } = bundle.played.deltas.created_hazards else {
        panic!("created hazards should be computed");
    };
    assert!(items.iter().any(|hazard| hazard.kind == "fork_threat"));
}

#[test]
fn checked_positions_keep_symmetric_hazards_explicitly_unavailable() {
    let bundle = build_teaching_fact_bundle(&TeachingFactsRequestV1 {
        refutation_uci: Some("g5f3".into()),
        ..request("6k1/8/8/6n1/8/8/8/R5K1 w - - 0 1", "a1e1")
    })
    .unwrap();
    let refutation = bundle.refutation.unwrap();
    assert!(matches!(
        refutation.position.hazards,
        FactCollection::Unavailable { .. }
    ));
}
