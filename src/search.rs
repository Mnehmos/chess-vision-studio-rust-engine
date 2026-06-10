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
    evaluate, evaluate_white_float_nonterminal, insufficient_material, js_round, Rung2Weights,
    ValueWeights,
};
use crate::movegen::{generate_legal, gives_check, in_check};
use crate::see::{see, SEE_VALUE};
use crate::{rank_of, Color, Move, MoveFlag, Piece, Position};
use std::time::Instant;

pub const MATE_SCORE: i32 = 1_000_000;
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 1000;
const INF: i32 = MATE_SCORE * 2;
const MAX_QUIESCENCE_PLY: u32 = 64;
// Forcing quiet-check quiescence extensions (the d4 lesson: chess danger is not
// only captures). Same caps as the TS searcher.
const QUIET_CHECK_MAX_PLY: u32 = 2;
const MAX_QUIET_CHECKS_PER_NODE: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    /// Iterative-deepening target depth. Default 4.
    pub depth: u32,
    /// Optional wall-clock budget; the last completed depth is returned.
    pub max_time_ms: Option<u64>,
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
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            depth: 4,
            max_time_ms: None,
            quiet_checks: true,
            use_tt: true,
            danger_extension: false,
            null_move: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Telemetry {
    pub nodes: u64,
    pub q_nodes: u64,
    pub q_capture_nodes: u64,
    pub quiet_check_extensions: u64,
    pub mate_threat_extensions: u64,        // scaffolded (not yet implemented)
    pub hanging_major_extensions: u64,      // scaffolded (not yet implemented)
    pub max_q_depth: u32,
    pub tt_hits: u64,
    pub beta_cutoffs: u64,
    pub elapsed_ms: u64,
    /// Extra root plies granted by the danger trigger this search (0–2).
    pub danger_extension_plies: u32,
    /// Quiet beta cutoffs where the cutting move was a stored killer.
    pub killer_cutoffs: u64,
    /// Quiet beta cutoffs ordered up purely by the history table.
    pub history_cutoffs: u64,
    /// Nodes pruned by the null-move heuristic (Patch 2).
    pub null_cutoffs: u64,
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

#[derive(Clone, Copy, PartialEq)]
enum Flag {
    Exact,
    Lower,
    Upper,
}

/// Fixed-size TT slot (audit: replace the dev-grade HashMap). `key == 0`
/// means empty; generation aging lets stale entries lose replacement fights
/// without a full clear, so the table survives across `search()` calls and
/// feeds move-to-move reuse under UCI.
#[derive(Clone, Copy)]
struct TtSlot {
    key: u64,
    depth: i32,
    score: i32,
    flag: Flag,
    mv: Option<Move>,
    generation: u8,
}

const EMPTY_SLOT: TtSlot =
    TtSlot { key: 0, depth: 0, score: 0, flag: Flag::Exact, mv: None, generation: 0 };
/// 2^21 slots ≈ 2M entries (~64 MB) — fixed, power of two for mask indexing.
const TT_BITS: u32 = 21;
const TT_SIZE: usize = 1 << TT_BITS;
const TT_MASK: u64 = (TT_SIZE as u64) - 1;

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
    tt: Vec<TtSlot>,
    tt_generation: u8,
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
}

impl Searcher {
    pub fn new(weights: ValueWeights, rung2: Option<Rung2Weights>) -> Searcher {
        Searcher {
            weights,
            rung2,
            tt: vec![EMPTY_SLOT; TT_SIZE],
            tt_generation: 0,
            tel: Telemetry::default(),
            deadline: None,
            aborted: false,
            opts: SearchOptions::default(),
            killers: vec![[None; 2]; MAX_KILLER_PLY],
            history: vec![0; 2 * 64 * 64],
        }
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
        let mut max_depth = opts.depth.max(1);
        // Danger-triggered root extension (gated): king danger buys 1–2 extra plies.
        let danger_plies = if opts.danger_extension { danger_level(pos) } else { 0 };
        max_depth += danger_plies;
        self.opts = opts;
        // Persistent TT: age the generation instead of clearing (audit fix).
        self.tt_generation = self.tt_generation.wrapping_add(1);
        // Killers/history reset per search call (kept across the iterative-
        // deepening iterations within it) — searches stay deterministic.
        self.killers.iter_mut().for_each(|k| *k = [None; 2]);
        self.history.iter_mut().for_each(|h| *h = 0);
        self.tel = Telemetry::default();
        self.tel.danger_extension_plies = danger_plies;
        self.aborted = false;
        let started = Instant::now();
        self.deadline = opts.max_time_ms.map(|ms| started + std::time::Duration::from_millis(ms));

        let mut result = SearchResult {
            best_move: None,
            score_cp: evaluate(pos, &self.weights, self.rung2.as_ref()),
            mate: None,
            pv: Vec::new(),
            depth: 0,
            telemetry: self.tel,
        };

        for depth in 1..=max_depth {
            let (score, best) = self.root(pos, depth as i32);
            if self.aborted {
                break;
            }
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
        }
        self.tel.elapsed_ms = started.elapsed().as_millis() as u64;
        result.telemetry = self.tel;
        result
    }

