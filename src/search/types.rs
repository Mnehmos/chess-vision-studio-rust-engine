use crate::eval::Rung2Weights;
use crate::Move;

pub const TELEMETRY_PLY_BUCKETS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lane {
    #[default]
    Fast,
    KingSafety,
    See,
    Tactics,
    DefenderRemoval,
    QuietDefense,
    PawnEndgame,
}

impl Lane {
    #[inline]
    pub fn id(self) -> u8 {
        match self {
            Lane::Fast => 0,
            Lane::KingSafety | Lane::QuietDefense => 1,
            Lane::See | Lane::DefenderRemoval => 2,
            Lane::Tactics | Lane::PawnEndgame => 3,
        }
    }

    pub fn eval_profile(self, base: Option<Rung2Weights>) -> Option<Rung2Weights> {
        let mut w = base.unwrap_or_default();
        match self {
            Lane::Fast => return base,
            Lane::KingSafety => {
                w.king_danger = 15.0;
                w.king_central_exposure = 20.0;
                w.open_center_king_penalty = 45.0;
                w.king_escape_deficit = 12.0;
                w.enemy_queen_near_king = 6.0;
                w.king_zone_pressure *= 3.0;
                w.king_open_file *= 3.0;
                w.king_shield *= 3.0;
            }
            Lane::QuietDefense => {
                w.king_shield *= 4.0;
                w.king_escape_deficit = 10.0;
                w.king_danger = 8.0;
                w.hanging_piece *= 2.0;
            }
            Lane::See | Lane::DefenderRemoval => {
                w.hanging_piece *= 4.0;
            }
            Lane::Tactics => {
                w.mobility_knight *= 3.0;
                w.mobility_bishop *= 3.0;
                w.mobility_rook *= 2.0;
                w.mobility_queen *= 3.0;
            }
            Lane::PawnEndgame => {
                w.passed_pawn_mg *= 4.0;
                w.passed_pawn_eg *= 4.0;
                w.connected_passed_pawn *= 3.0;
            }
        }
        Some(w)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    pub depth: u32,
    pub max_time_ms: Option<u64>,
    pub soft_time_ms: Option<u64>,
    pub quiet_checks: bool,
    pub use_tt: bool,
    pub danger_extension: bool,
    pub null_move: bool,
    pub lmr: bool,
    pub pvs: bool,
    pub rfp: bool,
    pub futility: bool,
    pub lmp: bool,
    pub see_prune: bool,
    pub countermove: bool,
    pub conthist: bool,
    pub tt_prune_store: bool,
    pub rule50_scale: bool,
    pub qsearch_tt: bool,
    pub hist_malus: bool,
    pub hist_lmr: bool,
    pub caphist: bool,
    pub tt2: bool,
    pub improving: bool,
    pub king_activity: bool,
    pub delta_prune: bool,
    pub threads: usize,
    pub cvs_trace: bool,
    pub cvs_core_trace: bool,
    pub cvs_bonus: bool,
    pub shuffled_geometry: bool,
    pub cvs_helpers: usize,
    pub lane: Lane,
    pub singular: bool,
    pub syzygy: bool,
    pub book: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            depth: 4,
            max_time_ms: None,
            soft_time_ms: None,
            quiet_checks: true,
            use_tt: true,
            danger_extension: false,
            null_move: true,
            lmr: true,
            pvs: true,
            rfp: true,
            futility: true,
            lmp: true,
            see_prune: true,
            delta_prune: true,
            countermove: true,
            conthist: true,
            tt_prune_store: true,
            rule50_scale: true,
            king_activity: true,
            qsearch_tt: true,
            hist_malus: true,
            hist_lmr: true,
            caphist: true,
            tt2: true,
            improving: true,
            threads: 1,
            cvs_trace: false,
            cvs_core_trace: false,
            cvs_bonus: true,
            shuffled_geometry: false,
            cvs_helpers: 0,
            lane: Lane::Fast,
            singular: true,
            syzygy: true,
            book: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Telemetry {
    pub nodes: u64,
    pub main_nodes: u64,
    pub q_nodes: u64,
    pub q_capture_nodes: u64,
    pub q_see_skips: u64,
    pub quiet_check_extensions: u64,
    pub mate_threat_extensions: u64,
    pub hanging_major_extensions: u64,
    pub max_q_depth: u32,
    pub tt_probes: u64,
    pub tt_entries: u64,
    pub tt_hits: u64,
    pub tt_miss_cold: u64,
    pub tt_miss_contended: u64,
    pub tt_cutoffs: u64,
    pub beta_cutoffs: u64,
    pub hash_move_cutoffs: u64,
    pub first_move_cutoffs: u64,
    pub cutoff_move_index_sum: u64,
    pub cutoff_move_index_count: u64,
    pub legal_move_nodes: u64,
    pub legal_move_sum: u64,
    pub searched_moves: u64,
    pub pruned_moves: u64,
    pub elapsed_ms: u64,
    pub danger_extension_plies: u32,
    pub killer_cutoffs: u64,
    pub history_cutoffs: u64,
    pub null_attempts: u64,
    pub null_cutoffs: u64,
    pub lmr_reductions: u64,
    pub lmr_researches: u64,
    pub pvs_researches: u64,
    pub aspiration_researches: u64,
    pub rfp_attempts: u64,
    pub rfp_cutoffs: u64,
    pub futility_attempts: u64,
    pub futility_skips: u64,
    pub lmp_attempts: u64,
    pub lmp_skips: u64,
    pub see_prune_attempts: u64,
    pub see_prune_skips: u64,
    pub delta_attempts: u64,
    pub delta_skips: u64,
    pub cvs_trace_features: u64,
    pub foreign_tt_hints: [u64; 4],
    pub foreign_tt_cutoffs: [u64; 4],
    pub ply_nodes: [u64; TELEMETRY_PLY_BUCKETS],
    pub ply_child_searches: [u64; TELEMETRY_PLY_BUCKETS],
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score_cp: i32,
    pub mate: Option<i32>,
    pub pv: Vec<Move>,
    pub depth: u32,
    pub telemetry: Telemetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootScope {
    All,
    Only(Move),
}

#[derive(Clone, Debug)]
pub struct RootGeometryCacheEntry {
    pub zobrist: u64,
    pub model_hash: u64,
    pub registry_hash: u64,
    pub move_scores: Vec<(Move, i32)>,
}

#[derive(Clone, Debug)]
pub struct RootMoveAttention {
    pub mv: Move,
    pub raw_score: i32,
    pub raw_diff: i32,
    pub quiet_safety: i32,
    pub ranker_logit: f32,
    pub confidence: f32,
    pub ordering_bonus: i32,
}

pub type RootAttentionCache = Vec<RootMoveAttention>;
