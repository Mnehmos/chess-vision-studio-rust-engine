//! Versioned teaching-fact extraction for analysis tools.
//!
//! This module is deliberately outside the search hot path. It emits board facts
//! and provenance only; topic classification and coaching language belong to the
//! application.

pub mod hazards;
pub mod motifs;
pub mod move_bundle;
pub mod pawn_structure;
pub mod piece_safety;
pub mod position;
pub mod reply_compression;
pub mod square_control;
pub mod types;

pub use move_bundle::build_teaching_fact_bundle;
pub use types::*;

pub const TEACHING_FACTS_SCHEMA_VERSION: u32 = 1;
// v2: validated fork motif enumeration (available_motifs).
// v3: validated pin enumeration (available_pins), same includeMotifOpportunities gate.
// v4: non-moving-side motif probes for proving an opportunity was newly allowed.
// v5: structured captures, symmetric capture probes, king safety, and king shields.
// v6: deterministic 64-square control + legal movers (square_facts).
// Schema stays additive at v1; the registry version bumps when a new validator
// produces facts.
pub const FACTS_REGISTRY_VERSION: u32 = 6;
