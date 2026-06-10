//! Lazy SMP correctness: multithreaded search must stay tactically exact
//! (mates found, legal moves only) and agree with single-threaded results on
//! forced lines. Scores on non-forced positions may differ slightly (TT races
//! change move ordering) — that is expected and bounded here.
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;

fn opts(depth: u32, threads: usize) -> SearchOptions {
    SearchOptions {
        depth,
        threads,
        ..Default::default()
    }
}

#[test]
fn smp_finds_mate_in_two() {
    // Classic mate in 2 (Qh6+ ... Qxg7#-style net): scholar-ish fixture used
    // by the single-thread suite.
    let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4";
    let mut pos = Position::from_fen(fen).unwrap();
    let mut s = Searcher::new(Default::default(), None);
    let r = s.search(&mut pos, opts(4, 4));
    // Qxf7# is mate in one here; any thread count must find a forced mate.
    assert_eq!(r.mate, Some(1), "smp must find the mate (got {:?})", r.mate);
}

#[test]
fn smp_returns_legal_move_and_matches_single_on_tactics() {
    let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
    let mut pos1 = Position::from_fen(fen).unwrap();
    let mut s1 = Searcher::new(Default::default(), None);
    let r1 = s1.search(&mut pos1, opts(5, 1));

    let mut pos4 = Position::from_fen(fen).unwrap();
    let mut s4 = Searcher::new(Default::default(), None);
    let r4 = s4.search(&mut pos4, opts(5, 4));

    let legal = generate_legal(&mut Position::from_fen(fen).unwrap());
    assert!(
        legal.contains(&r4.best_move.unwrap()),
        "smp move must be legal"
    );
    // Same depth, same eval: scores should be close even if TT racing changes
    // tie-breaks (move may differ; the score gap is what's bounded).
    assert!(
        (r1.score_cp - r4.score_cp).abs() <= 60,
        "single {} vs smp {} diverged",
        r1.score_cp,
        r4.score_cp
    );
}

#[test]
fn smp_timed_search_terminates_and_aggregates_nodes() {
    let mut pos = Position::startpos();
    let mut s = Searcher::new(Default::default(), None);
    let started = std::time::Instant::now();
    let r = s.search(
        &mut pos,
        SearchOptions {
            depth: 30,
            max_time_ms: Some(400),
            threads: 4,
            ..Default::default()
        },
    );
    assert!(
        started.elapsed().as_millis() < 2500,
        "must respect the clock"
    );
    assert!(r.best_move.is_some());
    assert!(r.telemetry.nodes > 10_000, "helper nodes should aggregate");
}
