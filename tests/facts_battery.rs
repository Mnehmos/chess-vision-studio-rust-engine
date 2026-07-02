//! Adversarial FEN battery for the BATTERY detector (ChessTempo "Battery" family:
//! battery / two-rooks-battery / queen-bishop-battery / alekhines-gun).
//!
//! A battery is a STANDING formation — OUR sliders doubled on one line with an empty
//! corridor between them, the rear projecting force through the front. It is a STATE
//! detector with NO material claim, so unlike every material-claiming detector there is
//! no SEE, no legal_material_quiescence, and no worst-case-over-replies loop: nothing is
//! claimed that an enemy reply could refute. The guards under test are therefore purely
//! structural:
//!
//!   G1 pairing     — only {R,Q} pair on orthogonals, only {B,Q} on diagonals.
//!   G2 alignment   — front ∈ blocker-aware rook/bishop_attacks(rear): corridor empty.
//!   G3 projection  — removing the front must reveal squares beyond it (muzzle not at
//!                    the board edge) and the line must not fire point-blank into an
//!                    own pawn wall with no enemy in the extension.
//!   G4 gun overlay — Q←R←R on one file emits ONE alekhines_gun and suppresses every
//!                    component pair inside the gun's square set.
//!   G5 determinism — canonical (rear.id, front.id) order.
//!
//! Subtypes are snake_case in the fact ("two_rooks_battery", "queen_bishop_battery",
//! "alekhines_gun", generic "battery"); the app maps them to the kebab-case taxonomy
//! slugs. Every asserted square below is hand-verified against the FEN diagram.

use cvs_bitboard_core::facts::motifs::{battery_opportunities, battery_opportunities_for};
use cvs_bitboard_core::facts::{BatteryFact, FactCollection, PieceType, Side};
use cvs_bitboard_core::{Color, Position};

fn bt(fen: &str) -> Vec<BatteryFact> {
    let pos = Position::from_fen(fen).unwrap();
    match battery_opportunities(&pos) {
        FactCollection::Computed { items } => items,
        other => panic!("battery should be computed, got {other:?}"),
    }
}

/// The single battery whose rear piece sits on `rear_sq`, if any.
fn on_rear<'a>(it: &'a [BatteryFact], rear_sq: &str) -> Option<&'a BatteryFact> {
    it.iter().find(|b| b.rear.square == rear_sq)
}

// ── positives (must fire) ─────────────────────────────────────────────────────

#[test]
fn two_rooks_doubled_on_a_file() {
    // (P1) Rd1 behind Rd4 on the open d-file: corridor d2/d3 empty, the pair projects
    // d5–d7 and hits the black pawn d7. The reversed ordering (rear d4, front d1) is
    // rejected by the projection guard — beyond d1 is off the board.
    let items = bt("6k1/3p4/8/8/3R4/8/8/3R2K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one battery: {items:?}");
    let b = &items[0];
    assert_eq!(b.kind, "battery");
    assert_eq!(b.validator, "battery_validation");
    assert_eq!(b.subtype, "two_rooks_battery");
    assert_eq!(b.rear.id, "white-rook-d1");
    assert_eq!(b.rear.piece_type, PieceType::Rook);
    assert_eq!(b.front.id, "white-rook-d4");
    assert_eq!(b.front.piece_type, PieceType::Rook);
    assert!(b.middle.is_none());
    assert_eq!(b.ray, vec!["d2", "d3"]);
    assert_eq!(b.line, "file");
}

#[test]
fn queen_behind_bishop_on_a_diagonal() {
    // (P2) Qb1 behind Bc2 on the b1–h7 diagonal; the reveal d3–h7 hits the black pawn
    // h7. The reversed ordering (rear c2, front b1) is rejected — beyond b1 is off the
    // board. Adjacent pieces ⇒ empty ray.
    let items = bt("6k1/7p/8/8/8/8/2B5/1Q4K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one battery: {items:?}");
    let b = &items[0];
    assert_eq!(b.subtype, "queen_bishop_battery");
    assert_eq!(b.rear.id, "white-queen-b1");
    assert_eq!(b.front.id, "white-bishop-c2");
    assert!(b.middle.is_none());
    assert!(b.ray.is_empty());
    assert_eq!(b.line, "diagonal");
}

