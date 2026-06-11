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

pub const MATE_SCORE: i32 = 1_000_000;
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 1000;
const INF: i32 = MATE_SCORE * 2;
const MAX_QUIESCENCE_PLY: u32 = 64;
pub const TELEMETRY_PLY_BUCKETS: usize = 32;
// Forcing quiet-check quiescence extensions (the d4 lesson: chess danger is not
// only captures). Same caps as the TS searcher.
const QUIET_CHECK_MAX_PLY: u32 = 2;
const MAX_QUIET_CHECKS_PER_NODE: usize = 3;

/// Specialist search lane (CVS_HETEROGENEOUS_SMP.md). Changes ONLY move
/// ordering on a helper thread (Level 1 / Channel A): the lane biases its
/// failure-family's moves to the front so its TT move propagates to the fast
/// main thread. Ordering never affects the alpha-beta value, so this is
/// correctness-safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lane {
    #[default]
    Fast,
    /// Bias king-defending moves first (king tropism + castling).
    KingSafety,
    /// Bias clean SEE-winning captures / material rescue first.
    See,
    /// Bias forcing moves (checks, then captures) first.
    Tactics,
    /// Bias captures of enemy pieces that DEFEND other enemy pieces
    /// (remove-the-guard), plus attacks on loose pieces.
    DefenderRemoval,
    /// Bias quiet moves whose destination covers our own king zone — the ugly
    /// human move that prevents collapse.
    QuietDefense,
    /// Bias pawn pushes toward promotion + king activity; phase-gated to low
    /// material.
    PawnEndgame,
}

impl Lane {
    /// Provenance id — 2 TT bits, so specialist lanes share id 1-3 by family
    /// group (King/Defense=1, See/DefenderRemoval=2, Tactics/Pawn=3).
    #[inline]
    pub fn id(self) -> u8 {
        match self {
            Lane::Fast => 0,
            Lane::KingSafety | Lane::QuietDefense => 1,
            Lane::See | Lane::DefenderRemoval => 2,
            Lane::Tactics | Lane::PawnEndgame => 3,
        }
    }

