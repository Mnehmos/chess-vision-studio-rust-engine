use super::*;

impl Searcher {
    /// Root: an explicit negamax move loop so the best move is tracked directly
    /// (the TS reference reads it back from the root TT entry — equivalent).
    pub(super) fn root(
        &mut self,
        pos: &mut Position,
        depth: i32,
        alpha0: i32,
        beta: i32,
    ) -> (i32, Option<Move>) {
        let mut legal = generate_legal_list(pos);
        if let RootScope::Only(forced) = self.root_scope {
            legal.retain(|mv| *mv == forced);
        }
        if legal.is_empty() {
            return (if in_check(pos) { -MATE_SCORE } else { 0 }, None);
        }
        self.tel.main_nodes += 1;
        self.tel.ply_nodes[0] += 1;
        self.tel.legal_move_nodes += 1;
        self.tel.legal_move_sum += legal.len() as u64;
        let tt_move = if self.opts.use_tt {
            self.tel.tt_probes += 1;
            if let Some(e) = self.tt_probe(self.tt_key(pos)) {
                self.tel.tt_entries += 1;
                e.mv
            } else {
                None
            }
        } else {
            None
        };
        self.order_moves(pos, legal.as_mut_slice(), tt_move, 0);
        self.last_root_order = legal.as_slice().to_vec();
        self.root_progress = self.opts.root_diagnostics.then(|| PartialIteration {
            depth: depth.max(0) as u32,
            alpha: alpha0,
            beta,
            root_order: self.last_root_order.clone(),
            completed_candidates: Vec::with_capacity(legal.len()),
            provisional_best: None,
            provisional_score_cp: None,
        });

        let mut alpha = alpha0;
        let mut best = -INF;
        let mut best_move: Option<Move> = None;
        for move_index in 0..legal.len() {
            let mv = legal.get(move_index);
            let candidate_started = self.opts.root_diagnostics.then(Instant::now);
            self.tel.searched_moves += 1;
            self.tel.ply_child_searches[0] += 1;
            self.acc_make(pos, mv);
            pos.make(mv);
            self.prev_moves[1] = Some(mv);
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
            if let Some(progress) = self.root_progress.as_mut() {
                progress.completed_candidates.push(RootCandidateProgress {
                    mv,
                    score_cp: score,
                    time_ms: candidate_started
                        .map(|started| started.elapsed().as_millis() as u64)
                        .unwrap_or(0),
                });
            }
            if score > best {
                best = score;
                best_move = Some(mv);
                if let Some(progress) = self.root_progress.as_mut() {
                    progress.provisional_best = best_move;
                    progress.provisional_score_cp = Some(best);
                }
            }
            if best > alpha {
                alpha = best;
            }
        }
        if self.opts.use_tt {
            // Bound-correct flag: inside the aspiration loop this node is searched
            // with the narrow window (alpha0, beta). On a fail-low `best` is only
            // an upper bound and on a fail-high a lower bound — storing it as Exact
            // pollutes the table and lets a bound satisfy an Exact cutoff on the
            // next iteration's re-search.
            let flag = if best <= alpha0 {
                Flag::Upper
            } else if best >= beta {
                Flag::Lower
            } else {
                Flag::Exact
            };
            self.store(self.tt_key(pos), depth, best, flag, best_move, 0);
        }
        self.root_progress = None;
        (best, best_move)
    }

