//! Stabilization report tests (#7). Synthetic iterative-deepening trajectories drive each
//! status: a settled line is StableAtBudget; best-move churn or a parity score-swing is
//! UnstableTrajectory; no/partial-shifted completion is UnresolvedAtBudget; a tablebase
//! source is ExactTablebase; and an otherwise-stable result with heavy negative-SEE skips is
//! OmissionRisk. The report is pure analysis, so these need no live search.
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{
    stabilization_report, PartialIteration, SearchIteration, SearchResult, SearchResultSource,
    SearchTermination, StabilizationStatus, Telemetry,
};
use cvs_bitboard_core::{Move, Position};

const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn two_moves() -> (Move, Move) {
    let mut p = Position::from_fen(STARTPOS).unwrap();
    let mvs: Vec<Move> = generate_legal(&mut p).iter().copied().collect();
    (mvs[0], mvs[1])
}

fn it(depth: u32, best: Move, score: i32) -> SearchIteration {
    SearchIteration {
        depth,
        best_move: Some(best),
        score_cp: score,
        nodes: 1000 * depth as u64,
        time_ms: depth as u64,
        pv: vec![best],
    }
}

fn result(
    iters: Vec<SearchIteration>,
    source: SearchResultSource,
    see_skips: u64,
    partial: Option<PartialIteration>,
) -> SearchResult {
    let mut tel = Telemetry::default();
    tel.see_prune_skips = see_skips;
    let last = iters.last();
    SearchResult {
        best_move: last.and_then(|i| i.best_move),
        score_cp: last.map(|i| i.score_cp).unwrap_or(0),
        mate: None,
        pv: last.map(|i| i.pv.clone()).unwrap_or_default(),
        depth: last.map(|i| i.depth).unwrap_or(0),
        telemetry: tel,
        iterations: iters,
        root_order: Vec::new(),
        attempted_depth: 0,
        termination: SearchTermination::DepthLimit,
        result_source: source,
        partial_iteration: partial,
    }
}

#[test]
fn settled_trajectory_is_stable_at_budget() {
    let (m1, _) = two_moves();
    let r = result(
        vec![
            it(17, m1, 20),
            it(18, m1, 25),
            it(19, m1, 22),
            it(20, m1, 20),
        ],
        SearchResultSource::CompletedIteration,
        0,
        None,
    );
    let rep = stabilization_report(&r);
    assert_eq!(rep.status, StabilizationStatus::StableAtBudget);
    assert_eq!(rep.best_move_changes, 0);
    assert!(rep.max_adjacent_swing_cp <= 30);
}

#[test]
fn best_move_churn_is_unstable() {
    let (m1, m2) = two_moves();
    let r = result(
        vec![
            it(17, m1, 20),
            it(18, m2, 30),
            it(19, m1, 25),
            it(20, m2, 28),
        ],
        SearchResultSource::CompletedIteration,
        0,
        None,
    );
    let rep = stabilization_report(&r);
    assert_eq!(rep.status, StabilizationStatus::UnstableTrajectory);
    assert!(rep.best_move_changes >= 1);
    assert!(rep.reasons.contains(&"best-move-churn"));
}

#[test]
fn parity_score_swing_is_unstable() {
    let (m1, _) = two_moves();
    let r = result(
        vec![
            it(17, m1, 20),
            it(18, m1, 220),
            it(19, m1, 20),
            it(20, m1, 220),
        ],
        SearchResultSource::CompletedIteration,
        0,
        None,
    );
    let rep = stabilization_report(&r);
    assert_eq!(rep.status, StabilizationStatus::UnstableTrajectory);
    assert!(rep.max_adjacent_swing_cp >= 200);
    assert!(rep.odd_even_oscillation);
}

#[test]
fn mate_flip_in_window_is_unstable() {
    let (m1, _) = two_moves();
    // an eval that resolves into a mate score (>= engine MATE_THRESHOLD = 999_000) within the
    // window is not "settled" — exercises the mate_flip branch + reason.
    let r = result(
        vec![it(18, m1, 30), it(19, m1, 35), it(20, m1, 999_995)],
        SearchResultSource::CompletedIteration,
        0,
        None,
    );
    let rep = stabilization_report(&r);
    assert_eq!(rep.status, StabilizationStatus::UnstableTrajectory);
    assert!(rep.mate_flip);
    assert!(rep.reasons.contains(&"mate-flip"));
}

#[test]
fn no_completed_iteration_is_unresolved() {
    let r = result(vec![], SearchResultSource::NoCompletedIteration, 0, None);
    assert_eq!(
        stabilization_report(&r).status,
        StabilizationStatus::UnresolvedAtBudget
    );
}

#[test]
fn partial_iteration_shift_is_unresolved() {
    let (m1, m2) = two_moves();
    let partial = PartialIteration {
        depth: 21,
        alpha: -100,
        beta: 100,
        root_order: vec![],
        completed_candidates: vec![],
        provisional_best: Some(m2), // shifting away from the completed best (m1)
        provisional_score_cp: Some(40),
    };
    let r = result(
        vec![it(18, m1, 20), it(19, m1, 22), it(20, m1, 21)],
        SearchResultSource::CompletedIteration,
        0,
        Some(partial),
    );
    let rep = stabilization_report(&r);
    assert_eq!(rep.status, StabilizationStatus::UnresolvedAtBudget);
    assert!(rep.reasons.contains(&"partial-iteration-move-shift"));
}

#[test]
fn high_negative_see_skips_flag_omission_risk() {
    let (m1, _) = two_moves();
    let r = result(
        vec![
            it(17, m1, 20),
            it(18, m1, 22),
            it(19, m1, 21),
            it(20, m1, 20),
        ],
        SearchResultSource::CompletedIteration,
        5000, // many negative-SEE qsearch skips on an otherwise-settled line
        None,
    );
    assert_eq!(
        stabilization_report(&r).status,
        StabilizationStatus::OmissionRisk
    );
}

#[test]
fn tablebase_source_is_exact() {
    let (m1, _) = two_moves();
    let r = result(vec![it(1, m1, 0)], SearchResultSource::Tablebase, 0, None);
    assert_eq!(
        stabilization_report(&r).status,
        StabilizationStatus::ExactTablebase
    );
}

#[test]
fn report_is_deterministic() {
    let (m1, _) = two_moves();
    let r = result(
        vec![it(18, m1, 20), it(19, m1, 22), it(20, m1, 21)],
        SearchResultSource::CompletedIteration,
        0,
        None,
    );
    let a = stabilization_report(&r);
    let b = stabilization_report(&r);
    assert_eq!(a.status, b.status);
    assert_eq!(a.reasons, b.reasons);
    assert_eq!(a.max_adjacent_swing_cp, b.max_adjacent_swing_cp);
}