    /// Level-2 eval profile: the lane's own OPINION, expressed as a modified
    /// rung2 weight set. Diversity lives in the evaluator (the 8-lane
    /// benchmark showed ordering alone is mute: ~97% agreement, while an
    /// eval-diverse lane spoke uniquely 51% of the time). The king family
    /// values are the targeted ks-strong set that flipped the 4fxkLVBb loss
    /// rows toward Stockfish's moves.
    pub fn eval_profile(self, base: Option<Rung2Weights>) -> Option<Rung2Weights> {
        let mut w = base.unwrap_or_default();
        match self {
            Lane::Fast => return base,
            Lane::KingSafety => {
                w.king_danger = 15.0;
                w.king_central_exposure = 20.0;
                w.open_center_king_penalty = 45.0;
                w.king_escape_deficit = 12.0;
                w.enemy_queen_near_king = 6.0;
                w.king_zone_pressure *= 3.0;
                w.king_open_file *= 3.0;
                w.king_shield *= 3.0;
            }
            Lane::QuietDefense => {
                w.king_shield *= 4.0;
                w.king_escape_deficit = 10.0;
                w.king_danger = 8.0;
                w.hanging_piece *= 2.0;
            }
            Lane::See | Lane::DefenderRemoval => {
                w.hanging_piece *= 4.0;
            }
            Lane::Tactics => {
                w.mobility_knight *= 3.0;
                w.mobility_bishop *= 3.0;
                w.mobility_rook *= 2.0;
                w.mobility_queen *= 3.0;
            }
            Lane::PawnEndgame => {
                w.passed_pawn_mg *= 4.0;
                w.passed_pawn_eg *= 4.0;
                w.connected_passed_pawn *= 3.0;
            }
        }
        Some(w)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    /// Iterative-deepening target depth. Default 4.
    pub depth: u32,
    /// Optional wall-clock budget; the last completed depth is returned.
    pub max_time_ms: Option<u64>,
    /// Smart time management (None = legacy fixed budget). Soft target checked
    /// at iteration boundaries: stable best move releases early (~55% of
    /// soft), root instability/score drops extend (~140%), and `max_time_ms`
    /// stays the hard mid-search abort ceiling.
    pub soft_time_ms: Option<u64>,
    /// Forcing quiet-check quiescence extensions (R3.3). Default on.
    pub quiet_checks: bool,
    /// Transposition table (R3.5). Default on.
    pub use_tt: bool,
    /// Danger-triggered root depth extension (RSI loop 1, gated OFF by default):
    /// when the side to move faces king danger (enemy queen + king-zone pressure
    /// / king off home rank), search 1–2 plies deeper. Motivated by the
    /// sf2200-g14 forensic: d5 missed defensive resources that d7 finds.
    pub danger_extension: bool,
    /// Null-move pruning (Search Patch 2). Default on; gate for A/B tests.
    pub null_move: bool,
    /// Late-move reductions (Search Patch 3). Default on; gate for A/B tests.
    pub lmr: bool,
    /// PVS null-window searches + root aspiration windows (Search Patch 5).
    pub pvs: bool,
    /// Reverse futility pruning (Search Patch 7): shallow non-PV nodes whose
    /// static eval beats beta by a depth-scaled margin return immediately.
    pub rfp: bool,
    /// Futility pruning (Patch 7): at depth ≤ 3 with static eval far below
    /// alpha, quiet non-checking moves are skipped.
    pub futility: bool,
    /// Late-move pruning (Patch 7): at shallow depth, stop trying quiet moves
    /// after a move-count budget — ordering has had its chance.
    pub lmp: bool,
    /// SEE pruning (Patch 7): at shallow depth, skip captures that lose
    /// material by a depth-scaled SEE margin.
    pub see_prune: bool,
    /// Delta pruning (Patch 7, quiescence): skip captures that cannot lift
    /// stand-pat back to alpha even with a safety margin.
    pub delta_prune: bool,
    /// Lazy SMP search threads (shared lock-free TT, per-thread killers/
    /// history, shared stop flag). 1 = single-threaded, byte-identical to the
    /// pre-SMP engine.
    pub threads: usize,
    /// CVS geometry trace (brief Gate 2): when on, extract CVS features at each
    /// leaf eval and fold the active-id count into telemetry. OBSERVATIONAL
    /// only - does not alter move choice. Default OFF; benchmark-mode upper
    /// bound on the per-node geometry cost.
    pub cvs_trace: bool,
    /// Heterogeneous CVS-SMP: of the N-1 helpers, the first K run the loaded
    /// NNUE (the geometry-aware stand-in) while the MAIN thread runs the fast
    /// classical eval. K=0 = homogeneous. Only meaningful with threads>1 and a
    /// loaded net. See CVS_HETEROGENEOUS_SMP.md.
    pub cvs_helpers: usize,
    /// Specialist ordering lane for THIS searcher (helpers get assigned a
    /// roster; the main thread stays Fast). Ordering-only, Channel-A safe.
    pub lane: Lane,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            depth: 4,
            max_time_ms: None,
            soft_time_ms: None,
            quiet_checks: true,
            use_tt: true,
            danger_extension: false,
            null_move: true,
            lmr: true,
            pvs: true,
            // Patch 7 verdict (2026-06-10): main-search prunes REJECTED at
            // -188 Elo (futility/LMP cut quiet defenses the optimistic eval
            // mislabels hopeless); delta+SEE measured neutral. All five stay
            // OFF until the eval can be trusted (post-NNUE) or ordering
            // improves enough to make the quiet tail safely prunable.
            rfp: false,
            futility: false,
            lmp: false,
            see_prune: false,
            delta_prune: false,
            threads: 1,
            cvs_trace: false,
            cvs_helpers: 0,
            lane: Lane::Fast,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Telemetry {
    pub nodes: u64,
    pub main_nodes: u64,
    pub q_nodes: u64,
    pub q_capture_nodes: u64,
    pub q_see_skips: u64,
    pub quiet_check_extensions: u64,
    pub mate_threat_extensions: u64, // scaffolded (not yet implemented)
    pub hanging_major_extensions: u64, // scaffolded (not yet implemented)
    pub max_q_depth: u32,
    pub tt_probes: u64,
    pub tt_entries: u64,
    pub tt_hits: u64,
    pub tt_cutoffs: u64,
    pub beta_cutoffs: u64,
    pub hash_move_cutoffs: u64,
    pub first_move_cutoffs: u64,
    pub cutoff_move_index_sum: u64,
    pub cutoff_move_index_count: u64,
    pub legal_move_nodes: u64,
    pub legal_move_sum: u64,
    pub searched_moves: u64,
    pub pruned_moves: u64,
    pub elapsed_ms: u64,
    /// Extra root plies granted by the danger trigger this search (0–2).
    pub danger_extension_plies: u32,
    /// Quiet beta cutoffs where the cutting move was a stored killer.
    pub killer_cutoffs: u64,
    /// Quiet beta cutoffs ordered up purely by the history table.
    pub history_cutoffs: u64,
    /// Nodes pruned by the null-move heuristic (Patch 2).
    pub null_attempts: u64,
    pub null_cutoffs: u64,
    /// Moves searched at reduced depth by LMR (Patch 3).
    pub lmr_reductions: u64,
    /// LMR re-searches at full depth after a reduced search raised alpha.
    pub lmr_researches: u64,
    /// PVS null-window probes that required a full-window re-search.
    pub pvs_researches: u64,
    /// Aspiration windows that failed and re-searched at full width.
    pub aspiration_researches: u64,
    /// Nodes cut by reverse futility pruning (Patch 7).
    pub rfp_attempts: u64,
    pub rfp_cutoffs: u64,
    /// Quiet moves skipped by futility pruning (Patch 7).
    pub futility_attempts: u64,
    pub futility_skips: u64,
    /// Quiet moves skipped by late-move pruning (Patch 7).
    pub lmp_attempts: u64,
    pub lmp_skips: u64,
    /// Captures skipped by SEE pruning in the main search (Patch 7).
    pub see_prune_attempts: u64,
    pub see_prune_skips: u64,
    /// Captures dropped by delta pruning in quiescence (Patch 7).
    pub delta_attempts: u64,
    pub delta_skips: u64,
    /// Sum of active CVS feature IDs seen at leaves when cvs_trace is on.
    pub cvs_trace_features: u64,
    /// Transfer telemetry (main thread): TT move hints consumed that were
    /// written by a FOREIGN specialist lane, indexed by lane id 0..4.
    pub foreign_tt_hints: [u64; 4],
    /// Foreign-lane TT moves that produced the beta cutoff at a node.
    pub foreign_tt_cutoffs: [u64; 4],
    /// Main-search nodes by ply bucket (last bucket includes all deeper plies).
    pub ply_nodes: [u64; TELEMETRY_PLY_BUCKETS],
    /// Child searches launched by ply bucket; child/nodes approximates local EBF.
    pub ply_child_searches: [u64; TELEMETRY_PLY_BUCKETS],
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    /// Centipawns from the side-to-move's perspective.
    pub score_cp: i32,
    /// Signed mate distance in plies when a forced mate is found.
    pub mate: Option<i32>,
    pub pv: Vec<Move>,
    pub depth: u32,
    pub telemetry: Telemetry,
}

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
    /// depth², so deep cutoffs teach more than leaf noise.
    history: Vec<i32>, // 2*64*64, flat for cache friendliness
    /// NNUE eval head — when loaded, replaces the classical eval at every
    /// static/leaf eval site (search shape unchanged).
    nnue: Option<Nnue>,
    /// Optional NNUE used only by the first `cvs_helpers` SMP helpers. When
    /// set, the main thread keeps `nnue` as the authoritative eval.
    helper_nnue: Option<Nnue>,
    /// Incremental NNUE accumulator path-stack (top = current node). Non-empty
    /// only when the loaded net supports incremental updates; empty means
    /// eval falls back to the full recompute. Maintained in lockstep with
    /// pos.make/unmake at the three search sites (root/negamax/qsearch);
    /// null moves change no pieces, so the top stays valid through them.
    acc_stack: Vec<crate::eval::Accumulator>,
    /// Index of the current node's accumulator in `acc_stack`
    /// (usize::MAX = incremental path inactive).
    acc_top: usize,
    /// Reusable scratch for CVS-Fast trace IDs (no per-node allocation).
    cvs_buf: Vec<u32>,
}

impl Searcher {
    pub fn new(weights: ValueWeights, rung2: Option<Rung2Weights>) -> Searcher {
        Searcher {
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
            nnue: None,
            helper_nnue: None,
            acc_stack: Vec::new(),
            acc_top: usize::MAX,
            cvs_buf: Vec::with_capacity(32),
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

    #[inline]
    fn history_idx(side: usize, mv: Move) -> usize {
        side * 4096 + mv.from as usize * 64 + mv.to as usize
    }

    /// Record a QUIET move that produced a beta cutoff (the only teacher).
    fn record_quiet_cutoff(&mut self, side: usize, mv: Move, depth: i32, ply: u32) {
        let p = ply as usize;
        if p < MAX_KILLER_PLY && self.killers[p][0] != Some(mv) {
            self.killers[p][1] = self.killers[p][0];
            self.killers[p][0] = Some(mv);
        }
        let idx = Self::history_idx(side, mv);
        self.history[idx] += depth * depth;
        if self.history[idx] >= HISTORY_CAP {
            for h in self.history.iter_mut() {
                *h /= 2;
            }
        }
    }

    pub fn search(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
        if opts.threads > 1 {
            return self.search_smp(pos, opts);
        }
        self.search_single(pos, opts)
    }

    /// Lazy SMP: helpers run the same iterative deepening on clones of the
    /// position, sharing only the lock-free TT; their work surfaces as deeper
    /// TT entries and better move ordering for the main thread. The main
    /// thread's result is authoritative; helpers stop when it finishes.
    fn search_smp(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
        let stop = Arc::new(AtomicBool::new(false));
        let single = SearchOptions { threads: 1, ..opts };
        // Heterogeneous CVS-SMP has two modes:
        // * With helper_nnue set, the main thread keeps `self.nnue`; first K
        //   helpers use helper_nnue; remaining helpers use the main net.
        // * Legacy mode, with no helper_nnue, detaches the main net so first K
        //   helpers carry it while the main thread runs classical.
        let helper_override = self.helper_nnue.clone();
        let legacy_detach =
            opts.cvs_helpers > 0 && helper_override.is_none() && self.nnue.is_some();
        let detached_net = if legacy_detach {
            self.nnue.take()
        } else {
            None
        };
        let main_net_for_helpers = self.nnue.clone();
        let lane_net = helper_override
            .or_else(|| detached_net.clone())
            .or_else(|| main_net_for_helpers.clone());
        let (mut result, helper_nodes) = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for t in 0..opts.threads - 1 {
                let tt = Arc::clone(&self.tt);
                let stop = Arc::clone(&stop);
                let weights = self.weights;
                let rung2 = self.rung2;
                let nnue = if t < opts.cvs_helpers {
                    lane_net.clone()
                } else {
                    main_net_for_helpers.clone()
                };
                let mut hpos = pos.clone();
                // Odd helpers aim one ply deeper — cheap diversity so threads
                // don't lockstep on identical trees.
                // Specialist lane roster for the first K helpers (Channel-A
                // ordering-only); remaining helpers stay Fast.
                const ROSTER: [Lane; 3] = [Lane::KingSafety, Lane::See, Lane::Tactics];
                let lane = if t < opts.cvs_helpers {
                    ROSTER[t % ROSTER.len()]
                } else {
                    Lane::Fast
                };
                let hopts = SearchOptions {
                    depth: single.depth + (t as u32 & 1),
                    lane,
                    ..single
                };
                let lead = 2 + (t as u32 % 6);
                handles.push(scope.spawn(move || {
                    let mut helper = Searcher::new(weights, rung2);
                    helper.tt = tt;
                    helper.nnue = nnue;
                    helper.stop = Some(stop);
                    helper.id_start = lead;
                    let r = helper.search_single(&mut hpos, hopts);
                    r.telemetry.nodes
                }));
            }
            let r = self.search_single(pos, single);
            stop.store(true, Ordering::Relaxed);
            let nodes: u64 = handles.into_iter().map(|h| h.join().unwrap_or(0)).sum();
            (r, nodes)
        });
        result.telemetry.nodes += helper_nodes;
        if legacy_detach {
            self.nnue = detached_net;
        }
        result
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
        self.tel = Telemetry::default();
        self.tel.danger_extension_plies = danger_plies;
        self.aborted = false;
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

        let mut result = SearchResult {
            best_move: None,
            score_cp: evaluate(pos, &self.weights, self.rung2.as_ref()),
            mate: None,
            pv: Vec::new(),
            depth: 0,
            telemetry: self.tel,
        };

        let mut prev_score: Option<i32> = None;
        // Smart-time state: best-move stability and last score for the
        // iteration-boundary stop/extend decision.
        let mut tm_prev_best: Option<Move> = None;
        let mut tm_stable: u32 = 0;
        let mut tm_last_score: Option<i32> = None;
        for depth in self.id_start.min(max_depth)..=max_depth {
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
            result = SearchResult {
                best_move: best,
                score_cp: score,
                mate,
                pv: self.extract_pv(pos, best, depth as usize),
                depth,
                telemetry: self.tel,
            };
            // A proven mate cannot be improved by searching deeper.
            if score.abs() > MATE_THRESHOLD {
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
                    break;
                }
            }
        }
        self.tel.elapsed_ms = started.elapsed().as_millis() as u64;
        result.telemetry = self.tel;
        result
    }

