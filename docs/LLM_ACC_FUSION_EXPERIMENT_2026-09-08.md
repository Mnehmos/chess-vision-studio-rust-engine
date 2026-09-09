# LLM experiment 1: fuse NNUE accumulator updates

**Measured result:** 5.24% higher aggregate search throughput; 170/170 paired fixed-node searches have identical non-timing output. Equal-clock screen: 18 wins, 14 losses, 8 draws (55%). **HOLD_FOR_MORE_DATA** at the time of this experiment: strength gain unproven.

> **Outcome (2026-09-08):** accepted as a **performance-only** change under the INV-2 tier
> added to `benchmarks/README.md`, after re-measurement against `master` `145eab9`:
> 110/110 parity, 345 tests green, median +4.77% over 9 repeats
> (9/9 positive, sign test p = 0.00195), peak RSS +0.13%. Record:
> `benchmarks/results/perf-acc-fusion-accept-20260908/` (`REPORT.md`, `perf-record.json`).
> No Elo claim is made.

## Change

`Nnue::acc_apply_from` combines copying the parent accumulator with removing/adding piece-square features. Quiet moves, captures, promotions and en passant use one pass per perspective. Castling retains the existing update path. Arithmetic order is preserved; no model weights, evaluation terms, search heuristics or ISA requirements change.

The search accumulator stack uses this path when reusing an allocated slot. Initial slot allocation retains the original path.

## Control

- Baseline: clean committed source `73df5bda9db5adbf7b3612cd7a3575659c59c98c`, freshly built in `target/llm-baseline`.
- Candidate: that source plus the accumulator patch, built in `target/llm-candidate`.
- Main net: Gen9 `matrix-raw.json`, unchanged SHA-256 `b23c75b8e25d7480038f270255b3625556ecbdb1d53964cd60719871ff47c4e1`.
- Both: release build, identical dependency lock, one thread, accepted default search flags explicitly pinned, book/tablebases/helpers disabled.
- This experiment excludes the main checkout's uncommitted changes. Its frozen control is distinct from the older N0 executable and the live bot process.

## Measurements

| Check | Result |
|---|---|
| Correctness | 37 targeted tests passed, zero failures |
| Accumulator equivalence | Every float bit matches the original update path; both colors, all promotion types, captures, en passant, castling; widths 1/7/32/256/511; >2,000 random branch/unmake updates |
| Search parity | Canonical 10 + fresh 100 positions, 80,000-node ceiling: 110/110 identical after excluding `timeMs`/`nps` |
| Repeated throughput | 12 predetermined positions × 5 repeats, 320,000-node ceiling: 21,879 ms baseline / 20,790 ms candidate |
| Gain | Aggregate throughput +5.24%; median repeat +4.96%; repeat range +3.99% to +6.35% |
| Runtime parity | All 60 repeated searches identical; gain present in every measured position |
| Equal-clock games | 40 games, color-paired openings, 5+0.05, concurrency 2: 18–14–8 (55%) |
| Statistical gate | LLR 0.2005; upper bound 2.9444; no boundary crossed: `hold_for_more_data` |

Cold search state and alternating baseline/candidate execution order were used. The engine can finish before the node ceiling on a solved position; the stored responses record actual consumption. Throughput is baseline time / candidate time for the same work. These measurements do not establish an Elo gain.

## Artifacts and reproduction

All paths below are relative to this isolated checkout.

- [Manifest](../benchmarks/results/llm-acc-fusion-20260908/manifest.json): source, compiler, dependency and executable hashes; test command.
- [Patch](../benchmarks/results/llm-acc-fusion-20260908/candidate.patch): engine-only change.
- [Parity](../benchmarks/results/llm-acc-fusion-20260908/parity.json), [throughput](../benchmarks/results/llm-acc-fusion-20260908/speed.json): complete raw responses and artifact identities.
- [Match plan](../benchmarks/results/llm-acc-fusion-20260908/match/match-plan.json): predeclared settings and 0/20 Elo SPRT hypotheses, alpha=beta=0.05.
- [Match record](../benchmarks/results/llm-acc-fusion-20260908/match/match-sprt.json), [PGN](../benchmarks/results/llm-acc-fusion-20260908/match/match.pgn), [log](../benchmarks/results/llm-acc-fusion-20260908/match/match.log): completed screen and statistical result. No illegal-move, disconnect, stall, time-forfeit, error or warning entries were found in the match log.

```powershell
python benchmarks/scripts/bench_acc_fusion.py --baseline target/llm-baseline/release/analyze.exe --candidate target/llm-candidate/release/analyze.exe --net F:/Github/chess-vision-studio-rust-engine/target-cvs/matrix-raw.json --phase parity --nodes 80000 --out target/recheck-parity.json
python benchmarks/scripts/bench_acc_fusion.py --baseline target/llm-baseline/release/analyze.exe --candidate target/llm-candidate/release/analyze.exe --net F:/Github/chess-vision-studio-rust-engine/target-cvs/matrix-raw.json --phase speed --nodes 320000 --repeats 5 --out target/recheck-speed.json
python benchmarks/scripts/bench_acc_fusion_match.py --baseline target/llm-baseline/release/uci.exe --candidate target/llm-candidate/release/uci.exe --net F:/Github/chess-vision-studio-rust-engine/target-cvs/matrix-raw.json --games 40 --tc 5+0.05 --concurrency 2 --out-dir target/recheck-match
```

Candidate remains isolated on `experiment/llm-acc-fusion-20260908`; no live activation or promotion has occurred.

## Next experiments

1. Run a separately declared larger paired strength gate for this candidate, to a statistical boundary or explicit cap. The 40-game screen is only a reason to continue testing.
2. Connect source-preserving game splits to the Gen9 trainer. Then use reviewed losses to propose targeted evaluation candidates and run them through the same frozen-baseline gates. Keep search optimization and evaluation changes separate.
