//! Versioned teaching-fact extraction for analysis tools.
//!
//! This module is deliberately outside the search hot path. It emits board facts
//! and provenance only; topic classification and coaching language belong to the
//! application.

pub mod motifs;
pub mod move_bundle;
pub mod pawn_structure;
pub mod piece_safety;
pub mod position;
pub mod types;

pub use move_bundle::build_teaching_fact_bundle;
pub use types::*;

pub const TEACHING_FACTS_SCHEMA_VERSION: u32 = 1;
// v2: adds validated fork motif enumeration (available_motifs, gated by
// options.includeMotifOpportunities). Schema is additive, so SCHEMA_VERSION
// stays 1; the registry version bumps because a new validator now produces facts.
pub const FACTS_REGISTRY_VERSION: u32 = 2;