    /// Root: an explicit negamax move loop so the best move is tracked directly
    /// (the TS reference reads it back from the root TT entry — equivalent).
    fn root(
        &mut self,
        pos: &mut Position,
        depth: i32,
        alpha0: i32,
        beta: i32,
    ) -> (i32, Option<Move>) {
        let mut legal = generate_legal_list(pos);
        if legal.is_empty() {
            return (if in_check(pos) { -MATE_SCORE } else { 0 }, None);
        }
        self.tel.main_nodes += 1;
        self.tel.ply_nodes[0] += 1;
        self.tel.legal_move_nodes += 1;
        self.tel.legal_move_sum += legal.len() as u64;
        let tt_move = if self.opts.use_tt {
            self.tel.tt_probes += 1;
            if let Some(e) = self.tt_probe(pos.hash) {
                self.tel.tt_entries += 1;
                e.mv
            } else {
                None
            }
        } else {
            None
        };
        self.order_moves(pos, legal.as_mut_slice(), tt_move, 0);

        let mut alpha = alpha0;
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for move_index in 0..legal.len() {
            let mv = legal.get(move_index);
            self.tel.searched_moves += 1;
            self.tel.ply_child_searches[0] += 1;
            self.acc_make(pos, mv);
            pos.make(mv);
            let mut score;
            if !self.opts.pvs || move_index == 0 || alpha <= alpha0 {
                score = -self.negamax(pos, depth - 1, -beta, -alpha, 1, true);
            } else {
                // PVS: probe with a null window; re-search on a raise.
                score = -self.negamax(pos, depth - 1, -alpha - 1, -alpha, 1, true);
                if score > alpha && score < beta && !self.aborted {
                    self.tel.pvs_researches += 1;
                    score = -self.negamax(pos, depth - 1, -beta, -alpha, 1, true);
                }
            }
            pos.unmake();
            self.acc_unmake();
            if self.aborted {
                return (best, best_move);
            }
            if score > best {
                best = score;
                best_move = Some(mv);
            }
            if best > alpha {
                alpha = best;
            }
        }
        if self.opts.use_tt {
            self.store(pos.hash, depth, best, Flag::Exact, best_move);
        }
        (best, best_move)
    }

