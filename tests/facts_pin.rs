use cvs_bitboard_core::facts::motifs::pin_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PinOpportunity};
use cvs_bitboard_core::Position;

fn pins(fen: &str) -> Vec<PinOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match pin_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("pins should be computed, got {other:?}"),
    }
}

#[test]
fn validates_an_absolute_pin_to_the_king() {
    // Black to move: Bc8-g4 pins the white knight on f3 to the king on d1.
    let items = pins("2b1k3/8/8/8/8/5N2/7P/1R1K4 b - - 0 1");
    let pin = items
        .iter()
        .find(|p| p.move_uci == "c8g4")
        .expect("Bg4 absolute pin should be validated");
    assert_eq!(pin.kind, "absolute");
    assert_eq!(pin.validator, "pin_validation");
    assert_eq!(pin.pinner.id, "black-bishop-g4");
    assert_eq!(pin.pinned.id, "white-knight-f3");
    assert_eq!(pin.anchor.id, "white-king-d1");
    assert_eq!(pin.ray, vec!["f3".to_string(), "e2".to_string()]);
    assert!(pin.pinned_immobile);
}

#[test]
fn rejects_a_pin_whose_pinner_is_captured() {
    // A white h3 pawn answers Bg4 with hxg4 — the pinner hangs, so it is no pin.
    let items = pins("2b1k3/8/8/8/8/5N1P/8/1R1K4 b - - 0 1");
    assert!(items.iter().all(|p| p.move_uci != "c8g4"));
}

#[test]
fn rejects_a_lookalike_with_no_anchor_behind() {
    // King on e1 (off the diagonal): Bg4 hits the knight but pins nothing.
    let items = pins("2b1k3/8/8/8/8/5N2/7P/1R2K3 b - - 0 1");
    assert!(items.iter().all(|p| p.pinned.id != "white-knight-f3"));
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "2b1k3/8/8/8/8/5N2/7P/1R1K4 b - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = pin_opportunities(&pos);
    assert_eq!(pos.to_fen(), fen, "pin enumeration must not mutate the board");
}
