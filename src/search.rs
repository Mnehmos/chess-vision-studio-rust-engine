//! Search — negamax/alpha-beta with iterative deepening, capture quiescence,
//! forcing quiet-check extensions, MVV-LVA move ordering, a Zobrist-keyed
//! transposition table, PV extraction, and telemetry.
//!
//! Semantics mirror the legacy TS `Searcher` (the R4 parity target): same mate
//! scoring (±MATE_SCORE − ply), same quiescence rules (stand-pat fail-hard,
//! SEE ≥ 0 capture filter, evasions when in check, capped quiet checks in the
//! first QUIET_CHECK_MAX_PLY q-plies), same TT bound semantics and deeper-entry
//! replacement, same ordering priorities (TT move ≫ captures by MVV-LVA ≫
//! promotions). Layers are config-gated (`quiet_checks`, `use_tt`) so each can
//! be exercised independently in tests.
use crate::attacks::{attackers_of, king_attacks};
use crate::eval::{
    evaluate, evaluate_white_float_nonterminal, insufficient_material, js_round, phase_units, Nnue,
    Rung2Weights, ValueWeights,
};
use crate::movegen::{
    generate_legal_list, generate_legal_noisy_list, gives_check, has_legal_move, in_check,
    MoveList, MAX_MOVES,
};
use crate::see::{see, SEE_VALUE};
use crate::tt::{Flag, SharedTt, TtEntry};
use crate::{rank_of, Color, Move, MoveFlag, Piece, Position};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

mod eval_adapter;
mod ordering;
mod qsearch;
mod root;
mod smp;
mod time_control;
mod types;
pub use types::*;

pub const MATE_SCORE: i32 = 1_000_000;
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 1000;
const INF: i32 = MATE_SCORE * 2;

/// BUG1 mate-TT: search mate scores are root-relative (`±(MATE_SCORE - ply_from_root)`),
/// but the TT is keyed by position, which can occur at different plies. Convert to a
/// node-intrinsic mate distance before STORING so the same position always stores the
/// same mate distance; convert back to root-relative on PROBE. No-op for non-mate scores.
#[inline]
pub fn mate_store_adjust(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score + ply
    } else if score < -MATE_THRESHOLD {
        score - ply
    } else {
        score
    }
}

/// Inverse of [`mate_store_adjust`]: node-intrinsic stored score → root-relative at `ply`.
#[inline]
pub fn mate_probe_adjust(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score - ply
    } else if score < -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}

/// Log-based LMR reduction (--loglmr): `r ≈ 0.75 + ln(d)·ln(i)/2.25` — the standard
/// shape strong engines use, replacing the flat 1-ply tier (depth/move-index aware:
/// reduce later + deeper moves more). Precomputed once into a 64×64 table.
pub fn log_lmr_reduction(depth: i32, move_index: usize) -> i32 {
    static TABLE: std::sync::OnceLock<[[i32; 64]; 64]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [[0i32; 64]; 64];
        for d in 1..64usize {
            for i in 1..64usize {
                t[d][i] = (0.75 + (d as f64).ln() * (i as f64).ln() / 2.25) as i32;
            }
        }
        t
    });
    t[(depth.max(0) as usize).min(63)][move_index.min(63)]
}
const MAX_QUIESCENCE_PLY: u32 = 64;
// Forcing quiet-check quiescence extensions (the d4 lesson: chess danger is not
// only captures). Same caps as the TS searcher.
const QUIET_CHECK_MAX_PLY: u32 = 2;
const MAX_QUIET_CHECKS_PER_NODE: usize = 3;

/// 2^21 slots ≈ 2M entries — fixed, power of two for mask indexing. The table
/// itself lives in `crate::tt::SharedTt` (lock-free, shared across SMP threads).
const TT_BITS: u32 = 21;

