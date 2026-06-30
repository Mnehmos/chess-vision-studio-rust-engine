//! Issue #6 — deterministic fixed-node diagnostic search.
//!
//! INV-2 requires that the same position + options + node budget produce identical
//! results and telemetry from cold state. These tests pin that contract for the
//! single-thread `max_nodes` path: bit-identical repeats, an exact (never-exceeded)
//! budget, NodeLimit termination, a budget that actually governs the work, and inert
//! behavior when the feature is unused.
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::search::{SearchOptions, SearchResult, SearchTermination, Searcher};
use cvs_bitboard_core::Position;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

/// One search from COLD state: a fresh searcher, so the persistent TT, history, killers,
/// and lane caches all start empty and no prior search can bias the result.
fn cold(fen: &str, max_nodes: u64) -> SearchResult {
    Searcher::new(ValueWeights::default(), None).search(
        &mut pos(fen),
        SearchOptions {
            depth: 30, // high cap so the node budget — not depth — is what stops us
            max_nodes: Some(max_nodes),
            threads: 1,
            ..Default::default()
        },
    )
}

const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
// Sharp middlegames (qsearch-heavy) — exercise the node counter through tactical fans.
const SHARP1: &str = "r1b2r1k/ppp3pp/2n5/4pP2/2BP1B2/q1P3Q1/P1K2PPP/R6R w - - 2 16";
const SHARP2: &str = "3r4/ppp1Qbkp/5r2/8/2B5/2P5/Pq3PPP/2R1K2R w - - 3 24";

#[test]
fn fixed_node_search_is_bit_identical_across_cold_repeats() {
    for fen in [STARTPOS, SHARP1, SHARP2] {
        let budget = 20_000;
        let base = cold(fen, budget);
        assert_eq!(
            base.termination,
            SearchTermination::NodeLimit,
            "budget should bind (not depth/mate) for {fen}"
        );
        // time_up() fires at node entry once tel.nodes reaches the budget, so the search
        // never exceeds it.
        assert!(
            base.telemetry.nodes <= budget,
            "consumed {} exceeded budget {budget} for {fen}",
            base.telemetry.nodes
        );
        for run in 1..10 {
            let r = cold(fen, budget);
            assert_eq!(r.best_move, base.best_move, "best_move drift, run {run}, {fen}");
            assert_eq!(r.score_cp, base.score_cp, "score drift, run {run}, {fen}");
            assert_eq!(r.mate, base.mate, "mate drift, run {run}, {fen}");
            assert_eq!(r.pv, base.pv, "pv drift, run {run}, {fen}");
            assert_eq!(r.depth, base.depth, "depth drift, run {run}, {fen}");
            assert_eq!(
                r.telemetry.nodes, base.telemetry.nodes,
                "node-count drift, run {run}, {fen}"
            );
            assert_eq!(r.termination, base.termination, "termination drift, run {run}, {fen}");
        }
    }
}

#[test]
fn the_node_budget_governs_the_work_done() {
    // A larger budget must search strictly more nodes and reach at least as deep —
    // proving the budget is enforced rather than ignored.
    let shallow = cold(SHARP1, 5_000);
    let deep = cold(SHARP1, 80_000);
    assert_eq!(shallow.termination, SearchTermination::NodeLimit);
    assert_eq!(deep.termination, SearchTermination::NodeLimit);
    assert!(
        deep.telemetry.nodes > shallow.telemetry.nodes,
        "deep {} !> shallow {}",
        deep.telemetry.nodes,
        shallow.telemetry.nodes
    );
    assert!(
        deep.depth >= shallow.depth,
        "deep depth {} < shallow {}",
        deep.depth,
        shallow.depth
    );
}

#[test]
fn no_node_budget_is_inert_never_node_limit() {
    // Baseline recovery: with max_nodes = None (the default) a normal depth-bounded
    // search must behave exactly as before — it can never report NodeLimit.
    let r = Searcher::new(ValueWeights::default(), None).search(
        &mut pos(SHARP1),
        SearchOptions {
            depth: 6,
            ..Default::default()
        },
    );
    assert_ne!(r.termination, SearchTermination::NodeLimit);
    assert!(r.best_move.is_some());
}
