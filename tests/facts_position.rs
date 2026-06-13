use cvs_bitboard_core::facts::position::position_facts;
use cvs_bitboard_core::facts::types::FactValue;
use cvs_bitboard_core::Position;

#[test]
fn stable_piece_ids_and_relationships_are_reported() {
    let pos = Position::from_fen("4k3/8/8/8/4q3/8/4R3/4K3 w - - 0 1").unwrap();
    let original = pos.to_fen();
    let facts = position_facts(&pos);

    let queen = facts
        .pieces
        .iter()
        .find(|piece| piece.piece.id == "black-queen-e4")
        .unwrap();
    assert!(queen.attacked);
    assert_eq!(queen.attackers[0].id, "white-rook-e2");
    match &queen.see {
        FactValue::Computed { value } => {
            assert!(value.losing);
            assert_eq!(value.best_capture_uci.as_deref(), Some("e2e4"));
            assert!(
                value.score_cp.unwrap() < 20_000,
                "king sentinel leaked into SEE"
            );
        }
        other => panic!("expected computed SEE, got {other:?}"),
    }
    assert_eq!(
        pos.to_fen(),
        original,
        "fact extraction mutated the position"
    );
}

#[test]
fn see_is_explicitly_unavailable_for_the_side_to_move_pieces() {
    let pos = Position::from_fen("4k3/8/8/8/4q3/8/4R3/4K3 w - - 0 1").unwrap();
    let facts = position_facts(&pos);
    let rook = facts
        .pieces
        .iter()
        .find(|piece| piece.piece.id == "white-rook-e2")
        .unwrap();
    assert!(matches!(rook.see, FactValue::Unavailable { .. }));
}

#[test]
fn illegal_fen_is_rejected_before_fact_extraction() {
    assert!(Position::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").is_err());
}
