//! R2 eval sanity tests — mirror the legacy TS `value.test.ts` cases, plus the
//! Rung-2 invariants (start-position feature symmetry, inert default, reachable
//! capacity). Full numeric parity vs TS is proven by the `eval_parity` binary
//! over the 628-FEN fixture suite (max diff 0.000000cp).
use cvs_bitboard_core::eval::{
    evaluate, evaluate_white, evaluate_white_with_net, extract_rung2, feature_vector,
    material_balance_cp, non_king_material_cp, Rung2Weights, ValueNet, ValueWeights,
    RUNG3_INPUT_DIM,
};
use cvs_bitboard_core::Position;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

#[test]
fn startpos_near_equality() {
    // Only the tempo term should remain; PSTs and material are symmetric.
    let mut p = pos(START_FEN);
    assert!(evaluate_white(&mut p, &ValueWeights::default(), None).abs() <= 20);
}

#[test]
fn rewards_white_queen_up() {
    let mut p = pos("4k3/8/8/8/8/8/8/3QK3 w - - 0 1");
    assert!(evaluate_white(&mut p, &ValueWeights::default(), None) > 800);
}

#[test]
fn rewards_black_rook_up() {
    let mut p = pos("r3k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert!(evaluate_white(&mut p, &ValueWeights::default(), None) < -400);
}

#[test]
fn evaluate_is_side_to_move_relative() {
    // Black is up a rook with Black to move -> positive from their view.
    let mut p = pos("r3k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert!(evaluate(&mut p, &ValueWeights::default(), None) > 400);
}

#[test]
fn fifty_move_pressure_discounts_winning_eval_before_the_draw() {
    let mut fresh = pos("4k3/8/8/8/8/8/8/3QK3 w - - 0 1");
    let mut stale = pos("4k3/8/8/8/8/8/8/3QK3 w - - 90 46");
    let fresh_score = evaluate_white(&mut fresh, &ValueWeights::default(), None);
    let stale_score = evaluate_white(&mut stale, &ValueWeights::default(), None);

    assert!(fresh_score > 800);
    assert!(stale_score > 0);
    assert!(
        stale_score < fresh_score * 3 / 4,
        "high halfmove clock should make conversion urgent: fresh={fresh_score}, stale={stale_score}"
    );
}

#[test]
fn simplify_bonus_prefers_traded_down_material_leads() {
    let crowded = pos("1nbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQk - 0 1");
    let simple = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    assert_eq!(material_balance_cp(&crowded), material_balance_cp(&simple));
    assert!(non_king_material_cp(&crowded) > non_king_material_cp(&simple));

    let mut crowded_eval = crowded;
    let mut simple_eval = simple;
    let crowded_score = evaluate_white(&mut crowded_eval, &ValueWeights::default(), None);
    let simple_score = evaluate_white(&mut simple_eval, &ValueWeights::default(), None);
    assert!(
        simple_score > crowded_score + 30,
        "same material lead should improve after trades: crowded={crowded_score}, simple={simple_score}"
    );
}

#[test]
fn mate_and_stalemate_short_circuit() {
    // Fool's mate: White is checkmated.
    let mut mated = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    assert!(evaluate_white(&mut mated, &ValueWeights::default(), None) <= -999_999);
    // Classic stalemate: Black to move, no legal moves, not in check -> 0.
    let mut stale = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert_eq!(
        evaluate_white(&mut stale, &ValueWeights::default(), None),
        0
    );
}

#[test]
fn rung2_startpos_features_symmetric_zero() {
    let p = pos(START_FEN);
    let f = extract_rung2(&p);
    for v in [
        f.mobility_knight,
        f.mobility_bishop,
        f.mobility_rook,
        f.mobility_queen,
        f.king_shield,
        f.king_zone_pressure,
        f.king_open_file,
        f.passed_pawn_mg,
        f.passed_pawn_eg,
        f.connected_passed_pawn,
        f.rook_open_file,
        f.rook_semi_open_file,
        f.rook_seventh,
        f.doubled_pawn,
        f.isolated_pawn,
        f.bishop_pair_mg,
        f.bishop_pair_eg,
        f.hanging_piece,
    ] {
        assert!(
            v.abs() < 1e-9,
            "startpos Rung-2 features must be symmetric-zero"
        );
    }
}

#[test]
fn rung2_inert_default_and_reachable_capacity() {
    let fen = "4k3/8/8/8/8/8/8/R3K3 w - - 0 1";
    let mut p = pos(fen);
    let base = evaluate_white(&mut p, &ValueWeights::default(), None);
    // All-zero Rung-2 weights are byte-inert.
    let zero = Rung2Weights::default();
    assert_eq!(
        evaluate_white(&mut p, &ValueWeights::default(), Some(&zero)),
        base
    );
    // A non-zero weight changes the eval (open-file rook helps White).
    let mut w = Rung2Weights::default();
    w.rook_open_file = 50.0;
    assert!(evaluate_white(&mut p, &ValueWeights::default(), Some(&w)) > base);
}

#[test]
fn rung2_rook_open_file_signal() {
    // White rook a1 on an empty board: open file + rook mobility, White-positive.
    let p = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let f = extract_rung2(&p);
    assert!(f.rook_open_file > 0.0);
    assert!(f.mobility_rook > 0.0);
}

fn constant_net(cp: f64) -> ValueNet {
    ValueNet {
        input_dim: RUNG3_INPUT_DIM,
        hidden_dim: 0,
        w1: Vec::new(),
        b1: Vec::new(),
        w2: Vec::new(),
        b2: 1.0,
        output_scale_cp: cp,
    }
}

#[test]
fn rung3_feature_vector_has_stable_dimension() {
    let p = pos(START_FEN);
    let x = feature_vector(&p);
    assert_eq!(x.len(), RUNG3_INPUT_DIM);
}

#[test]
fn value_net_is_optional_and_white_pov() {
    let mut p = pos(START_FEN);
    let base = evaluate_white(&mut p, &ValueWeights::default(), None);
    assert_eq!(
        evaluate_white_with_net(&mut p, &ValueWeights::default(), None, None),
        base
    );

    let net = constant_net(37.0);
    net.validate().unwrap();
    assert_eq!(
        evaluate_white_with_net(&mut p, &ValueWeights::default(), None, Some(&net)),
        base + 37
    );
}
