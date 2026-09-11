# Engine inventory — 2026-09-10

One page answering: **what builds exist, what is deployed, what each one can do, and what
every flag is set to.**

## 1. The flagship (what runs live on Lichess)

| | |
|---|---|
| source | `master` @ `331ee75` |
| binaries | `target/release/uci.exe` (sha16 `55cfa1448d81c32d`), `target/release/analyze.exe` (`a84604f21ced33ff`) |
| interface | UCI (`uci`) + JSON-line serve (`analyze --serve`) |
| eval | raw NNUE `target-cvs/matrix-raw.json` (`b23c75b8…`) **+ core104 residual helper** `matrix-residual.json` (`d2a98888…`) **+ rung2** `arena/out/rung2-weights-mixed.json` (`a91268e5…`), base weights `value-weights-mixed.json` (`a3435d1a…`) — the pinned N0 identity |
| tablebases | `--syzygy F:/tablebases/syzygy345` (145× .rtbw/.rtbz, 3-4-5 piece) — working as of #95 |
| measured | **~2520 Elo** vs native Stockfish @ 10+0.1 (800 games / 100 positions; `ANCHOR_2026-09-10.md`) |
| live bot invocation | `analyze --serve --depth 30 --nnue matrix-raw.json --helper-nnue matrix-residual.json --syzygy F:/tablebases/syzygy345 --futility --rfp --tt-prune-store --qtt --histmalus --histlmr --lmp --smarttime --threads 4 --cvs-helpers 2 --base value-weights-mixed.json --rung2 rung2-weights-mixed.json` |

## 2. Version families

| generation | eval | representative artifact | role |
|---|---|---|---|
| gen6 | classical | `cvs-baselines/uci-gen6-full.exe` | rollback |
| gen7 | raw NNUE h256, sf-d12 corpus | `cvs-baselines/uci-gen7-acc-futility.exe` (`3376c2ac…`), `uci-gen7-ponder.exe` | the gate-ladder baseline (`snapshot/gen7-acc-futility-2026-06-11`) |
| gen7 patch series | classical + one search patch | `uci-patch1-killers`, `-patch2-null`, `-patch2b-nullhard`, `-patch7-pruning` | historical A/B binary per patch |
| gen8 | raw NNUE h256, mixed d12+d20 | `cvs-baselines/uci-gen8v2-champion.exe` (`fbecae9d…`) | historical champion |
| gen9 | raw / flat / residual / ranker nets + rung2 | `cvs-baselines/analyze-gen9-N0-00ccf79f.exe` (`00ccf79f…`) | the frozen N0 reference |
| current | gen9 + all gated search work | `target/release/*` above | **flagship** |

Registered generations (`benchmarks/engines.json`, all `champion-2026-06-12` profile unless
noted): `g7.raw-h256.sf-d12.v3` (frozen-baseline), `g8.raw-h256.mixed-d12-d20.v2`
(historical-champion), `g9.raw-h256.sf-d20`, `g9.flat-core104.h256`,
`g9.residual-core104.h256x32`, `g9.hybrid-a.raw-plus-residual` (analysis-only),
`g9.raw-control.matrix-raw` (live-candidate), `g9.hybrid-b.raw-plus-ranker`,
`g9.current-default.raw-plus-residual` (audit-control = N0), `g9.audit-all-on` (rejected).

## 3. Candidate builds in worktrees (not deployed)

| build | date | what it is |
|---|---|---|
| `…-gatefix/target/release/uci.exe` | 09-10 07:55 | champion build the gate chains use |
| `…-gatefix/target-std/release/uci.exe` | 09-10 08:36 | champion + razoring/ProbCut/recapture (flags off by default) |
| `…-ladder/target/release/uci.exe` | 09-08 22:06 | the 9.1.0-era ladder harness binary |
| `…-sing/target/release/uci.exe` | 09-09 09:44 | singular-exclusion-search experiment |
| `…-chm/target/release/uci.exe` | 09-09 09:56 | conthist-malus experiment |
| `…-performance/target/release/uci.exe` | 09-08 16:54 | INV-2 perf-gate build |

Unmerged branches carrying engine work: `feature/singular-exclusion-search`,
`feature/conthist-malus`, `feature/endgame-weighted-training`, `wip/conversion-rung3`,
`experiment/*` (ladder baseline, futility-pv), `agent/*` (facts-side detectors).

## 4. Flags

Every flag below is a boolean toggle in `src/search/types.rs`; `--flag` opts an experiment
in, `--no-flag` forces it off. "default" is `SearchOptions::default()`; "live" is what the
deployed bot runs (defaults + its env list); "evidence" is the record on the fixed harness
(2000-game gates, independent positions) unless marked historical.

### On in the flagship

