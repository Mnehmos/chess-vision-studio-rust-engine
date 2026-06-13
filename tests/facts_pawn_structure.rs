use cvs_bitboard_core::facts::position::position_facts;
use cvs_bitboard_core::facts::{FactCollection, Side};
use cvs_bitboard_core::Position;

#[test]
fn reports_named_doubled_isolated_passed_pawns_and_islands() {
    let pos = Position::from_fen("4k3/7p/8/8/2P5/2P5/6P1/4K3 w - - 0 1").unwrap();
    let pawns = position_facts(&pos).pawn_structure;

    let doubled = pawns.doubled.iter().find(|fact| fact.file == "c").unwrap();
    assert_eq!(doubled.squares, vec!["c3", "c4"]);
    assert!(pawns.isolated.iter().any(|pawn| pawn.square == "g2"));
    assert!(pawns.passed.iter().any(|pawn| pawn.square == "c4"));

    let white_islands: Vec<_> = pawns
        .islands
        .iter()
        .filter(|island| island.id.starts_with("white-"))
        .collect();
    assert_eq!(white_islands.len(), 2);
}

#[test]
fn adjacent_file_enemy_pawn_prevents_passed_status() {
    let pos = Position::from_fen("4k3/8/3p4/8/2P5/8/8/4K3 w - - 0 1").unwrap();
    let pawns = position_facts(&pos).pawn_structure;
    assert!(!pawns.passed.iter().any(|pawn| pawn.square == "c4"));
}

#[test]
fn reports_exact_missing_king_shield_squares() {
    let pos = Position::from_fen("6k1/8/8/8/8/8/6PP/6K1 w - - 0 1").unwrap();
    let facts = position_facts(&pos).pawn_structure;
    let FactCollection::Computed { items } = facts.king_shield_missing else {
        panic!("king shield should be computed");
    };
    let white = items.iter().find(|fact| fact.side == Side::White).unwrap();
    assert_eq!(white.king_square, "g1");
    assert_eq!(white.missing_squares, vec!["f2"]);
}
