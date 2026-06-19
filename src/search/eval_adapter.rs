use super::*;

impl Searcher {
    /// Push the child accumulator for `mv` (call with the PRE-make position).
    /// Slot-reuse: the stack never shrinks, so steady-state pushes are two
    /// memcpys + the feature deltas — no per-node allocation.
    #[inline]
    pub(super) fn acc_make(&mut self, pos: &Position, mv: Move) {
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
    pub(super) fn acc_unmake(&mut self) {
        if self.acc_top != usize::MAX && self.acc_top > 0 {
            self.acc_top -= 1;
        }
    }

    /// Leaf eval = the TS `evaluate()` (stm POV, terminal-aware), given that the
    /// caller already knows whether legal moves exist (avoids a second movegen).
    /// Side-to-move static eval — NNUE when loaded, classical otherwise.
    /// Rule-50 conversion pressure (--rule50): scale the eval toward 0 as the
    /// halfmove clock climbs, so a side that is ahead watches its score bleed
    /// while it shuffles — lines that reset the counter (pawn pushes, trades)
    /// outscore repetition at the same material. Targets the measured failed
    /// conversions (7/11 SPRT draws where we peaked +1.2..+4.4 then drew).
    /// v1 REJECTED (2026-06-11): hm is path state outside the Zobrist key, so
    /// scaled scores contaminated the TT across hm contexts. v2 (research
    /// 2026-06-12) pairs the damp with SF's adjust_key50 bucketing + the
    /// hm≥96 cutoff guard — the full anti-contamination kit.
    #[inline]
    pub(super) fn rule50_scale(&self, pos: &Position, v: i32) -> i32 {
        if self.opts.rule50_scale {
            v * (256 - pos.halfmove.min(100) as i32) / 256
        } else {
            v
        }
    }

    /// TT key with SF's adjust_key50 (rule50-v2): a damped score is only valid
    /// near the halfmove count it was computed at, so re-bucket the key every
    /// 8 halfmoves past 14. Damped scores can then never satisfy probes at
    /// distant counters — the exact failure that rejected v1.
    #[inline]
    pub(super) fn tt_key(&self, pos: &Position) -> u64 {
        if self.opts.rule50_scale && pos.halfmove >= 14 {
            pos.hash ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul((pos.halfmove as u64 - 14) / 8 + 1)
        } else {
            pos.hash
        }
    }

    /// King-activity conversion pressure (--king-activity): when one side is
    /// clearly ahead in a simplified position, reward marching the winning
    /// king toward the enemy king and toward the center. Position-only (TT-
    /// safe, unlike rule50): creates the progress gradient that converts won
    /// endings instead of shuffling on the back rank. Magnitude ≤ ~50cp.
    #[inline]
    pub(super) fn king_activity(&self, pos: &Position, v: i32) -> i32 {
        if !self.opts.king_activity {
            return v;
        }
        // Simplified = no queens-and-rooks armies left: total non-pawn,
        // non-king material under ~rook+minor per side on average.
        let mut nonpawn = 0i32;
        for c in 0..2 {
            for p in 1..5 {
                // N,B,R,Q
                nonpawn += pos.pieces[c][p].count_ones() as i32 * SEE_VALUE[p];
            }
        }
        if nonpawn > 1700 || v.abs() < 150 || v.abs() >= MATE_THRESHOLD {
            return v;
        }
        // From stm POV: the side that is AHEAD gets the activity term.
        let (winner, sign) = if v > 0 {
            (pos.stm, 1)
        } else {
            (pos.stm.flip(), -1)
        };
        let wk = pos.king_sq(winner) as i32;
        let lk = pos.king_sq(winner.flip()) as i32;
        let (wf, wr, lf, lr) = (wk % 8, wk / 8, lk % 8, lk / 8);
        let cheb = ((wf - lf).abs()).max((wr - lr).abs());
        // Centralization metric: (2f−7)+(2r−7) is 2 (center) .. 14 (corner).
        let lcentral = (2 * lf - 7).abs() + (2 * lr - 7).abs();
        // v2 mop-up (v1 NEUTRAL at 100g — still drew from +11.6): the dominant
        // term drives the LOSING king to the edge — the thing that separates
        // progress moves in won-but-shuffling endings — plus king proximity.
        let bonus = 8 * (lcentral - 2) + 6 * (6 - cheb);
        v + sign * bonus
    }

    pub(super) fn static_eval(&self, pos: &mut Position) -> i32 {
        if let Some(n) = &self.nnue {
            if self.acc_top != usize::MAX {
                return self.king_activity(
                    pos,
                    self.rule50_scale(pos, n.eval_acc(pos, &self.acc_stack[self.acc_top], pos.stm)),
                );
            }
            return self.king_activity(pos, self.rule50_scale(pos, n.eval_stm(pos)));
        }
        let v = evaluate(pos, &self.weights, self.rung2.as_ref());
        self.king_activity(pos, self.rule50_scale(pos, v))
    }

    pub(super) fn leaf_eval(&mut self, pos: &Position, no_legal: bool, checked: bool) -> i32 {
        if self.opts.cvs_trace {
            crate::eval::cvs_features::extract_cvs_ids_into(pos, &mut self.cvs_buf);
            self.tel.cvs_trace_features += self.cvs_buf.len() as u64;
        } else if self.opts.cvs_core_trace {
            crate::eval::cvs_features::extract_cvs_core_ids_into(pos, &mut self.cvs_buf);
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
                return self.king_activity(
                    pos,
                    self.rule50_scale(pos, n.eval_acc(pos, &self.acc_stack[self.acc_top], pos.stm)),
                );
            }
            return self.king_activity(pos, self.rule50_scale(pos, n.eval_stm(pos)));
        }
        let white = js_round(evaluate_white_float_nonterminal(
            pos,
            &self.weights,
            self.rung2.as_ref(),
        ));
        self.king_activity(
            pos,
            self.rule50_scale(
                pos,
                if pos.stm == Color::White {
                    white
                } else {
                    -white
                },
            ),
        )
    }
}
