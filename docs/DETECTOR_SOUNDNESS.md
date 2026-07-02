# Detector Soundness

How the teaching-fact motif detectors in `src/facts/motifs.rs` (and
`mate_patterns.rs`) stay at **zero false positives**, and the specific failure
classes that were found by adversarial fuzzing and fixed. Read this before writing
or reviewing a detector; every rule here was paid for with an engine-confirmed
false positive.

A detector emits a *claim*: "this legal move (or state) wins `materialGain`
centipawns against best defense." The bar is **precision over recall** — a missed
motif is a smaller cost than a wrong claim, because the app's teaching layer
presents these facts as truth.

## The standard guard set

Every move-enumeration detector applies, in some order:

1. **Legality at the root** — candidates come from `generate_legal`, never raw
   attack geometry.
2. **Hung-mover guard** — `forker_capturable_for_gain(&mut after.clone(), mv.to)`:
   the moved piece must not be simply lost on its landing square.
3. **Causality** — the claimed win must be *created by* the move:
   `best_see_capture(pos, target) <= 0` on the pre-move board (a target that was
   already winnable is a different, pre-existing fact).
4. **Counting proof** — the win is proven with SEE (`see`, `best_see_capture`) or
   a worst-case over enemy replies, never a bare attack count.
5. **In-check bail** — opposite-side probes come from
   `position_for_analysis_side(...).ok()?`; when the probe is impossible (our king
   in check, or the move gives check), the detector declines rather than fakes a
   turn (a documented false negative).
6. **King/pawn exclusions** where the piece class makes the claim meaningless
   (a king is never "won"; pawns bring promotion/EP subtleties).
7. **Determinism** — stable sort keys (move UCI, then piece ids); ties keep the
   first candidate; enumeration must not mutate the input position.
8. **Disjointness** — a new detector must not re-emit a sibling's fact
   (e.g. deflection requires the defender NOT capturable in place — the exact
   complement of attacking-the-defender's gate; double-attack rejects fork
   geometry via its distinct-piece guard and discovery geometry via a
   vacated-line check).

## The four false-positive classes (all fuzz-found, all engine-confirmed)

The single root cause behind all four: **`see()` / `best_see_capture` /
`attackers_of` are pin-blind and square-local.** They resolve an exchange on one
square under pseudo-legal capture geometry. Four distinct manifestations reached
production or review before being caught:

### 1. The pinned mover / pinned slider
A piece lands on (or is unveiled onto) a square where it is **absolutely pinned**
to its own king. Attack geometry says it attacks the target; it can never legally
capture it.

- double-attack: a mover that interposed against a check "threatened" a piece it
  could never take (9 FPs / 2608 fuzz ops).
- xray-attack: the xrayer itself pinned after moving (2 FPs / 1303).
- discovered-defense: a pinned unveiled slider credited as a defender.
- discovery (shipped for weeks before the audit caught it): a pinned unveiled
  slider credited with a discovered attack — 45 FPs / 19,671 ops. Example FEN:
  `r3kbr1/p1p1ppp1/1pn4n/8/7P/2QP1P2/PP2q1BK/RbB3NR w q - 2 17`, `f3f4` "unveils"
  `Bg2` onto `c6`, but `Bg2` is pinned to `Kh2` by `Qe2` along the 2nd rank.

**Fix:** require a *legal* capture. Either probe `generate_legal` on the
us-to-move board (respects pins), or — when that probe is unavailable because the
move gives check — use `capture_legal_wrt_pin(pos, from, to, us)`: a pure
king-ray pin test that still allows movement *along* the pin ray.

### 2. The too-valuable last recapturer
SEE's stand-pat rule is right, but `best_see_capture` accepted a "defense" whose
only defender is too valuable to be the last recapturer (a queen unveiled behind
pawns onto a square with more attackers than safe defenders). The piece is
legally lost anyway; the "rescue" is fake.

**Fix:** `legal_capture_gain(pos, sq, depth)` — a single-square recursive SEE
built on `generate_legal` with the standard stop rule.

