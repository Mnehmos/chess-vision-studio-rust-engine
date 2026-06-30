use super::*;

impl Searcher {
    pub(super) fn time_up(&mut self) -> bool {
        // Fixed-node diagnostic budget: exact and deterministic. Unlike the time/stop
        // polls below it is NOT gated by the 1023-node mask (the comparison is cheap and
        // syscall-free), so the same position + budget always stops at the same node with an
        // identical node count and search result (wall-clock timing fields still vary).
        // time_up() runs at the entry of every negamax/qsearch node before its counter
        // increment, so the search consumes exactly max_nodes nodes.
        // Single-thread is enforced at search entry, so tel.nodes is the only counter.
        if let Some(max_nodes) = self.max_nodes {
            if self.tel.nodes >= max_nodes {
                self.abort_reason = Some(SearchTermination::NodeLimit);
                return true;
            }
        }
        if let Some(stop) = &self.stop {
            if (self.tel.nodes & 1023) == 0 && stop.load(Ordering::Relaxed) {
                self.abort_reason = Some(SearchTermination::ExternalStop);
                return true;
            }
        }
        if let Some(deadline) = self.deadline {
            if (self.tel.nodes & 1023) == 0 && Instant::now() >= deadline {
                self.abort_reason = Some(SearchTermination::HardTime);
                return true;
            }
        }
        false
    }
}
