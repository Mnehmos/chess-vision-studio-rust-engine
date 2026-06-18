//! Batch analyze CLI — the engine-backend interface for the gate + gauntlet
//! harnesses. Two modes:
//!   --fens <file>   batch: search every FEN in the file, one JSON line each
//!   --serve         loop: read FEN per stdin line, emit one JSON line per FEN
//!                   (one process serves a whole gauntlet run)
//!
//!   analyze (--fens <file> | --serve) --depth N [--base w.json --rung2 r.json]
//!           [--no-quiet-checks] [--no-tt]
use cvs_bitboard_core::eval::{evaluate_white, Nnue, Rung2Weights, ValueWeights};
use cvs_bitboard_core::facts::{
    build_teaching_fact_bundle, TeachingFactsOptionsV1, TeachingFactsRequestV1,
};
use cvs_bitboard_core::movegen::generate_legal_list;
use cvs_bitboard_core::search::{RootScope, SearchOptions, Searcher, Telemetry};
use cvs_bitboard_core::{Move, Position};
use std::io::{BufRead, Write};

/// Serve-mode JSON request — carries game history so the search root knows
/// about repetitions (the Lichess bot's wire format; see rust-engine.ts):
/// {"cmd":"go","budgetMs":500,"fen":"...","initialFen":"...","moves":[...]}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServeJsonRequest {
    cmd: Option<String>,
    fen: Option<String>,
    initial_fen: Option<String>,
    moves: Option<Vec<String>>,
    budget_ms: Option<u64>,
    schema_version: Option<u32>,
    fen_before: Option<String>,
    played_move_uci: Option<String>,
    best_move_uci: Option<String>,
    refutation_uci: Option<String>,
    principal_variation_uci: Option<Vec<String>>,
    options: Option<TeachingFactsOptionsV1>,
    forced_move_uci: Option<String>,
}

fn pct(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        (num as f64) * 100.0 / (den as f64)
    }
}

fn avg(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        (num as f64) / (den as f64)
    }
}