/// Cheap root-level king-danger classifier (RSI loop 1). 0 = normal, 1 = danger
/// (+1 ply), 2 = critical (+2 plies). Fires only with an enemy queen on the
/// board; combines king-zone pressure (attacked squares in the king's 9-square
/// zone) with the king being off its home rank. Cost: ≤9 attack queries, once
/// per search call. Evidence: the sf2200-g14 loss FENs trigger 2; quiet
/// openings trigger 0.
pub fn danger_level(pos: &Position) -> u32 {
    let us = pos.stm;
    let them = us.flip();
    if pos.pieces[them.index()][Piece::Queen.index()] == 0 {
        return 0;
    }
    let ksq = pos.king_sq(us);
    let mut zone = king_attacks(ksq) | (1u64 << ksq);
    let mut pressure = 0u32;
    while zone != 0 {
        let sq = zone.trailing_zeros() as u8;
        zone &= zone - 1;
        if attackers_of(&pos.pieces, sq, them, pos.all) != 0 {
            pressure += 1;
        }
    }
    let home_rank = if us == Color::White { 0 } else { 7 };
    let off_home = rank_of(ksq) != home_rank;
    let mut danger = 0;
    if pressure >= 2 {
        danger += 1;
    }
    if (off_home && pressure >= 1) || pressure >= 4 {
        danger += 1;
    }
    danger.min(2)
}
/// Killer slots per ply — two is the classical sweet spot.
const MAX_KILLER_PLY: usize = 128;
/// History scores are halved when any cell reaches this, keeping recent
/// experience dominant without ever overflowing into capture territory.
const HISTORY_CAP: i32 = 1 << 14;

pub struct Searcher {
    pub root_scope: RootScope,
    weights: ValueWeights,
    rung2: Option<Rung2Weights>,
    /// Pristine rung2 weights (lane profiles derive from this, never compound).
    rung2_base: Option<Rung2Weights>,
    tt: Arc<SharedTt>,
    tt_generation: u8,
    /// SMP stop flag — helpers abort when the main thread finishes.
    stop: Option<Arc<AtomicBool>>,
    /// First iterative-deepening depth (1 for the main thread; helpers start
    /// deeper so they run AHEAD and seed the shared TT instead of trailing in
    /// lockstep — the classic lazy-SMP staggering fix).
    id_start: u32,
    tel: Telemetry,
    deadline: Option<Instant>,
    aborted: bool,
    opts: SearchOptions,
    /// Two killer moves per ply: quiet moves that caused a beta cutoff at this
    /// ply elsewhere in the tree — cheap, position-independent ordering signal.
    killers: Vec<[Option<Move>; 2]>,
    /// History heuristic: [side][from][to] — quiet cutoff counts weighted by
    /// depth², so quiet cutoffs teach more than leaf noise.
    history: Vec<i32>, // 2*64*64, flat for cache friendliness
    /// Countermove heuristic (--countermove): keyed by the opponent's
    /// previous (piece, to-square) -> the quiet that refuted it last time.
    /// v2 re-key (research 2026-06-12): from-to keying split each refutation
    /// across 64 from-squares; piece+to is how Ethereal/Berserk key it.
    counters: Vec<Option<Move>>, // 2 colors * 6 pieces * 64 squares
    /// Continuation history (--conthist): [prev_piece][prev_to][piece][to]
    /// depth²-weighted quiet-cutoff counts for the move PAIR — the table that
    /// usually closes the first-move-cutoff gap butterfly history can't.
    conthist: Vec<i32>, // 6*64*6*64
    /// Capture history (--caphist): [side][piece][to][victim] gravity-weighted
    /// capture-cutoff counts. Pure ordering — captures were MVV-LVA only, so
    /// cutoffs taught nothing. Sharpens hit quality with no reduction tradeoff.
    caphist: Vec<i32>, // 2*6*64*6
    /// prev_moves[ply] = the move that led to the node at `ply` (None at the
    /// root and after a null move — a null subtree must not teach or consult
    /// counters keyed to a move the opponent never answered).
    prev_moves: Vec<Option<Move>>,
    /// Per-ply static eval (--improving): the eval-trajectory primitive. A node
    /// is "improving" when its static eval exceeds the same side's eval two
    /// plies back; pruning leans harder when NOT improving, reductions ease
    /// evaluation thresholds at PV nodes.
    eval_stack: Vec<i32>,
    /// NNUE eval head — when loaded, replaces the classical eval at every
    /// static/leaf eval site (search shape unchanged).
    nnue: Option<Nnue>,
    /// Secondary net for SMP specialist lanes, ensuring thread diversity when
    /// playing helper roles.
    helper_nnue: Option<Nnue>,
    /// Stack of NNUE accumulators to avoid recalculation.
    acc_stack: Vec<crate::eval::Accumulator>,
    /// Index of the current node's accumulator in `acc_stack`
    /// (usize::MAX = incremental path inactive).
    acc_top: usize,
    /// Reusable scratch for CVS-Fast trace IDs (no per-node allocation).
    cvs_buf: Vec<u32>,
    /// Syzygy tablebases.
    pub tb: Option<Arc<crate::syzygy::Syzygy>>,
    /// Polyglot opening book.
    pub book: Option<Arc<std::sync::Mutex<crate::book::Book>>>,
    /// Excluded move for singular search.
    excluded_move: Option<Move>,
    /// Cache for Hybrid A root geometry/residual move scoring.
    pub root_geom_cache: Option<RootGeometryCacheEntry>,
    pub root_attention_cache: Option<RootAttentionCache>,
    pub root_attention_zobrist: Option<u64>,
    last_root_order: Vec<Move>,
    root_progress: Option<PartialIteration>,
    abort_reason: Option<SearchTermination>,
}

