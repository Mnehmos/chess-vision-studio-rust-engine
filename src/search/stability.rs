//! Stabilization report (#7): "How stable is this result under the completed search
//! evidence and verification budget?"
//!
//! Pure analysis over the iterative-deepening trajectory already in `SearchResult` — it
//! changes no search behavior (so it is provably baseline-preserving). We never assert
//! unqualified convergence: `StableAtBudget` is the strongest *trajectory-only* verdict.
//! `ExactTablebase` comes from a tablebase result; `VerifiedForcedMate` and the fully
//! verified `OmissionRisk` / `VerifierConflict` require the #3 sacrifice-verifier and are
//! produced there. This slice produces ExactTablebase / StableAtBudget / UnstableTrajectory /
//! UnresolvedAtBudget, plus a conservative OmissionRisk flag from negative-SEE qsearch skips.

use super::types::{SearchResult, SearchResultSource};
// Reuse the engine-wide mate threshold (MATE_SCORE - 1000) as the single source of truth,
// rather than redeclaring a divergent local value.
use super::MATE_THRESHOLD;

/// A score delta must be at least this many cp to count toward the odd-even parity pattern
/// (filters sub-cp accumulator wiggle).
const MIN_OSC_SWING_CP: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabilizationStatus {
    ExactTablebase,
    VerifiedForcedMate,
    StableAtBudget,
    UnstableTrajectory,
    OmissionRisk,
    VerifierConflict,
    UnresolvedAtBudget,
}

impl StabilizationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactTablebase => "exact-tablebase",
            Self::VerifiedForcedMate => "verified-forced-mate",
            Self::StableAtBudget => "stable-at-budget",
            Self::UnstableTrajectory => "unstable-trajectory",
            Self::OmissionRisk => "omission-risk",
            Self::VerifierConflict => "verifier-conflict",
            Self::UnresolvedAtBudget => "unresolved-at-budget",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StabilizationConfig {
    /// Trailing completed iterations to analyze.
    pub window: usize,
    /// A settled score's largest adjacent swing stays <= this (cp).
    pub stable_max_swing_cp: i32,
    /// A settled result's best move changes at most this many times across the window.
    /// Default 0 is deliberately strict: a single best-move flip in the trailing window marks
    /// the result UnstableTrajectory. This is a confidence/quarantine gate, so a false
    /// "unstable" is the safe direction; loosen via config where a softer gate is wanted.
    pub stable_max_changes: u32,
    /// Negative-SEE qsearch skips at/above this flag an omission risk on an otherwise-stable
    /// result (a sacrificial line the qsearch filter never searched). Coarse by design: it is
    /// dormant unless `see_prune` is enabled, and when enabled even quiet tactical searches can
    /// exceed a small count, so this is a *superset* flag whose verdict is deferred to the #3
    /// sacrifice-verifier (which actually searches the omitted line). Calibrate alongside #3.
    pub omission_skip_threshold: u64,
}

