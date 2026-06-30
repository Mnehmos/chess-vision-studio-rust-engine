//! Opponent option-compression specialist (#1, v1).
//!
//! After a candidate move, measure how strongly it compresses the opponent's *viable* decision
//! space: of all the opponent's legal replies, how many stay within a centipawn tolerance of the
//! best defense. Legal-reply count alone is a poor proxy for practical pressure — a position with
//! 25 replies but one defense is very different from one with 25 equal replies.
//!
//! This emits CHESS FACTS ONLY — raw legal count, viable counts at 25/50/100 cp, the best/second
//! reply evals, the punishment cliff, only-move flags, and the viable fraction stay SEPARATELY
//! visible. It decides no prose, no move quality, and never claims the opponent is "likely" to
//! blunder. Deterministic and outside the search hot path: each reply is scored by a fixed-depth
//! search, and the search depth + a confidence tag travel with the facts.

use crate::eval::ValueWeights;
use crate::movegen::generate_legal;
use crate::search::{SearchOptions, Searcher};
use crate::{Move, Position};

pub const REPLY_COMPRESSION_SCHEMA_VERSION: u32 = 1;

/// Confidence in the per-reply evaluations, derived from the search depth used. Shallow probes may
/// mis-rank replies in sharp/tactical lines; a consumer can require `Standard` before acting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyConfidence {
    Shallow,
    Standard,
}

/// One scored opponent reply: its uci and the opponent's achieved eval (cp, from the OPPONENT's
/// point of view — higher is a better reply for the opponent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyQualityFact {
    pub reply: String,
    pub eval_cp: i32,
}

/// Versioned reply-quality distribution for one candidate move (#1, v1). A subset of the issue's
/// suggested shape — entropy and the full per-reply table beyond `top_replies` are deferred.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplyCompressionFactsV1 {
    pub candidate_move: String,
    pub legal_reply_count: u16,
    pub viable_reply_count_25cp: u16,
    pub viable_reply_count_50cp: u16,
    pub viable_reply_count_100cp: u16,
    pub best_reply: Option<String>,
    pub best_reply_eval_cp: Option<i32>,
    pub second_reply_eval_cp: Option<i32>,
    /// cp gap between the last viable defense (at 50 cp) and the strongest clearly-inferior reply.
    pub punishment_cliff_cp: Option<i32>,
    pub only_move_25cp: bool,
    pub only_move_50cp: bool,
    pub viable_fraction_50cp: f32,
    /// Best-first, capped at `config.top_replies_cap` — the summary fields above describe the rest.
    pub top_replies: Vec<ReplyQualityFact>,
    pub search_depth: u16,
    pub confidence: ReplyConfidence,
    pub schema_version: u32,
}

/// Analysis configuration. Defaults: a depth-8 probe, top-6 replies, `Standard` at depth >= 8.
#[derive(Clone, Copy, Debug)]
pub struct ReplyCompressionConfig {
    pub search_depth: u16,
    pub top_replies_cap: usize,
    /// `search_depth >= standard_depth` is `Standard` confidence; below is `Shallow`.
    pub standard_depth: u16,
}

impl Default for ReplyCompressionConfig {
    fn default() -> Self {
        Self {
            search_depth: 6,
            top_replies_cap: 6,
            standard_depth: 8,
        }
    }
}

/// Compute the reply-compression distribution for `candidate` from `pos`. `candidate` must be legal
/// in `pos`; returns `None` otherwise (the position is left unchanged either way). The opponent's
/// value of each reply is `-search(after reply).score_cp` (our best eval after their reply, negated
/// to the opponent's POV), so the BEST reply is the one that leaves the opponent best off.
pub fn reply_compression_facts(
    pos: &mut Position,
    candidate: Move,
    weights: &ValueWeights,
    config: ReplyCompressionConfig,
) -> Option<ReplyCompressionFactsV1> {
    let candidate_uci = candidate.to_uci();
    if !generate_legal(pos).iter().any(|m| m.to_uci() == candidate_uci) {
        return None; // illegal candidate — do not corrupt the position
    }

    pos.make(candidate);
    let replies = generate_legal(pos);
    let legal_reply_count = replies.len() as u16;

    // A FRESH searcher per reply (cold TT) so each reply's eval is INDEPENDENT. A shared TT lets
    // sibling reply subtrees transpose and short-circuit one another, making the scores order-
    // dependent and unequal to the per-reply truth (observed to diverge from depth ~6, flipping the
    // reported best reply). This matches the analyze.rs convention of a fresh searcher per scored
    // position. Per-reply allocation is cheap relative to the search and keeps the result correct.
    let mut scored: Vec<ReplyQualityFact> = Vec::with_capacity(replies.len());
    for r in &replies {
        pos.make(*r);
        let mut searcher = Searcher::new(*weights, None);
        let res = searcher.search(
            pos,
            SearchOptions {
                depth: config.search_depth as u32,
                threads: 1,
                ..Default::default()
            },
        );
        pos.unmake();
        scored.push(ReplyQualityFact {
            reply: r.to_uci(),
            eval_cp: -res.score_cp,
        });
    }
    pos.unmake(); // undo the candidate; `pos` is now exactly as it was passed in

    // Best-first from the opponent's POV (higher eval_cp = stronger reply for the opponent). Ties
    // broken by uci so the ordering — and thus the whole result — is fully deterministic.
    scored.sort_by(|a, b| b.eval_cp.cmp(&a.eval_cp).then_with(|| a.reply.cmp(&b.reply)));

    let best_eval = scored.first().map(|f| f.eval_cp);
    let second_eval = scored.get(1).map(|f| f.eval_cp);

    let viable_at = |tol: i32| -> u16 {
        match best_eval {
            Some(b) => scored.iter().filter(|f| b - f.eval_cp <= tol).count() as u16,
            None => 0,
        }
    };
    let viable_25 = viable_at(25);
    let viable_50 = viable_at(50);
    let viable_100 = viable_at(100);

    // Punishment cliff at 50 cp: the eval drop between the last viable defense and the next reply.
    let punishment_cliff_cp = best_eval.and_then(|b| {
        let last_viable = scored.iter().rposition(|f| b - f.eval_cp <= 50)?;
        let next = scored.get(last_viable + 1)?;
        Some(scored[last_viable].eval_cp - next.eval_cp)
    });

    let viable_fraction_50cp = if legal_reply_count > 0 {
        viable_50 as f32 / legal_reply_count as f32
    } else {
        0.0
    };

    let confidence = if config.search_depth >= config.standard_depth {
        ReplyConfidence::Standard
    } else {
        ReplyConfidence::Shallow
    };

    let mut top_replies = scored;
    top_replies.truncate(config.top_replies_cap);

    Some(ReplyCompressionFactsV1 {
        candidate_move: candidate_uci,
        legal_reply_count,
        viable_reply_count_25cp: viable_25,
        viable_reply_count_50cp: viable_50,
        viable_reply_count_100cp: viable_100,
        best_reply: top_replies.first().map(|f| f.reply.clone()),
        best_reply_eval_cp: best_eval,
        second_reply_eval_cp: second_eval,
        punishment_cliff_cp,
        only_move_25cp: viable_25 == 1,
        only_move_50cp: viable_50 == 1,
        viable_fraction_50cp,
        top_replies,
        search_depth: config.search_depth,
        confidence,
        schema_version: REPLY_COMPRESSION_SCHEMA_VERSION,
    })
}