    pub(super) fn negamax(
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
            let probe_key = self.tt_key(pos);
            if let Some(mut e) = self.tt_probe(probe_key) {
                self.tel.tt_entries += 1;
                // BUG1 mate-TT: stored mate scores are node-intrinsic; convert back to
                // root-relative at this ply before any cutoff/bound use below.
                if self.opts.matett {
                    e.score = super::mate_probe_adjust(e.score, ply as i32);
                }
                tt_move = e.mv;
                tt_move_lane = e.lane;
                // Transfer telemetry: this node consumed a move hint written by
                // a foreign specialist lane.
                if e.mv.is_some() && e.lane != self.opts.lane.id() {
                    self.tel.foreign_tt_hints[(e.lane & 3) as usize] += 1;
                }
                let same_lane = e.lane == self.opts.lane.id();
                // rule50-v2: near the 50-move boundary every stored score is
                // suspect (graph-history interaction) — keep the move hint,
                // refuse the cutoff. Mirrors SF's rule50_count() >= 96 guard.
                let r50_block = self.opts.rule50_scale && pos.halfmove >= 96;
                // Channel-A specialist isolation: foreign lanes can seed move
                // ordering through `tt_move`, but their eval-profiled scores and
                // bounds must not prune a different lane's tree.
                if same_lane && e.depth >= depth && !r50_block {
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
            } else if self.tt.slot_occupied(probe_key) {
                self.tel.tt_miss_contended += 1;
            } else {
                self.tel.tt_miss_cold += 1;
            }
        }

        // 1. Syzygy Tablebase WDL Probe
        if self.opts.syzygy {
            if let Some(tb) = &self.tb {
                if pos.castling == 0 && pos.all.count_ones() as u32 <= tb.max_pieces() {
                    if let Some(wdl) = tb.probe_wdl(pos) {
                        let score = match wdl {
                            pyrrhic_rs::WdlProbeResult::Win => 900_000 - ply as i32,
                            pyrrhic_rs::WdlProbeResult::Loss => -900_000 + ply as i32,
                            _ => 0,
                        };
                        let flag = match wdl {
                            pyrrhic_rs::WdlProbeResult::Win => Flag::Lower,
                            pyrrhic_rs::WdlProbeResult::Loss => Flag::Upper,
                            _ => Flag::Exact,
                        };
                        if self.opts.use_tt {
                            self.store(self.tt_key(pos), depth, score, flag, None, ply as i32);
                        }
                        return score;
                    }
                }
            }
        }

        // 2. Singular Extensions Probe
        let mut extension = 0;
        if self.opts.singular
            && self.excluded_move.is_none()
            && depth >= 8
            && tt_move.is_some()
            && !(checked && rule_draw)
        {
            if let Some(e) = self.tt_probe(self.tt_key(pos)) {
                if e.lane == self.opts.lane.id()
                    && e.depth >= depth - 3
                    && e.flag != Flag::Upper
                    && e.score.abs() < MATE_THRESHOLD
                {
                    let rdepth = (depth - 1) / 2;
                    let margin = 2 * depth as i32;
                    let s_beta = e.score - margin;

                    self.excluded_move = tt_move;
                    let s_score = self.negamax(pos, rdepth, s_beta - 1, s_beta, ply, false);
                    self.excluded_move = None;

                    if s_score < s_beta {
                        extension = 1;
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

        // --improving: record this node's static eval and compare to the same
        // side's eval two plies back. Gated so flag-off computes nothing extra.
        let improving = if self.opts.improving {
            let p = ply as usize;
            let se = if checked { i32::MIN } else { static_eval!() };
            if p < self.eval_stack.len() {
                self.eval_stack[p] = se;
            }
            // Unknown (in-check now, or no ply-2 baseline) defaults to true —
            // the conservative choice that does NOT prune harder.
            if checked || p < 2 || self.eval_stack[p - 2] == i32::MIN {
                true
            } else {
                se > self.eval_stack[p - 2]
            }
        } else {
            true
        };

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
                // --tt-prune-store: an RFP cut is a position-only fail-high
                // (static eval, no path dependence — unlike null) so it is a
                // legitimate Lower(beta) entry. Without it the ~60% of shallow
                // nodes RFP cuts are re-probed empty every ID iteration
                // (measured 19% any-entry rate at d8).
                if self.opts.tt_prune_store && self.opts.use_tt {
                    self.store(self.tt_key(pos), depth, beta, Flag::Lower, None, ply as i32);
                }
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
            if let Some(slot) = self.prev_moves.get_mut((ply + 1) as usize) {
                *slot = None; // null subtrees must not key counters on a stale move
            }
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

        // Internal Iterative Deepening (move-ordering patch). The strength audit
        // pins the search ceiling on ordering: ~82% of TT probes are cold, so a
        // node often enters the move loop with no principal move and the
        // first-move cutoff rate sits near 35% (strong engines ~90%+). When this
        // node has no hint and is deep enough to be worth it, search it once at a
        // reduced depth purely to populate the TT, then re-probe to seed
        // `tt_move`. The node's own (alpha,beta) is reused, so non-PV (zero-
        // window) nodes get a cheap IID; the depth reduction bounds the nested
        // recursion. Gated behind --iid for a clean A/B.
        const IID_MIN_DEPTH: i32 = 7;
        const IID_DEPTH_REDUCTION: i32 = 2;
        if self.opts.iid
            && tt_move.is_none()
            && depth >= IID_MIN_DEPTH
            && self.excluded_move.is_none()
        {
            self.tel.iid_searches += 1;
            self.negamax(
                pos,
                depth - IID_DEPTH_REDUCTION,
                alpha,
                beta,
                ply,
                allow_null,
            );
            if self.aborted {
                return self.static_eval(pos);
            }
            if let Some(e) = self.tt_probe(self.tt_key(pos)) {
                if e.mv.is_some() {
                    self.tel.iid_found += 1;
                    tt_move = e.mv;
                    tt_move_lane = e.lane;
                }
            }
        }

        self.order_moves(pos, legal.as_mut_slice(), tt_move, ply);
        let key = self.tt_key(pos);
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
        // v2 budget (v1 = 4+d² screened RED 37.5% while skipping 83% of
        // attempted quiets — standard budgets assume 85-95% first-move
        // cutoffs; ours is ~35%, so the tail still matters): doubled.
        let lmp_budget = (8 + 2 * depth * depth) as usize;
        // --histmalus: quiets actually searched at this node, so a quiet
        // cutoff can penalize everything ordered (wrongly) ahead of it.
        let mut tried_quiets = MoveList::new();
        let mut tried_caps = MoveList::new();
        for move_index in 0..legal.len() {
            let mv = legal.get(move_index);
            if Some(mv) == self.excluded_move {
                continue;
            }
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
                    if futile {
                        self.tel.futility_attempts += 1;
                        if !gives_check(pos, mv) {
                            self.tel.futility_skips += 1;
                            self.tel.pruned_moves += 1;
                            continue;
                        }
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
            // --histlmr (research 2026-06-12): the history table is consumed,
            // not just sorted on — strong-history quiets escape the reduction,
            // proven-bad ones (negative entries exist only under --histmalus)
            // are reduced an extra ply. SF: r -= statScore*445/4096.
            let mut lmr_r: i32 = if reduce {
                if self.opts.loglmr {
                    super::log_lmr_reduction(depth, move_index as usize).max(1)
                } else {
                    1
                }
            } else {
                0
            };
            if reduce && self.opts.hist_lmr {
                let h = self.history[Self::history_idx(side, mv)];
                if h > 2048 {
                    lmr_r = 0;
                } else if h < -2048 {
                    lmr_r = 2;
                }
            }
            // --improving: a non-improving node is going the wrong way, so its
            // late quiets are even less likely to matter — reduce one more ply.
            if reduce && self.opts.improving && !improving && lmr_r > 0 {
                lmr_r += 1;
            }
            self.acc_make(pos, mv);
            self.tel.searched_moves += 1;
            self.tel.ply_child_searches[ply_bucket] += 1;
            pos.make(mv);
            if let Some(slot) = self.prev_moves.get_mut((ply + 1) as usize) {
                *slot = Some(mv);
            }
            let ext = if Some(mv) == tt_move { extension } else { 0 };
            let mut score;
            if lmr_r > 0 {
                self.tel.lmr_reductions += 1;
                score = -self.negamax(
                    pos,
                    depth - 1 + ext - lmr_r,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                );
                if score > alpha && !self.aborted {
                    self.tel.lmr_researches += 1;
                    score = -self.negamax(pos, depth - 1 + ext, -beta, -alpha, ply + 1, true);
                }
            } else if self.opts.pvs && move_index > 0 && best > alpha_in {
                // PVS (Patch 5): once a PV move raised alpha, probe the rest
                // with a null window and re-search only on a raise.
                score = -self.negamax(pos, depth - 1 + ext, -alpha - 1, -alpha, ply + 1, true);
                if score > alpha && score < beta && !self.aborted {
                    self.tel.pvs_researches += 1;
                    score = -self.negamax(pos, depth - 1 + ext, -beta, -alpha, ply + 1, true);
                }
            } else {
                score = -self.negamax(pos, depth - 1 + ext, -beta, -alpha, ply + 1, true);
            }
            pos.unmake();
            self.acc_unmake();
            if self.aborted {
                return if best > -INF { best } else { score };
            }
            if self.opts.hist_malus && !mv.flag.is_capture() && mv.flag.promo_piece().is_none() {
                tried_quiets.push(mv);
            }
            if self.opts.caphist && mv.flag.is_capture() {
                tried_caps.push(mv);
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
                    self.record_quiet_cutoff(pos, side, mv, depth, ply);
                    if self.opts.hist_malus {
                        let tried: Vec<Move> = (0..tried_quiets.len())
                            .map(|i| tried_quiets.get(i))
                            .collect();
                        self.punish_tried_quiets(side, &tried, mv, depth);
                    }
                } else if self.opts.caphist && mv.flag.is_capture() {
                    // Capture cutoff: reward the cutter, penalize captures tried
                    // before it (ordered too high). pos is at the cutoff node.
                    let bonus = (150 * depth).min(1500);
                    self.caphist_update(pos, side, mv, bonus);
                    let malus = (150 * depth).min(1500);
                    for i in 0..tried_caps.len() {
                        let c = tried_caps.get(i);
                        if c != mv {
                            self.caphist_update(pos, side, c, -malus);
                        }
                    }
                }
                if self.opts.use_tt {
                    self.store(key, depth, best, Flag::Lower, best_move, ply as i32);
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
            self.store(key, depth, best, flag, best_move, ply as i32);
        }
        best
    }
}
