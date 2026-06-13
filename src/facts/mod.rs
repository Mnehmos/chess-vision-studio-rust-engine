//! Versioned teaching-fact extraction for analysis tools.
//!
//! This module is deliberately outside the search hot path. It emits board facts
//! and provenance only; topic classification and coaching language belong to the
//! application.

pub mod move_bundle;
pub mod pawn_structure;
pub mod piece_safety;
pub mod position;
pub mod types;

pub use move_bundle::build_teaching_fact_bundle;
pub use types::*;

pub const TEACHING_FACTS_SCHEMA_VERSION: u32 = 1;
pub const FACTS_REGISTRY_VERSION: u32 = 1;
