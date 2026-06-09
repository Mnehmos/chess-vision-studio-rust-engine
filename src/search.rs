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
use crate::eval::{
    evaluate, evaluate_white_float_nonterminal, insufficient_material, js_round, Rung2Weights,
    ValueWeights,
};
use crate::movegen::{generate_legal, gives_check, in_check};
use crate::see::{see, SEE_VALUE};
use crate::{Color, Move, MoveFlag, Piece, Position};
use std::collections::HashMap;
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
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions { depth: 4, max_time_ms: None, quiet_checks: true, use_tt: true }
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

#[derive(Clone, Copy)]
struct TtEntry {
    depth: i32,
    score: i32,
    flag: Flag,
    mv: Option<Move>,
}

pub struct Searcher {
    weights: ValueWeights,
    rung2: Option<Rung2Weights>,
    tt: HashMap<u64, TtEntry>,
    tel: Telemetry,
    deadline: Option<Instant>,
    aborted: bool,
    opts: SearchOptions,
}

impl Searcher {
    pub fn new(weights: ValueWeights, rung2: Option<Rung2Weights>) -> Searcher {
        Searcher {
            weights,
            rung2,
            tt: HashMap::new(),
            tel: Telemetry::default(),
            deadline: None,
            aborted: false,
            opts: SearchOptions::default(),
        }
    }

    pub fn search(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
        let max_depth = opts.depth.max(1);
        self.opts = opts;
        self.tt.clear();
        self.tel = Telemetry::default();
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
        let tt_move = if self.opts.use_tt { self.tt.get(&pos.hash).and_then(|e| e.mv) } else { None };
        self.order_moves(pos, &mut legal, tt_move);

        let mut alpha = -INF;
        let beta = INF;
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for mv in legal {
            pos.make(mv);
            let score = -self.negamax(pos, depth - 1, -beta, -alpha, 1);
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

    fn negamax(&mut self, pos: &mut Position, depth: i32, alpha_in: i32, beta_in: i32, ply: u32) -> i32 {
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
        if depth <= 0 {
            return self.quiesce_with(pos, legal, checked, alpha_in, beta_in, ply, 0);
        }

        let mut alpha = alpha_in;
        let mut beta = beta_in;
        let mut tt_move: Option<Move> = None;
        if self.opts.use_tt {
            if let Some(e) = self.tt.get(&pos.hash).copied() {
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

        self.order_moves(pos, &mut legal, tt_move);
        let key = pos.hash;
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for mv in legal {
            pos.make(mv);
            let score = -self.negamax(pos, depth - 1, -beta, -alpha, ply + 1);
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

    /// Main-search ordering: TT move ≫ captures (MVV-LVA) ≫ promotions ≫ quiets.
    /// (The TS reference also bonuses checking moves via SAN; here that signal is
    /// ordering-only and intentionally omitted — values are unaffected.)
    fn order_moves(&self, pos: &Position, moves: &mut [Move], tt_move: Option<Move>) {
        let score_of = |m: &Move| -> i32 {
            if Some(*m) == tt_move {
                return 1_000_000_000;
            }
            let mut s = 0;
            if m.flag.is_capture() {
                s += 100_000 + self.capture_order(pos, *m);
            }
            if m.flag.promo_piece().is_some() {
                s += 90_000;
            }
            s
        };
        moves.sort_by_key(|m| -score_of(m));
    }

    fn store(&mut self, key: u64, depth: i32, score: i32, flag: Flag, mv: Option<Move>) {
        // Deeper-entry-wins replacement, like the TS reference.
        if let Some(existing) = self.tt.get(&key) {
            if existing.depth > depth {
                return;
            }
        }
        self.tt.insert(key, TtEntry { depth, score, flag, mv });
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
            let Some(entry) = self.tt.get(&pos.hash) else { break };
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