| ON / OFF | field | default | live | evidence |
|---|---|---|---|---|
| `--loglmr` / `--no-loglmr` | log-based LMR | on | on | **PROMOTED** — LLR +2.965 upper crossed (1030g) |
| `--seeprune` / `--no-seeprune` | SEE pruning | on | on | re-gate HOLD +0.291 — unproven, kept |
| `--caphist` / `--no-caphist` | capture history | on | on | re-gate HOLD −1.163 — unproven, kept |
| `--improving` / `--no-improving` | improving flag | on | on | re-gate HOLD −0.966 — unproven, kept |
| `--tt2` / `--no-tt2` | two-bucket TT | on | on | re-gate HOLD −0.498 — unproven, kept |
| `--king-activity` / `--no-king-activity` | endgame king term | on | on | re-gate HOLD +0.470 — unproven, kept |
| `--futility` / `--no-futility` | futility pruning | on | on | historical: fixed-N +34, accepted-with-note |
| `--rfp` / `--no-rfp` | reverse futility | on | on | historical: formal SPRT +68.8 |
| `--tt-prune-store` / `--no-tt-prune-store` | store RFP cuts | on | on | historical: fixed-N +15.6, accepted-with-note |
| `--qtt` / `--no-qtt` | qsearch TT | on | on | historical: −7.4% nodes, accepted-with-note |
| `--histmalus` / `--no-histmalus` | history maluses | on | on | historical: fixed-N 53.2%/400, accepted-with-note |
| `--histlmr` / `--no-histlmr` | history-informed LMR | on | on | same record as histmalus |
| `--lmp` / `--no-lmp` | late move pruning | **off** | **on** | historical NEGATIVE note; on anyway in the bot and in `N0_FLAGS` |
| `--null` / `--no-null` | null-move pruning | on | on | baseline since gen2 |
| `--lmr` / `--no-lmr` | late move reductions | on | on | baseline |
| `--pvs` / `--no-pvs` | principal variation search | on | on | baseline |
| `--tt` / `--no-tt` | transposition table | on | on | baseline |
| `--quiet-checks` / `--no-quiet-checks` | quiet checks in qsearch | on | on | baseline |
| `--matett` / `--no-matett` | mate-TT ply normalization | on | on | audit #60: correctness, not strength |
| `--syzygy` / `--no-syzygy` | tablebase probing | on | on | needs `--syzygy <dir>`; fixed in #95 |
| `--book-enabled` / `--no-book` | polyglot book | on | on | inert without `--book <file>` |
| `--cvs-bonus` / `--no-cvs-bonus` | CVS geometry bonus | on | on | unflagged legacy default |

### Off in the flagship (available, gated)

| ON / OFF | field | default | live | evidence |
|---|---|---|---|---|
| `--razoring` / `--no-razoring` | razoring | off | off | gate HOLD −0.400 (2000g) |
| `--probcut` / `--no-probcut` | ProbCut | off | off | gating now |
| `--recapture` / `--no-recapture` | recapture extension | off | off | gating now |
| `--delta` / `--no-delta` | delta pruning | off | off | HOLD +0.490; high-power gate queued |
| `--conthist` / `--no-conthist` | continuation history | off | off | HOLD +0.127; high-power gate queued |
| `--countermove` / `--no-countermove` | countermove heuristic | off | off | HOLD −1.904 |
| `--iid` / `--no-iid` | internal iterative deepening | off | off | HOLD −0.704 |
| `--singular` / `--no-singular` | singular extensions | off | off | HOLD −0.401 |
| `--rule50` / `--no-rule50` | rule-50 score scaling | off | off | historical reject |
| `--seeverify` / `--no-seeverify` | SEE verification | off | off | **REJECT** LLR −2.958 (hurts) |
| `--rootsafequiet` / `--no-rootsafequiet` | root safe-quiet ordering | off | off | HOLD −0.732 |
| `--shuffled-geometry` / `--no-shuffled-geometry` | geometry shuffle | off | off | untested |
| `--root-diagnostics` / `--no-root-diagnostics` | root order diagnostics | off | off | analysis only |

### Value flags (take an argument)

| flag | meaning | flagship value |
|---|---|---|
| `--depth N` | depth cap | 30 |
| `--threads N` | search threads | 4 (bot) / 1 (gates, anchors) |
| `--cvs-helpers N` | specialist lane threads | 2 (bot) / 0 (gates) |
| `--lmr-div V` | log-LMR divisor | 2.25 (2.0/1.75/2.5/2.75 all gated worse or equal) |
| `--nnue FILE` | main net | `target-cvs/matrix-raw.json` |
| `--nnue-cal FILE` | eval output calibration curve (INV-1 promoted 2026-09-11; static MAE vs Stockfish 126 -> 85) | `target-cvs/eval-cal-20260911.json` |
| `--helper-nnue FILE` | residual helper net | `target-cvs/matrix-residual.json` |
| `--base FILE` / `--rung2 FILE` | handcrafted/TS weight files | Studio `arena/out/value-weights-mixed.json`, `rung2-weights-mixed.json` |
| `--syzygy DIR` | tablebase directory | `F:/tablebases/syzygy345` |
| `--book FILE` | polyglot book | none |
| `--smarttime` | soft/hard clock split (UCI + serve) | on (bot) |
| `--lane king\|see\|tactics\|defender\|quietdef\|pawn` | specialist lane | Fast (off) |
| `--allow-unverified-net` | skip net registry verification | off |
| `--serve`, `--fens`, `--features`, `--movetime`, `--cvs-ids`, `--cvs-core-ids`, `--cvs-trace`, `--cvs-core-trace`, `--cvs-deltas`, `--danger` | analyze-mode plumbing | n/a |

## 5. Gate evidence trail

`benchmarks/results/*/sprt.json` — one record per gate, each valid only with a crossed
boundary. Recent promotes: `loglmr` (2026-09-09), `conthist` (2026-09-10),
`lmp` re-gate (2026-09-10), **`--nnue-cal` eval calibration (2026-09-11, 1000 games,
479-375-146, LLR +2.971, no-adjudication gate)**. Rejects: `seeverify`, `delta`,
`seeprune`, `improving`, `tt2`, and the SF-constant selectivity bundles `sfprune`
at 12k nodes (-2.945) and `sfprune+sfnull` at equal time (-2.958). The 2026-09-09 promotion batch (caphist, seeprune,
improving, tt2, kingact) sits on records that are superseded
(`benchmarks/INV1_GATE_INTEGRITY_2026-09-09.md`) and their re-gates are neutral.
