//! RSI loop 1 — danger-trigger tests. The sf2200-g14 loss FENs (king exposed,
//! enemy queen active) must fire the trigger; quiet/normal positions must not.
//! The depth-extension behavior itself is gated by the arena harness (gates
//! decide promotion); these tests pin the trigger's shape.
use cvs_bitboard_core::search::{danger_level, SearchOptions, Searcher};
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::Position;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

// sf2200-g14 regression FENs (see arena rsi/regressions.jsonl):
const G14_PLY24: &str = "r1b2r1k/ppp3pp/2n5/4pP2/2BP1B2/q1P3Q1/P1K2PPP/R6R w - - 2 16"; // Kc2, q a3
const G14_PLY40: &str = "3r4/ppp1Qbkp/5r2/8/2B5/2P5/Pq3PPP/2R1K2R w - - 3 24"; // q b2, Rf6/Bf7 battery

#[test]
fn danger_fires_on_the_g14_loss_positions() {
    assert!(danger_level(&pos(G14_PLY24)) >= 1, "ply-24 position must trigger danger");
    assert_eq!(danger_level(&pos(G14_PLY40)), 2, "ply-40 mate-net position must be critical");
}

#[test]
fn danger_quiet_positions_do_not_fire() {
    assert_eq!(danger_level(&pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")), 0);
    // No enemy queen → never fires, however exposed the king.
    assert_eq!(danger_level(&pos("8/2k5/8/8/3K4/8/5R2/8 w - - 0 1")), 0);
    assert_eq!(danger_level(&pos("8/2k5/8/8/3K4/8/5r2/8 w - - 0 1")), 0);
}

#[test]
fn danger_extension_is_off_by_default_and_bounded_when_on() {
    let mut p = pos(G14_PLY40);
    let mut s = Searcher::new(ValueWeights::default(), None);
    let off = s.search(&mut p, SearchOptions { depth: 2, ..Default::default() });
    assert_eq!(off.telemetry.danger_extension_plies, 0);
    assert_eq!(off.depth, 2);
    let mut s2 = Searcher::new(ValueWeights::default(), None);
    let on = s2.search(&mut p, SearchOptions { depth: 2, danger_extension: true, ..Default::default() });
    assert_eq!(on.telemetry.danger_extension_plies, 2);
    assert!(on.depth <= 4, "extension is capped at +2");
}
