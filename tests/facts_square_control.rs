use cvs_bitboard_core::facts::square_control::square_control_facts;
use cvs_bitboard_core::facts::types::{FactCollection, PieceRef, SquareFact};
use cvs_bitboard_core::Position;

fn facts_for(fen: &str) -> Vec<SquareFact> {
    let pos = Position::from_fen(fen).expect("valid fen");
    match square_control_facts(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("square_control_facts should be computed, got {other:?}"),
    }
}

fn square<'a>(facts: &'a [SquareFact], name: &str) -> &'a SquareFact {
    facts
        .iter()
        .find(|fact| fact.square == name)
        .unwrap_or_else(|| panic!("missing square {name}"))
}

fn ids(refs: &[PieceRef]) -> Vec<String> {
    refs.iter().map(|piece| piece.id.clone()).collect()
}

fn computed_ids(collection: &FactCollection<PieceRef>) -> Vec<String> {
    match collection {
        FactCollection::Computed { items } => ids(items),
        other => panic!("expected computed legal movers, got {other:?}"),
    }
}

#[test]
fn covers_all_sixty_four_squares_in_ascending_order() {
    let facts = facts_for("4k3/8/8/8/8/5N2/8/4K3 w - - 0 1");
    assert_eq!(facts.len(), 64);
    assert_eq!(facts[0].square, "a1");
    assert_eq!(facts[63].square, "h8");
}

#[test]
fn empty_square_controlled_by_exactly_one_side_lists_that_attacker() {
    // White knight on f3 reaches e5; no black piece attacks e5. e5 is empty.
    let facts = facts_for("4k3/8/8/8/8/5N2/8/4K3 w - - 0 1");
    let e5 = square(&facts, "e5");

    assert!(!e5.occupied);
    assert!(e5.controlled_by_white);
    assert!(!e5.controlled_by_black);
    assert_eq!(ids(&e5.attacked_by_white), vec!["white-knight-f3"]);
    assert!(e5.attacked_by_black.is_empty());
}

#[test]
fn empty_square_controlled_by_both_sides() {
    // White knight f3 and black knight c6 both attack d4 (empty).
    let facts = facts_for("4k3/8/2n5/8/8/5N2/8/4K3 w - - 0 1");
    let d4 = square(&facts, "d4");

    assert!(!d4.occupied);
    assert!(d4.controlled_by_white);
    assert!(d4.controlled_by_black);
    assert_eq!(ids(&d4.attacked_by_white), vec!["white-knight-f3"]);
    assert_eq!(ids(&d4.attacked_by_black), vec!["black-knight-c6"]);
}

#[test]
fn occupied_square_reports_occupied_and_still_lists_attackers() {
    // Black pawn on d5, attacked by the white pawn on e4.
    let facts = facts_for("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
    let d5 = square(&facts, "d5");

    assert!(d5.occupied);
    assert!(d5.controlled_by_white);
    assert_eq!(ids(&d5.attacked_by_white), vec!["white-pawn-e4"]);
}

#[test]
fn opposite_side_legal_movers_are_unavailable_when_side_to_move_is_in_check() {
    // White to move and in check from the black rook on e8 down the e-file.
    // (Black king parked on a8 so the FEN is legal.)
    let facts = facts_for("k3r3/8/8/8/8/8/8/4K3 w - - 0 1");
    // Pick any square; the opposite-side (black) probe must be Unavailable
    // because granting black a move while white is in check is illegal.
    let d1 = square(&facts, "d1");

    assert!(matches!(
        d1.legal_movers_black,
        FactCollection::Unavailable { .. }
    ));
    assert!(matches!(
        d1.legal_movers_white,
        FactCollection::Computed { .. }
    ));
    // White king can legally step to d1 to escape the check.
    assert_eq!(computed_ids(&d1.legal_movers_white), vec!["white-king-e1"]);
}

#[test]
fn en_passant_square_is_a_legal_mover_target_for_the_side_to_move_only() {
    // Black just played d7-d5; white pawn on e5 may capture en passant onto d6.
    // d6 is reachable ONLY by that en-passant capture for white.
    let fen = "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1";
    let facts = facts_for(fen);
    let d6 = square(&facts, "d6");

    // Side to move (white) can legally move a pawn onto d6 via en passant.
    let white_movers = computed_ids(&d6.legal_movers_white);
    assert_eq!(white_movers, vec!["white-pawn-e5"]);

    // The opposite side's probe clears the ep square, so black has no mover that
    // reaches d6 (no black piece can land there), confirming EP is excluded.
    let black_movers = computed_ids(&d6.legal_movers_black);
    assert!(
        black_movers.is_empty(),
        "ep destination must not appear for the non-moving side, got {black_movers:?}"
    );
}