impl Searcher {
    pub fn new(weights: ValueWeights, rung2: Option<Rung2Weights>) -> Searcher {
        Searcher {
            root_scope: RootScope::All,
            weights,
            rung2_base: rung2,
            rung2,
            tt: Arc::new(SharedTt::new(TT_BITS)),
            tt_generation: 0,
            stop: None,
            id_start: 1,
            tel: Telemetry::default(),
            deadline: None,
            aborted: false,
            opts: SearchOptions::default(),
            killers: vec![[None; 2]; MAX_KILLER_PLY],
            history: vec![0; 2 * 64 * 64],
            counters: vec![None; 2 * 6 * 64],
            conthist: vec![0; 6 * 64 * 6 * 64],
            caphist: vec![0; 2 * 6 * 64 * 6],
            prev_moves: vec![None; MAX_KILLER_PLY],
            eval_stack: vec![i32::MIN; MAX_KILLER_PLY + 2],
            nnue: None,
            helper_nnue: None,
            acc_stack: Vec::new(),
            acc_top: usize::MAX,
            cvs_buf: Vec::with_capacity(32),
            tb: None,
            book: None,
            excluded_move: None,
            root_geom_cache: None,
            root_attention_cache: None,
            root_attention_zobrist: None,
            last_root_order: Vec::new(),
            root_progress: None,
            abort_reason: None,
        }
    }

    /// NNUE-backed searcher: the net replaces the classical eval at every
    /// leaf/static-eval site (search shape is otherwise identical).
    pub fn with_nnue(weights: ValueWeights, rung2: Option<Rung2Weights>, nnue: Nnue) -> Searcher {
        let mut s = Searcher::new(weights, rung2);
        s.nnue = Some(nnue);
        s
    }

    /// Configure an alternate helper-only NNUE for heterogeneous SMP.
    pub fn set_helper_nnue(&mut self, helper_nnue: Option<Nnue>) {
        self.helper_nnue = helper_nnue;
    }

    /// External stop hook (UCI ponder / async control). The flag is polled
    /// every 1024 nodes in `time_up`; setting it aborts the current iteration
    /// and the search returns the last completed depth's result.
    pub fn set_stop(&mut self, stop: Option<Arc<AtomicBool>>) {
        self.stop = stop;
    }

    pub fn search(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
        if opts.threads > 1 {
            return self.search_smp(pos, opts);
        }
        self.search_single(pos, opts)
    }

