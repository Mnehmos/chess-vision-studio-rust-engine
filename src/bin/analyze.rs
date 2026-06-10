//! Batch analyze CLI — the engine-backend interface for the gate + gauntlet
//! harnesses. Two modes:
//!   --fens <file>   batch: search every FEN in the file, one JSON line each
//!   --serve         loop: read FEN per stdin line, emit one JSON line per FEN
//!                   (one process serves a whole gauntlet run)
//!
//!   analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]
//!           [--net net.json] [--no-quiet-checks] [--no-tt]
use cvs_bitboard_core::eval::{
    evaluate_white_with_net, feature_vector, Rung2Weights, ValueNet, ValueWeights,
    RUNG3_FEATURE_KEYS,
};
use cvs_bitboard_core::position::STARTPOS_FEN;
use cvs_bitboard_core::search::{SearchOptions, Searcher};
use cvs_bitboard_core::Position;
use std::io::{BufRead, Write};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServeJsonRequest {
    cmd: Option<String>,
    fen: Option<String>,
    initial_fen: Option<String>,
    moves: Option<Vec<String>>,
    budget_ms: Option<u64>,
}

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
    let net: Option<ValueNet> = get("--net").map(|p| {
        let net: ValueNet = serde_json::from_str(&std::fs::read_to_string(p).expect("net weights"))
            .expect("parse net");
        net.validate().expect("validate net");
        net
    });
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
    };

    // --features: emit the eval + Rung-2 feature vector per FEN instead of
    // searching — the training-data faucet for head fitting (TS orchestration
    // does the regression; Rust owns extraction).
    let features_mode = args.iter().any(|a| a == "--features");
    let features_one = |fen: &str| -> String {
        match Position::from_fen(fen) {
            Ok(mut pos) => {
                let x = feature_vector(&pos);
                let eval_cp =
                    evaluate_white_with_net(&mut pos, &base, rung2.as_ref(), net.as_ref());
                let features = RUNG3_FEATURE_KEYS
                    .iter()
                    .zip(x.iter())
                    .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
                    .collect::<serde_json::Map<String, serde_json::Value>>();
                serde_json::json!({
                    "fen": fen,
                    "evalWhiteCp": eval_cp,
                    "featureNames": RUNG3_FEATURE_KEYS,
                    "featureVector": x,
                    "features": features,
                })
                .to_string()
            }
            Err(e) => serde_json::json!({ "fen": fen, "error": e }).to_string(),
        }
    };

    let request_position = |req: &ServeJsonRequest| -> Result<(Position, String), String> {
        if let Some(moves) = req.moves.as_ref() {
            let initial = req.initial_fen.as_deref().unwrap_or(STARTPOS_FEN);
            let initial = if initial == "startpos" {
                STARTPOS_FEN
            } else {
                initial
            };
            let echo = req.fen.as_deref().unwrap_or(initial).to_string();
            Ok((Position::from_fen_with_uci_history(initial, moves)?, echo))
        } else {
            let fen = req.fen.as_deref().unwrap_or(STARTPOS_FEN);
            let fen = if fen == "startpos" { STARTPOS_FEN } else { fen };
            Ok((Position::from_fen(fen)?, fen.to_string()))
        }
    };

    let analyze_pos = |mut pos: Position, fen: &str, timed_ms: Option<u64>| -> String {
        let mut searcher = Searcher::new_with_net(base, rung2, net.clone());
        let search_opts = match timed_ms {
            Some(ms) => SearchOptions {
                max_time_ms: Some(ms),
                ..opts
            },
            None => opts,
        };
        let r = searcher.search(&mut pos, search_opts);
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

    let eval_pos = |mut pos: Position, fen: &str| -> String {
        serde_json::json!({
            "fen": fen,
            "evalWhiteCp": evaluate_white_with_net(&mut pos, &base, rung2.as_ref(), net.as_ref()),
        })
        .to_string()
    };

    let analyze_one = |fen: &str| -> String {
        let pos = match Position::from_fen(fen) {
            Ok(p) => p,
            Err(e) => {
                return serde_json::json!({ "fen": fen, "error": e }).to_string();
            }
        };
        analyze_pos(pos, fen, None)
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
            // JSON requests carry game history for repetition detection:
            // {"cmd":"go","budgetMs":500,"fen":"...","initialFen":"...","moves":[...]}
            let out = if fen.starts_with('{') {
                match serde_json::from_str::<ServeJsonRequest>(fen) {
                    Ok(req) => {
                        let cmd = req.cmd.as_deref().unwrap_or("analyze");
                        match request_position(&req) {
                            Ok((pos, echo_fen)) => match cmd {
                                "eval" => eval_pos(pos, &echo_fen),
                                "go" => {
                                    analyze_pos(pos, &echo_fen, Some(req.budget_ms.unwrap_or(500)))
                                }
                                _ => analyze_pos(pos, &echo_fen, None),
                            },
                            Err(e) => serde_json::json!({
                                "fen": req.fen.as_deref().unwrap_or(""),
                                "error": e
                            })
                            .to_string(),
                        }
                    }
                    Err(e) => serde_json::json!({ "fen": fen, "error": e.to_string() }).to_string(),
                }
            // `go <ms> <fen>` → search with a per-request wall-clock budget (the
            // Lichess bot's clock-budgeted picks; depth acts as the cap).
            } else if let Some(rest) = fen.strip_prefix("go ") {
                let mut it = rest.splitn(2, ' ');
                let ms: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(500);
                let gfen = it.next().unwrap_or("").trim();
                match Position::from_fen(gfen) {
                    Ok(pos) => analyze_pos(pos, gfen, Some(ms)),
                    Err(e) => serde_json::json!({ "fen": gfen, "error": e }).to_string(),
                }
            } else if let Some(efen) = fen.strip_prefix("eval ") {
                match Position::from_fen(efen.trim()) {
                    Ok(pos) => eval_pos(pos, efen.trim()),
                    Err(e) => serde_json::json!({ "fen": efen.trim(), "error": e }).to_string(),
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
