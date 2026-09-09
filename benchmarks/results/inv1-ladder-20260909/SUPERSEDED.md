# SUPERSEDED — this ladder replayed the same games

Every gate in this directory was played with a 12-position book
(`F:/tools/openings.epd`) and `cutechess-cli order=sequential`, so each 120-game batch
replayed the same 24 distinct games. The recorded LLRs treat the repeats as independent
samples and are not valid promotion evidence at the stated sample sizes.

The full analysis, the harness fix, and the re-measurement on independent positions are
in [`../../INV1_GATE_INTEGRITY_2026-09-09.md`](../../INV1_GATE_INTEGRITY_2026-09-09.md).

The per-game rows are real and the records recompute from them; only the sample-size
accounting is wrong. Do not cite these records as SPRT crossings.
