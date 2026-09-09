# Performance-only report: fused NNUE accumulator update (`acc_apply_from`)

Change class: **performance-only (INV-2)**. Claims a throughput gain and **no** strength
gain. First record accepted under this tier.

- baseline: `master` `145eab9` (#70), freshly built (`target/accept-master-base2`,
  sha256 `2c82ab54…`)
- candidate: `perf/accept-acc-fusion` — one change: `Nnue::acc_apply_from` fuses the parent
  accumulator copy with the remove/add feature deltas (one pass per perspective for quiet
  moves, captures, promotions, en passant; castling keeps the existing path)
  (`target/accept-candidate2`, sha256 `70c226e1…`)
- unchanged: net weights (`matrix-raw.json`, sha256 `b23c75b8…`), evaluation
  terms, search heuristics, move ordering, pruning, ISA requirements, arithmetic order
- environment: 1 thread, cold searcher per search, accepted default flags pinned
  explicitly, book/tablebases/helpers off, Windows-10-10.0.19045-SP0, 16 cores

## Gates

| gate | requirement | result |
|---|---|---|
| parity | ≥100 paired cold fixed-node searches identical (excl. `timeMs`/`nps`) | **110/110** @ 80k nodes (canonical 10 + fresh 100); a further 108/108 identical inside the timed run |
| tests | zero failures | **345 passed, 0 failed** across 53 suites (`cargo test --release --offline`) |
| speedup | median ≥ 2%, sign test p ≤ 0.05 over ≥5 repeats, worst ≥ −2% | median **+4.77%**, **9/9** repeats positive (p = **0.00195**), range **+1.95%…+7.38%**, aggregate **+4.39%** (42,730 ms → 40,934 ms) |
| resources | peak RSS and binary size within +5% | peak working set **+0.13%** (40.17 MB → 40.23 MB), binary **+0.19%** |
| screen (informational) | cannot promote or block | 40 games @ 5+0.05: 18–14–8 (55%), LLR 0.2005, no boundary crossed |

Per-repeat throughput gain: +2.46%, +4.84%, +2.84%, +6.64%, +6.35%, +7.38%, +3.11%, +4.77%, +1.95%.

## Unit-level equivalence

`tests/nnue_fused_accumulator.rs`: every float of the fused path matches the original
copy-then-update path bit for bit — both colors, all promotion types, captures, en
passant, castling, accumulator widths 1/7/32/256/511, and >2,000 random
branch/make/unmake sequences.

## Interpretation

The engine searches the same tree and returns the same moves and scores; it just gets
there about 4.4% faster at one thread. That is throughput, not Elo. The 40-game screen
(run during the original experiment, against the `73df5bd` baseline) did not cross an SPRT
boundary and is recorded only as a sanity check that nothing broke — it is not evidence of
added strength, and under INV-2 it is not required to be.

This record replaces an earlier measurement against `066174c`, which was superseded when
`master` advanced to `145eab9`; that run showed the same parity result and median +4.16%
with one −1.01% repeat. Both runs are consistent; only the current one, taken against the
`master` this change was integrated into, is the record.

## Rollback path

Revert the integration commit on `master`; the pre-integration baseline binary stays at
`target/accept-master-base2` (sha256 `2c82ab54…`) and
`145eab9` is its source.

## Next gate

None for this change. The next work is strength-class (INV-1): the never-gated search
flags that ship off by default (`--conthist`, `--countermove`, `--caphist`, `--lmp`,
`--seeprune`, `--singular`, `--improving`, `--delta`, `--iid`, `--tt2`), one variable per
SPRT gate.

Decision: ACCEPT_PERFORMANCE

<!-- Linked record: perf-record.json (schemas/perf-result.schema.json), lints clean under
     scripts/lint_promotion.py. Raw artifacts: parity.json, speed-r9.json (the record's
     source), peak-rss.json. -->
