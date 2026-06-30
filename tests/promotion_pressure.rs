//! Promotion-pressure specialist (#2, v1 geometric slice). Verifies passed-pawn detection, the
//! promotion geometry, the must-address-now proxy, and the committed-defender (promotion-shadow) set.
use cvs_bitboard_core::facts::promotion_pressure::{
    promotion_pressure_facts, PromotionConfidence, PromotionPressureFactV1,
};
use cvs_bitboard_core::facts::types::Side;
use cvs_bitboard_core::Position;

fn facts(fen: &str) -> Vec<PromotionPressureFactV1> {
    promotion_pressure_facts(&Position::from_fen(fen).unwrap())
}

fn find<'a>(fs: &'a [PromotionPressureFactV1], pawn: &str) -> &'a PromotionPressureFactV1 {
    fs.iter()
        .find(|f| f.pawn_square == pawn)
        .unwrap_or_else(|| panic!("no promotion-pressure fact for {pawn}"))
}

#[test]
fn passed_pawn_one_from_promotion_must_be_addressed() {
    let fs = facts("8/2P5/8/8/8/8/8/k6K w - - 0 1");
    assert_eq!(fs.len(), 1);
    let f = &fs[0];
    assert_eq!(f.pawn_square, "c7");
    assert_eq!(f.color, Side::White);
    assert_eq!(f.promotion_distance, 1);
    assert_eq!(f.promotion_square, "c8");
    assert_eq!(f.stop_square, "c8");
    assert!(f.push_square_empty && f.can_advance_now);
    assert!(f.must_address_now);
    assert_eq!(f.committed_defender_count, 0);
    assert!(f.committed_defenders.is_empty());
    assert_eq!(f.confidence, PromotionConfidence::Geometric);
    assert_eq!(f.schema_version, 1);
}

#[test]
fn blocked_pawns_are_not_passed() {
    // White c6 and Black c7 block each other on the file -> neither is passed -> no facts.
    assert!(facts("8/2p5/2P5/8/8/8/8/K6k w - - 0 1").is_empty());
}

#[test]
fn defender_controlling_the_promotion_square_clears_must_address() {
    // Black Ra8 rakes the 8th rank and controls c8 (the promotion square), so the pawn does NOT
    // promote unopposed and the rook is a committed defender.
    let fs = facts("r7/2P5/8/8/8/8/8/3k3K w - - 0 1");
    let f = find(&fs, "c7");
    assert_eq!(f.promotion_distance, 1);
    assert!(!f.must_address_now, "the promotion square is controlled");
    assert!(f.promotion_square_controllers.iter().any(|d| d.square == "a8"));
    assert!(f.committed_defender_count >= 1);
    assert!(f.committed_defenders.iter().any(|d| d.square == "a8"));
}

#[test]
fn passed_pawns_reported_for_both_colors() {
    // White c7 (one from promotion) and Black f3 (two from promotion) are both passed.
    let fs = facts("8/2P5/8/8/8/5p2/8/K6k b - - 0 1");
    assert_eq!(fs.len(), 2);
    let white = find(&fs, "c7");
    assert_eq!(white.color, Side::White);
    assert_eq!(white.promotion_distance, 1);
    let black = find(&fs, "f3");
    assert_eq!(black.color, Side::Black);
    assert_eq!(black.promotion_distance, 2);
    assert_eq!(black.promotion_square, "f1");
    assert_eq!(black.stop_square, "f2");
    assert!(black.push_square_empty);
}

#[test]
fn rook_behind_the_pawn_is_blocked_from_the_promotion_square() {
    // White Pc7 with a Black rook BEHIND it on c1: the rook is blocked by the pawn, so it does NOT
    // control the promotion square c8 (the promotion shadow must not be inverted). It DOES attack
    // the pawn on the open file below, so the pawn is addressable -> must_address_now is false.
    let fs = facts("k7/2P5/8/8/8/8/7K/2r5 w - - 0 1");
    let f = find(&fs, "c7");
    assert!(
        !f.promotion_square_controllers.iter().any(|d| d.square == "c1"),
        "a rook behind the pawn is blocked from c8 and must not be a promotion-square controller"
    );
    assert!(
        f.pawn_attackers.iter().any(|d| d.square == "c1"),
        "the rook attacks the pawn along the open file below it"
    );
    assert!(!f.must_address_now, "the pawn is capturable, so it is addressable");
}
