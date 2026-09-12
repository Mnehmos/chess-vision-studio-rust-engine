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
    /// SEE-advisory verification (#3): when set, qsearch admits a negative-SEE capture/promo
    /// that fires a tactical-volatility trigger (gives check or is a promotion) instead of
    /// vetoing it on the static exchange alone. Default off => the plain SEE veto is unchanged.
    pub see_verify: bool,
    pub countermove: bool,
    /// BUG1: ply-adjust mate scores on TT store/probe so a mate score stored at one
    /// ply reads correctly when probed at another (node-intrinsic TT mate distance).
    pub matett: bool,
    /// Depth cap for reverse futility pruning (default 4). SF scales RFP to every
    /// depth; the cap was lowered 6->4 in 2026-06 for a hard-100 miss.
    pub rfp_depth: i32,
    /// Depth cap for futility pruning (default 3).
    pub fut_depth: i32,
    /// Depth cap for late-move pruning (default 4).
    pub lmp_depth: i32,
    /// ProbCut beta margin in centipawns (default 120).
    pub probcut_margin: i32,
    /// Aspiration window half-width in centipawns at the first try (default 50).
    pub asp_window: i32,
    /// First move index that can receive an LMR reduction (default 3).
    pub lmr_min_index: usize,
    /// SF-shaped LMR (--lmr2): reduce from the SECOND move (SF: moveCount > 1)
    /// instead of the fourth, and buy PV nodes one ply back (SF's `+ PvNode` in
    /// `d = max(1, min(newDepth - r/1024, newDepth + 2)) + PvNode`). The first
    /// three moves currently search at full depth at every node — the single
    /// largest remaining node-count difference from Stockfish at equal depth.
    pub lmr2: bool,
    /// Ply bonus added to every LMR reduction, before the cap (default 0).
    pub lmr_bonus: i32,
    /// Hard cap on the LMR reduction (default 6).
    pub lmr_max: i32,
    /// LMP budget = lmp_base + lmp_sq * depth^2 / 100 (defaults 8 and 200, i.e.
    /// the historical 8 + 2*d^2 — which at depth 6 is move 80 of ~39 legal moves,
    /// so LMP never fired above depth 3-4).
    pub lmp_base: i32,
    pub lmp_sq: i32,
    /// RFP margin = rfp_scale * depth (default 90).
    pub rfp_scale: i32,
    /// Futility margin = fut_base + fut_scale * depth (defaults 120 and 150).
    pub fut_base: i32,
    pub fut_scale: i32,
    /// Razoring: a shallow non-PV node far below alpha is verified in quiescence.
    pub razoring: bool,
    /// ProbCut: a good capture searched at reduced depth with raised beta may cut.
    pub probcut: bool,
    /// Recapture extension: extend a capture on the square the opponent just moved to.
    pub recapture: bool,
    /// Log-based LMR reduction (r ≈ 0.75 + ln(d)·ln(i)/lmr_div) vs the flat 1-ply tier.
    pub loglmr: bool,
    /// Divisor in the log-LMR reduction formula: smaller = more aggressive reduction.
    /// Default 2.25 (the 2026-09-09 gated value); `--lmr-div <v>` tunes it.
    pub lmr_div: f32,
    pub conthist: bool,
    pub tt_prune_store: bool,
    pub rule50_scale: bool,
    pub qsearch_tt: bool,
    pub hist_malus: bool,
    pub hist_lmr: bool,
    pub caphist: bool,
    /// Second continuation history (--conthist2): the move TWO plies back keys a
    /// second ordering term, matching SF's `contHist[1]` in the quiet statScore.
    /// Targets the measured internal-node ordering gap (SF's best move sits past
    /// rank 10 in our ordering in ~1/3 of positions) — the prerequisite for
    /// every movecount pruning decision.
    pub conthist2: bool,
    /// Pawn history (--pawnhist): a pawn-structure-keyed quiet history, SF's
    /// `sharedHistory.pawn_entry`. Quiets get a bonus when the SAME pawn skeleton
    /// has seen them cut off before — the one big ordering signal we never had.
    /// Targets the measured blocker: first-move cutoff 45% vs SF ~90%.
    pub pawnhist: bool,
    /// Quantized eval (--quant-eval): i16 weights + accumulator instead of f32.
    /// Measured quantization error: 5.6 cp MAE (S1=128), negligible vs the 85 cp
    /// eval MAE. The i16 accumulator gives 16 AVX2 lanes vs 8 for f32.
    pub quant_eval: bool,
    /// Pin-aware legal move generation (--pinmovegen): compute pinned pieces and
    /// checkers once, then only make/unmake-verify king moves, pinned-piece moves,
    /// and en-passant. All other moves are legal by construction when not in check.
    pub pinmovegen: bool,
    /// Balanced history updates (--histbal): give the quiet-cutoff BONUS the same
    /// magnitude as the tried-quiet MALUS (SF uses one `bonus` for both). Ours
    /// penalizes at 300*depth but rewards at only 150*depth, so the history drifts
    /// net-negative and the ordering signal compresses toward the floor.
    pub hist_bal: bool,
    /// Internal iterative reduction (--iir): at a cut node of depth >= 6 with no
    /// TT move, search one ply shallower (SF's IIR) instead of paying a full IID
    /// re-search. Cheap ordering fix for the ~70% of probes with no entry.
    pub iir: bool,
    /// SF frontier bundle (--sfprune): Stockfish's pruning *structure* with its
    /// published constants converted to centipawns (x100/208) — extended RFP,
    /// quiet futility, SF's SEE margins, capture futility, and the (3+d^2)/2
    /// movecount budget applied at every depth. Our rejected variants were
    /// harsh-but-shallow; SF is mild-but-everywhere.
    pub sfprune: bool,
    /// SF-shaped null move (--sfnull): SF's graded condition
    /// (`eval + 365 >= beta - 13*depth`, SF units) and depth-scaled reduction
    /// `R = 7 + depth/3 + max((eval-beta)/256, 0)` instead of our flat R = 2/3.
    pub sfnull: bool,
    /// SF-shaped quiescence (--sfqs): after two moves only checks and promotions
    /// are searched, and the first two are filtered by SF's qsearch futility and
    /// SEE-vs-alpha tests. Our qsearch is table-wide by comparison and is ~40% of
    /// the tree; SF's q-tree barely branches.
    pub sfqs: bool,
    /// Sub-toggle of --sfprune: the quiet-move SEE pruning (`see < -23*lmrDepth^2`).
    /// It is the only bundle member that needs an SEE/attack query at EVERY quiet
    /// move, and it is what costs the bundle its nodes-per-second (measured 1.8x
    /// before the unattacked fast path, 1.3x after). Off = "sfprune-lite".
    pub sf_quiet_see: bool,
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
            // INV-1 promotion 2026-09-10 (high-power re-gate): LMP crossed the upper bound
            // at 515 games (271-174-70, LLR +2.948) — the historical negative note is
            // overturned; it was already on in the bot and in the gate baseline by flag.
            // benchmarks/results/lmp-regate-hp-20260910/.
            lmp: true,
            // Mate-TT ply normalization ON by default (audit #60): raw
            // root-relative mate scores in the TT corrupt cross-ply probes.
            // The adjustment is exact (node-intrinsic distance), not a
            // strength heuristic; --no-matett remains available for A/B.
            matett: true,
            // INV-1 promotion 2026-09-09 (first gate on the fixed harness): loglmr
            // crossed the upper SPRT bound at 1030 games (497-392-141, LLR +2.965)
            // over 540 distinct book positions — benchmarks/results/loglmr-gate-20260909b/.
            loglmr: true,
            lmr_div: 2.25,
            rfp_depth: 4,
            fut_depth: 3,
            lmp_depth: 4,
            probcut_margin: 120,
            asp_window: 50,
            lmr_min_index: 3,
            lmr2: false,
            lmr_bonus: 0,
            lmr_max: 6,
            lmp_base: 8,
            lmp_sq: 200,
            rfp_scale: 90,
            fut_base: 120,
            fut_scale: 150,
            razoring: false,
            probcut: false,
            recapture: false,
            // 2026-09-10 high-power re-gates (4910 distinct positions) settled the
            // 2026-09-09 batch that was promoted on superseded records
            // (benchmarks/INV1_GATE_INTEGRITY_2026-09-09.md):
            //   seeprune  REJECT lower (LLR -2.958, 2748g) -> OFF
            //   improving REJECT lower (LLR -2.974, 2784g) -> OFF
            //   tt2       REJECT lower (LLR -2.945, 1214g) -> OFF
            //   caphist   HOLD +0.411 (4000g)              -> retained
            //   kingact   HOLD +0.370 (4000g)              -> retained
            see_prune: false,
            see_verify: false,
            delta_prune: false,
            countermove: false,
            // INV-1 promotion 2026-09-10 (high-power gate on 4910 distinct positions):
            // continuation history crossed the upper SPRT bound at 1781 games
            // (824-709-248, LLR +2.949) — benchmarks/results/conthist-big-gate-20260910/.
            conthist: true,
            tt_prune_store: true,
            rule50_scale: false,
            king_activity: true,
            qsearch_tt: true,
            hist_malus: true,
            hist_lmr: true,
            // caphist held (+0.411) at the high-power re-gate -> retained; tt2 and
            // improving crossed the LOWER bound there -> off.
            caphist: true,
            // Search-efficiency campaign (2026-09-10): off until each clears the
            // fixed-node SPRT gate.
            conthist2: false,
            pawnhist: false,
            quant_eval: false,
            pinmovegen: false,
            hist_bal: false,
            iir: false,
            sfprune: false,
            sfnull: false,
            sfqs: false,
            sf_quiet_see: true,
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
        self.lmr2 = toggle("--lmr2", "--no-lmr2", self.lmr2);
        let num = |flag: &str| -> Option<i32> {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|v| *v >= 0)
        };
        if let Some(v) = num("--rfp-depth") { self.rfp_depth = v; }
        if let Some(v) = num("--fut-depth") { self.fut_depth = v; }
        if let Some(v) = num("--lmp-depth") { self.lmp_depth = v; }
        if let Some(v) = num("--probcut-margin") { self.probcut_margin = v; }
        if let Some(v) = num("--lmp-base") { self.lmp_base = v; }
        if let Some(v) = num("--lmp-sq") { self.lmp_sq = v; }
        if let Some(v) = num("--rfp-scale") { self.rfp_scale = v; }
        if let Some(v) = num("--fut-base") { self.fut_base = v; }
        if let Some(v) = num("--fut-scale") { self.fut_scale = v; }
        if let Some(v) = num("--lmr-min-index") { self.lmr_min_index = v as usize; }
        if let Some(v) = num("--lmr-bonus") { self.lmr_bonus = v; }
        if let Some(v) = num("--lmr-max") { self.lmr_max = v; }
        if let Some(v) = num("--asp-window") { self.asp_window = v; }
        self.razoring = toggle("--razoring", "--no-razoring", self.razoring);
        self.probcut = toggle("--probcut", "--no-probcut", self.probcut);
        self.recapture = toggle("--recapture", "--no-recapture", self.recapture);
        if let Some(pos) = args.iter().position(|a| a == "--lmr-div") {
            if let Some(v) = args.get(pos + 1).and_then(|s| s.parse::<f32>().ok()) {
                if v.is_finite() && v > 0.0 {
                    self.lmr_div = v;
                }
            }
        }
        self.see_prune = toggle("--seeprune", "--no-seeprune", self.see_prune);
        self.see_verify = toggle("--seeverify", "--no-seeverify", self.see_verify);
        self.delta_prune = toggle("--delta", "--no-delta", self.delta_prune);
        self.countermove = toggle("--countermove", "--no-countermove", self.countermove);
        self.conthist = toggle("--conthist", "--no-conthist", self.conthist);
        self.conthist2 = toggle("--conthist2", "--no-conthist2", self.conthist2);
        self.pawnhist = toggle("--pawnhist", "--no-pawnhist", self.pawnhist);
        self.quant_eval = toggle("--quant-eval", "--no-quant-eval", self.quant_eval);
        self.pinmovegen = toggle("--pinmovegen", "--no-pinmovegen", self.pinmovegen);
        self.hist_bal = toggle("--histbal", "--no-histbal", self.hist_bal);
        self.iir = toggle("--iir", "--no-iir", self.iir);
        self.sfprune = toggle("--sfprune", "--no-sfprune", self.sfprune);
        self.sfnull = toggle("--sfnull", "--no-sfnull", self.sfnull);
        self.sfqs = toggle("--sfqs", "--no-sfqs", self.sfqs);
        self.sf_quiet_see = toggle("--sfquietsee", "--no-sfquietsee", self.sf_quiet_see);
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
        self.root_safe_quiet = toggle(
            "--rootsafequiet",
            "--no-rootsafequiet",
            self.root_safe_quiet,
        );
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
    /// Negative-SEE captures/promos kept by the SEE-advisory verification trigger (#3).
    pub see_verify_kept: u64,
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
    pub razor_attempts: u64,
    pub razor_cutoffs: u64,
    pub probcut_attempts: u64,
    pub probcut_cutoffs: u64,
    pub recapture_extensions: u64,
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
    /// Mate distance in FULL MOVES (UCI convention), signed by the side to move.
    /// `Some(1)` = the side to move mates in one move / is mated in one; `Some(0)` = already mated.
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