    fn search_single(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
        // Level-2 lane eval profile: derive this lane's evaluator opinion from
        // the pristine base weights (never from a previously-profiled set).
        self.rung2 = opts.lane.eval_profile(self.rung2_base);
        let mut max_depth = opts.depth.max(1);
        // Danger-triggered root extension (gated): king danger buys 1–2 extra plies.
        let danger_plies = if opts.danger_extension {
            danger_level(pos)
        } else {
            0
        };
        max_depth += danger_plies;
        self.opts = opts;
        // Persistent TT: age the generation instead of clearing (audit fix).
        self.tt_generation = self.tt.new_generation();
        // Killers/history reset per search call (kept across the iterative-
        // deepening iterations within it) — searches stay deterministic.
        self.killers.iter_mut().for_each(|k| *k = [None; 2]);
        self.history.iter_mut().for_each(|h| *h = 0);
        self.counters.iter_mut().for_each(|c| *c = None);
        self.conthist.iter_mut().for_each(|h| *h = 0);
        self.caphist.iter_mut().for_each(|h| *h = 0);
        self.prev_moves.iter_mut().for_each(|m| *m = None);
        self.tel = Telemetry::default();
        self.tel.danger_extension_plies = danger_plies;
        self.aborted = false;
        self.root_attention_cache = None;
        self.root_attention_zobrist = None;
        self.last_root_order.clear();
        self.root_progress = None;
        self.abort_reason = None;
        // Incremental NNUE: root accumulator rebuilt fresh per search (also
        // bounds f32 drift to the search path length).
        self.acc_top = usize::MAX;
        if let Some(n) = &self.nnue {
            if n.supports_incremental() {
                let fresh = n.fresh_acc(pos);
                if self.acc_stack.is_empty() {
                    self.acc_stack.push(fresh);
                } else {
                    self.acc_stack[0] = fresh;
                }
                self.acc_top = 0;
            }
        }
        let started = Instant::now();
        self.deadline = opts
            .max_time_ms
            .map(|ms| started + std::time::Duration::from_millis(ms));

        // 1. Opening Book Probe
        if opts.book {
            if let Some(book) = &self.book {
                if let Ok(mut book_guard) = book.lock() {
                    if let Some(mv) = book_guard.query(pos) {
                        return SearchResult {
                            best_move: Some(mv),
                            score_cp: 0,
                            mate: None,
                            pv: vec![mv],
                            depth: 1,
                            telemetry: self.tel,
                            iterations: Vec::new(),
                            root_order: vec![mv],
                            attempted_depth: 1,
                            termination: SearchTermination::Book,
                            result_source: SearchResultSource::Book,
                            partial_iteration: None,
                        };
                    }
                }
            }
        }

        // 2. Syzygy Tablebase Root Probe
        if opts.syzygy {
            if let Some(tb) = &self.tb {
                if pos.castling == 0 && pos.all.count_ones() as u32 <= tb.max_pieces() {
                    if let Some((mv, wdl)) = tb.probe_root(pos) {
                        let score: i32 = match wdl {
                            pyrrhic_rs::WdlProbeResult::Win => 900_000,
                            pyrrhic_rs::WdlProbeResult::Loss => -900_000,
                            _ => 0,
                        };
                        let mate = if score.abs() > MATE_THRESHOLD {
                            let plies = MATE_SCORE - score.abs();
                            Some(if score > 0 { plies } else { -plies })
                        } else {
                            None
                        };
                        return SearchResult {
                            best_move: Some(mv),
                            score_cp: score,
                            mate,
                            pv: vec![mv],
                            depth: 1,
                            telemetry: self.tel,
                            iterations: Vec::new(),
                            root_order: vec![mv],
                            attempted_depth: 1,
                            termination: SearchTermination::Tablebase,
                            result_source: SearchResultSource::Tablebase,
                            partial_iteration: None,
                        };
                    }
                }
            }
        }

        let mut result = SearchResult {
            best_move: None,
            score_cp: evaluate(pos, &self.weights, self.rung2.as_ref()),
            mate: None,
            pv: Vec::new(),
            depth: 0,
            telemetry: self.tel,
            iterations: Vec::new(),
            root_order: Vec::new(),
            attempted_depth: 0,
            termination: SearchTermination::DepthLimit,
            result_source: SearchResultSource::NoCompletedIteration,
            partial_iteration: None,
        };

        let mut prev_score: Option<i32> = None;
        let mut iterations = Vec::new();
        let mut termination = SearchTermination::DepthLimit;
        // Smart-time state: best-move stability and last score for the
        // iteration-boundary stop/extend decision.
        let mut tm_prev_best: Option<Move> = None;
        let mut tm_stable: u32 = 0;
        let mut tm_last_score: Option<i32> = None;
        for depth in self.id_start.min(max_depth)..=max_depth {
            result.attempted_depth = depth;
            // Aspiration windows (Patch 5): start from a tight window around
            // the previous iteration's score; on any fail, re-search at full
            // width (one-step widen — simple and safe).
            const ASPIRATION_CP: i32 = 50;
            let (mut a, mut b) = match prev_score {
                Some(p) if self.opts.pvs && depth >= 3 && p.abs() < MATE_THRESHOLD => {
                    (p - ASPIRATION_CP, p + ASPIRATION_CP)
                }
                _ => (-INF, INF),
            };
            let (score, best) = loop {
                let (sc, bm) = self.root(pos, depth as i32, a, b);
                if self.aborted {
                    break (sc, bm);
                }
                if sc <= a && a > -INF {
                    self.tel.aspiration_researches += 1;
                    (a, b) = (-INF, INF);
                    continue;
                }
                if sc >= b && b < INF {
                    self.tel.aspiration_researches += 1;
                    (a, b) = (-INF, INF);
                    continue;
                }
                break (sc, bm);
            };
            if self.aborted {
                termination = self.abort_reason.unwrap_or(SearchTermination::HardTime);
                result.partial_iteration = self.root_progress.clone();
                break;
            }
            prev_score = Some(score);
            let mate = if score.abs() > MATE_THRESHOLD {
                let plies = MATE_SCORE - score.abs();
                Some(if score > 0 { plies } else { -plies })
            } else {
                None
            };
            self.tel.elapsed_ms = started.elapsed().as_millis() as u64;
            let pv = self.extract_pv(pos, best, depth as usize);
            iterations.push(SearchIteration {
                depth,
                best_move: best,
                score_cp: score,
                nodes: self.tel.nodes,
                time_ms: self.tel.elapsed_ms,
                pv: pv.clone(),
            });
            result = SearchResult {
                best_move: best,
                score_cp: score,
                mate,
                pv,
                depth,
                telemetry: self.tel,
                iterations: iterations.clone(),
                root_order: self.last_root_order.clone(),
                attempted_depth: depth,
                termination: SearchTermination::DepthLimit,
                result_source: SearchResultSource::CompletedIteration,
                partial_iteration: None,
            };
            // A proven mate cannot be improved by searching deeper.
            if score.abs() > MATE_THRESHOLD {
                termination = SearchTermination::ProvenMate;
                break;
            }
            // Smart time: decide continue/stop at the iteration boundary.
            if let Some(soft) = self.opts.soft_time_ms {
                let critical = (tm_prev_best.is_some() && best != tm_prev_best && depth >= 5)
                    || tm_last_score.is_some_and(|p| score + 40 < p);
                if best.is_some() && best == tm_prev_best {
                    tm_stable += 1;
                } else {
                    tm_stable = 0;
                }
                tm_prev_best = best;
                tm_last_score = Some(score);
                let target = if critical {
                    soft + soft * 2 / 5 // extend toward the hard cap
                } else if tm_stable >= 3 {
                    soft * 11 / 20 // easy move: bank the rest
                } else {
                    soft
                };
                if started.elapsed().as_millis() as u64 >= target {
                    termination = SearchTermination::SoftTime;
                    break;
                }
            }
        }
        self.tel.elapsed_ms = started.elapsed().as_millis() as u64;
        result.telemetry = self.tel;
        result.termination = termination;
        result
    }