#[test]
fn alekhines_gun_emits_one_fact_and_suppresses_component_pairs() {
    // (P3) The classic gun: Qd1 ← Rd2 ← Rd3 on the d-file, muzzle reveal d4–d7 hitting
    // the pawn d7. The component pairs (Qd1,Rd2), (Rd2,Rd3) AND the reversed inner pair
    // (Rd3,Rd2) — which passes the own-pawn-muzzle guard because the queen on d1 is not
    // a pawn — must ALL be suppressed in favour of the single gun fact.
    let items = bt("6k1/3p4/8/8/8/3R4/3R4/3Q2K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one fact (the gun): {items:?}");
    let b = &items[0];
    assert_eq!(b.subtype, "alekhines_gun");
    assert_eq!(b.rear.id, "white-queen-d1");
    assert_eq!(b.front.id, "white-rook-d3", "the muzzle rook");
    let middle = b.middle.as_ref().expect("the gun names its middle rook");
    assert_eq!(middle.id, "white-rook-d2");
    assert_eq!(b.ray, vec!["d2"], "the gun ray contains the middle rook's square");
    assert_eq!(b.line, "file");
}

#[test]
fn bishop_behind_queen_is_a_queen_bishop_battery_too() {
    // (P4) Either order counts: Ba1 REAR behind Qe5 on the a1–h8 diagonal; reveal f6/g7
    // hits the black pawn g7. The reversed ordering dies at the a1 corner edge.
    let items = bt("6k1/6p1/8/4Q3/8/8/8/B5K1 w - - 0 1");
    assert_eq!(items.len(), 1, "exactly one battery: {items:?}");
    let b = &items[0];
    assert_eq!(b.subtype, "queen_bishop_battery");
    assert_eq!(b.rear.id, "white-bishop-a1");
    assert_eq!(b.front.id, "white-queen-e5");
    assert_eq!(b.ray, vec!["b2", "c3", "d4"]);
    assert_eq!(b.line, "diagonal");
}

// ── adversarial negatives (must NOT fire) ─────────────────────────────────────

#[test]
fn rejects_doubled_rooks_with_a_blocker_in_the_corridor() {
    // (N1) Rd1 and Rd5 share the d-file but the black knight d3 sits between them. A
    // naive same-file scan fires; blocker-aware rook_attacks(d1) stops at d3, so no
    // aligned pair exists.
    let items = bt("6k1/8/8/3R4/8/3n4/8/3R2K1 w - - 0 1");
    assert!(items.is_empty(), "a blocked corridor is no battery: {items:?}");
}

#[test]
fn rejects_a_battery_firing_into_its_own_pawn_wall() {
    // (N2) Rd1+Rd2 aligned, but the reveal beyond d2 is exactly the friendly pawn d3
    // with no enemy anywhere in the extension — the formation fires point-blank into
    // its own wall. The reversed ordering dies at the d1 edge.
    let items = bt("6k1/8/8/8/8/3P4/3R4/3R2K1 w - - 0 1");
    assert!(items.is_empty(), "own-pawn muzzle is no battery: {items:?}");
}

#[test]
fn rejects_a_queen_xraying_its_own_bishop_on_a_file() {
    // (N3) Qd1 behind the friendly Bd4 on the d-FILE: the bishop does not bear on
    // files, so this is a discovery precursor, not a battery. The orthogonal pass
    // excludes bishops as fronts; the diagonal pass finds no alignment.
    let items = bt("6k1/8/8/8/3B4/8/8/3Q2K1 w - - 0 1");
    assert!(items.is_empty(), "cross-class front is no battery: {items:?}");
}