    /// Push the child accumulator for `mv` (call with the PRE-make position).
    /// Slot-reuse: the stack never shrinks, so steady-state pushes are two
    /// memcpys + the feature deltas — no per-node allocation.
    #[inline]
    fn acc_make(&mut self, pos: &Position, mv: Move) {
        if let Some(n) = &self.nnue {
            if self.acc_top == usize::MAX {
                return; // incremental path inactive (cvs model)
            }
            let top = self.acc_top;
            if self.acc_stack.len() <= top + 1 {
                let clone = self.acc_stack[top].clone();
                self.acc_stack.push(clone);
            } else {
                let (head, tail) = self.acc_stack.split_at_mut(top + 1);
                tail[0].white.copy_from_slice(&head[top].white);
                tail[0].black.copy_from_slice(&head[top].black);
            }
            n.acc_apply(&mut self.acc_stack[top + 1], pos, mv);
            self.acc_top = top + 1;
        }
    }

    #[inline]
    fn acc_unmake(&mut self) {
        if self.acc_top != usize::MAX && self.acc_top > 0 {
            self.acc_top -= 1;
        }
    }

    fn time_up(&mut self) -> bool {
        if let Some(stop) = &self.stop {
            if (self.tel.nodes & 1023) == 0 && stop.load(Ordering::Relaxed) {
                return true;
            }
        }
        if let Some(deadline) = self.deadline {
            if (self.tel.nodes & 1023) == 0 && Instant::now() >= deadline {
                return true;
            }
        }
        false
    }

    /// Leaf eval = the TS `evaluate()` (stm POV, terminal-aware), given that the
    /// caller already knows whether legal moves exist (avoids a second movegen).
    /// Side-to-move static eval — NNUE when loaded, classical otherwise.
    fn static_eval(&self, pos: &mut Position) -> i32 {
        if let Some(n) = &self.nnue {
            if self.acc_top != usize::MAX {
                return n.eval_acc(&self.acc_stack[self.acc_top], pos.stm);
            }
            return n.eval_stm(pos);
        }
        evaluate(pos, &self.weights, self.rung2.as_ref())
    }

    fn leaf_eval(&mut self, pos: &Position, no_legal: bool, checked: bool) -> i32 {
        if self.opts.cvs_trace {
            crate::eval::cvs_features::extract_cvs_ids_into(pos, &mut self.cvs_buf);
            self.tel.cvs_trace_features += self.cvs_buf.len() as u64;
        }
        if no_legal {
            return if checked { -MATE_SCORE } else { 0 };
        }
        if pos.halfmove >= 100 || insufficient_material(pos) {
            return 0;
        }
        if let Some(n) = &self.nnue {
            if self.acc_top != usize::MAX {
                return n.eval_acc(&self.acc_stack[self.acc_top], pos.stm);
            }
            return n.eval_stm(pos);
        }
        let white = js_round(evaluate_white_float_nonterminal(
            pos,
            &self.weights,
            self.rung2.as_ref(),
        ));
        if pos.stm == Color::White {
            white
        } else {
            -white
        }
    }