    #[inline]
    fn tt_probe(&self, key: u64) -> Option<TtEntry> {
        if self.opts.tt2 {
            self.tt.probe2(key)
        } else {
            self.tt.probe(key)
        }
    }

    fn store(&mut self, key: u64, depth: i32, score: i32, flag: Flag, mv: Option<Move>, ply: i32) {
        let (gen, lane) = (self.tt_generation, self.opts.lane.id());
        let score = if self.opts.matett { mate_store_adjust(score, ply) } else { score };
        if self.opts.tt2 {
            self.tt.store2(key, depth, score, flag, mv, gen, lane);
        } else {
            self.tt.store(key, depth, score, flag, mv, gen, lane);
        }
    }

    /// Walk the TT from the root following stored moves (the TS PV extraction).
    /// Falls back to just the root best move when the TT is off.
    fn extract_pv(&self, pos: &mut Position, root_best: Option<Move>, max_len: usize) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut undo = 0usize;
        if let Some(mv) = root_best {
            let legal = generate_legal_list(pos);
            if legal.as_slice().contains(&mv) {
                pv.push(mv);
                seen.insert(pos.hash);
                pos.make(mv);
                undo += 1;
            }
        }
        if !self.opts.use_tt || pv.len() >= max_len {
            for _ in 0..undo {
                pos.unmake();
            }
            return pv;
        }
        while pv.len() < max_len {
            if !seen.insert(pos.hash) {
                break;
            }
            let Some(entry) = self.tt_probe(self.tt_key(pos)) else {
                break;
            };
            let Some(mv) = entry.mv else { break };
            // Only follow strictly legal continuations.
            let legal = generate_legal_list(pos);
            if !legal.as_slice().contains(&mv) {
                break;
            }
            pv.push(mv);
            pos.make(mv);
            undo += 1;
        }
        for _ in 0..undo {
            pos.unmake();
        }
        pv
    }

    pub fn prepare_root_attention(
        &self,
        pos: &Position,
        legal_moves: &[Move],
        raw_nnue: &Nnue,
        ranker: &Nnue,
    ) -> RootAttentionCache {
        let mut cache = Vec::new();
        let mut quiet_moves = Vec::new();
        let mut raw_scores = Vec::new();
        let mut best_raw_score = i32::MIN;

        for &mv in legal_moves {
            let quiet = !mv.flag.is_capture() && mv.flag.promo_piece().is_none();
            if quiet {
                let mut child = pos.clone();
                child.make(mv);
                let raw_score = -raw_nnue.eval_stm(&child);
                quiet_moves.push(mv);
                raw_scores.push(raw_score);
                if raw_score > best_raw_score {
                    best_raw_score = raw_score;
                }
            }
        }

        if quiet_moves.is_empty() {
            return cache;
        }

        let mut parent_ids = Vec::new();
        crate::eval::cvs_features::extract_cvs_ids_into(pos, &mut parent_ids);
        let parent_bitset = crate::eval::cvs_features::ids_to_bitset(&parent_ids);
        let ctx = crate::eval::cvs_features::RootGeometryContext {
            parent_bitset,
            mover: pos.stm,
        };

        let mut logits = Vec::with_capacity(quiet_moves.len());
        let mut sparse_bufs = Vec::with_capacity(quiet_moves.len());
        let mut dense_bufs = Vec::with_capacity(quiet_moves.len());

        for i in 0..quiet_moves.len() {
            let mv = quiet_moves[i];
            let raw_score = raw_scores[i];
            let mut sparse_buf = Vec::new();
            let mut dense_buf = [0.0f32; 32];
            crate::eval::cvs_features::extract_candidate_delta(
                &ctx,
                pos,
                mv,
                &mut sparse_buf,
                &mut dense_buf,
                raw_score,
                best_raw_score,
            );
            let logit = ranker.eval_ranker_raw(&sparse_buf, &dense_buf);
            logits.push(logit);
            sparse_bufs.push(sparse_buf);
            dense_bufs.push(dense_buf);
        }

        let count = logits.len() as f32;
        let sum_logits: f32 = logits.iter().sum();
        let mean_logit = sum_logits / count;

        let temp = if ranker.ranker_temperature == 0.0 {
            1.0
        } else {
            ranker.ranker_temperature
        };

        for i in 0..quiet_moves.len() {
            let mv = quiet_moves[i];
            let raw_score = raw_scores[i];
            let logit = logits[i];

            let centered_logit = logit - mean_logit;
            let scaled_logit = (centered_logit / temp).tanh();

            let raw_diff = best_raw_score - raw_score;
            let raw_ambiguity = (1.0 - raw_diff as f32 / 80.0).clamp(0.0, 1.0);
            let quiet_safety = see(pos, mv.from, mv.to);
            let tactical_safety = if quiet_safety >= 0 { 1.0 } else { 0.0 };
            let confidence = raw_ambiguity * tactical_safety;

            let ordering_bonus =
                (scaled_logit * confidence * ranker.ranker_max_bonus as f32).round() as i32;

            cache.push(RootMoveAttention {
                mv,
                raw_score,
                raw_diff,
                quiet_safety,
                ranker_logit: logit,
                confidence,
                ordering_bonus,
            });
        }

        cache
    }
}

