# Capability audit — 2026-09-08

**Verdict:** engine, Lichess play, training components, and statistical gates exist. A production-connected recursive improvement loop is incomplete.

Scope: local Rust engine `abc7288` (pre-existing modified files), Studio `8e5a788`, local artifacts and read-only Lichess authentication. Existing binaries were exercised; current modified Rust sources were not rebuilt. RepoWise returned no useful hits; findings use live files.

## Capabilities

| Area | Available capability | Evidence / boundary |
|---|---|---|
| Engine | Bitboards, alpha-beta/PVS, iterative deepening, TT, quiescence, pruning, SMP, NNUE, optional book/Syzygy, pondering/time controls | `src/search.rs`, `src/eval/`, `src/book.rs`, `src/syzygy.rs`; feature presence is not measured strength |
| Interfaces | UCI, persistent JSON analysis, deterministic node budgets, search telemetry | `src/bin/uci.rs`, `src/bin/analyze.rs`; existing Gen9 binary passed depth-2 smoke |
| Teaching | Registry v22; legal branches, control, piece safety, pawn structure, motifs, hazards, promotion pressure, reply compression | `src/facts/`; detector accuracy/coverage not re-audited here |
| Studio | Rust bridge, gauntlets, oracle scoring, loss forensics, training UI | Sibling `chess-vision-studio`; `npm run train:loop` trains the TypeScript policy/value engine, not the Rust Gen9 NNUE |
| Lichess | Authenticated BOT `ChessVisionStudioEng`; challenge/play and review/harvest implementation | `npm run lichess:account` succeeded; existing bot process PID 30996 observed. No new games launched; live harvesting not verified |
| Training resources | Base corpus 755 MB, Lichess corpus 10.4 MB, disagreement seeds 316 KB, prepared shards; PyTorch CUDA on RTX 4070 | Paths below verified present; data quality/provenance not certified |
| Evaluation | Registry validation, speed, SF decision/cp-loss, sentinel suites, fixed-node paired matches, SPRT and promotion lint | `benchmarks/scripts/`; Stockfish, cutechess, openings and UCI binary present |

## Improvement-loop wiring

1. **Collect:** Studio `arena/lichess/harvest.ts` reviews games and retains `gameId`/`sourceKey`; Rust `training/gen9/scripts/disagreement_selfplay.py` generates oracle-labelled disagreements.
2. **Prepare/train:** `training/gen9/scripts/rsi_run_loop.py` deduplicates, excludes reserved FENs, prepares features and invokes `train_matrix.py`. It ends after training: no evaluation, promotion, or repeated-generation controller.
3. **Gate:** Studio `arena/rsi_orchestrator.py::run_generation` accepts injected training/match callbacks, checks source splits, gates on SPRT, and appends a hash-chained ledger. Caller search in Studio arena found tests only. It returns a champion decision; it does not install/restart the live bot.
4. **Match:** `benchmarks/scripts/match_fixed_nodes.py --sprt` connects real cutechess games to the canonical statistical result. This component is not connected to the Gen9 importer/trainer.

## Capability gaps to close

1. **Promotion-eligible training:** `train_matrix.py` explicitly refuses its row-split training path by default. Confirmed with `--epochs 0`; refusal occurs before training. `verify-split.py` exists, but the matrix trainer does not consume separate train/validation inputs. The unsafe override produces non-promotable development runs.
2. **Preserve source identity:** importer `normalize_row` reduces rows to `fen/cp/res`, discarding harvested game/source identity required for game-separated holdouts.
3. **Isolate candidates:** default RSI output is `target-cvs`, which contains the registered current models. Connect training to generation-specific artifacts before any automatic loop.
4. **Connect the controller:** wire collection → source split → candidate training → same-budget quality screen → paired games/SPRT → ledger → champion selection, with generation/budget limits. Real callbacks and production activation are missing from the audited wiring.
5. **Pin the baseline:** all four N0 model/weight hashes match. Current `target/release/analyze.exe` differs from pinned N0; `F:/tools/cvs-baselines/analyze-gen9-N0-00ccf79f.exe` still matches. Source checkout, current executable, frozen executable and live process identity must be distinguished.
6. **Repair comparison coverage:** 8/10 registry entries validate. Missing nets: `gen8-raw-h256-v2.json`, `gen9-raw-h256-d20.json` under Studio `arena/out`. Other Gen9 variants remain available.

## Checks executed

- Gen9 `g9.current-default.raw-plus-residual` smoke: `d2d4`, depth 2, 278 nodes.
- Four-position throughput screen, one thread, one repeat: 50 ms budget → average depth 4.0 / 0.67 M nodes/s; 250 ms → depth 5.5 / 0.72 M nodes/s. Small operational screen, not a strength estimate.
- 25 checks passed: orchestrator 10; SPRT 9; fixed-node match wiring 4; reserved-holdout importer 2. These verify components, not an end-to-end training run.
- No RSI/trainer process matched the inspected process command lines. A Lichess bot process was present.
- No new Elo estimate, full quality audit, training generation, match campaign, or promotion performed. Historical strength claims were not refreshed.

Fresh machine-readable results:

- `benchmarks/results/20260908-160824-generation-validation.json`
- `benchmarks/results/20260908-160845-generation-smoke.json`
- `benchmarks/results/20260908-160847-generation-speed.json`

## Reusable commands

From this repository:

```powershell
python benchmarks/scripts/bench_generations.py validate
python benchmarks/scripts/bench_generations.py smoke --engines g9.current-default.raw-plus-residual --depth 2
python benchmarks/scripts/bench_generations.py speed --engines g9.current-default.raw-plus-residual --times 50,250 --threads 1 --repeats 3
python training/gen9/scripts/rsi_run_loop.py --dry-run
```

The last command only inspects import/preparation plans; it does not establish training readiness. Default data inputs are `F:/tools/gen9-train.jsonl`, `F:/tools/gen9-disagreement-seeds.jsonl`, and `F:/Github/chess-vision-studio/arena/out/lichess-dataset.jsonl`.

From Studio: `npm run lichess:account` performs the read-only account check.
