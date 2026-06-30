//! Residual-hybrid eval scaffold (#10): the zeroability contract (INV-7), additive separation, and
//! the steer-only/shippable split. The baseline is supplied by the real `eval::evaluate`.
use cvs_bitboard_core::eval::residual_hybrid::{
    handcrafted_delta_white, refuse_steer_only_for_promotion, residual_hybrid_eval, ProfileKind,
    ResidualHybridConfig, HANDCRAFTED_FEATURES, LAMBDA_SCALE,
};
use cvs_bitboard_core::eval::{evaluate, ValueWeights};
use cvs_bitboard_core::Position;

const POSITIONS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 0 1",
    "8/2p5/3k4/8/8/3K4/5P2/8 w - - 0 1",
    "4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1", // white bishop pair
];

fn baseline(fen: &str) -> (Position, i32) {
    let mut pos = Position::from_fen(fen).unwrap();
    let s = evaluate(&mut pos, &ValueWeights::default(), None);
    (pos, s)
}

#[test]
fn zeroed_config_recovers_baseline_byte_exact_without_extracting_features() {
    let cfg = ResidualHybridConfig::zeroed("champion-b23c75b8");
    assert!(cfg.is_zeroed());
    for fen in POSITIONS {
        let (pos, base) = baseline(fen);
        let r = residual_hybrid_eval(base, &pos, &cfg);
        assert_eq!(r.score_cp, base, "zeroed hybrid must recover the baseline exactly ({fen})");
        assert!(
            !r.telemetry.features_extracted,
            "the zero-weight fast path must skip feature extraction"
        );
        assert_eq!(r.telemetry.handcrafted_delta_cp, 0);
        assert_eq!(r.telemetry.residual_delta_cp, 0);
    }
}

#[test]
fn handcrafted_component_is_additive_and_stm_signed() {
    // White has the bishop pair (Bc1 + Bf1), Black has none -> white-POV balance = +1.
    let (pos, base) = baseline("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1");
    assert_eq!(handcrafted_delta_white(&pos, &[1]), 1);
    // lambda_h = LAMBDA_SCALE (= 1.0 in fixed-point), weight 100 cp/unit -> +100 cp for White to move.
    let mut cfg = ResidualHybridConfig::zeroed("champ");
    cfg.lambda_h = LAMBDA_SCALE;
    cfg.handcrafted_weights = vec![100];
    let r = residual_hybrid_eval(base, &pos, &cfg);
    assert!(r.telemetry.features_extracted);
    assert_eq!(r.telemetry.handcrafted_delta_cp, 100);
    assert_eq!(r.score_cp, base + 100);
}

#[test]
fn handcrafted_delta_is_stm_negated_for_black_to_move() {
    // Same material, Black to move: the white-POV +1 bishop-pair balance is -100 cp in stm POV.
    let (pos, base) = baseline("4k3/8/8/8/8/8/8/2B1KB2 b - - 0 1");
    let mut cfg = ResidualHybridConfig::zeroed("champ");
    cfg.lambda_h = LAMBDA_SCALE;
    cfg.handcrafted_weights = vec![100];
    let r = residual_hybrid_eval(base, &pos, &cfg);
    assert_eq!(r.score_cp, base - 100);
}

#[test]
fn bishop_pair_balance_signs() {
    let wp = Position::from_fen("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1").unwrap();
    let bp = Position::from_fen("2b1kb2/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let none = Position::from_fen("4k3/8/8/8/8/8/8/4KB2 w - - 0 1").unwrap();
    assert_eq!(handcrafted_delta_white(&wp, &[1]), 1);
    assert_eq!(handcrafted_delta_white(&bp, &[1]), -1);
    assert_eq!(handcrafted_delta_white(&none, &[1]), 0);
}

#[test]
fn steer_only_is_refused_for_promotion() {
    let mut steer = ResidualHybridConfig::zeroed("champ");
    steer.kind = ProfileKind::SteerOnly;
    assert!(refuse_steer_only_for_promotion(&steer).is_err());
    let ship = ResidualHybridConfig::zeroed("champ");
    assert_eq!(ship.kind, ProfileKind::Shippable);
    assert!(refuse_steer_only_for_promotion(&ship).is_ok());
}

#[test]
fn learned_residual_is_inert_until_a_model_is_wired() {
    // The learned-residual branch is a stub (returns 0) in slice 1, so a non-zero lambda_r does NOT
    // change the score yet. This documents the contract; it must be updated when the trained residual
    // lands — the test breaking is the signal that the branch went live.
    let (pos, base) = baseline("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let mut cfg = ResidualHybridConfig::zeroed("champ");
    cfg.lambda_r = LAMBDA_SCALE; // non-zero, but the stub residual is 0
    let r = residual_hybrid_eval(base, &pos, &cfg);
    assert_eq!(r.score_cp, base, "lambda_r is inert while learned_residual is a stub");
    assert_eq!(r.telemetry.residual_delta_cp, 0);
}

#[test]
fn registry_is_nonempty_and_named() {
    assert!(!HANDCRAFTED_FEATURES.is_empty());
    assert_eq!(HANDCRAFTED_FEATURES[0].name, "bishop_pair_balance");
}