#[cfg(test)]
mod mate_tt_tests {
    use super::{mate_probe_adjust, mate_store_adjust, MATE_SCORE, MATE_THRESHOLD};

    #[test]
    fn non_mate_scores_unchanged() {
        for &s in &[0, 50, -50, MATE_THRESHOLD - 1, -(MATE_THRESHOLD - 1)] {
            assert_eq!(mate_store_adjust(s, 7), s, "store no-op for non-mate {s}");
            assert_eq!(mate_probe_adjust(s, 7), s, "probe no-op for non-mate {s}");
        }
    }

    #[test]
    fn store_probe_roundtrip_same_ply() {
        for &s in &[MATE_SCORE - 8, -(MATE_SCORE - 8), MATE_SCORE - 1, -(MATE_SCORE - 1)] {
            for &ply in &[0, 3, 12, 40] {
                assert_eq!(mate_probe_adjust(mate_store_adjust(s, ply), ply), s);
            }
        }
    }

    #[test]
    fn mate_distance_preserved_across_plies() {
        // "Mate in 3 from the node", found at ply 5: root-relative = MATE_SCORE-(5+3).
        let (node_distance, store_ply) = (3, 5);
        let root_rel_at_store = MATE_SCORE - (store_ply + node_distance);
        let stored = mate_store_adjust(root_rel_at_store, store_ply);
        assert_eq!(stored, MATE_SCORE - node_distance, "node-intrinsic = mate in 3");
        // Probe the SAME entry at a different ply (2): root-relative re-derives correctly.
        let probe_ply = 2;
        let root_rel_at_probe = mate_probe_adjust(stored, probe_ply);
        assert_eq!(root_rel_at_probe, MATE_SCORE - (probe_ply + node_distance));
        // The node mate distance is preserved regardless of the probing ply.
        assert_eq!((MATE_SCORE - root_rel_at_probe) - probe_ply, node_distance);
    }

    #[test]
    fn loss_side_symmetric() {
        // Being mated in 3, found at ply 5.
        let (node_distance, store_ply) = (3, 5);
        let root_rel = -(MATE_SCORE - (store_ply + node_distance));
        let stored = mate_store_adjust(root_rel, store_ply);
        assert_eq!(stored, -(MATE_SCORE - node_distance));
        let recovered = mate_probe_adjust(stored, 2);
        assert_eq!(recovered, -(MATE_SCORE - (2 + node_distance)));
    }
}
