//! Versioned teaching-fact extraction for analysis tools.
//!
//! This module is deliberately outside the search hot path. It emits board facts
//! and provenance only; topic classification and coaching language belong to the
//! application.

pub mod hazards;
pub mod mate_patterns;
pub mod motifs;
pub mod move_bundle;
pub mod pawn_structure;
pub mod piece_safety;
pub mod position;
pub mod promotion_pressure;
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
// v7: validated skewer enumeration (available_skewers + opponent probe).
// v8: validated discovered-attack enumeration (available_discoveries + opponent probe).
// v9: validated capturing-the-defender enumeration (available_remove_guard + opponent probe).
// v10: validated trapped-piece state enumeration (available_trapped + opponent probe).
// v11: named mate-pattern classification (available_mate_patterns + opponent probe).
// v12: validated overloaded-defender state enumeration (available_overload + opponent probe).
// v13: validated attacking-the-defender move enumeration (available_attack_defender + opp probe).
// v14: validated interference move enumeration (available_interference + opponent probe).
// v15: validated double-attack move enumeration (available_double_attack + opponent probe).
// v16: validated x-ray-attack move enumeration (available_xray_attack + opponent probe).
// v17: validated x-ray-defense move enumeration (available_xray_defense + opponent probe).
// v18: validated discovered-defense move enumeration (available_discovered_defense + opponent probe).
// v19: validated deflection/distraction move enumeration (available_deflection + opponent probe).
// Schema stays additive at v1; the registry version bumps when a new validator
// produces facts.
pub const FACTS_REGISTRY_VERSION: u32 = 19;
