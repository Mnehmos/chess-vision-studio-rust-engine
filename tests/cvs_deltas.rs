//! CVS Delta Reconstruction and Color Symmetry Tests.
use cvs_bitboard_core::eval::cvs_features::{
    extract_candidate_delta, extract_cvs_ids_into, ids_to_bitset,
    RootGeometryContext, MIRROR_FEATURE_ID,
};
use cvs_bitboard_core::movegen::generate_legal;
use cvs_bitboard_core::{Color, Move, Position};

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

fn unflip_id(id: u32, flip: bool) -> u32 {
    if flip {
        MIRROR_FEATURE_ID[(id % 168) as usize] as u32 + (id / 168) * 168
    } else {
        id
    }
}

fn mirror_position(pos: &Position) -> Position {
    let fen = pos.to_fen();
    let parts: Vec<&str> = fen.split_whitespace().collect();
    let board_part = parts[0];
    let stm_part = parts[1];
    let castling_part = parts[2];
    let ep_part = parts[3];
    let halfmove = parts[4];
    let fullmove = parts[5];

    // Mirror board vertically and swap piece colors
    let rows: Vec<&str> = board_part.split('/').collect();
    let mut mirrored_rows = Vec::new();
    for row in rows.iter().rev() {
        let mut mirrored_row = String::new();
        for c in row.chars() {
            if c.is_ascii_digit() {
                mirrored_row.push(c);
            } else if c.is_lowercase() {
                mirrored_row.push(c.to_ascii_uppercase());
            } else {
                mirrored_row.push(c.to_ascii_lowercase());
            }
        }
        mirrored_rows.push(mirrored_row);
    }
    let mirrored_board = mirrored_rows.join("/");

    // Swap side to move
    let mirrored_stm = if stm_part == "w" { "b" } else { "w" };

    // Swap castling rights colors
    let mirrored_castling = if castling_part != "-" {
        let mut chars: Vec<char> = castling_part
            .chars()
            .map(|c| {
                if c.is_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        chars.sort_by(|a, b| {
            let val = |c: char| match c {
                'K' => 0,
                'Q' => 1,
                'k' => 2,
                'q' => 3,
                _ => 4,
            };
            val(*a).cmp(&val(*b))
        });
        chars.into_iter().collect()
    } else {
        "-".to_string()
    };

    // Mirror en passant square
    let mirrored_ep = if ep_part != "-" {
        let file = ep_part.chars().next().unwrap();
        let rank = ep_part.chars().nth(1).unwrap();
        let new_rank = if rank == '3' { '6' } else { '3' };
        format!("{}{}", file, new_rank)
    } else {
        "-".to_string()
    };

    let mirrored_fen = format!(
        "{} {} {} {} {} {}",
        mirrored_board, mirrored_stm, mirrored_castling, mirrored_ep, halfmove, fullmove
    );
    Position::from_fen(&mirrored_fen).unwrap()
}

fn mirror_square(sq: u8) -> u8 {
    sq ^ 56
}

fn mirror_move(mv: Move) -> Move {
    Move {
        from: mirror_square(mv.from),
        to: mirror_square(mv.to),
        flag: mv.flag,
    }
}

fn assert_approx_eq(a: f32, b: f32, msg: &str) {
    assert!(
        (a - b).abs() < 1e-5,
        "{}: {} != {} (diff {})",
        msg,
        a,
        b,
        (a - b).abs()
    );
}

#[test]
fn test_delta_reconstruction_and_color_symmetry() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", // startpos
        "r1b3nr/1pp1bkpp/p1n5/1q3p2/3P4/B1PNQ1P1/P4PBP/RN2R1K1 b - - 3 19", // tactical midgame
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", // complex
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", // endgame
    ];

    for fen in fens {
        let mut pos = pos(fen);
        let moves = generate_legal(&mut pos);
        
        // Find a couple of quiet moves to test
        let quiet_moves: Vec<Move> = moves
            .into_iter()
            .filter(|mv| !mv.flag.is_capture() && mv.flag.promo_piece().is_none())
            .collect();

        if quiet_moves.is_empty() {
            continue;
        }

        // Cache parent features
        let mut parent_ids = Vec::new();
        extract_cvs_ids_into(&pos, &mut parent_ids);
        let parent_bitset = ids_to_bitset(&parent_ids);
        let ctx = RootGeometryContext {
            parent_bitset,
            mover: pos.stm,
        };

        for mv in quiet_moves.into_iter().take(3) {
            let mut sparse_buf = Vec::new();
            let mut dense_buf = [0.0f32; 32];
            extract_candidate_delta(&ctx, &pos, mv, &mut sparse_buf, &mut dense_buf, 100, 150);

            // --- 1. Delta Reconstruction Verification ---
            let flip = pos.stm == Color::Black;
            let mut reconstructed_parent = Vec::new();
            let mut added = Vec::new();
            let mut removed = Vec::new();

            for &id in &sparse_buf {
                let unflipped = unflip_id(id, flip);
                if unflipped < 168 {
                    reconstructed_parent.push(unflipped);
                } else if unflipped < 336 {
                    added.push(unflipped - 168);
                } else if unflipped < 504 {
                    removed.push(unflipped - 336);
                }
            }

            // Verify parent reconstruction
            let mut expected_parent = parent_ids.clone();
            expected_parent.sort_unstable();
            let mut actual_reconstructed_parent = reconstructed_parent.clone();
            actual_reconstructed_parent.sort_unstable();
            assert_eq!(
                expected_parent, actual_reconstructed_parent,
                "Parent features mismatch on FEN: {}, move: {}",
                fen,
                mv.to_uci()
            );

            // Compute child position features
            let mut child = pos.clone();
            child.make(mv);
            let mut child_ids = Vec::new();
            extract_cvs_ids_into(&child, &mut child_ids);
            child_ids.sort_unstable();

            // child_reconstructed = parent - removed + added
            let mut child_reconstructed = parent_ids.clone();
            for r in removed {
                if let Some(pos) = child_reconstructed.iter().position(|&x| x == r) {
                    child_reconstructed.remove(pos);
                } else {
                    panic!("removed feature {} not in parent!", r);
                }
            }
            for a in added {
                child_reconstructed.push(a);
            }
            child_reconstructed.sort_unstable();
            child_reconstructed.dedup();

            assert_eq!(
                child_ids, child_reconstructed,
                "Child features delta reconstruction mismatch on FEN: {}, move: {}",
                fen,
                mv.to_uci()
            );

            // --- 2. Color Symmetry Verification ---
            let mpos = mirror_position(&pos);
            let mmv = mirror_move(mv);

            let mut mparent_ids = Vec::new();
            extract_cvs_ids_into(&mpos, &mut mparent_ids);
            let mparent_bitset = ids_to_bitset(&mparent_ids);
            let mctx = RootGeometryContext {
                parent_bitset: mparent_bitset,
                mover: mpos.stm,
            };

            let mut msparse_buf = Vec::new();
            let mut mdense_buf = [0.0f32; 32];
            extract_candidate_delta(&mctx, &mpos, mmv, &mut msparse_buf, &mut mdense_buf, 100, 150);

            // Sparse features must be EXACTLY identical under perspective mirroring
            assert_eq!(
                sparse_buf, msparse_buf,
                "Sparse feature symmetry mismatch on FEN: {}, move: {}",
                fen,
                mv.to_uci()
            );

            // Dense features symmetry checks
            // 0..6 piece one-hot: must be equal
            for j in 0..6 {
                assert_approx_eq(dense_buf[j], mdense_buf[j], &format!("piece one-hot {}", j));
            }
            // Files (6, 8) must be equal
            assert_approx_eq(dense_buf[6], mdense_buf[6], "source file");
            assert_approx_eq(dense_buf[8], mdense_buf[8], "dest file");
            // Ranks (7, 9) must be mirrored: mdense = 1.0 - dense
            assert_approx_eq(mdense_buf[7], 1.0 - dense_buf[7], "source rank");
            assert_approx_eq(mdense_buf[9], 1.0 - dense_buf[9], "dest rank");

            // 10 gives check
            assert_approx_eq(dense_buf[10], mdense_buf[10], "gives check");
            // 11 attacks higher
            assert_approx_eq(dense_buf[11], mdense_buf[11], "attacks higher");
            // 12 SEE
            assert_approx_eq(dense_buf[12], mdense_buf[12], "SEE");
            // 13 side to move: mdense = 1.0 - dense
            assert_approx_eq(mdense_buf[13], 1.0 - dense_buf[13], "side to move");
            // 14 material balance
            assert_approx_eq(dense_buf[14], mdense_buf[14], "material balance");
            // 15 game phase
            assert_approx_eq(dense_buf[15], mdense_buf[15], "game phase");

            // 16..20 king squares
            // Files (16, 18) must be equal
            assert_approx_eq(dense_buf[16], mdense_buf[16], "king us file");
            assert_approx_eq(dense_buf[18], mdense_buf[18], "king them file");
            // Ranks (17, 19) must be mirrored
            assert_approx_eq(mdense_buf[17], 1.0 - dense_buf[17], "king us rank");
            assert_approx_eq(mdense_buf[19], 1.0 - dense_buf[19], "king them rank");

            // 20..24 castling rights: must be equal
            for j in 20..24 {
                assert_approx_eq(dense_buf[j], mdense_buf[j], &format!("castling right {}", j));
            }
            // 24..26 raw score & diff: must be equal
            assert_approx_eq(dense_buf[24], mdense_buf[24], "raw score");
            assert_approx_eq(dense_buf[25], mdense_buf[25], "raw diff");
            // 26..29 move categories: must be equal
            for j in 26..29 {
                assert_approx_eq(dense_buf[j], mdense_buf[j], &format!("move category {}", j));
            }
        }
    }
}

#[test]
fn test_load_and_evaluate_ranker() {
    let path = "target-cvs/matrix-ranker.json";
    if !std::path::Path::new(path).exists() {
        return;
    }
    
    let nnue = cvs_bitboard_core::eval::Nnue::load(path, false).expect("load ranker model");
    assert!(nnue.is_ranker);
    assert_eq!(nnue.cvs_hidden, 32);
    
    let pos = Position::startpos();
    let mut parent_ids = Vec::new();
    extract_cvs_ids_into(&pos, &mut parent_ids);
    let parent_bitset = ids_to_bitset(&parent_ids);
    let ctx = RootGeometryContext {
        parent_bitset,
        mover: pos.stm,
    };
    
    let mut moves = generate_legal(&mut pos.clone());
    let mv = moves.remove(0);
    let mut sparse_buf = Vec::new();
    let mut dense_buf = [0.0f32; 32];
    extract_candidate_delta(&ctx, &pos, mv, &mut sparse_buf, &mut dense_buf, 0, 0);
    
    let logit = nnue.eval_ranker_raw(&sparse_buf, &dense_buf);
    assert!(!logit.is_nan());
}
