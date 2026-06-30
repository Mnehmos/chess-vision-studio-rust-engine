//! Opponent option-compression specialist (#1). Verifies the reply-quality distribution:
//! position-preserving, the only-move / checkmate edges, and the monotonic viable-count invariant.
use cvs_bitboard_core::eval::ValueWeights;
use cvs_bitboard_core::facts::reply_compression::{
    reply_compression_facts, ReplyCompressionConfig, ReplyConfidence,
};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::{Move, Position};

// A shallow probe keeps the per-reply searches cheap; the count/structure assertions below do not
// depend on the exact evals, only on their ordering.
fn shallow() -> ReplyCompressionConfig {
    ReplyCompressionConfig {
        search_depth: 4,
        top_replies_cap: 6,
        standard_depth: 8,
    }
}

fn move_by_uci(fen: &str, uci: &str) -> Move {
    generate_legal(&mut Position::from_fen(fen).unwrap())
        .into_iter()
        .find(|m| m.to_uci() == uci)
        .unwrap_or_else(|| panic!("no legal move {uci} in {fen}"))
}

fn legal_ucis(pos: &mut Position) -> Vec<String> {
    let mut v: Vec<String> = generate_legal(pos).iter().map(|m| m.to_uci()).collect();
    v.sort();
    v
}

#[test]
fn illegal_candidate_returns_none_and_preserves_position() {
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let before = legal_ucis(&mut pos);
    // a1a8 is legal on this bare-rook board but NOT in the start position.
    let bogus = move_by_uci("7k/8/8/8/8/8/8/R6K w - - 0 1", "a1a8");
    assert!(reply_compression_facts(&mut pos, bogus, &ValueWeights::default(), shallow()).is_none());
    assert_eq!(legal_ucis(&mut pos), before, "position must be unchanged");
}

#[test]
fn preserves_position_after_a_valid_call() {
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let before = legal_ucis(&mut pos);
    let e4 = move_by_uci(fen, "e2e4");
    let facts = reply_compression_facts(&mut pos, e4, &ValueWeights::default(), shallow()).unwrap();
    assert_eq!(facts.candidate_move, "e2e4");
    assert_eq!(facts.schema_version, 1);
    assert_eq!(facts.confidence, ReplyConfidence::Shallow); // depth 4 < standard 8
    assert_eq!(legal_ucis(&mut pos), before, "make/search/unmake must restore the position");
}

#[test]
fn monotonic_viable_counts_and_best_first_ordering() {
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let e4 = move_by_uci(fen, "e2e4");
    let f = reply_compression_facts(&mut pos, e4, &ValueWeights::default(), shallow()).unwrap();

    assert_eq!(f.legal_reply_count, 20); // Black has 20 replies after 1.e4
    // viable counts are nested by tolerance and bounded by the legal count.
    assert!(f.viable_reply_count_25cp >= 1);
    assert!(f.viable_reply_count_25cp <= f.viable_reply_count_50cp);
    assert!(f.viable_reply_count_50cp <= f.viable_reply_count_100cp);
    assert!(f.viable_reply_count_100cp <= f.legal_reply_count);
    // best is the first scored reply; second never exceeds it.
    assert_eq!(f.best_reply.as_deref(), f.top_replies.first().map(|r| r.reply.as_str()));
    assert_eq!(f.best_reply_eval_cp, f.top_replies.first().map(|r| r.eval_cp));
    if let (Some(b), Some(s)) = (f.best_reply_eval_cp, f.second_reply_eval_cp) {
        assert!(s <= b);
    }
    assert!(f.top_replies.len() <= 6);
    assert!((f.viable_fraction_50cp - f.viable_reply_count_50cp as f32 / 20.0).abs() < 1e-6);
}

#[test]
fn single_legal_reply_is_an_only_move() {
    // After Ra1-a8+, the only legal reply is Kg8-h7 (f8/h8 are 8th-rank checks; f7/g7 occupied).
    let fen = "6k1/5pp1/7p/8/8/8/8/R6K w - - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let ra8 = move_by_uci(fen, "a1a8");
    let f = reply_compression_facts(&mut pos, ra8, &ValueWeights::default(), shallow()).unwrap();
    assert_eq!(f.legal_reply_count, 1);
    assert_eq!(f.best_reply.as_deref(), Some("g8h7"));
    assert_eq!(f.viable_reply_count_25cp, 1);
    assert_eq!(f.viable_reply_count_100cp, 1);
    assert!(f.only_move_25cp && f.only_move_50cp);
    assert_eq!(f.second_reply_eval_cp, None);
    assert_eq!(f.punishment_cliff_cp, None); // no inferior reply to fall to
    assert!((f.viable_fraction_50cp - 1.0).abs() < 1e-6);
}

#[test]
fn checkmating_candidate_has_zero_replies() {
    // Ra1-a8 is mate here (f7/g7/h7 all occupied, so the king has no escape).
    let fen = "6k1/5ppp/8/8/8/8/8/R6K w - - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let ra8 = move_by_uci(fen, "a1a8");
    let f = reply_compression_facts(&mut pos, ra8, &ValueWeights::default(), shallow()).unwrap();
    assert_eq!(f.legal_reply_count, 0);
    assert_eq!(f.best_reply, None);
    assert_eq!(f.best_reply_eval_cp, None);
    assert_eq!(f.viable_reply_count_25cp, 0);
    assert!(!f.only_move_25cp);
    assert!(f.top_replies.is_empty());
    assert_eq!(f.viable_fraction_50cp, 0.0);
}

#[test]
fn best_reply_is_the_opponents_strongest_defense_not_the_worst() {
    // After Qd1-d4?? the white queen is en prise to Black's d5 queen; the opponent's clearly best
    // reply is Qxd4 (winning a queen). A sign inversion would surface a quiet move instead.
    let fen = "4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let qd4 = move_by_uci(fen, "d1d4");
    let f = reply_compression_facts(&mut pos, qd4, &ValueWeights::default(), shallow()).unwrap();
    assert_eq!(f.best_reply.as_deref(), Some("d5d4"), "best reply must be the queen capture");
    assert!(f.best_reply_eval_cp.unwrap() > 300, "the recapture is winning for the opponent");
}

#[test]
fn per_reply_eval_is_independent_of_sibling_replies() {
    // The fresh-searcher-per-reply contract: each reply's eval must equal an INDEPENDENT search of
    // that reply. A shared TT would let sibling reply subtrees contaminate one another (observed to
    // diverge from depth ~6). Depth 6 is where the contamination showed up in review.
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let cfg = ReplyCompressionConfig {
        search_depth: 6,
        top_replies_cap: 6,
        standard_depth: 8,
    };
    let mut pos = Position::from_fen(fen).unwrap();
    let e4 = move_by_uci(fen, "e2e4");
    let f = reply_compression_facts(&mut pos, e4, &ValueWeights::default(), cfg).unwrap();

    pos.make(e4);
    for fact in &f.top_replies {
        let r = generate_legal(&mut pos)
            .into_iter()
            .find(|m| m.to_uci() == fact.reply)
            .unwrap();
        pos.make(r);
        let mut s = Searcher::new(ValueWeights::default(), None);
        let independent = -s
            .search(
                &mut pos,
                SearchOptions {
                    depth: 6,
                    threads: 1,
                    ..Default::default()
                },
            )
            .score_cp;
        pos.unmake();
        assert_eq!(
            fact.eval_cp, independent,
            "reply {} eval must match an independent search",
            fact.reply
        );
    }
    pos.unmake();
}