impl Default for StabilizationConfig {
    fn default() -> Self {
        Self {
            window: 4,
            stable_max_swing_cp: 30,
            stable_max_changes: 0,
            omission_skip_threshold: 64,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StabilizationReport {
    pub status: StabilizationStatus,
    pub window: usize,
    /// max - min score over the trailing window (robust range).
    pub score_range_cp: i32,
    /// largest |score[i] - score[i-1]| in the window.
    pub max_adjacent_swing_cp: i32,
    /// number of best-move changes across the window.
    pub best_move_changes: u32,
    /// adjacent score deltas alternate sign (depth-parity swing).
    pub odd_even_oscillation: bool,
    /// a mate <-> non-mate transition occurred in the window.
    pub mate_flip: bool,
    /// score sign flips (win/loss class changes) in the window.
    pub sign_flips: u32,
    /// negative-SEE qsearch skips (omission-risk input).
    pub see_prune_skips: u64,
    /// machine-readable reasons for the status.
    pub reasons: Vec<&'static str>,
}

/// Stabilization report with default config.
pub fn stabilization_report(result: &SearchResult) -> StabilizationReport {
    stabilization_report_with(result, &StabilizationConfig::default())
}

pub fn stabilization_report_with(
    result: &SearchResult,
    cfg: &StabilizationConfig,
) -> StabilizationReport {
    let mut reasons: Vec<&'static str> = Vec::new();
    let mut score_range_cp = 0;
    let mut max_adjacent_swing_cp = 0;
    let mut best_move_changes = 0u32;
    let mut odd_even_oscillation = false;
    let mut mate_flip = false;
    let mut sign_flips = 0u32;
    let see_prune_skips = result.telemetry.see_prune_skips;

    let iters = &result.iterations;
    let status = if result.result_source == SearchResultSource::Tablebase {
        reasons.push("tablebase-source");
        StabilizationStatus::ExactTablebase
    } else if result.result_source == SearchResultSource::NoCompletedIteration || iters.is_empty() {
        reasons.push("no-completed-iteration");
        StabilizationStatus::UnresolvedAtBudget
    } else {
        // Trailing window of completed iterations.
        let start = iters.len().saturating_sub(cfg.window.max(1));
        let w = &iters[start..];
        let scores: Vec<i32> = w.iter().map(|it| it.score_cp).collect();
        score_range_cp =
            scores.iter().copied().max().unwrap_or(0) - scores.iter().copied().min().unwrap_or(0);

        let mut osc_signs: Vec<i32> = Vec::new();
        for pair in scores.windows(2) {
            let d = pair[1] - pair[0];
            max_adjacent_swing_cp = max_adjacent_swing_cp.max(d.abs());
            if pair[0] != 0 && pair[1] != 0 && (pair[0] > 0) != (pair[1] > 0) {
                sign_flips += 1;
            }
            if d.abs() >= MIN_OSC_SWING_CP {
                osc_signs.push(d.signum());
            }
        }
        odd_even_oscillation = osc_signs.len() >= 3 && osc_signs.windows(2).all(|s| s[0] == -s[1]);

        for pair in w.windows(2) {
            if pair[0].best_move.map(|m| m.to_uci()) != pair[1].best_move.map(|m| m.to_uci()) {
                best_move_changes += 1;
            }
        }

        let is_mate = |s: i32| s.abs() >= MATE_THRESHOLD;
        mate_flip = w
            .windows(2)
            .any(|p| is_mate(p[0].score_cp) != is_mate(p[1].score_cp));
        if mate_flip {
            reasons.push("mate-flip");
        }

        // Partial-iteration risk: the budget cut the search off while the root move was
        // still shifting away from the last completed iteration's choice.
        let last_best = w.last().and_then(|it| it.best_move).map(|m| m.to_uci());
        let partial_shift = result.partial_iteration.as_ref().is_some_and(|p| {
            let prov = p.provisional_best.map(|m| m.to_uci());
            prov.is_some() && prov != last_best
        });

        let would_be_stable = best_move_changes <= cfg.stable_max_changes
            && max_adjacent_swing_cp <= cfg.stable_max_swing_cp
            && !odd_even_oscillation
            && !mate_flip;

        if partial_shift {
            reasons.push("partial-iteration-move-shift");
            StabilizationStatus::UnresolvedAtBudget
        } else if !would_be_stable {
            if best_move_changes > cfg.stable_max_changes {
                reasons.push("best-move-churn");
            }
            if max_adjacent_swing_cp > cfg.stable_max_swing_cp {
                reasons.push("score-swing");
            }
            if odd_even_oscillation {
                reasons.push("odd-even-oscillation");
            }
            StabilizationStatus::UnstableTrajectory
        } else if see_prune_skips >= cfg.omission_skip_threshold {
            reasons.push("negative-see-omission");
            StabilizationStatus::OmissionRisk
        } else {
            reasons.push("trajectory-settled");
            StabilizationStatus::StableAtBudget
        }
    };

    StabilizationReport {
        status,
        window: cfg.window,
        score_range_cp,
        max_adjacent_swing_cp,
        best_move_changes,
        odd_even_oscillation,
        mate_flip,
        sign_flips,
        see_prune_skips,
        reasons,
    }
}
