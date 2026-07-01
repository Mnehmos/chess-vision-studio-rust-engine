use cvs_bitboard_core::facts::motifs::xray_attack_opportunities;
use cvs_bitboard_core::facts::{FactCollection, PieceType, XRayOpportunity};
use cvs_bitboard_core::Position;

fn xr(fen: &str) -> Vec<XRayOpportunity> {
    let pos = Position::from_fen(fen).unwrap();
    match xray_attack_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("xray attacks should be computed, got {other:?}"),
    }
}

// ── Positives ────────────────────────────────────────────────────────────────

#[test]
fn validates_a_rook_xray_of_a_defended_knight_through_a_rook() {
    // White to move: Ra1-c1 attacks the black knight on c4, which is defended by the
    // black rook on c6 directly behind it. Neither pin (the rear piece is a rook, not
    // the king) nor skewer (VALUE[N]=300 <= VALUE[R]=500, so the knight is not forced
    // to flee). Counting through the defender wins the knight, because the white rook
    // on c7 backs the capture: Rc1xc4 Rc6xc4 Rc7xc4.
    let items = xr("1k6/2R5/2r5/8/2n1K3/8/8/R3B2r w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "a1c1")
        .expect("Rc1 x-ray of the defended knight should be validated");
    assert_eq!(x.kind, "xray_attack");
    assert_eq!(x.validator, "xray_attack_validation");
    assert_eq!(x.xrayer.piece_type, PieceType::Rook);
    assert_eq!(x.xrayer.square, "c1");
    assert_eq!(x.front.piece_type, PieceType::Knight);
    assert_eq!(x.front.square, "c4");
    assert_eq!(x.back.piece_type, PieceType::Rook);
    assert_eq!(x.back.square, "c6");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 320);
}

#[test]
fn validates_a_rook_xray_won_through_an_offline_recapture() {
    // White to move: Rh8-h2 attacks the black knight on d2 (defended by the black rook
    // on c2 behind it on the rank). The knight is won because the white bishop on c1
    // provides the through-recapture: Rh2xd2 Rc2xd2 Bc1xd2.
    let items = xr("7R/8/8/2k5/8/8/2rn4/K1B2B2 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "h8h2")
        .expect("Rh2 x-ray of the defended knight should be validated");
    assert_eq!(x.front.piece_type, PieceType::Knight);
    assert_eq!(x.front.square, "d2");
    assert_eq!(x.back.piece_type, PieceType::Rook);
    assert_eq!(x.back.square, "c2");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 320);
}

#[test]
fn validates_a_bishop_xray_winning_the_exchange() {
    // White to move: Bg2-f1 attacks the black rook on b5 (defended by the black queen
    // on a6 behind it on the a6-f1 diagonal). VALUE[R]=500 <= VALUE[Q]=900, the rear
    // is not the king, so it is neither pin nor skewer. Bxb5 Qxb5 wins the exchange
    // (rook for bishop).
    let items = xr("8/1n2K3/q7/1r6/8/4k3/6B1/8 w - - 0 1");
    let x = items
        .iter()
        .find(|x| x.move_uci == "g2f1")
        .expect("Bf1 x-ray of the defended rook should be validated");
    assert_eq!(x.xrayer.piece_type, PieceType::Bishop);
    assert_eq!(x.front.piece_type, PieceType::Rook);
    assert_eq!(x.front.square, "b5");
    assert_eq!(x.back.piece_type, PieceType::Queen);
    assert_eq!(x.back.square, "a6");
    assert!(!x.gives_check);
    assert_eq!(x.material_gain, 170);
    assert_eq!(x.ray, vec!["e2", "d3", "c4", "b5"]);
}

// ── Negatives ────────────────────────────────────────────────────────────────

#[test]
fn does_not_report_a_pin_as_an_xray() {
    // Re1 attacks Re5 with the black KING on e8 directly behind it: the rear piece is
    // the king, so this is an absolute pin, not an x-ray.
    let items = xr("4k3/8/8/4r3/8/8/8/4R1K1 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "e1e5"),
        "a piece pinned to the king must not be reported as an x-ray"
    );
}

