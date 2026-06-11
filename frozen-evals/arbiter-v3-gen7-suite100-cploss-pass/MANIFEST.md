# Freeze: arbiter-v3-gen7-suite100-cploss-pass (2026-06-11)

Gen7-centered arbiter v3 passed the cp-loss milestone on suite-100.
**Fresh-suite validation: PASSED 2026-06-11 (see below) — generalization confirmed.**

## Fresh-suite validation (100 never-before-used positions, 82/18, seed 20260611)

Suite `f:/tmp/fresh-100.txt`, built by `arena/build-fresh-suite.py` from
nnue-all.jsonl (stride 9973 offset 1234, original-100 excluded, CVS
hanging/king-danger detector for the 82/18 split). Same settings, margins
frozen before results, NO tuning after.

| config | match% | avgCP | p90 | bl100 | bl200 | danger | quiet | give-back | harvest |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| gen7-alone | 51 | 22.9 | 84 | 6% | 2% | 23.8 | 18.8 | — | — |
| arb 5/15 | 63 | **8.9** | **36** | 1% | **0%** | 9.6 | 5.6 | 11% of 28 | **70%** |
| arb 10/25 | 63 | 9.5 | 36 | 1% | **0%** | 10.4 | 5.6 | 8% of 24 | 63% |
| arb 15/35 | 63 | 10.3 | 40 | 1% | **0%** | 10.4 | 9.8 | 9% of 22 | 60% |
| arb 25/50 | 60 | 11.1 | 41 | 1% | **0%** | 11.5 | 9.3 | 0% of 14 | 47% |

All pass criteria met at every margin: avgCP beaten (−52% to −61%), p90 beaten
(84→36-41), bl>=200 NOT increased (2%→0%), danger improves (23.8→~10),
give-back controlled. gen7-alone reads nearly identically on both suites
(22.7/22.9 avg, 2% bl200) — suites are comparable; the gain is real.

## Result (same-run SF-d12 rescore, `arena/bench-arbiter-v3.py`)

| config | match% | avgCP | medCP | p90 | bl100 | bl200 | danger | quiet | switches | give-back | harvest |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| gen7-alone | 51 | 22.7 | 0 | 79 | 8% | 2% | 23.6 | 18.4 | — | — | — |
| arb 5/15 | 55 | 15.4 | 0 | 58 | 5% | 1% | 15.7 | 14.0 | 31 | 6 (19%) | 18/30 (60%) |
| arb 10/25 | 56 | 16.0 | 0 | 69 | 5% | 1% | 16.4 | 14.4 | 23 | 3 (13%) | 14/30 (47%) |
| arb 15/35 | 56 | 16.8 | 0 | 73 | 5% | 1% | 16.4 | 18.4 | 19 | 3 (16%) | 12/30 (40%) |
| arb 25/50 | 57 | 15.8 | 0 | 69 | 5% | 1% | 15.2 | 18.4 | 16 | 1 (6%) | 11/30 (37%) |

Milestone targets met at every margin: avgCP <23.2, p90 <87, bl>=200 <=1%,
danger <23.9, match >=51% with no cp-loss cost.

## Provenance

| item | value |
|---|---|
| gen7 net | `raw-nnue-h256-sf-d12-v3.json` sha256[:16] `4cc9765c35c92d51` |
| engine exe | `target-cand/release/analyze.exe` sha256[:16] `ac6de316c1ece1f3` |
| suite | `f:/tmp/diversity-100.txt` sha256[:16] `d0f553383f1b2e4c` (82 danger / 18 quiet, danger from danger-suite.epd) |
| saved moves | `f:/tmp/diversity-results.json` sha256[:16] `bcc608b7f6c4c45d` |
| oracle | SF d14 bestmove (stored in saved moves) |
| cp-loss scoring | SF d12 child evals, mover POV, loss = max(0, best_child - chosen_child) |
| verification | gen7 child search d7 (d8 on danger positions), 1 thread |
| candidates | 9 lanes: fast(gen6), cvs-v3, net(nnue-gen1-h256), king, see, tactics, defender, quietdef, pawn — moves at d5 |
| margins | challenger >= main + margin; supported (>=2 lanes) / unsupported: (5/15),(10/25),(15/35),(25/50) |
| commit | `c0e5b7e` (local, NOT pushed — push withheld pending user approval) |
| caveats | suite-100 used during lane design (overfit risk); benchmark-level only, in-engine arbiter unbuilt; SF-d12 rescore variance ~0.5cp between runs |

## Promotion condition

Fresh suite (100 never-before-used positions, same 82/18 composition, same
settings, NO tuning after seeing results) must show: arbiter beats gen7-alone
on avgCP and p90, bl>=200 not increased, danger subset improves, give-back
controlled. Exact-match is secondary.