    fn negamax(
        &mut self,
        pos: &mut Position,
        depth: i32,
        alpha_in: i32,
        beta_in: i32,
        ply: u32,
        allow_null: bool,
    ) -> i32 {
        if self.time_up() {
            self.aborted = true;
            return self.static_eval(pos);
        }
        self.tel.nodes += 1;
        self.tel.main_nodes += 1;
        let ply_bucket = (ply as usize).min(TELEMETRY_PLY_BUCKETS - 1);
        self.tel.ply_nodes[ply_bucket] += 1;

        // Node reorder (audit/Patch 6): everything that can cut this node off
        // WITHOUT generating moves runs first - draw rules, repetition, TT
        // probe, quiescence dispatch. Full legal movegen is the expensive step
        // and now only runs for nodes that truly expand.
        let checked = in_check(pos);
        let rule_draw = pos.halfmove >= 100 || insufficient_material(pos);
        if !checked && rule_draw {
            return 0;
        }
        // Draw by repetition: one prior occurrence in the path (or the game
        // history the position was built with) scores 0. Checked BEFORE the
        // TT probe and returned WITHOUT storing - repetition scores are
        // path-dependent and must not leak into other lines via the table.
        if ply > 0 && pos.is_repetition() {
            return 0;
        }
        if depth <= 0 {
            return self.quiesce(pos, alpha_in, beta_in, ply, 0);
        }
        let mut alpha = alpha_in;
        let mut beta = beta_in;
        let mut tt_move: Option<Move> = None;
        let mut tt_move_lane: u8 = 0;
        if self.opts.use_tt && !(checked && rule_draw) {
            self.tel.tt_probes += 1;
            if let Some(e) = self.tt_probe(pos.hash) {
                self.tel.tt_entries += 1;
                tt_move = e.mv;
                tt_move_lane = e.lane;
                // Transfer telemetry: this node consumed a move hint written by
                // a foreign specialist lane.
                if e.mv.is_some() && e.lane != self.opts.lane.id() {
                    self.tel.foreign_tt_hints[(e.lane & 3) as usize] += 1;
                }
                if e.depth >= depth {
                    self.tel.tt_hits += 1;
                    match e.flag {
                        Flag::Exact => {
                            self.tel.tt_cutoffs += 1;
                            return e.score;
                        }
                        Flag::Lower => {
                            if e.score > alpha {
                                alpha = e.score;
                            }
                        }
                        Flag::Upper => {
                            if e.score < beta {
                                beta = e.score;
                            }
                        }
                    }
                    if alpha >= beta {
                        self.tel.tt_cutoffs += 1;
                        return e.score;
                    }
                }
            }
        }

        let mut legal = generate_legal_list(pos);
        if legal.is_empty() {
            return if checked { -MATE_SCORE + ply as i32 } else { 0 };
        }
        self.tel.legal_move_nodes += 1;
        self.tel.legal_move_sum += legal.len() as u64;
        if checked && (pos.halfmove >= 100 || insufficient_material(pos)) {
            return 0; // evasions exist so it is not mate; the draw rule wins
        }

        // Lazy per-node static eval, shared by null-move / RFP / futility so
        // the (expensive) eval runs at most once per node.
        let mut static_cache: Option<i32> = None;
        macro_rules! static_eval {
            () => {{
                if static_cache.is_none() {
                    static_cache = Some(self.static_eval(pos));
                }
                static_cache.unwrap()
            }};
        }
        let is_pv = beta_in - alpha_in > 1;

        // Reverse futility pruning (Search Patch 7): at a shallow non-PV node
        // whose static eval beats beta by a depth-scaled margin, a quiet
        // continuation is overwhelmingly likely to hold — cut without moving.
        // Guards mirror null: never in check, never around mate scores.
        // rfp-v2 (2026-06-11): depth cap 6->4 after hard-100 exposed a
        // mate-scale miss at deeper cuts (pos 64, +9259cp) — forcing lines
        // can hide below a d5/d6 static cut even with a calibrated eval.
        if self.opts.rfp && !is_pv && !checked && depth <= 4 && beta.abs() < MATE_THRESHOLD {
            self.tel.rfp_attempts += 1;
            if static_eval!() - 90 * depth >= beta {
                self.tel.rfp_cutoffs += 1;
                return beta;
            }
        }

        // Null-move pruning (Search Patch 2, hardened per audit): if passing
        // the turn STILL fails high on a reduced search, a real move surely
        // will — prune. Guards: never in check, never two nulls in a row,
        // depth ≥ 3, never around mate scores, the static eval must already
        // be ≥ beta (don't speculate from below — the classic guard that also
        // keeps null out of PV-ish nodes pre-PVS), and a strong zugzwang
        // filter (a major piece, or at least two minors). R scales with depth.
        // The cutoff is fail-hard beta and deliberately NOT stored in the TT
        // (null results are window/path-dependent).
        if allow_null
            && self.opts.null_move
            && !checked
            && depth >= 3
            && beta.abs() < MATE_THRESHOLD
            && Self::null_material_ok(pos)
            && static_eval!() >= beta
        {
            self.tel.null_attempts += 1;
            let null_r = if depth >= 6 { 3 } else { 2 };
            let undo = pos.make_null();
            let score = -self.negamax(pos, depth - 1 - null_r, -beta, -beta + 1, ply + 1, false);
            pos.unmake_null(undo);
            if self.aborted {
                return score;
            }
            if score >= beta {
                self.tel.null_cutoffs += 1;
                return beta;
            }
        }

        self.order_moves(pos, legal.as_mut_slice(), tt_move, ply);
        let key = pos.hash;
        let side = pos.stm.index();
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        let killer_pair = self.killers.get(ply as usize).copied().unwrap_or([None; 2]);
        // Futility precondition (Patch 7): at frontier depths with the static
        // eval hopelessly below alpha, quiet non-checking moves cannot recover
        // — only tactics can, so only tactics get searched.
        let futile = self.opts.futility
            && !checked
            && depth <= 3
            && alpha.abs() < MATE_THRESHOLD
            && static_eval!() + 120 + 150 * depth <= alpha;
        // Late-move pruning budget (Patch 7): after ordering has surfaced the
        // TT move, killers, and history leaders, the quiet tail at shallow
        // depth is almost never the refutation.
        let lmp_budget = (4 + depth * depth) as usize;
        for move_index in 0..legal.len() {
            let mv = legal.get(move_index);
            // Patch 7 skips. All require one searched move already (best is
            // real, so a pruned node still returns a legal score), never fire
            // in check, and exempt the TT move and killers.
            if best > -INF && !checked && alpha.abs() < MATE_THRESHOLD {
                let quiet = !mv.flag.is_capture() && mv.flag.promo_piece().is_none();
                let exempt = Some(mv) == tt_move || killer_pair.contains(&Some(mv));
                if quiet && !exempt {
                    if self.opts.lmp && depth <= 4 {
                        self.tel.lmp_attempts += 1;
                        if move_index >= lmp_budget {
                            self.tel.lmp_skips += 1;
                            self.tel.pruned_moves += 1;
                            continue;
                        }
                    }
                    if futile && !gives_check(pos, mv) {
                        self.tel.futility_attempts += 1;
                        self.tel.futility_skips += 1;
                        self.tel.pruned_moves += 1;
                        continue;
                    }
                } else if self.opts.see_prune
                    && mv.flag.is_capture()
                    && mv.flag.promo_piece().is_none()
                    && depth <= 5
                {
                    self.tel.see_prune_attempts += 1;
                    if see(pos, mv.from, mv.to) < -50 * depth {
                        self.tel.see_prune_skips += 1;
                        self.tel.pruned_moves += 1;
                        continue;
                    }
                }
            }
            // Late-move reductions (Search Patch 3, conservative tier): with
            // good ordering, late quiet non-checking moves rarely matter —
            // search them shallower first, and re-search at full depth only
            // if the reduced probe beats alpha. Never reduce: captures,
            // promotions, checks (given or escaped), the TT move, killers,
            // or the first three moves.
            let reduce = self.opts.lmr
                && depth >= 3
                && move_index >= 3
                && !checked
                && !mv.flag.is_capture()
                && mv.flag.promo_piece().is_none()
                && Some(mv) != tt_move
                && !killer_pair.contains(&Some(mv))
                && !gives_check(pos, mv);
            self.acc_make(pos, mv);
            self.tel.searched_moves += 1;
            self.tel.ply_child_searches[ply_bucket] += 1;
            pos.make(mv);
            let mut score;
            if reduce {
                self.tel.lmr_reductions += 1;
                score = -self.negamax(pos, depth - 2, -alpha - 1, -alpha, ply + 1, true);
                if score > alpha && !self.aborted {
                    self.tel.lmr_researches += 1;
                    score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, true);
                }
            } else if self.opts.pvs && move_index > 0 && best > alpha_in {
                // PVS (Patch 5): once a PV move raised alpha, probe the rest
                // with a null window and re-search only on a raise.
                score = -self.negamax(pos, depth - 1, -alpha - 1, -alpha, ply + 1, true);
                if score > alpha && score < beta && !self.aborted {
                    self.tel.pvs_researches += 1;
                    score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, true);
                }
            } else {
                score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, true);
            }
            pos.unmake();
            self.acc_unmake();
            if self.aborted {
                return if best > -INF { best } else { score };
            }
            if score > best {
                best = score;
                best_move = Some(mv);
            }
            if best > alpha {
                alpha = best;
            }
            if alpha >= beta {
                self.tel.beta_cutoffs += 1;
                if move_index == 0 {
                    self.tel.first_move_cutoffs += 1;
                }
                if Some(mv) == tt_move {
                    self.tel.hash_move_cutoffs += 1;
                }
                self.tel.cutoff_move_index_sum += (move_index as u64) + 1;
                self.tel.cutoff_move_index_count += 1;
                if Some(mv) == tt_move && tt_move_lane != self.opts.lane.id() {
                    self.tel.foreign_tt_cutoffs[(tt_move_lane & 3) as usize] += 1;
                }
                // Quiet cutoffs teach the killers/history tables (Patch 1).
                if !mv.flag.is_capture() && mv.flag.promo_piece().is_none() {
                    let p = ply as usize;
                    if p < MAX_KILLER_PLY && self.killers[p].contains(&Some(mv)) {
                        self.tel.killer_cutoffs += 1;
                    } else if self.history[Self::history_idx(side, mv)] > 0 {
                        self.tel.history_cutoffs += 1;
                    }
                    self.record_quiet_cutoff(side, mv, depth, ply);
                }
                if self.opts.use_tt {
                    self.store(key, depth, best, Flag::Lower, best_move);
                }
                return best;
            }
        }
        if self.opts.use_tt {
            let flag = if best > alpha_in {
                Flag::Exact
            } else {
                Flag::Upper
            };
            self.store(key, depth, best, flag, best_move);
        }
        best
    }

    /// Quiescence (Search Patch 6: specialized generation). Evasion nodes
    /// still pay full legal gen (exact mate detection); all other q-nodes
    /// generate NOISY moves directly - captures, promotions, ep - the bulk of
    /// the q-tree. Quiet-check candidates only force full gen inside the small
    /// QUIET_CHECK_MAX_PLY window. Stalemate on no-noisy nodes uses the
    /// early-exit has_legal_move probe.
    fn quiesce(
        &mut self,
        pos: &mut Position,
        alpha_in: i32,
        beta: i32,
        ply: u32,
        q_depth: u32,
    ) -> i32 {
        let checked = in_check(pos);
        if self.time_up() {
            self.aborted = true;
            return self.leaf_eval(pos, false, checked);
        }
        self.tel.nodes += 1;
        self.tel.q_nodes += 1;
        if q_depth > self.tel.max_q_depth {
            self.tel.max_q_depth = q_depth;
        }

        if checked {
            let legal = generate_legal_list(pos);
            if legal.is_empty() {
                return -MATE_SCORE + ply as i32;
            }
            return self.quiesce_moves(pos, legal, true, alpha_in, beta, ply, q_depth);
        }

        let mut noisy = generate_legal_noisy_list(pos);
        let mut alpha = alpha_in;
        if noisy.is_empty() && !has_legal_move(pos) {
            return 0; // stalemate (rare path; early-exit probe keeps it cheap)
        }
        let stand = self.leaf_eval(pos, false, false);
        if stand >= beta {
            return beta; // fail-hard stand-pat, like the TS reference
        }
        if stand > alpha {
            alpha = stand;
        }
        if ply >= MAX_QUIESCENCE_PLY {
            return stand;
        }

        let before_see = noisy.len();
        noisy.retain(|m| see(pos, m.from, m.to) >= 0);
        self.tel.q_see_skips += (before_see - noisy.len()) as u64;
        // Delta pruning (Patch 7): a capture whose victim value plus a safety
        // margin cannot lift stand-pat back to alpha is hopeless. Promotions
        // are exempt, and the whole filter switches off in low-material
        // endgames where insufficient-material/zugzwang effects dominate.
        if self.opts.delta_prune && phase_units(pos) > 6 {
            let before = noisy.len();
            self.tel.delta_attempts += before as u64;
            noisy.retain(|m| {
                if m.flag.promo_piece().is_some() {
                    return true;
                }
                let victim = pos
                    .piece_at(m.to)
                    .map(|(_, p)| SEE_VALUE[p.index()])
                    .unwrap_or(SEE_VALUE[Piece::Pawn.index()]);
                stand + victim + 200 > alpha
            });
            self.tel.delta_skips += (before - noisy.len()) as u64;
        }
        if self.opts.quiet_checks && q_depth < QUIET_CHECK_MAX_PLY {
            // The forcing quiet-check window is the ONLY non-evasion path that
            // still pays full legal generation - by design, it is tiny.
            let legal = generate_legal_list(pos);
            let mut quiet_checks = MoveList::new();
            for i in 0..legal.len() {
                let m = legal.get(i);
                if !m.flag.is_capture()
                    && m.flag.promo_piece().is_none()
                    && see(pos, m.from, m.to) >= 0
                    && gives_check(pos, m)
                {
                    quiet_checks.push(m);
                }
            }
            Self::sort_by_key_desc(quiet_checks.as_mut_slice(), |m| self.capture_order(pos, *m));
            let quiet_len = quiet_checks.len().min(MAX_QUIET_CHECKS_PER_NODE);
            if quiet_len > 0 {
                self.tel.quiet_check_extensions += quiet_len as u64;
            }
            for i in 0..quiet_len {
                noisy.push(quiet_checks.get(i));
            }
        }
        self.quiesce_moves(pos, noisy, false, alpha, beta, ply, q_depth)
    }

    #[allow(clippy::too_many_arguments)]
    fn quiesce_moves(
        &mut self,
        pos: &mut Position,
        mut moves: MoveList,
        checked: bool,
        alpha_in: i32,
        beta: i32,
        ply: u32,
        q_depth: u32,
    ) -> i32 {
        let mut alpha = alpha_in;
        Self::sort_by_key_desc(moves.as_mut_slice(), |m| self.capture_order(pos, *m));

        let mut best = if checked { -INF } else { alpha };
        for i in 0..moves.len() {
            let mv = moves.get(i);
            if mv.flag.is_capture() {
                self.tel.q_capture_nodes += 1;
            }
            self.acc_make(pos, mv);
            pos.make(mv);
            let score = -self.quiesce(pos, -beta, -alpha, ply + 1, q_depth + 1);
            pos.unmake();
            self.acc_unmake();
            if self.aborted {
                return best;
            }
            if score > best {
                best = score;
            }
            if best > alpha {
                alpha = best;
            }
            if alpha >= beta {
                self.tel.beta_cutoffs += 1;
                break;
            }
        }
        best
    }

    /// Strong zugzwang guard for null-move pruning: the side to move needs a
    /// major piece (rook/queen) or at least two minor pieces. Knight-only and
    /// single-minor endings are classic null-move blind spots.
    fn null_material_ok(pos: &Position) -> bool {
        let c = pos.stm.index();
        let majors = pos.pieces[c][Piece::Rook.index()] | pos.pieces[c][Piece::Queen.index()];
        if majors != 0 {
            return true;
        }
        let minors = pos.pieces[c][Piece::Knight.index()] | pos.pieces[c][Piece::Bishop.index()];
        minors.count_ones() >= 2
    }

    /// MVV-LVA ordering score (captures first, by victim/attacker), matching the
    /// TS `captureOrder`: victim·16 − attacker (ep victim = pawn; quiets victim 0).
    fn capture_order(&self, pos: &Position, mv: Move) -> i32 {
        let attacker = pos.piece_at(mv.from).map(|(_, p)| p).unwrap_or(Piece::Pawn);
        let victim_value = if mv.flag == MoveFlag::EnPassant {
            SEE_VALUE[Piece::Pawn.index()]
        } else if mv.flag.is_capture() {
            pos.piece_at(mv.to)
                .map(|(_, p)| SEE_VALUE[p.index()])
                .unwrap_or(0)
        } else {
            0
        };
        victim_value * 16 - SEE_VALUE[attacker.index()]
    }

    fn sort_by_key_desc<F>(moves: &mut [Move], mut key_of: F)
    where
        F: FnMut(&Move) -> i32,
    {
        debug_assert!(moves.len() <= MAX_MOVES);
        let mut keys = [0i32; MAX_MOVES];
        for i in 0..moves.len() {
            keys[i] = key_of(&moves[i]);
        }
        for i in 1..moves.len() {
            let mv = moves[i];
            let key = keys[i];
            let mut j = i;
            while j > 0 && keys[j - 1] < key {
                moves[j] = moves[j - 1];
                keys[j] = keys[j - 1];
                j -= 1;
            }
            moves[j] = mv;
            keys[j] = key;
        }
    }

    /// Main-search ordering (Search Patch 1):
    ///   TT move ≫ winning captures/promotions ≫ killers ≫ history quiets ≫
    ///   plain quiets ≫ losing captures.
    /// Ordering-only — values are unaffected; sort is stable so equal scores
    /// keep generation order and searches stay deterministic.
    fn order_moves(&self, pos: &Position, moves: &mut [Move], tt_move: Option<Move>, ply: u32) {
        let side = pos.stm.index();
        let killers = self.killers.get(ply as usize).copied().unwrap_or([None; 2]);
        let score_of = |m: &Move| -> i32 {
            if Some(*m) == tt_move {
                return 1_000_000_000;
            }
            if m.flag.promo_piece().is_some() {
                return 900_000 + self.capture_order(pos, *m);
            }
            if m.flag.is_capture() {
                // En passant is always a pawn-takes-pawn — never losing.
                let winning = m.flag == MoveFlag::EnPassant || see(pos, m.from, m.to) >= 0;
                return if winning {
                    800_000 + self.capture_order(pos, *m)
                } else {
                    -100_000 + self.capture_order(pos, *m)
                };
            }
            if killers[0] == Some(*m) {
                return 700_000 + self.lane_bonus(pos, *m, ply);
            }
            if killers[1] == Some(*m) {
                return 699_999 + self.lane_bonus(pos, *m, ply);
            }
            self.history[Self::history_idx(side, *m)] + self.lane_bonus(pos, *m, ply)
        };
        Self::sort_by_key_desc(moves, score_of);
    }

    /// Lane ordering bonus (Level-1 specialist lanes). Applied to quiet/killer
    /// tiers so it reorders WITHIN ordering without displacing the TT move or
    /// winning captures. Ordering-only -> cannot change the search value.
    fn lane_bonus(&self, pos: &Position, m: Move, ply: u32) -> i32 {
        // Shallow-node gate: lane ordering exists to steer which TT moves get
        // written near the root (the ones the main thread inherits). Beyond
        // ply 3 the clone/tropism cost dwarfs the propagation value - measured
        // at ~40% nps loss ungated.
        if ply > 3 {
            return 0;
        }
        match self.opts.lane {
            Lane::Fast => 0,
            Lane::Tactics => {
                // Forcing first: checks lifted above the quiet tail.
                let mut p = pos.clone();
                if gives_check(&mut p, m) {
                    60_000
                } else {
                    0
                }
            }
            Lane::See => {
                // Reward moves to/that leave a clean material footing; for quiets
                // this is mild, the lane's force is on captures (already tiered).
                let s = see(pos, m.from, m.to);
                s.clamp(-200, 200) * 50
            }
            Lane::DefenderRemoval => {
                // Remove-the-guard: capturing a piece that itself defends other
                // enemy material is worth more than its raw exchange value.
                if !m.flag.is_capture() {
                    return 0;
                }
                if let Some((vc, vp)) = pos.piece_at(m.to) {
                    // How many enemy pieces does the victim defend from its square?
                    let occ = pos.all;
                    let att = match vp {
                        Piece::Pawn => crate::attacks::pawn_attacks(vc, m.to),
                        Piece::Knight => crate::attacks::knight_attacks(m.to),
                        Piece::Bishop => crate::attacks::bishop_attacks(m.to, occ),
                        Piece::Rook => crate::attacks::rook_attacks(m.to, occ),
                        Piece::Queen => crate::attacks::queen_attacks(m.to, occ),
                        Piece::King => king_attacks(m.to),
                    };
                    let guards = (att & pos.occ[vc.index()]).count_ones() as i32;
                    guards * 20_000
                } else {
                    0
                }
            }
            Lane::QuietDefense => {
                // Quiet move whose destination covers our king zone.
                if m.flag.is_capture() || m.flag.promo_piece().is_some() {
                    return 0;
                }
                let Some((_, mp)) = pos.piece_at(m.from) else {
                    return 0;
                };
                if mp == Piece::King {
                    return 0;
                }
                let occ = pos.all;
                let att = match mp {
                    Piece::Pawn => crate::attacks::pawn_attacks(pos.stm, m.to),
                    Piece::Knight => crate::attacks::knight_attacks(m.to),
                    Piece::Bishop => crate::attacks::bishop_attacks(m.to, occ),
                    Piece::Rook => crate::attacks::rook_attacks(m.to, occ),
                    Piece::Queen => crate::attacks::queen_attacks(m.to, occ),
                    Piece::King => 0,
                };
                let zone = king_attacks(pos.king_sq(pos.stm));
                ((att & zone).count_ones() as i32) * 15_000
            }
            Lane::PawnEndgame => {
                // Phase-gated: only speak in low material.
                if phase_units(pos) > 10 {
                    return 0;
                }
                let Some((_, mp)) = pos.piece_at(m.from) else {
                    return 0;
                };
                match mp {
                    Piece::Pawn => {
                        // Push toward promotion: rank progress from mover POV.
                        let r = crate::rank_of(m.to) as i32;
                        let prog = if pos.stm == Color::White { r } else { 7 - r };
                        prog * 8_000
                    }
                    Piece::King => 10_000, // king activity matters in endings
                    _ => 0,
                }
            }
            Lane::KingSafety => {
                // Castling is the archetypal king-safety move.
                if matches!(m.flag, MoveFlag::KingCastle | MoveFlag::QueenCastle) {
                    return 80_000;
                }
                // Otherwise prefer moves that REDUCE enemy king-tropism toward
                // our own king (cheap: enemy piece weight / distance to our king).
                let before = self.king_tropism(pos);
                let mut p = pos.clone();
                p.make(m);
                let after = self.king_tropism_of(&p, pos.stm);
                p.unmake();
                // Lower tropism after = safer = higher bonus.
                ((before - after) * 1500).clamp(-40_000, 40_000)
            }
        }
    }

    /// Enemy king-tropism toward the side-to-move's own king (higher = more
    /// enemy pressure). Cheap king-safety proxy: Σ enemyWeight / Chebyshev dist.
    fn king_tropism(&self, pos: &Position) -> i32 {
        self.king_tropism_of(pos, pos.stm)
    }

    fn king_tropism_of(&self, pos: &Position, own: Color) -> i32 {
        let ksq = pos.king_sq(own);
        let kf = crate::file_of(ksq) as i32;
        let kr = crate::rank_of(ksq) as i32;
        let enemy = own.flip().index();
        let mut t = 0i32;
        // Knights/bishops 2, rooks 3, queens 5 (attack-unit weights).
        for (pc, w) in [
            (Piece::Knight, 2),
            (Piece::Bishop, 2),
            (Piece::Rook, 3),
            (Piece::Queen, 5),
        ] {
            let mut bb = pos.pieces[enemy][pc.index()];
            while bb != 0 {
                let sq = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                let df = (crate::file_of(sq) as i32 - kf).abs();
                let dr = (crate::rank_of(sq) as i32 - kr).abs();
                let dist = df.max(dr).max(1);
                t += w * (8 - dist).max(0);
            }
        }
        t
    }

    /// Test/inspection: the lane-ordered root moves for a position.
    pub fn debug_ordered_root_moves(&mut self, pos: &mut Position, lane: Lane) -> Vec<Move> {
        self.opts.lane = lane;
        let mut moves = generate_legal_list(pos);
        self.order_moves(pos, moves.as_mut_slice(), None, 0);
        moves.into_vec()
    }

    #[inline]
    fn tt_probe(&self, key: u64) -> Option<TtEntry> {
        self.tt.probe(key)
    }

    fn store(&mut self, key: u64, depth: i32, score: i32, flag: Flag, mv: Option<Move>) {
        self.tt.store(
            key,
            depth,
            score,
            flag,
            mv,
            self.tt_generation,
            self.opts.lane.id(),
        );
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
            let Some(entry) = self.tt_probe(pos.hash) else {
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
}
