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
    /// Fixed-node diagnostic budget (INV-2): stop deterministically once this many
    /// nodes (main + qsearch) have been searched. `search()` routes any max_nodes
    /// request through the single-thread path so the stop point is reproducible.
    /// None = no node limit (normal play).
    pub max_nodes: Option<u64>,
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
    /// BUG1: ply-adjust mate scores on TT store/probe so a mate score stored at one
    /// ply reads correctly when probed at another (node-intrinsic TT mate distance).
    pub matett: bool,
    /// Log-based LMR reduction (r ≈ 0.75 + ln(d)·ln(i)/2.25) vs the flat 1-ply tier.
    pub loglmr: bool,
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
    /// Internal Iterative Deepening: at a node with no TT move hint, a reduced
    /// search populates the TT so move ordering has a real first move. Targets
    /// the audited move-ordering bottleneck (82% cold TT probes, ~35% first-cut).
    pub iid: bool,
    pub syzygy: bool,
    pub book: bool,
    pub root_diagnostics: bool,
    /// Root safe-quiet ordering (--rootsafequiet): at the root, boost quiet moves
    /// whose destination is NOT controlled by the opponent (a "safe quiet") so the
    /// calm, sound improvements CVS empirically skips get tried earlier among quiets.
    /// Ordering-only within the quiet band -> cannot change the alpha-beta value.
    pub root_safe_quiet: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            depth: 4,
            max_time_ms: None,
            soft_time_ms: None,
            max_nodes: None,
            quiet_checks: true,
            use_tt: true,
            danger_extension: false,
            null_move: true,
            lmr: true,
            pvs: true,
            rfp: true,
            futility: true,
            lmp: false,
            matett: false,
            loglmr: false,
            see_prune: false,
            delta_prune: false,
            countermove: false,
            conthist: false,
            tt_prune_store: true,
            rule50_scale: false,
            king_activity: false,
            qsearch_tt: true,
            hist_malus: true,
            hist_lmr: true,
            caphist: false,
            tt2: false,
            improving: false,
            threads: 1,
            cvs_trace: false,
            cvs_core_trace: false,
            cvs_bonus: true,
            shuffled_geometry: false,
            cvs_helpers: 0,
            lane: Lane::Fast,
            singular: false,
            iid: false,
            syzygy: true,
            book: true,
            root_diagnostics: false,
            root_safe_quiet: false,
        }
    }
}

impl SearchOptions {
    /// Apply explicit CLI feature toggles without changing the registered
    /// profile's defaults. Positive flags opt experiments in; `--no-*` flags
    /// provide a deterministic override for A/B controls.
    pub fn with_cli_flags(mut self, args: &[String]) -> Self {
        let toggle = |on: &str, off: &str, default: bool| {
            if args.iter().any(|arg| arg == off) {
                false
            } else if args.iter().any(|arg| arg == on) {
                true
            } else {
                default
            }
        };

        self.quiet_checks = toggle("--quiet-checks", "--no-quiet-checks", self.quiet_checks);
        self.use_tt = toggle("--tt", "--no-tt", self.use_tt);
        self.null_move = toggle("--null", "--no-null", self.null_move);
        self.lmr = toggle("--lmr", "--no-lmr", self.lmr);
        self.pvs = toggle("--pvs", "--no-pvs", self.pvs);
        self.rfp = toggle("--rfp", "--no-rfp", self.rfp);
        self.futility = toggle("--futility", "--no-futility", self.futility);
        self.lmp = toggle("--lmp", "--no-lmp", self.lmp);
        self.matett = toggle("--matett", "--no-matett", self.matett);
        self.loglmr = toggle("--loglmr", "--no-loglmr", self.loglmr);
        self.see_prune = toggle("--seeprune", "--no-seeprune", self.see_prune);
        self.delta_prune = toggle("--delta", "--no-delta", self.delta_prune);
        self.countermove = toggle("--countermove", "--no-countermove", self.countermove);
        self.conthist = toggle("--conthist", "--no-conthist", self.conthist);
        self.tt_prune_store = toggle(
            "--tt-prune-store",
            "--no-tt-prune-store",
            self.tt_prune_store,
        );
        self.rule50_scale = toggle("--rule50", "--no-rule50", self.rule50_scale);
        self.qsearch_tt = toggle("--qtt", "--no-qtt", self.qsearch_tt);
        self.hist_malus = toggle("--histmalus", "--no-histmalus", self.hist_malus);
        self.hist_lmr = toggle("--histlmr", "--no-histlmr", self.hist_lmr);
        self.caphist = toggle("--caphist", "--no-caphist", self.caphist);
        self.tt2 = toggle("--tt2", "--no-tt2", self.tt2);
        self.improving = toggle("--improving", "--no-improving", self.improving);
        self.king_activity = toggle("--king-activity", "--no-king-activity", self.king_activity);
        self.singular = toggle("--singular", "--no-singular", self.singular);
        self.iid = toggle("--iid", "--no-iid", self.iid);
        self.syzygy = toggle("--syzygy", "--no-syzygy", self.syzygy);
        self.book = toggle("--book-enabled", "--no-book", self.book);
        self.root_diagnostics = toggle(
            "--root-diagnostics",
            "--no-root-diagnostics",
            self.root_diagnostics,
        );
        self.cvs_bonus = toggle("--cvs-bonus", "--no-cvs-bonus", self.cvs_bonus);
        self.shuffled_geometry = toggle(
            "--shuffled-geometry",
            "--no-shuffled-geometry",
            self.shuffled_geometry,
        );
        self.root_safe_quiet = toggle("--rootsafequiet", "--no-rootsafequiet", self.root_safe_quiet);
        self
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
    /// IID: nodes where a reduced search was run to seed a missing TT move,
    /// and the subset where the re-probe then yielded a usable move hint.
    pub iid_searches: u64,
    pub iid_found: u64,
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
    pub iterations: Vec<SearchIteration>,
    pub root_order: Vec<Move>,
    pub attempted_depth: u32,
    pub termination: SearchTermination,
    pub result_source: SearchResultSource,
    pub partial_iteration: Option<PartialIteration>,
}

#[derive(Clone, Debug)]
pub struct SearchIteration {
    pub depth: u32,
    pub best_move: Option<Move>,
    pub score_cp: i32,
    pub nodes: u64,
    pub time_ms: u64,
    pub pv: Vec<Move>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchTermination {
    DepthLimit,
    SoftTime,
    HardTime,
    NodeLimit,
    ExternalStop,
    ProvenMate,
    Book,
    Tablebase,
}

impl SearchTermination {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DepthLimit => "depth-limit",
            Self::SoftTime => "soft-time",
            Self::HardTime => "hard-time",
            Self::NodeLimit => "node-limit",
            Self::ExternalStop => "external-stop",
            Self::ProvenMate => "proven-mate",
            Self::Book => "book",
            Self::Tablebase => "tablebase",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchResultSource {
    CompletedIteration,
    NoCompletedIteration,
    Book,
    Tablebase,
}

impl SearchResultSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompletedIteration => "completed-iteration",
            Self::NoCompletedIteration => "no-completed-iteration",
            Self::Book => "book",
            Self::Tablebase => "tablebase",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RootCandidateProgress {
    pub mv: Move,
    pub score_cp: i32,
    pub time_ms: u64,
}

#[derive(Clone, Debug)]
pub struct PartialIteration {
    pub depth: u32,
    pub alpha: i32,
    pub beta: i32,
    pub root_order: Vec<Move>,
    pub completed_candidates: Vec<RootCandidateProgress>,
    pub provisional_best: Option<Move>,
    pub provisional_score_cp: Option<i32>,
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
