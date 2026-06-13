use cvs_bitboard_core::facts::piece_safety::{capture_opportunities, king_safety_facts};
use cvs_bitboard_core::facts::{FactCollection, Side};
use cvs_bitboard_core::Position;

#[test]
fn reports_structured_safe_capture_and_highest_value_marker() {
    let pos = Position::from_fen("6k1/8/8/4q3/8/5N2/8/4K3 w - - 0 1").unwrap();
    let FactCollection::Computed { items } = capture_opportunities(&pos) else {
        panic!("captures should be computed");
    };
    let capture = items
        .iter()
        .find(|capture| capture.move_uci == "f3e5")
        .expect("Nxe5 capture");
    assert_eq!(capture.attacker.id, "white-knight-f3");
    assert_eq!(capture.victim.id, "black-queen-e5");
    assert_eq!(capture.victim_square, "e5");
    assert_eq!(capture.see_cp, 900);
    assert!(capture.capturing_piece_survives);
    assert!(capture.highest_value_safe_capture);
}

#[test]
fn reports_check_attackers_pressure_and_escape_squares() {
    let pos = Position::from_fen("4r1k1/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let FactCollection::Computed { items } = king_safety_facts(&pos) else {
        panic!("king safety should be computed");
    };
    let white = items.iter().find(|fact| fact.side == Side::White).unwrap();
    assert!(white.in_check);
    assert_eq!(white.attackers[0].id, "black-rook-e8");
    assert!(white.pressured_squares.contains(&"e1".to_string()));
    let FactCollection::Computed { items: escapes } = &white.legal_escape_squares else {
        panic!("moving side escapes should be computed");
    };
    assert!(escapes.contains(&"d1".to_string()));
    assert!(!escapes.contains(&"e2".to_string()));

    let black = items.iter().find(|fact| fact.side == Side::Black).unwrap();
    assert!(matches!(
        black.legal_escape_squares,
        FactCollection::Unavailable { .. }
    ));
}
