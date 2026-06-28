use super::*;

impl Searcher {
    #[inline]
    pub(super) fn history_idx(side: usize, mv: Move) -> usize {
        side * 4096 + mv.from as usize * 64 + mv.to as usize
    }

    #[inline]
    pub(super) fn counter_idx(side: usize, piece: Piece, to: u8) -> usize {
        (side * 6 + piece as usize) * 64 + to as usize
    }

    #[inline]
    pub(super) fn conthist_idx(prev_piece: Piece, prev_to: u8, piece: Piece, to: u8) -> usize {
        ((prev_piece as usize * 64 + prev_to as usize) * 6 + piece as usize) * 64 + to as usize
    }

    #[inline]
    pub(super) fn caphist_idx(side: usize, piece: Piece, to: u8, victim: Piece) -> usize {
        ((side * 6 + piece as usize) * 64 + to as usize) * 6 + victim as usize
    }

    /// Capture-history gravity update for one (piece,to,victim) at `pos`.
    pub(super) fn caphist_update(&mut self, pos: &Position, side: usize, mv: Move, delta: i32) {
        let Some((_, piece)) = pos.piece_at(mv.from) else {
            return;
        };
        let victim = if mv.flag == MoveFlag::EnPassant {
            Piece::Pawn
        } else {
            match pos.piece_at(mv.to) {
                Some((_, v)) => v,
                None => return,
            }
        };
        let i = Self::caphist_idx(side, piece, mv.to, victim);
        const D: i32 = 8192;
        self.caphist[i] += delta - self.caphist[i] * delta.abs() / D;
    }

    /// Gravity-form history update: entry += delta − entry·|delta|/D, which
    /// self-limits to ±D and lets recent evidence dominate stale counts.
    #[inline]
    pub(super) fn hist_gravity(&mut self, side: usize, mv: Move, delta: i32) {
        const D: i32 = 8192;
        let e = &mut self.history[Self::history_idx(side, mv)];
        *e += delta - *e * delta.abs() / D;
    }

    /// History maluses (--histmalus): every quiet tried BEFORE the cutoff was
    /// ordered too high — penalize it. Without this, bonus-only updates drift
    /// every entry positive until quiet ordering is saturation noise (the
    /// measured 35% first-move-cutoff plateau).
    pub(super) fn punish_tried_quiets(
        &mut self,
        side: usize,
        tried: &[Move],
        cutter: Move,
        depth: i32,
    ) {
        let malus = (300 * depth).min(2200);
        for &q in tried {
            if q != cutter {
                self.hist_gravity(side, q, -malus);
            }
        }
    }

    /// Record a QUIET move that produced a beta cutoff (the only teacher).
    /// `pos` is at the cutoff node (mv unmade): mv.from holds the mover,
    /// prev.to holds the opponent piece whose move this quiet refuted.
    pub(super) fn record_quiet_cutoff(
        &mut self,
        pos: &Position,
        side: usize,
        mv: Move,
        depth: i32,
        ply: u32,
    ) {
        let p = ply as usize;
        if p < MAX_KILLER_PLY && self.killers[p][0] != Some(mv) {
            self.killers[p][1] = self.killers[p][0];
            self.killers[p][0] = Some(mv);
        }
        if self.opts.countermove {
            // The opponent's previous (piece, to) is the key; the piece that
            // just landed on prev.to is still there at this node.
            if let Some(prev) = self.prev_moves.get(p).copied().flatten() {
                if let Some((_, pp)) = pos.piece_at(prev.to) {
                    self.counters[Self::counter_idx(side ^ 1, pp, prev.to)] = Some(mv);
                }
            }
        }
        if self.opts.conthist {
            if let Some(prev) = self.prev_moves.get(p).copied().flatten() {
                if let (Some((_, pp)), Some((_, cp))) =
                    (pos.piece_at(prev.to), pos.piece_at(mv.from))
                {
                    let ci = Self::conthist_idx(pp, prev.to, cp, mv.to);
                    // Gravity-bounded to ±8192 — the SAME scale as the butterfly
                    // history it is summed with at ordering time. The old uncapped
                    // depth² update saturated near +16384 (2× history) and was
                    // never penalized, so it swamped the history signal (including
                    // the malus that drives the first-move-cutoff rate) and made
                    // --conthist a net regression.
                    const D: i32 = 8192;
                    let bonus = (150 * depth).min(1500);
                    let e = &mut self.conthist[ci];
                    *e += bonus - *e * bonus.abs() / D;
                }
            }
        }
        if self.opts.hist_malus {
            // Gravity update (research 2026-06-12, SF/Ethereal): capped LINEAR
            // bonus — depth² uncapped lets a few deep nodes saturate entries —
            // and the entry self-limits to ±HIST_GRAVITY_D.
            let bonus = (150 * depth).min(1500);
            self.hist_gravity(side, mv, bonus);
        } else {
            let idx = Self::history_idx(side, mv);
            self.history[idx] += depth * depth;
            if self.history[idx] >= HISTORY_CAP {
                for h in self.history.iter_mut() {
                    *h /= 2;
                }
            }
        }
    }