### 3. The un-debited counter-capture
A worst-case-over-enemy-replies loop credited *our* recovery on a square but
never **debited the piece the enemy's reply just captured**. Review FEN:
`r5k1/3bbrpp/p1qp4/3QpR2/N2BP3/2p5/PPP3PP/6K1 w` — `Nb6` "deflects" `Qc6`, but
`Qxd5!` takes our queen; our recapture `exd5` even drops the f5 rook. The helper
scored the reply +900 (recapture) and dropped it as non-minimizing, reporting a
bogus +10.

**Fix (in the shared `attack_defender_worst_case`, hardening every caller):**
`our_net = our_best_recovery − enemy_take` per reply — a reply must pay for what
it grabs.

### 4. Off-square collateral beyond one reply
Even the debited one-reply model overcredits: the recovery's own collateral is
invisible one ply deeper. Desperado's original `see(pos, q_sq, m.to)` gain had
**69/640 engine-confirmed FPs** — an in-between grab of our hanging queen
elsewhere, a promotion reply, an off-square counter-capture. A 1-reply debit
still left 52 magnitude discrepancies.

**Fix:** `legal_material_quiescence(pos, depth)` — a full-board, alternating,
stand-pat capture/promotion quiescence on `generate_legal`. For a claim exposed
to arbitrary enemy replies: `claim = banked − legal_material_quiescence(after)`.
Captures-only means it can *under*-claim against quiet threats — the safe
direction.

## Choosing the counting proof

| Claim shape | Proof |
|---|---|
| Pure exchange on one square, we move first | `see(pos, from, to)` + a `generate_legal` legality check |
| "Target winnable once X changes" (single square) | `best_see_capture` for the estimate **and** `legal_capture_gain` as the legality gate |
| "Enemy must answer; every reply loses something" | `attack_defender_worst_case` (min over ALL enemy replies, debited) |
| "We bank material now, then survive anything" | `banked − legal_material_quiescence(after, ~5)` |

Report `materialGain` from the refined SEE values (`SEE_VALUE`: 100/320/330/500/900)
when a sibling detector already does, and use the legality-aware machinery as the
*gate*; keep values consistent across detectors.

## Verification protocol (before merge)

1. **Battery** (`tests/facts_<name>.rs`): ≥3 positive geometries, ≥4 negatives
   including the refuting reply, purity (no board mutation), determinism, and a
   regression test for every fuzz-found FP.
2. **Adversarial FP fuzz**: deterministic LCG playouts (no `rand`, no clock),
   every emitted op independently re-derived — legality-aware, min over all enemy
   replies. Scratch harnesses are deleted after use.
3. **Engine arbiter for verdicts**: hand-rolled single-square or capture-qsearch
   oracles **underestimate multi-move threats** — three deflection "FPs" flagged
   by such an oracle were engine-confirmed *real wins* (+255/+1005/+1424). A
   position is only a confirmed FP after `Searcher` (depth 10–12) agrees the
   claim fails against best defense.
4. **Full gates**: `cargo test` green, `cargo clippy -p cvs-bitboard-core --lib`
   = 0, eval parity 8/8, `nnue_accumulator` 1/1 (detectors are analysis-only —
   the champion eval must stay byte-identical), fixtures regenerated additively
   (`UPDATE_TEACHING_FIXTURES=1 cargo test --test facts_protocol`).
5. **Docs-vs-registry**: update the motif's `detectedBy` in
   `benchmarks/data/motif-taxonomy.json`, add the mapping in
   `benchmarks/scripts/check_detector_coverage.py`, and run it (CI fails on
   unbacked claims).

## Merge mechanics (parallel detector branches)

Detector branches collide only in mechanical shared files. Sequential merge
recipe: checkout → `git rebase master` → resolve by **union**: keep ALL
validators in `move_bundle.rs` + `tests/facts_protocol.rs`, keep all
`PositionFacts` fields / `position.rs` inits / `types.rs` structs / `motifs.rs`
functions, bump `FACTS_REGISTRY_VERSION` (+1 per detector, one history line
each), take `--theirs` on fixture conflicts then regenerate.

**Stale-branch trap:** a branch forked before a master bug-fix will silently
*delete* that fix on a naive merge (two review agents independently caught
branches reverting the discovery pin-fix). Always rebase onto current master and
verify the fix's helper + regression test survive before merging.
