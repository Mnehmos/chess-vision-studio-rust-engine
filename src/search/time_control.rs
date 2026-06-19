use super::*;

impl Searcher {
    pub(super) fn time_up(&mut self) -> bool {
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
}
