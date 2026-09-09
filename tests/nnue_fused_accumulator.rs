use cvs_bitboard_core::eval::{Accumulator, Nnue};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::{Move, MoveFlag, Position};

struct ModelFile(std::path::PathBuf);
impl Drop for ModelFile {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

fn model(hidden: usize) -> Nnue {
    let mut seed = 0x123456789abcdefu64;
    let mut value = || {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        ((seed % 20001) as f32 - 10000.0) / 17003.0
    };
    let w1: Vec<Vec<f32>> = (0..768).map(|_| (0..hidden).map(|_| value()).collect()).collect();
    let b1: Vec<f32> = (0..hidden).map(|_| value()).collect();
    let w2: Vec<f32> = (0..hidden).map(|_| value()).collect();
    let path = ModelFile(std::env::temp_dir().join(format!("cvs-fused-{}-{hidden}.json", std::process::id())));
    std::fs::write(&path.0, serde_json::to_vec(&serde_json::json!({
        "hidden": hidden, "outputScaleCp": 400.0, "w1": w1, "b1": b1, "w2": w2, "b2": 0.1,
    })).unwrap()).unwrap();
    Nnue::load(path.0.to_str().unwrap(), false).unwrap()
}

fn assert_bits(a: &Accumulator, b: &Accumulator) {
    for (left, right) in [(&a.white, &b.white), (&a.black, &b.black)] {
        assert_eq!(left.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                   right.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    }
}

fn check(net: &Nnue, parent: &Accumulator, pos: &Position, mv: Move) -> Accumulator {
    let original = parent.clone();
    let mut reference = parent.clone();
    net.acc_apply(&mut reference, pos, mv);
    // Dirty destination exercises slot reuse rather than accidentally relying on its contents.
    let mut child = Accumulator { white: vec![f32::NAN; parent.white.len()], black: vec![f32::NAN; parent.black.len()] };
    net.acc_apply_from(&mut child, parent, pos, mv);
    assert_bits(&child, &reference);
    assert_bits(parent, &original);
    child
}

#[test]
fn fused_matches_original_bits_for_every_special_move_and_odd_widths() {
    let fens = [
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
        "4k1r1/7P/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/7p/4K1R1 b - - 0 1",
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1",
    ];
    for hidden in [1, 7, 32, 256, 511] {
        let net = model(hidden);
        let (mut castles, mut ep, mut promotions, mut capture_promotions) = (0, 0, 0, 0);
        for fen in fens {
            let mut pos = Position::from_fen(fen).unwrap();
            let parent = net.fresh_acc(&pos);
            for mv in generate_legal(&mut pos) {
                check(&net, &parent, &pos, mv);
                castles += matches!(mv.flag, MoveFlag::KingCastle | MoveFlag::QueenCastle) as usize;
                ep += (mv.flag == MoveFlag::EnPassant) as usize;
                promotions += mv.flag.promo_piece().is_some() as usize;
                capture_promotions += (mv.flag.promo_piece().is_some() && mv.flag.is_capture()) as usize;
            }
        }
        assert_eq!((castles, ep, promotions, capture_promotions), (4, 2, 16, 8));
    }
}

#[test]
fn fused_matches_original_bits_through_random_branches_and_unmakes() {
    let net = model(256);
    let mut seed = 0x987654321abcdefu64;
    let mut count = 0;
    for _ in 0..24 {
        let mut pos = Position::startpos();
        let mut acc = net.fresh_acc(&pos);
        for ply in 0..100 {
            let moves = generate_legal(&mut pos);
            if moves.is_empty() { break; }
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
            let mv = moves[seed as usize % moves.len()];
            let next = check(&net, &acc, &pos, mv);
            pos.make(mv);
            count += 1;
            if ply % 7 == 0 {
                pos.unmake();
            } else {
                acc = next;
            }
        }
    }
    assert!(count > 2000);
}
