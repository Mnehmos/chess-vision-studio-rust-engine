//! SEE-advisory verification mode (#3). With `see_verify` set, qsearch admits a negative-SEE
//! capture/promo that fires a tactical-volatility trigger (gives check or is a promotion)
//! instead of vetoing it on the static exchange alone — verifying the sacrifice. Default off
//! is byte-for-byte the plain SEE veto (baseline-preserving).
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::search::{SearchOptions, SearchResult, Searcher};
use cvs_bitboard_core::Position;

// A sharp position (exposed king, active queen) whose q-tree contains negative-SEE checking
// captures.
const SHARP: &str = "r1b2r1k/ppp3pp/2n5/4pP2/2BP1B2/q1P3Q1/P1K2PPP/R6R w - - 2 16";

fn search(fen: &str, see_verify: bool, depth: u32) -> SearchResult {
    Searcher::new(ValueWeights::default(), None).search(
        &mut Position::from_fen(fen).unwrap(),
        SearchOptions {
            depth,
            see_verify,
            threads: 1,
            ..Default::default()
        },
    )
}

#[test]
fn see_verify_off_admits_nothing() {
    // Baseline: the verify counter only moves under the flag.
    let r = search(SHARP, false, 8);
    assert_eq!(r.telemetry.see_verify_kept, 0);
}

#[test]
fn see_verify_on_admits_negative_see_sacrifices() {
    let off = search(SHARP, false, 8);
    let on = search(SHARP, true, 8);
    assert_eq!(off.telemetry.see_verify_kept, 0);
    assert!(
        on.telemetry.see_verify_kept > 0,
        "expected a tactical-volatility trigger to admit a negative-SEE sacrifice"
    );
    // verifying the sacrifices widens the q-tree -> the ON search demonstrably differs from OFF.
    assert!(
        on.telemetry.nodes != off.telemetry.nodes,
        "see_verify should change the search on a sharp position"
    );
}

#[test]
fn see_verify_is_inert_with_no_volatility_trigger() {
    // KR vs K: no captures of a defended piece and no pawns -> no negative-SEE capture/promo
    // ever fires the trigger, so the verify path produces exactly the plain SEE veto: ON == OFF.
    const QUIET: &str = "8/2k5/8/8/3K4/8/5R2/8 w - - 0 1";
    let off = search(QUIET, false, 10);
    let on = search(QUIET, true, 10);
    assert_eq!(on.telemetry.see_verify_kept, 0);
    assert_eq!(
        on.best_move.map(|m| m.to_uci()),
        off.best_move.map(|m| m.to_uci())
    );
    assert_eq!(on.score_cp, off.score_cp);
    assert_eq!(on.telemetry.nodes, off.telemetry.nodes);
}

#[test]
fn see_verify_off_is_deterministic_baseline() {
    let a = search(SHARP, false, 8);
    let b = search(SHARP, false, 8);
    assert_eq!(
        a.best_move.map(|m| m.to_uci()),
        b.best_move.map(|m| m.to_uci())
    );
    assert_eq!(a.score_cp, b.score_cp);
    assert_eq!(a.telemetry.nodes, b.telemetry.nodes);
    assert_eq!(a.telemetry.see_verify_kept, 0);
}
