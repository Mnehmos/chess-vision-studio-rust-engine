use cvs_bitboard_core::facts::motifs::motif_opportunities;
use cvs_bitboard_core::facts::{FactCollection, MotifOpportunity};
use cvs_bitboard_core::Position;

fn forks(fen: &str) -> Vec<MotifOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match motif_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("motifs should be computed, got {other:?}"),
    }
}

#[test]
fn validates_a_knight_fork_of_king_and_rook() {
    // Black to move: Ng5-f3+ forks the white king on g1 and the rook on e1.
    let items = forks("6k1/8/8/6n1/4P3/8/8/4R1K1 b - - 0 1");
    let fork = items
        .iter()
        .find(|m| m.move_uci == "g5f3")
        .expect("Nf3+ fork should be validated");
    assert_eq!(fork.kind, "fork");
    assert_eq!(fork.validator, "fork_validation");
    assert!(fork.gives_check);
    assert!(fork.king_target);
    assert_eq!(fork.material_gain, 500); // the rook is forced to fall
    assert_eq!(fork.forking_piece.id, "black-knight-f3");
    let target_ids: Vec<&str> = fork.targets.iter().map(|t| t.id.as_str()).collect();
    assert!(target_ids.contains(&"white-king-g1"));
    assert!(target_ids.contains(&"white-rook-e1"));
}

#[test]
fn rejects_a_single_attack_lookalike() {
    // King on h1 instead of g1: Nf3 only ever hits the rook — not a fork.
    let items = forks("6k1/8/8/6n1/8/8/4P3/4R2K b - - 0 1");
    assert!(items.iter().all(|m| m.move_uci != "g5f3"));
}

#[test]
fn rejects_a_fork_whose_piece_is_simply_captured() {
    // A white g2 pawn answers Nf3+ with gxf3, refuting the would-be fork.
    let items = forks("6k1/8/8/6n1/8/8/4P1P1/4R1K1 b - - 0 1");
    assert!(items.iter().all(|m| m.move_uci != "g5f3"));
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "6k1/8/8/6n1/4P3/8/8/4R1K1 b - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = motif_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "fork enumeration must not mutate the board"
    );
}

#[test]
fn targets_are_sorted_by_id_for_stable_output() {
    let items = forks("6k1/8/8/6n1/4P3/8/8/4R1K1 b - - 0 1");
    for m in &items {
        let mut sorted = m.targets.clone();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(m.targets, sorted);
    }
}
