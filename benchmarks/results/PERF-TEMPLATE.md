# Performance-only report: <candidate name>

Change class: **performance-only (INV-2)**. This report claims a throughput or resource
gain and **no** strength gain. If the change alters what the engine decides, stop: use
`TEMPLATE.md` and the INV-1 SPRT gate instead.

- baseline: `<commit/tag>` (freshly built, same lock, same flags)
- candidate: `<commit/branch>` — one change: `<exactly one>`
- what is claimed to be unchanged: `<eval terms / search heuristics / net weights / ISA>`
- exact command(s): `<...>`
- environment: `<threads / node ceiling / hash / machine>`
- artifact SHAs: engine `<sha>` net `<sha>`

## Gates

| gate | requirement | result |
|---|---|---|
| parity | ≥100 paired cold fixed-node searches identical (excl. `timeMs`/`nps`) | `<n>/<n>` |
| tests | zero failures | `<passed>` passed, `<failed>` failed |
| speedup | median ≥ 2%, sign test p ≤ 0.05 over ≥5 repeats, worst ≥ −2% | median `<x>%`, `<k>/<n>` positive (p=`<p>`), range `<min>%`..`<max>%` |
| resources | peak RSS and binary size within +5% | RSS `<x>%`, binary `<y>%` |
| screen (optional) | informational only | `<W-L-D>` |

## Unit-level equivalence

<what proves the new path computes the same values: exact-bit tests, widths, edge cases>

## Interpretation

<what the number means; it is throughput, not Elo>

## Rollback path

<frozen artifact / revert commit>

Decision: ACCEPT_PERFORMANCE | REJECT_PERFORMANCE | HOLD_FOR_MORE_DATA

<!-- ACCEPT_PERFORMANCE requires a linked perf record (schemas/perf-result.schema.json)
     that passes scripts/lint_promotion.py. A perf record never claims Elo. -->