    /// Root: an explicit negamax move loop so the best move is tracked directly
    /// (the TS reference reads it back from the root TT entry — equivalent).
    fn root(&mut self, pos: &mut Position, depth: i32) -> (i32, Option<Move>) {
        let mut legal = generate_legal(pos);
        if legal.is_empty() {
            return (if in_check(pos) { -MATE_SCORE } else { 0 }, None);
        }
        let tt_move = if self.opts.use_tt { self.tt_probe(pos.hash).and_then(|e| e.mv) } else { None };
        self.order_moves(pos, &mut legal, tt_move, 0);

        let mut alpha = -INF;
        let beta = INF;
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for mv in legal {
            pos.make(mv);
            let score = -self.negamax(pos, depth - 1, -beta, -alpha, 1, true);
            pos.unmake();
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

    fn time_up(&mut self) -> bool {
        if let Some(deadline) = self.deadline {
            if (self.tel.nodes & 1023) == 0 && Instant::now() >= deadline {
                return true;
            }
        }
        false
    }

    /// Leaf eval = the TS `evaluate()` (stm POV, terminal-aware), given that the
    /// caller already knows whether legal moves exist (avoids a second movegen).
    fn leaf_eval(&self, pos: &Position, no_legal: bool, checked: bool) -> i32 {
        if no_legal {
            return if checked { -MATE_SCORE } else { 0 };
        }
        if pos.halfmove >= 100 || insufficient_material(pos) {
            return 0;
        }
        let white = js_round(evaluate_white_float_nonterminal(pos, &self.weights, self.rung2.as_ref()));
        if pos.stm == Color::White {
            white
        } else {
            -white
        }
    }

    fn negamax(&mut self, pos: &mut Position, depth: i32, alpha_in: i32, beta_in: i32, ply: u32, allow_null: bool) -> i32 {
        if self.time_up() {
            self.aborted = true;
            return evaluate(pos, &self.weights, self.rung2.as_ref());
        }
        self.tel.nodes += 1;

        let mut legal = generate_legal(pos);
        let checked = in_check(pos);
        if legal.is_empty() {
            return if checked { -MATE_SCORE + ply as i32 } else { 0 };
        }
        if pos.halfmove >= 100 || insufficient_material(pos) {
            return 0;
        }
        // Draw by repetition: one prior occurrence in the path (or the game
        // history the position was built with) scores 0. Checked BEFORE the
        // TT probe and returned WITHOUT storing — repetition scores are
        // path-dependent and must not leak into other lines via the table.
        if ply > 0 && pos.is_repetition() {
            return 0;
        }
        if depth <= 0 {
            return self.quiesce_with(pos, legal, checked, alpha_in, beta_in, ply, 0);
        }

        let mut alpha = alpha_in;
        let mut beta = beta_in;
        let mut tt_move: Option<Move> = None;
        if self.opts.use_tt {
            if let Some(e) = self.tt_probe(pos.hash) {
                tt_move = e.mv;
                if e.depth >= depth {
                    self.tel.tt_hits += 1;
                    match e.flag {
                        Flag::Exact => return e.score,
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
                        return e.score;
                    }
                }
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
            && evaluate(pos, &self.weights, self.rung2.as_ref()) >= beta
        {
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

        self.order_moves(pos, &mut legal, tt_move, ply);
        let key = pos.hash;
        let side = pos.stm.index();
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for mv in legal {
            pos.make(mv);
            let score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1, true);
            pos.unmake();
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
            let flag = if best > alpha_in { Flag::Exact } else { Flag::Upper };
            self.store(key, depth, best, flag, best_move);
        }
        best
    }

    #[allow(clippy::too_many_arguments)]
    fn quiesce_with(
        &mut self,
        pos: &mut Position,
        legal: Vec<Move>,
        checked: bool,
        alpha_in: i32,
        beta: i32,
        ply: u32,
        q_depth: u32,
    ) -> i32 {
        if self.time_up() {
            self.aborted = true;
            return self.leaf_eval(pos, legal.is_empty(), checked);
        }
        self.tel.nodes += 1;
        self.tel.q_nodes += 1;
        if q_depth > self.tel.max_q_depth {
            self.tel.max_q_depth = q_depth;
        }

        let mut alpha = alpha_in;
        if !checked {
            let stand = self.leaf_eval(pos, legal.is_empty(), checked);
            if stand >= beta {
                return beta; // fail-hard stand-pat, like the TS reference
            }
            if stand > alpha {
                alpha = stand;
            }
            if ply >= MAX_QUIESCENCE_PLY {
                return stand;
            }
        }
        if legal.is_empty() {
            return if checked { -MATE_SCORE + ply as i32 } else { 0 };
        }

        let mut moves: Vec<Move>;
        if checked {
            moves = legal; // search all evasions
        } else {
            // Winning/equal captures and promotions...
            moves = legal
                .iter()
                .copied()
                .filter(|m| m.flag.is_capture() || m.flag.promo_piece().is_some())
                .filter(|m| see(pos, m.from, m.to) >= 0)
                .collect();
            // ...plus a capped set of forcing QUIET checks near the top of
            // quiescence, so quiet refutations are seen — not only captures.
            if self.opts.quiet_checks && q_depth < QUIET_CHECK_MAX_PLY {
                let candidates: Vec<Move> = legal
                    .iter()
                    .copied()
                    .filter(|m| !m.flag.is_capture() && m.flag.promo_piece().is_none())
                    .filter(|m| see(pos, m.from, m.to) >= 0)
                    .collect();
                let mut quiet_checks: Vec<Move> =
                    candidates.into_iter().filter(|m| gives_check(pos, *m)).collect();
                quiet_checks.sort_by_key(|m| -self.capture_order(pos, *m));
                quiet_checks.truncate(MAX_QUIET_CHECKS_PER_NODE);
                if !quiet_checks.is_empty() {
                    self.tel.quiet_check_extensions += quiet_checks.len() as u64;
                }
                moves.extend(quiet_checks);
            }
        }
        moves.sort_by_key(|m| -self.capture_order(pos, *m));

        let mut best = if checked { -INF } else { alpha };
        for mv in moves {
            if mv.flag.is_capture() {
                self.tel.q_capture_nodes += 1;
            }
            pos.make(mv);
            let next_legal = generate_legal(pos);
            let next_checked = in_check(pos);
            let score = -self.quiesce_with(pos, next_legal, next_checked, -beta, -alpha, ply + 1, q_depth + 1);
            pos.unmake();
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
            pos.piece_at(mv.to).map(|(_, p)| SEE_VALUE[p.index()]).unwrap_or(0)
        } else {
            0
        };
        victim_value * 16 - SEE_VALUE[attacker.index()]
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
                return 700_000;
            }
            if killers[1] == Some(*m) {
                return 699_999;
            }
            self.history[Self::history_idx(side, *m)]
        };
        moves.sort_by_key(|m| -score_of(m));
    }

    #[inline]
    fn tt_probe(&self, key: u64) -> Option<TtSlot> {
        let slot = &self.tt[(key & TT_MASK) as usize];
        if slot.key == key { Some(*slot) } else { None }
    }

    fn store(&mut self, key: u64, depth: i32, score: i32, flag: Flag, mv: Option<Move>) {
        // Replacement: same position keeps the deeper entry; entries from an
        // older generation always lose; otherwise depth decides.
        let idx = (key & TT_MASK) as usize;
        let slot = &self.tt[idx];
        let replace = slot.key == 0
            || slot.key == key && depth >= slot.depth
            || slot.key != key
                && (slot.generation != self.tt_generation || depth >= slot.depth);
        if !replace {
            return;
        }
        self.tt[idx] =
            TtSlot { key, depth, score, flag, mv, generation: self.tt_generation };
    }

    /// Walk the TT from the root following stored moves (the TS PV extraction).
    /// Falls back to just the root best move when the TT is off.
    fn extract_pv(&self, pos: &mut Position, root_best: Option<Move>, max_len: usize) -> Vec<Move> {
        let mut pv = Vec::new();
        if !self.opts.use_tt {
            if let Some(m) = root_best {
                pv.push(m);
            }
            return pv;
        }
        let mut seen = std::collections::HashSet::new();
        let mut undo = 0usize;
        for _ in 0..max_len {
            if !seen.insert(pos.hash) {
                break;
            }
            let Some(entry) = self.tt_probe(pos.hash) else { break };
            let Some(mv) = entry.mv else { break };
            // Only follow strictly legal continuations.
            if !generate_legal(pos).contains(&mv) {
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
