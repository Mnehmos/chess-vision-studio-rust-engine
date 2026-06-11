//! Batch analyze CLI — the engine-backend interface for the gate + gauntlet
//! harnesses. Two modes:
//!   --fens <file>   batch: search every FEN in the file, one JSON line each
//!   --serve         loop: read FEN per stdin line, emit one JSON line per FEN
//!                   (one process serves a whole gauntlet run)
//!
//!   analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]
//!           [--no-quiet-checks] [--no-tt]
use cvs_bitboard_core::eval::{evaluate_white, Nnue, Rung2Weights, ValueWeights};
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let serve = args.iter().any(|a| a == "--serve");
    let fens_path = get("--fens");
    if !serve && fens_path.is_none() {
        eprintln!(
            "usage: analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]"
        );
        std::process::exit(2);
    }
    let depth: u32 = get("--depth")
        .expect("--depth required")
        .parse()
        .expect("depth");
    let base: ValueWeights = match get("--base") {
        Some(p) => serde_json::from_str(&std::fs::read_to_string(p).expect("base weights"))
            .expect("parse base"),
        None => ValueWeights::default(),
    };
    let rung2: Option<Rung2Weights> = get("--rung2").map(|p| {
        serde_json::from_str(&std::fs::read_to_string(p).expect("rung2 weights"))
            .expect("parse rung2")
    });
    let nnue: Option<Nnue> = get("--nnue").map(|p| Nnue::load(&p).expect("load nnue"));
    let opts = SearchOptions {
        depth,
        // --movetime <ms>: wall-clock cap for equal-clock matches (R4 fairness run).
        max_time_ms: get("--movetime").and_then(|s| s.parse().ok()),
        quiet_checks: !args.iter().any(|a| a == "--no-quiet-checks"),
        use_tt: !args.iter().any(|a| a == "--no-tt"),
        // --danger: danger-triggered root depth extension (RSI loop 1, gated).
        danger_extension: args.iter().any(|a| a == "--danger"),
        null_move: !args.iter().any(|a| a == "--no-null"),
        lmr: !args.iter().any(|a| a == "--no-lmr"),
        pvs: !args.iter().any(|a| a == "--no-pvs"),
        // Patch 7 prunes are opt-in for experiments (rejected as defaults).
        rfp: args.iter().any(|a| a == "--rfp"),
        futility: args.iter().any(|a| a == "--futility"),
        lmp: args.iter().any(|a| a == "--lmp"),
        see_prune: args.iter().any(|a| a == "--seeprune"),
        delta_prune: args.iter().any(|a| a == "--delta"),
        threads: get("--threads").and_then(|s| s.parse().ok()).unwrap_or(1),
        cvs_trace: args.iter().any(|a| a == "--cvs-trace"),
        cvs_helpers: get("--cvs-helpers")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        lane: cvs_bitboard_core::search::Lane::Fast,
    };

    // --features: emit the eval + Rung-2 feature vector per FEN instead of
    // searching — the training-data faucet for head fitting (TS orchestration
    // does the regression; Rust owns extraction).
    let features_mode = args.iter().any(|a| a == "--features");
    let features_one = |fen: &str| -> String {
        match Position::from_fen(fen) {
            Ok(mut pos) => {
                let f = cvs_bitboard_core::eval::extract_rung2(&pos);
                let eval_cp = evaluate_white(&mut pos, &base, rung2.as_ref());
                serde_json::json!({
                    "fen": fen,
                    "evalWhiteCp": eval_cp,
                    "features": {
                        "kingCentralExposure": f.king_central_exposure,
                        "enemyQueenNearKing": f.enemy_queen_near_king,
                        "openCenterKingPenalty": f.open_center_king_penalty,
                        "kingEscapeDeficit": f.king_escape_deficit,
                        "hangingPiece": f.hanging_piece,
                        "kingZonePressure": f.king_zone_pressure,
                        "kingShield": f.king_shield,
                        "kingOpenFile": f.king_open_file,
                        "kingDanger": f.king_danger,
                    },
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "fen": fen, "error": e }).to_string(),
        }
    };

    let analyze_one = |fen: &str| -> String {
        let mut pos = match Position::from_fen(fen) {
            Ok(p) => p,
            Err(e) => {
                return serde_json::json!({ "fen": fen, "error": e }).to_string();
            }
        };
        let mut searcher = match &nnue {
            Some(n) => Searcher::with_nnue(base, rung2, n.clone()),
            None => Searcher::new(base, rung2),
        };
        let r = searcher.search(&mut pos, opts);
        let t = r.telemetry;
        let uci = r.best_move.map(|m| m.to_uci());
        let pv: Vec<String> = r.pv.iter().map(|m| m.to_uci()).collect();
        serde_json::json!({
            "fen": fen,
            "uci": uci,
            "scoreCp": r.score_cp,
            "mate": r.mate,
            "pv": pv,
            "depth": r.depth,
            "nodes": t.nodes,
            "qNodes": t.q_nodes,
            "qCaptures": t.q_capture_nodes,
            "quietExt": t.quiet_check_extensions,
            "ttHits": t.tt_hits,
            "cutoffs": t.beta_cutoffs,
            "killerCutoffs": t.killer_cutoffs,
            "historyCutoffs": t.history_cutoffs,
            "nullCutoffs": t.null_cutoffs,
            "timeMs": t.elapsed_ms,
        })
        .to_string()
    };

    if serve {
        // Loop mode: one process serves a whole gauntlet run. Flush per line so
        // the orchestrator's request/response cycle never stalls on buffering.
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let fen = line.trim();
            if fen.is_empty() || fen == "quit" {
                if fen == "quit" {
                    break;
                }
                continue;
            }
            // `go <ms> <fen>` → search with a per-request wall-clock budget (the
            // Lichess bot's clock-budgeted picks; depth acts as the cap).
            let out = if let Some(rest) = fen.strip_prefix("go ") {
                let mut it = rest.splitn(2, ' ');
                let ms: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(500);
                let gfen = it.next().unwrap_or("").trim();
                match Position::from_fen(gfen) {
                    Ok(mut pos) => {
                        let mut searcher = match &nnue {
                            Some(n) => Searcher::with_nnue(base, rung2, n.clone()),
                            None => Searcher::new(base, rung2),
                        };
                        let timed = SearchOptions {
                            max_time_ms: Some(ms),
                            ..opts
                        };
                        let r = searcher.search(&mut pos, timed);
                        let t = r.telemetry;
                        serde_json::json!({
                            "fen": gfen,
                            "uci": r.best_move.map(|m| m.to_uci()),
                            "scoreCp": r.score_cp,
                            "mate": r.mate,
                            "pv": r.pv.iter().map(|m| m.to_uci()).collect::<Vec<_>>(),
                            "depth": r.depth,
                            "nodes": t.nodes,
                            "qNodes": t.q_nodes,
                            "ttHits": t.tt_hits,
                            "timeMs": t.elapsed_ms,
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({ "fen": gfen, "error": e }).to_string(),
                }
            } else if let Some(efen) = fen.strip_prefix("eval ") {
                match Position::from_fen(efen.trim()) {
                    Ok(mut pos) => {
                        let mut j = serde_json::json!({
                            "fen": efen.trim(),
                            "evalWhiteCp": evaluate_white(&mut pos, &base, rung2.as_ref()),
                        });
                        if let Some(n) = &nnue {
                            j["nnueStmCp"] = n.eval_stm(&pos).into();
                        }
                        j.to_string()
                    }
                    Err(e) => serde_json::json!({ "fen": efen.trim(), "error": e }).to_string(),
                }
            } else if let Some(cfen) = fen.strip_prefix("cvs ") {
                // CVS Feature Registry debug dump (first milestone): FEN -> active
                // CVS-NNUE feature IDs + readable fact names.
                match Position::from_fen(cfen.trim()) {
                    Ok(pos) => {
                        let f = cvs_bitboard_core::eval::extract_cvs_features(&pos);
                        serde_json::json!({
                            "fen": cfen.trim(),
                            "registryVersion": cvs_bitboard_core::eval::cvs_features::CVS_REGISTRY_VERSION,
                            "registryHash": format!("{:016x}", cvs_bitboard_core::eval::registry_hash()),
                            "inputDim": cvs_bitboard_core::eval::CVS_INPUT_DIM,
                            "activeIds": f.ids,
                            "activeNames": f.names,
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({ "fen": cfen.trim(), "error": e }).to_string(),
                }
            } else {
                analyze_one(fen)
            };
            writeln!(stdout, "{out}").expect("stdout");
            stdout.flush().expect("flush");
        }
        return;
    }

    let fens = std::fs::read_to_string(fens_path.unwrap()).expect("read fens");
    for fen in fens.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if features_mode {
            println!("{}", features_one(fen));
        } else {
            println!("{}", analyze_one(fen));
        }
    }
}
