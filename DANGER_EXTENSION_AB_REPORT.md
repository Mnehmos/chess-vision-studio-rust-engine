# Danger-triggered depth extension — A/B report (RSI loop 1, Track B)

**Patch:** `danger_extension` in the Rust search (gated, **OFF by default**). A cheap
root-level king-danger classifier (`danger_level`: enemy queen on board + own
king-zone pressure / king off home rank; ≤9 attack queries once per search) buys
+1 ply at danger 1, +2 plies at danger 2. CLI: `--danger`. Telemetry:
`danger_extension_plies`.

**Motivation:** the sf2200-g14 loss forensic — d5 missed defensive resources that
d7 finds, in exactly the positions the trigger detects.

## A/B on the named regression rows (`arena/rsi/regressions.jsonl`)

| Row | d5 baseline | d5 + danger | d7 reference | SF d20 best | Verdict |
|---|---|---|---|---|---|
| sf2200-g14-**ply20**-exf5 | e4f5 ✗ | e4f5 ✗ (ext→d7) | e4f5 ✗ | Bc4 | **value_miseval confirmed** — wrong even at d7 ⇒ eval-head territory (loop 2B king-exposure), not search |
| sf2200-g14-**ply24**-dxe5 | d4e5 ✗ | **f5f6 ✓** (ext→d7) | f5f6 ✓ | f6 | **fixed by trigger** — search_horizon |
| sf2200-g14-**ply40**-Rc2 | c1c2 ✗ (mates) | **f2f3 ✓** (ext→d7) | f2f3 ✓ | f3 | **fixed by trigger** — mate-defense resource found |

Exactly the predicted split: 2/3 fixed by danger-triggered depth; 1/3 isolated as
a real eval miss. The recovered moves are SF's depth-20 first choices (deep-oracle
agreement by construction).

## Trigger validation

- `cargo test` 38/38 (incl. `tests/danger.rs`: g14 ply-24 fires ≥1, ply-40 fires 2
  (critical), startpos / no-enemy-queen positions fire 0; extension off by
  default; capped at +2).
- Cost profile: ~3s on the (deliberately critical) regression positions vs
  ~0.2–0.4s at d5; quiet positions pay nothing (trigger 0). Game-level average
  cost requires the mini-gauntlet (below).

## Gate status — NOT yet promoted

| Gate | Status |
|---|---|
| Targeted A/B improves | ✅ 2/3 fixed, 1/3 correctly reclassified |
| d20+ oracle confirms recovered moves | ✅ (f6, f3 are SF-d20 firsts) |
| cargo test / perft / sanity | ✅ 38/38 |
| Normal R4 quality gate (no regression) | ⏳ pending |
| Mini-gauntlet rematch (blunder rate not worse, runtime acceptable) | ⏳ pending — 20 games vs SF-2200, experimental identity, NOT ladder-official |
| illegal = 0, mate missed = 0 | ⏳ measured in mini-gauntlet |

**Current promotion stance: keep OFF** (experimental) until the mini-gauntlet +
quality gate pass. Promotion options ranked: game-mode-under-time-budget is the
likely landing spot (danger plies are exactly where think-time is best spent);
default-on only if gates are strong.

## Next

1. Mini-gauntlet rematch vs SF-2200 with `--danger` (experimental identity).
2. Normal quality gate (eval-r4 harness) with danger on vs off.
3. Loop 2B evidence file: ply20-class (king-exposure value miseval) accumulating —
   candidate head: kingCentralExposure / enemyQueenNearKing / openCenterKingPenalty
   / kingEscapeDeficit, only after d20 oracle + more examples from 2400/2600 rungs.