#[test]
fn side_routing_black_batteries_via_the_probe() {
    // (N4) White to move, but BLACK has Rd8 behind Rd5 doubled on the d-file (the d5
    // muzzle projects down the open file; the d8 ordering dies at the edge). The
    // stm-view must be empty; the black-side probe must see exactly the one battery.
    let fen = "3r2k1/8/8/3r4/8/8/8/6K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    assert!(bt(fen).is_empty(), "white has no sliders at all");
    let FactCollection::Computed { items } = battery_opportunities_for(&pos, Color::Black)
    else {
        panic!("black probe should be computed (white is not in check)");
    };
    assert_eq!(items.len(), 1, "exactly one black battery: {items:?}");
    let b = &items[0];
    assert_eq!(b.subtype, "two_rooks_battery");
    assert_eq!(b.rear.id, "black-rook-d8");
    assert_eq!(b.rear.side, Side::Black);
    assert_eq!(b.front.id, "black-rook-d5");
    assert_eq!(b.line, "file");
}

#[test]
fn rejects_the_gun_impostor_with_the_queen_in_the_middle() {
    // (N5) Rd1 ← Qd2 ← Rd3: a sandwich, not Alekhine's Gun (the queen must be
    // rearmost). Expect exactly the three generic Q/R mixed pairs — (Qd2,Rd3) up,
    // (Rd1,Qd2) up, (Rd3,Qd2) down — in canonical rear-id order, and emphatically no
    // alekhines_gun and no two_rooks_battery (Rd1–Rd3 is blocked by the queen).
    let items = bt("6k1/3p4/8/8/8/3R4/3Q4/3R2K1 w - - 0 1");
    assert_eq!(items.len(), 3, "exactly three generic batteries: {items:?}");
    assert!(items.iter().all(|b| b.subtype == "battery"));
    assert!(items.iter().all(|b| b.middle.is_none()));
    let pairs: Vec<(&str, &str)> = items
        .iter()
        .map(|b| (b.rear.id.as_str(), b.front.id.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("white-queen-d2", "white-rook-d3"),
            ("white-rook-d1", "white-queen-d2"),
            ("white-rook-d3", "white-queen-d2"),
        ],
        "canonical order: white-queen-d2 < white-rook-d1 < white-rook-d3"
    );
}

#[test]
fn batteries_are_reported_even_while_in_check_but_the_enemy_probe_is_not() {
    // (N6) White to move and IN CHECK from re1. A battery is a standing structural
    // truth with no move claim, so the stm view stays COMPUTED and contains the upward
    // Rd2←Rd3 two-rooks battery (contrast overload, which must bail in check). The
    // downward (Rd3,Rd2) ordering also survives — it projects into the empty d1 — so
    // two facts total. The BLACK probe is Unavailable: granting black another turn
    // while it already checks would fake an illegal king-capture state.
    let fen = "6k1/3p4/8/8/8/3R4/3R4/4r1K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let items = bt(fen);
    assert_eq!(items.len(), 2, "both directions project: {items:?}");
    let up = on_rear(&items, "d2").expect("the upward battery must be present in check");
    assert_eq!(up.subtype, "two_rooks_battery");
    assert_eq!(up.front.square, "d3");
    match battery_opportunities_for(&pos, Color::Black) {
        FactCollection::Unavailable { reason } => {
            assert_eq!(reason, "opposite_side_probe_while_in_check");
        }
        other => panic!("black probe must be unavailable while black checks, got {other:?}"),
    }
}

// ── invariants ────────────────────────────────────────────────────────────────

#[test]
fn no_battery_in_the_opening_position() {
    // Every slider pair in the start position is blocked (knights/bishops/queen in the
    // corridors) or has no line-class alignment at all.
    assert!(
        bt("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_empty(),
        "the start position has no batteries"
    );
}

#[test]
fn enumeration_does_not_mutate_the_position() {
    let fen = "6k1/3p4/8/8/8/3R4/3R4/3Q2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let _ = battery_opportunities(&pos);
    let _ = battery_opportunities_for(&pos, Color::Black);
    assert_eq!(
        pos.to_fen(),
        fen,
        "battery enumeration must not mutate the board"
    );
}

#[test]
fn enumeration_is_deterministic() {
    let fen = "6k1/3p4/8/8/8/3R4/3Q4/3R2K1 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let a = battery_opportunities(&pos);
    let b = battery_opportunities(&pos);
    assert_eq!(a, b, "same position, same facts, same order");
}
