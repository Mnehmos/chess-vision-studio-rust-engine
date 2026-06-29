use super::*;

impl Searcher {
    /// Quiescence (Search Patch 6: specialized generation). Evasion nodes
    /// still pay full legal gen (exact mate detection); all other q-nodes
    /// generate NOISY moves directly - captures, promotions, ep - the bulk of
    /// the q-tree. Quiet-check candidates only force full gen inside the small
    /// QUIET_CHECK_MAX_PLY window. Stalemate on no-noisy nodes uses the
    /// early-exit has_legal_move probe.
    pub(super) fn quiesce(
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

        // --qtt: q-nodes are 55-62% of the tree and were table-blind. Probe
        // before the (more expensive) noisy movegen + NNUE eval; depth-0
        // entries from sibling q-trees cut re-searches. Mate-scale scores are
        // excluded both ways (no ply normalization in the table); evasion
        // nodes above keep their exact-mate logic table-free.
        let qkey = self.tt_key(pos);
        if self.opts.qsearch_tt && self.opts.use_tt {
            self.tel.tt_probes += 1;
            if let Some(e) = self.tt_probe(qkey) {
                self.tel.tt_entries += 1;
                // Specialist Channel-A isolation: qsearch entries carry no move
                // hint, so a foreign-lane qTT entry has no safe cross-lane use.
                if e.lane == self.opts.lane.id() && e.score.abs() < MATE_THRESHOLD {
                    match e.flag {
                        Flag::Exact => {
                            self.tel.tt_cutoffs += 1;
                            return e.score;
                        }
                        Flag::Lower if e.score >= beta => {
                            self.tel.tt_cutoffs += 1;
                            return e.score;
                        }
                        Flag::Upper if e.score <= alpha_in => {
                            self.tel.tt_cutoffs += 1;
                            return e.score;
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut noisy = generate_legal_noisy_list(pos);
        let mut alpha = alpha_in;
        if noisy.is_empty() && !has_legal_move(pos) {
            return 0; // stalemate (rare path; early-exit probe keeps it cheap)
        }
        let stand = self.leaf_eval(pos, false, false);
        if stand >= beta {
            if self.opts.qsearch_tt && self.opts.use_tt && beta.abs() < MATE_THRESHOLD {
                self.store(qkey, 0, beta, Flag::Lower, None, ply as i32);
            }
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
        let score = self.quiesce_moves(pos, noisy, false, alpha, beta, ply, q_depth);
        if self.opts.qsearch_tt && self.opts.use_tt && !self.aborted && score.abs() < MATE_THRESHOLD
        {
            let flag = if score >= beta {
                Flag::Lower
            } else if score <= alpha_in {
                Flag::Upper
            } else {
                Flag::Exact
            };
            self.store(qkey, 0, score, flag, None, ply as i32);
        }
        score
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn quiesce_moves(
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
}