#[test]
fn does_not_report_a_king_front_skewer_as_an_xray() {
    // Ra1-e1+ attacks the black king on e4 (front) with the rook on e8 behind it. A
    // king front is always a skewer (it is forced to move), never an x-ray.
    let items = xr("4r3/8/8/8/4k3/8/8/R6K w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1e1"),
        "a king-front skewer must not be reported as an x-ray"
    );
}

#[test]
fn rejects_an_xrayer_that_is_simply_captured() {
    // Ra1-e1 would align on the e-file, but the black bishop on a5 plays Bxe1 winning
    // the rook: the x-raying piece is simply lost.
    let items = xr("4r3/8/8/b7/4k3/8/8/R6K w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "a1e1"),
        "an x-rayer that is immediately captured for gain is not an x-ray"
    );
}

#[test]
fn does_not_report_a_value_skewer_as_an_xray() {
    // Bg2-f1 attacks the black QUEEN on b5 (front) with the black rook on a6 behind it.
    // The front is worth MORE than the rear (VALUE[Q] > VALUE[R]), so the queen is
    // forced to flee: this is a skewer, not an x-ray.
    let items = xr("8/1n2K3/r7/1q6/8/4k3/6B1/8 w - - 0 1");
    assert!(
        items.iter().all(|x| x.move_uci != "g2f1"),
        "a front-heavier alignment is a skewer, not an x-ray"
    );
}

#[test]
fn respects_causality_when_the_front_is_already_winnable() {
    // The white rook is ALREADY on c1 pre-move (with Rc7 backing it), so the knight on
    // c4 is already winnable by a naive count — no move CREATES the x-ray. The detector
    // must report nothing (every candidate fails the causality guard).
    let items = xr("1k6/2R5/2r5/8/2n1K3/8/8/2R1B2r w - - 0 1");
    assert!(
        items.is_empty(),
        "a pre-existing winnable front is not a newly created x-ray, got {items:?}"
    );
}

#[test]
fn rejects_a_pinned_xrayer() {
    // Bd7-c6 lands the bishop on the a4-e8 diagonal where the white bishop b5 pins it to
    // the black king on e8 (b5-c6-d7-e8). The counting proof see(s, e4) is positive because
    // see() ignores the pin, but the bishop can NEVER legally capture e4 — moving off the
    // diagonal exposes the king. The legality guard (G7) must reject it. Regression for the
    // pinned-xrayer false positive found by oracle fuzzing.
    let items = xr("rq2kbnr/1ppbppp1/p2p1n1Q/1B6/3PP3/1P4P1/P1PN1P1P/R3K2R b KQkq - 8 12");
    assert!(
        items.iter().all(|x| x.move_uci != "d7c6"),
        "a pinned x-rayer cannot execute its capture, got {items:?}"
    );
}

#[test]
fn no_xrays_in_the_opening_position() {
    assert!(
        xr("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no x-ray attacks"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "1k6/2R5/2r5/8/2n1K3/8/8/R3B2r w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = xray_attack_opportunities(&pos);
    assert_eq!(
        pos.to_fen(),
        fen,
        "x-ray enumeration must not mutate the board"
    );
}

#[test]
fn emitted_opportunities_report_a_positive_material_gain() {
    // Every reported x-ray must carry a proven positive SEE gain.
    for fen in [
        "1k6/2R5/2r5/8/2n1K3/8/8/R3B2r w - - 0 1",
        "7R/8/8/2k5/8/8/2rn4/K1B2B2 w - - 0 1",
        "8/1n2K3/q7/1r6/8/4k3/6B1/8 w - - 0 1",
    ] {
        for x in xr(fen) {
            assert!(
                x.material_gain > 0,
                "x-ray on {fen} reported non-positive gain: {x:?}"
            );
        }
    }
}
