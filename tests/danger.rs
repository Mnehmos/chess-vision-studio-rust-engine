//! RSI loop 1 — danger-trigger tests. The sf2200-g14 loss FENs (king exposed,
//! enemy queen active) must fire the trigger; quiet/normal positions must not.
//! The depth-extension behavior itself is gated by the arena harness (gates
//! decide promotion); these tests pin the trigger's shape.
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::search::{danger_level, SearchOptions, Searcher};
use cvs_bitboard_core::Position;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

// sf2200-g14 regression FENs (see arena rsi/regressions.jsonl):
const G14_PLY24: &str = "r1b2r1k/ppp3pp/2n5/4pP2/2BP1B2/q1P3Q1/P1K2PPP/R6R w - - 2 16"; // Kc2, q a3
const G14_PLY40: &str = "3r4/ppp1Qbkp/5r2/8/2B5/2P5/Pq3PPP/2R1K2R w - - 3 24"; // q b2, Rf6/Bf7 battery

// Lichess 4fxkLVBb, halcyonbot-CVS: black king walked from e8/f8/f7
// into a mate net. This is the last black move before Qe8+ forces mate.
const HALCYON_PLY38: &str = "r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19";

#[test]
fn danger_fires_on_the_g14_loss_positions() {
    assert!(
        danger_level(&pos(G14_PLY24)) >= 1,
        "ply-24 position must trigger danger"
    );
    assert_eq!(
        danger_level(&pos(G14_PLY40)),
        2,
        "ply-40 mate-net position must be critical"
    );
}

#[test]
fn danger_fires_on_the_halcyon_king_walk_position() {
    assert_eq!(
        danger_level(&pos(HALCYON_PLY38)),
        2,
        "halcyon pre-Bd6 mate-net position must be critical"
    );
}

#[test]
fn danger_quiet_positions_do_not_fire() {
    assert_eq!(
        danger_level(&pos(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        )),
        0
    );
    // No enemy queen → never fires, however exposed the king.
    assert_eq!(danger_level(&pos("8/2k5/8/8/3K4/8/5R2/8 w - - 0 1")), 0);
    assert_eq!(danger_level(&pos("8/2k5/8/8/3K4/8/5r2/8 w - - 0 1")), 0);
}

#[test]
fn danger_extension_is_off_by_default_and_bounded_when_on() {
    let mut p = pos(G14_PLY40);
    let mut s = Searcher::new(ValueWeights::default(), None);
    let off = s.search(
        &mut p,
        SearchOptions {
            depth: 2,
            ..Default::default()
        },
    );
    assert_eq!(off.telemetry.danger_extension_plies, 0);
    assert_eq!(off.depth, 2);
    let mut s2 = Searcher::new(ValueWeights::default(), None);
    let on = s2.search(
        &mut p,
        SearchOptions {
            depth: 2,
            danger_extension: true,
            ..Default::default()
        },
    );
    assert_eq!(on.telemetry.danger_extension_plies, 2);
    assert!(on.depth <= 4, "extension is capped at +2");
}

// --- 2B v3: nonlinear king-danger feature shape ---
use cvs_bitboard_core::eval::extract_rung2;

#[test]
fn king_danger_zero_on_quiet_positions() {
    let f = extract_rung2(&pos(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ));
    assert_eq!(f.king_danger, 0.0, "start position has no king attack");
    // Lone rook near the king: one attacker is a nuisance, not an attack.
    let f = extract_rung2(&pos("8/2k5/8/8/3K4/8/5r2/8 w - - 0 1"));
    assert_eq!(
        f.king_danger, 0.0,
        "single attacker must not register danger"
    );
}

#[test]
fn king_danger_fires_on_the_g14_loss_positions() {
    // White king under a queen+rook (and worse) attack: feature must be
    // NEGATIVE (white-POV signed; danger at the white king favors Black).
    let f40 = extract_rung2(&pos(G14_PLY40));
    assert!(
        f40.king_danger < 0.0,
        "g14 ply-40 mate net must register white-king danger, got {}",
        f40.king_danger
    );
    // The quadratic must make the ply-40 mate net clearly worse than a mild
    // two-attacker poke.
    let f24 = extract_rung2(&pos(G14_PLY24));
    assert!(
        f40.king_danger <= f24.king_danger,
        "mate-net danger ({}) must be at least as severe as ply-24 ({})",
        f40.king_danger,
        f24.king_danger
    );
}

#[test]
fn king_danger_fires_on_the_halcyon_king_walk_position() {
    let f = extract_rung2(&pos(HALCYON_PLY38));
    assert!(
        f.king_danger > 4.0,
        "halcyon pre-Bd6 position must register black-king danger, got {}",
        f.king_danger
    );
    assert!(
        f.king_central_exposure > 0.0,
        "black king should be marked as centrally exposed, got {}",
        f.king_central_exposure
    );
    assert!(
        f.king_escape_deficit < 0.0,
        "black king should have an escape deficit from white POV, got {}",
        f.king_escape_deficit
    );
}