fn telemetry_json(t: &Telemetry) -> serde_json::Value {
    let mut branching_by_ply = Vec::new();
    for (ply, (&nodes, &children)) in t
        .ply_nodes
        .iter()
        .zip(t.ply_child_searches.iter())
        .enumerate()
    {
        if nodes == 0 && children == 0 {
            continue;
        }
        branching_by_ply.push(serde_json::json!({
            "ply": ply,
            "nodes": nodes,
            "childSearches": children,
            "effectiveBranching": avg(children, nodes),
        }));
    }

    let mut out = serde_json::Map::new();
    macro_rules! field {
        ($key:literal, $value:expr) => {
            out.insert($key.to_string(), serde_json::json!($value));
        };
    }

    field!("nodes", t.nodes);
    field!("mainNodes", t.main_nodes);
    field!("qNodes", t.q_nodes);
    field!("qNodePct", pct(t.q_nodes, t.nodes));
    field!("qCaptures", t.q_capture_nodes);
    field!("qSeeSkips", t.q_see_skips);
    field!("quietExt", t.quiet_check_extensions);
    field!("maxQDepth", t.max_q_depth);
    field!("ttProbes", t.tt_probes);
    field!("ttEntries", t.tt_entries);
    field!("ttHits", t.tt_hits);
    field!("ttMissCold", t.tt_miss_cold);
    field!("ttMissContended", t.tt_miss_contended);
    field!("ttHitPct", pct(t.tt_hits, t.tt_probes));
    field!("ttEntryPct", pct(t.tt_entries, t.tt_probes));
    field!("ttCutoffs", t.tt_cutoffs);
    field!("ttCutoffPct", pct(t.tt_cutoffs, t.tt_probes));
    field!("cutoffs", t.beta_cutoffs);
    field!("hashMoveCutoffs", t.hash_move_cutoffs);
    field!(
        "hashMoveCutoffPct",
        pct(t.hash_move_cutoffs, t.beta_cutoffs)
    );
    field!("firstMoveCutoffs", t.first_move_cutoffs);
    field!(
        "firstMoveCutoffPct",
        pct(t.first_move_cutoffs, t.beta_cutoffs)
    );
    field!("killerCutoffs", t.killer_cutoffs);
    field!("historyCutoffs", t.history_cutoffs);
    field!("cutoffMoveIndexSum", t.cutoff_move_index_sum);
    field!("cutoffMoveIndexCount", t.cutoff_move_index_count);
    field!(
        "avgCutoffMoveIndex",
        avg(t.cutoff_move_index_sum, t.cutoff_move_index_count)
    );
    field!("legalMoveNodes", t.legal_move_nodes);
    field!("legalMoveSum", t.legal_move_sum);
    field!("avgLegalMoves", avg(t.legal_move_sum, t.legal_move_nodes));
    field!("searchedMoves", t.searched_moves);
    field!("prunedMoves", t.pruned_moves);
    field!(
        "searchedEffectiveBranching",
        avg(t.searched_moves, t.legal_move_nodes)
    );
    field!("nullAttempts", t.null_attempts);
    field!("nullCutoffs", t.null_cutoffs);
    field!("nullCutoffPct", pct(t.null_cutoffs, t.null_attempts));
    field!("rfpAttempts", t.rfp_attempts);
    field!("rfpCutoffs", t.rfp_cutoffs);
    field!("rfpCutoffPct", pct(t.rfp_cutoffs, t.rfp_attempts));
    field!("futilityAttempts", t.futility_attempts);
    field!("futilitySkips", t.futility_skips);
    field!(
        "futilitySkipPct",
        pct(t.futility_skips, t.futility_attempts)
    );
    field!("lmpAttempts", t.lmp_attempts);
    field!("lmpSkips", t.lmp_skips);
    field!("lmpSkipPct", pct(t.lmp_skips, t.lmp_attempts));
    field!("seePruneAttempts", t.see_prune_attempts);
    field!("seePruneSkips", t.see_prune_skips);
    field!(
        "seePruneSkipPct",
        pct(t.see_prune_skips, t.see_prune_attempts)
    );
    field!("deltaAttempts", t.delta_attempts);
    field!("deltaSkips", t.delta_skips);
    field!("deltaSkipPct", pct(t.delta_skips, t.delta_attempts));
    field!("lmrReductions", t.lmr_reductions);
    field!("lmrResearches", t.lmr_researches);
    field!("lmrResearchPct", pct(t.lmr_researches, t.lmr_reductions));
    field!("pvsResearches", t.pvs_researches);
    field!("aspirationResearches", t.aspiration_researches);
    field!("dangerExtensionPlies", t.danger_extension_plies);
    field!("foreignHints", t.foreign_tt_hints);
    field!("foreignCutoffs", t.foreign_tt_cutoffs);
    field!("branchingByPly", branching_by_ply);
    field!("timeMs", t.elapsed_ms);

    serde_json::Value::Object(out)
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
    let allow_unverified = args.iter().any(|a| a == "--allow-unverified-net");
    let nnue: Option<Nnue> = get("--nnue").map(|p| Nnue::load(&p, allow_unverified).expect("load nnue"));
    let helper_nnue: Option<Nnue> =
        get("--helper-nnue").map(|p| Nnue::load(&p, allow_unverified).expect("load helper nnue"));
    let syzygy_path = get("--syzygy");
    let book_path = get("--book");
    let make_searcher = |base: ValueWeights, rung2: Option<Rung2Weights>| {
        let mut searcher = match &nnue {
            Some(n) => Searcher::with_nnue(base, rung2, n.clone()),
            None => Searcher::new(base, rung2),
        };
        if let Some(n) = &helper_nnue {
            searcher.set_helper_nnue(Some(n.clone()));
        }
        if let Some(path) = &syzygy_path {
            if let Ok(tb) = cvs_bitboard_core::syzygy::Syzygy::new(path) {
                searcher.tb = Some(std::sync::Arc::new(tb));
            }
        }
        if let Some(path) = &book_path {
            if let Ok(b) = cvs_bitboard_core::book::Book::new(path) {
                searcher.book = Some(std::sync::Arc::new(std::sync::Mutex::new(b)));
            }
        }
        searcher
    };
    let opts = SearchOptions {
        depth,
        // --movetime <ms>: wall-clock cap for equal-clock matches (R4 fairness run).
        max_time_ms: get("--movetime").and_then(|s| s.parse().ok()),
        soft_time_ms: None,
        quiet_checks: !args.iter().any(|a| a == "--no-quiet-checks"),
        use_tt: !args.iter().any(|a| a == "--no-tt"),
        // --danger: danger-triggered root depth extension (RSI loop 1, gated).
        danger_extension: args.iter().any(|a| a == "--danger"),
        null_move: !args.iter().any(|a| a == "--no-null"),
        lmr: !args.iter().any(|a| a == "--no-lmr"),
        pvs: !args.iter().any(|a| a == "--no-pvs"),
        // Standardized gated search features (now enabled by default, --no-xxx to opt-out).
        rfp: !args.iter().any(|a| a == "--no-rfp"),
        futility: !args.iter().any(|a| a == "--no-futility"),
        lmp: !args.iter().any(|a| a == "--no-lmp"),
        see_prune: !args.iter().any(|a| a == "--no-seeprune"),
        delta_prune: !args.iter().any(|a| a == "--no-delta"),
        countermove: !args.iter().any(|a| a == "--no-countermove"),
        conthist: !args.iter().any(|a| a == "--no-conthist"),
        tt_prune_store: !args.iter().any(|a| a == "--no-tt-prune-store"),
        rule50_scale: !args.iter().any(|a| a == "--no-rule50"),
        qsearch_tt: !args.iter().any(|a| a == "--no-qtt"),
        hist_malus: !args.iter().any(|a| a == "--no-histmalus"),
        hist_lmr: !args.iter().any(|a| a == "--no-histlmr"),
        caphist: !args.iter().any(|a| a == "--no-caphist"),
        tt2: !args.iter().any(|a| a == "--no-tt2"),
        improving: !args.iter().any(|a| a == "--no-improving"),
        king_activity: !args.iter().any(|a| a == "--no-king-activity"),
        threads: get("--threads").and_then(|s| s.parse().ok()).unwrap_or(1),
        cvs_trace: args.iter().any(|a| a == "--cvs-trace"),
        cvs_core_trace: args.iter().any(|a| a == "--cvs-core-trace"),
        cvs_helpers: get("--cvs-helpers")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        // --lane king|see|tactics: run THIS searcher as a specialist lane
        // (single-thread lane-voice measurement; SMP assigns its own roster).
        lane: match get("--lane").as_deref() {
            Some("king") => cvs_bitboard_core::search::Lane::KingSafety,
            Some("see") => cvs_bitboard_core::search::Lane::See,
            Some("tactics") => cvs_bitboard_core::search::Lane::Tactics,
            Some("defender") => cvs_bitboard_core::search::Lane::DefenderRemoval,
            Some("quietdef") => cvs_bitboard_core::search::Lane::QuietDefense,
            Some("pawn") => cvs_bitboard_core::search::Lane::PawnEndgame,
            _ => cvs_bitboard_core::search::Lane::Fast,
        },
        singular: !args.iter().any(|a| a == "--no-singular"),
        syzygy: !args.iter().any(|a| a == "--no-syzygy"),
        book: !args.iter().any(|a| a == "--no-book"),
    };

    // --features: emit the eval + Rung-2 feature vector per FEN instead of
    // searching — the training-data faucet for head fitting (TS orchestration
    // does the regression; Rust owns extraction).
    let features_mode = args.iter().any(|a| a == "--features");
    // --cvs-ids: ultra-fast batch (fens file -> one line of comma-separated
    // active CVS feature ids per fen). Training-data faucet for cvs_nnue.
    if args.iter().any(|a| a == "--cvs-ids") {
        let path = fens_path.expect("--cvs-ids requires --fens");
        let file = std::fs::File::open(path).expect("fens file");
        let mut w = std::io::BufWriter::new(std::io::stdout());
        let mut buf: Vec<u32> = Vec::with_capacity(32);
        for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
            let Ok(l) = line else { break };
            let fen = l.trim();
            if fen.is_empty() {
                continue;
            }
            match Position::from_fen(fen) {
                Ok(pos) => {
                    cvs_bitboard_core::eval::cvs_features::extract_cvs_ids_into(&pos, &mut buf);
                    let strs: Vec<String> = buf.iter().map(|i| i.to_string()).collect();
                    writeln!(w, "{}", strs.join(",")).expect("stdout");
                }
                Err(_) => {
                    writeln!(w, "ERR").expect("stdout");
                }
            }
        }
        return;
    }
    // --cvs-core-ids: ultra-fast batch (fens file -> one line of comma-separated
    // active CVS core feature ids per fen).
    if args.iter().any(|a| a == "--cvs-core-ids") {
        let path = fens_path.expect("--cvs-core-ids requires --fens");
        let file = std::fs::File::open(path).expect("fens file");
        let mut w = std::io::BufWriter::new(std::io::stdout());
        let mut buf: Vec<u32> = Vec::with_capacity(32);
        for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
            let Ok(l) = line else { break };
            let fen = l.trim();
            if fen.is_empty() {
                continue;
            }
            match Position::from_fen(fen) {
                Ok(pos) => {
                    cvs_bitboard_core::eval::cvs_features::extract_cvs_core_ids_into(&pos, &mut buf);
                    let strs: Vec<String> = buf.iter().map(|i| i.to_string()).collect();
                    writeln!(w, "{}", strs.join(",")).expect("stdout");
                }
                Err(_) => {
                    writeln!(w, "ERR").expect("stdout");
                }
            }
        }
        return;
    }
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
        let mut searcher = make_searcher(base, rung2);
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
            "telemetry": telemetry_json(&t),
            "timeMs": t.elapsed_ms,
        })
        .to_string()
    };

    if serve {
        // Loop mode: one process serves a whole gauntlet run. Flush per line so
        // the orchestrator's request/response cycle never stalls on buffering.
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        // Shared search-and-emit for prebuilt positions (JSON requests carry
        // move history so the root sees repetitions).
        let search_pos = |mut pos: Position,
                          echo: &str,
                          timed_ms: Option<u64>,
                          forced_move: Option<Move>|
         -> String {
            let mut searcher = make_searcher(base, rung2);
            if let Some(mv) = forced_move {
                searcher.root_scope = RootScope::Only(mv);
            }
            let search_opts = match timed_ms {
                Some(ms) => SearchOptions {
                    max_time_ms: Some(ms),
                    ..opts
                },
                None => opts,
            };
            let r = searcher.search(&mut pos, search_opts);
            let t = r.telemetry;
            serde_json::json!({
                "fen": echo,
                "uci": r.best_move.map(|m| m.to_uci()),
                "scoreCp": r.score_cp,
                "mate": r.mate,
                "pv": r.pv.iter().map(|m| m.to_uci()).collect::<Vec<_>>(),
                "depth": r.depth,
                "nodes": t.nodes,
                "qNodes": t.q_nodes,
                "ttHits": t.tt_hits,
                "timeMs": t.elapsed_ms,
                "telemetry": telemetry_json(&t),
                "foreignHints": t.foreign_tt_hints,
                "foreignCutoffs": t.foreign_tt_cutoffs,
            })
            .to_string()
        };
        let request_position = |req: &ServeJsonRequest| -> Result<(Position, String), String> {
            const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
            if let Some(moves) = req.moves.as_ref() {
                let initial = match req.initial_fen.as_deref() {
                    Some("startpos") | None => STARTPOS,
                    Some(f) => f,
                };
                let echo = req.fen.as_deref().unwrap_or(initial).to_string();
                Ok((Position::from_fen_with_uci_history(initial, moves)?, echo))
            } else {
                let fen = match req.fen.as_deref() {
                    Some("startpos") | None => STARTPOS,
                    Some(f) => f,
                };
                Ok((Position::from_fen(fen)?, fen.to_string()))
            }
        };
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
            // JSON requests (the bot's wire format) carry game history:
            // {"cmd":"go","budgetMs":500,"fen":"...","initialFen":"...","moves":[...]}
            let out = if fen.starts_with('{') {
                match serde_json::from_str::<ServeJsonRequest>(fen) {
                    Ok(req) if req.cmd.as_deref() == Some("facts") => {
                        let facts_request = match (
                            req.schema_version,
                            req.fen_before.clone(),
                            req.played_move_uci.clone(),
                        ) {
                            (Some(schema_version), Some(fen_before), Some(played_move_uci)) => {
                                Ok(TeachingFactsRequestV1 {
                                    schema_version,
                                    fen_before,
                                    played_move_uci,
                                    best_move_uci: req.best_move_uci.clone(),
                                    refutation_uci: req.refutation_uci.clone(),
                                    principal_variation_uci: req.principal_variation_uci.clone(),
                                    options: req.options.clone(),
                                })
                            }
                            _ => Err("facts requires schemaVersion, fenBefore, and playedMoveUci"
                                .to_string()),
                        };
                        match facts_request.and_then(|request| {
                            build_teaching_fact_bundle(&request).and_then(|bundle| {
                                serde_json::to_string(&bundle).map_err(|e| e.to_string())
                            })
                        }) {
                            Ok(json) => json,
                            Err(error) => serde_json::json!({
                                "schemaVersion": req.schema_version,
                                "fenBefore": req.fen_before,
                                "error": error,
                            })
                            .to_string(),
                        }
                    }
                    Ok(req) => match request_position(&req) {
                        Ok((mut pos, echo)) => {
                            let forced_move = if let Some(forced_uci) = req.forced_move_uci.as_ref()
                            {
                                let mut pos_clone = pos.clone();
                                let legal = generate_legal_list(&mut pos_clone);
                                if let Some(mv) = legal
                                    .as_slice()
                                    .iter()
                                    .find(|mv| mv.to_uci() == *forced_uci)
                                {
                                    Some(*mv)
                                } else {
                                    serde_json::json!({
                                        "fen": req.fen.as_deref().unwrap_or(""),
                                        "error": format!("forced move {} is illegal in this position", forced_uci)
                                    })
                                    .to_string();
                                    None // this won't be hit because we return early or handle it, but wait:
                                         // Let's print the error JSON line directly and continue
                                }
                            } else {
                                None
                            };

                            // Wait, let's structure this cleanly to avoid compile warnings/errors:
                            if req.forced_move_uci.is_some() && forced_move.is_none() {
                                serde_json::json!({
                                    "fen": req.fen.as_deref().unwrap_or(""),
                                    "error": format!("forced move {} is illegal in this position", req.forced_move_uci.as_ref().unwrap())
                                })
                                .to_string()
                            } else {
                                match req.cmd.as_deref().unwrap_or("analyze") {
                                    "eval" => {
                                        let mut j = serde_json::json!({
                                            "fen": echo,
                                            "evalWhiteCp": evaluate_white(&mut pos, &base, rung2.as_ref()),
                                        });
                                        if let Some(n) = &nnue {
                                            j["nnueStmCp"] = n.eval_stm(&pos).into();
                                        }
                                        j.to_string()
                                    }
                                    "go" => search_pos(
                                        pos,
                                        &echo,
                                        Some(req.budget_ms.unwrap_or(500)),
                                        forced_move,
                                    ),
                                    _ => search_pos(pos, &echo, None, forced_move),
                                }
                            }
                        }
                        Err(e) => serde_json::json!({
                            "fen": req.fen.as_deref().unwrap_or(""),
                            "error": e
                        })
                        .to_string(),
                    },
                    Err(e) => serde_json::json!({ "fen": fen, "error": e.to_string() }).to_string(),
                }
            // `go <ms> <fen>` → search with a per-request wall-clock budget (the
            // Lichess bot's clock-budgeted picks; depth acts as the cap).
            } else if let Some(rest) = fen.strip_prefix("go ") {
                let mut it = rest.splitn(2, ' ');
                let ms: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(500);
                let gfen = it.next().unwrap_or("").trim();
                match Position::from_fen(gfen) {
                    Ok(mut pos) => {
                        let mut searcher = make_searcher(base, rung2);
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
                            "telemetry": telemetry_json(&t),
                            "foreignHints": t.foreign_tt_hints,
                            "foreignCutoffs": t.foreign_tt_cutoffs,
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