    /// Strong zugzwang guard for null-move pruning: the side to move needs a
    /// major piece (rook/queen) or at least two minor pieces. Knight-only and
    /// single-minor endings are classic null-move blind spots.
    pub(super) fn null_material_ok(pos: &Position) -> bool {
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
    pub(super) fn capture_order(&self, pos: &Position, mv: Move) -> i32 {
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
        let mut score = victim_value * 16 - SEE_VALUE[attacker.index()];
        if self.opts.caphist && mv.flag.is_capture() {
            let victim = if mv.flag == MoveFlag::EnPassant {
                Piece::Pawn
            } else {
                pos.piece_at(mv.to).map(|(_, v)| v).unwrap_or(Piece::Pawn)
            };
            // caphist (±8192)/16 ≈ ±512 — reorders within an equal-victim band
            // without crossing the winning/losing-capture tiers.
            score += self.caphist[Self::caphist_idx(pos.stm.index(), attacker, mv.to, victim)] / 16;
        }
        score
    }

    pub(super) fn sort_by_key_desc<F>(moves: &mut [Move], mut key_of: F)
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
    pub(super) fn order_moves(
        &mut self,
        pos: &Position,
        moves: &mut [Move],
        tt_move: Option<Move>,
        ply: u32,
    ) {
        if ply == 0 {
            if let Some(helper) = &self.helper_nnue {
                let model_hash = helper.model_hash;
                let registry_hash = helper.registry_hash();
                let zobrist = pos.hash;

                if helper.is_ranker {
                    let cache_hit = if let (Some(_), Some(cache_zobrist)) =
                        (&self.root_attention_cache, self.root_attention_zobrist)
                    {
                        cache_zobrist == zobrist
                    } else {
                        false
                    };
                    if !cache_hit {
                        if let Some(raw_nnue) = &self.nnue {
                            let mut cache =
                                self.prepare_root_attention(pos, moves, raw_nnue, helper);
                            if self.opts.shuffled_geometry {
                                let mut seed = zobrist;
                                let n = cache.len();
                                for i in (1..n).rev() {
                                    seed = seed
                                        .wrapping_mul(6364136223846793005)
                                        .wrapping_add(1442695040888963407);
                                    let j = (seed % (i as u64 + 1)) as usize;
                                    let temp = cache[i].ordering_bonus;
                                    cache[i].ordering_bonus = cache[j].ordering_bonus;
                                    cache[j].ordering_bonus = temp;
                                }
                            }
                            self.root_attention_cache = Some(cache);
                            self.root_attention_zobrist = Some(zobrist);
                        }
                    }
                } else {
                    let cache_hit = if let Some(cache) = &self.root_geom_cache {
                        cache.zobrist == zobrist
                            && cache.model_hash == model_hash
                            && cache.registry_hash == registry_hash
                    } else {
                        false
                    };

                    if !cache_hit {
                        let mut move_scores = Vec::with_capacity(moves.len());
                        for &mv in moves.iter() {
                            let mut child = pos.clone();
                            child.make(mv);
                            let score = -helper.eval_stm(&child);
                            move_scores.push((mv, score));
                        }
                        if self.opts.shuffled_geometry {
                            let mut seed = zobrist;
                            let n = move_scores.len();
                            for i in (1..n).rev() {
                                seed = seed
                                    .wrapping_mul(6364136223846793005)
                                    .wrapping_add(1442695040888963407);
                                let j = (seed % (i as u64 + 1)) as usize;
                                let temp = move_scores[i].1;
                                move_scores[i].1 = move_scores[j].1;
                                move_scores[j].1 = temp;
                            }
                        }
                        self.root_geom_cache = Some(RootGeometryCacheEntry {
                            zobrist,
                            model_hash,
                            registry_hash,
                            move_scores,
                        });
                    }
                }
            }
        }

        let side = pos.stm.index();
        let killers = self.killers.get(ply as usize).copied().unwrap_or([None; 2]);
        // Countermove: the stored refutation of the opponent's previous move,
        // ordered just below the killers (killers carry sibling-position
        // evidence at this ply; the countermove carries move-pair evidence).
        let counter = if self.opts.countermove {
            self.prev_moves
                .get(ply as usize)
                .copied()
                .flatten()
                .and_then(|prev| {
                    pos.piece_at(prev.to)
                        .map(|(_, pp)| self.counters[Self::counter_idx(side ^ 1, pp, prev.to)])
                })
                .flatten()
        } else {
            None
        };
        // Continuation-history key: the opponent piece that just moved and its
        // square. Resolved once per node; each quiet adds its pair counter.
        let ch_key = if self.opts.conthist {
            self.prev_moves
                .get(ply as usize)
                .copied()
                .flatten()
                .and_then(|prev| pos.piece_at(prev.to).map(|(_, pp)| (pp, prev.to)))
        } else {
            None
        };
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
            let mut s = self.history[Self::history_idx(side, *m)] + self.lane_bonus(pos, *m, ply);
            if counter == Some(*m) {
                // Countermove as an additive bonus within the quiet band (≈ half
                // the ±8192 history bound) — reorders among quiets rather than
                // hard-promoting an often-stale countermove above genuine high-
                // history quiets (the old fixed 650k tier regressed).
                s += 4096;
            }
            if let Some((pp, pt)) = ch_key {
                if let Some((_, cp)) = pos.piece_at(m.from) {
                    s += self.conthist[Self::conthist_idx(pp, pt, cp, m.to)];
                }
            }
            if ply == 0 && self.opts.cvs_bonus {
                if let Some(helper) = &self.helper_nnue {
                    if helper.is_ranker {
                        if let Some(cache) = &self.root_attention_cache {
                            if let Some(att) = cache.iter().find(|att| att.mv == *m) {
                                s += att.ordering_bonus;
                            }
                        }
                    } else {
                        if let Some(cache) = &self.root_geom_cache {
                            if let Some(&(_, geom_score)) =
                                cache.move_scores.iter().find(|(mv, _)| mv == m)
                            {
                                let geom_bonus = (geom_score * 10).clamp(-4000, 4000);
                                s += geom_bonus;
                            }
                        }
                    }
                }
            }
            s
        };
        Self::sort_by_key_desc(moves, score_of);
    }

    /// Lane ordering bonus (Level-1 specialist lanes). Applied to quiet/killer
    /// tiers so it reorders WITHIN ordering without displacing the TT move or
    /// winning captures. Ordering-only -> cannot change the search value.
    pub(super) fn lane_bonus(&self, pos: &Position, m: Move, ply: u32) -> i32 {
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
    pub(super) fn king_tropism(&self, pos: &Position) -> i32 {
        self.king_tropism_of(pos, pos.stm)
    }

    pub(super) fn king_tropism_of(&self, pos: &Position, own: Color) -> i32 {
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
}
