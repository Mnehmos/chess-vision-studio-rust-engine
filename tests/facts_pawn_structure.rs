use cvs_bitboard_core::facts::position::position_facts;
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
