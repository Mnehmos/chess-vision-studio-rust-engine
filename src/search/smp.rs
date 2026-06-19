use super::*;

impl Searcher {
    /// Lazy SMP: helpers run the same iterative deepening on clones of the
    /// position, sharing only the lock-free TT; their work surfaces as deeper
    /// TT entries and better move ordering for the main thread. The main
    /// thread's result is authoritative; helpers stop when it finishes.
    pub(super) fn search_smp(&mut self, pos: &mut Position, opts: SearchOptions) -> SearchResult {
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
                let root_scope = self.root_scope;
                let tb = self.tb.clone();
                let book = self.book.clone();
                handles.push(scope.spawn(move || {
                    let mut helper = Searcher::new(weights, rung2);
                    helper.root_scope = root_scope;
                    helper.tt = tt;
                    helper.nnue = nnue;
                    helper.stop = Some(stop);
                    helper.id_start = lead;
                    helper.tb = tb;
                    helper.book = book;
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
}
