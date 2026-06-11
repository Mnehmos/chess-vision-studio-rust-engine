# Architecture exploration: Heterogeneous CVS-SMP

> "Main core runs fast; multi-core usage gets CVS attention." — the directive.

## The idea, precisely

Standard lazy SMP runs **identical** searchers on every thread — same eval,
diversified only by depth/move-ordering noise, all sharing one transposition
table. The proposal **breaks the symmetry by evaluator**:

- **Main thread** → the lean, fast eval (classical material+PST+rung2, or raw
  piece-square NNUE). Maximizes raw nps and depth. It is authoritative for the
  returned move.
- **Helper threads (the extra cores)** → the **CVS-geometry-aware** search
  (CVS-NNUE eval, or CVS-trace-informed ordering/extensions). Slower per node
  (~0.62× nps, measured), but they *understand king danger, hanging material,
  defender removal* — the exact blind spots that cost us games.

The shared TT is the bridge. A geometry-aware helper that discovers "this quiet
move actually defends the king" deposits that discovery; the fast main thread
inherits it. **The cheap search gets CVS attention injected exactly where
material eval is blind, without paying the CVS tax on the main line.**

## Why this is the right shape for *us* specifically

Two facts we measured today make this fit unusually well:

1. **CVS costs ~0.62× nps as an always-on per-node tax** (flat across depth).
   Too expensive to put on every node of the *whole* search — but perfectly
   affordable if it only runs on the *helper* cores, which are otherwise
   spending their cycles on near-duplicate work anyway. Lazy SMP helpers have
   low marginal value (we measured the 4-thread gate at +42 Elo / LOS 97% but
   not a formal pass — helpers are *barely* earning their keep doing identical
   work). Giving them a *different, complementary* job is strictly better use
   of those cores than more of the same.

2. **Our losses are eval-blindness, not depth-blindness** (the whole RSI
   record: king-in-center mates, defender removal, slow positional drift). More
   raw depth from homogeneous helpers doesn't fix an eval that can't see the
   danger. A *geometry-aware* helper can — and only needs to surface ONE better
   move through the TT to change the game.

This turns the weakest part of our engine (lazy-SMP helpers doing redundant
work) into the delivery vehicle for the strongest new asset (CVS geometry),
without slowing the main line.

## The crux: how does helper insight propagate, soundly?

Two channels through the shared TT, very different safety profiles:

### Channel A — TT *move* hint (SOUND, recommended first)
The TT stores a best-move per entry. Move ordering is a pure heuristic: trying
the CVS-found move first costs nothing if it's wrong and prunes hugely if it's
right. A CVS helper that orders the king-defending move first stores it; the
main thread tries it first. **No score contamination** — alpha-beta correctness
is independent of move order. This is the safe, high-value version: helpers
become a *geometry-aware move-ordering oracle* for the fast main search.

### Channel B — TT *score* / bound (POWERFUL but needs care)
If the CVS helper also stores its *score*, the main thread may cut on a
CVS-flavored bound. This is where the real strength transfer lives (the main
thread would inherit CVS's correct pessimism about an exposed king) — but
mixing two eval scales in one table risks search instability: the main line's
alpha-beta assumes a self-consistent eval, and a CVS bound 200cp different from
what material eval would produce can cause re-search thrash or unstable PVs.

Lazy SMP already tolerates *some* score noise (helpers at different depths
store different scores). Different *evals* is a larger perturbation, bounded but
real. Mitigations: (1) only let helpers store bounds, never exact scores, at
depths ≥ the main thread's; (2) tag entries with eval-kind and let the main
thread treat foreign-eval entries as ordering-only (degrade Channel B → A on
read); (3) scale-align the CVS eval to the material eval's cp range so a stored
bound is at least same-units.

**Recommended path: ship Channel A first (provably safe, likely most of the
gain), measure, then cautiously open Channel B with eval-kind tagging.**

## Mapping to existing code (the diff is tiny)

`search_smp` (src/search.rs) already constructs each helper as an independent
`Searcher` sharing only `Arc<SharedTt>`. The heterogeneous version:

```rust
// main thread: self.search_single(pos, single)  // unchanged, fast eval
// helpers: give SOME of them the CVS eval instead of inheriting main's
let helper_eval = if t < cvs_helper_count { EvalKind::Cvs } else { EvalKind::Fast };
let mut helper = Searcher::new(weights, rung2);
helper.tt = tt;                       // shared bridge — already wired
match helper_eval {
    EvalKind::Cvs  => helper.nnue = cvs_net.clone(),   // geometry-aware
    EvalKind::Fast => {}                                // material
}
```

Pieces already in place: `Arc<SharedTt>` (lock-free, XOR-verified), per-thread
killers/history, the `with_nnue` eval swap, the `cvs_trace` hook, and the
`registry_hash` model-compat guard. What's missing for a real prototype: a
trained `cvs_nnue` (next on the build list) and an `eval-kind` tag on TT entries
for Channel B.

## Topology knob

`--threads N --cvs-helpers K`: N-1 helpers total, K of them CVS-aware, N-1-K
fast. Tune K to taste:
- K=0 → today's homogeneous SMP.
- K=all → "main fast, every core CVS" (the directive's pure form).
- K=1..2 → likely sweet spot: a couple of geometry scouts feeding the TT while
  the rest pile on raw depth.

## Risks / open questions

- **Does eval-diverse SMP actually transfer strength, or just noise?** Unknown
  until measured. The honest test: heterogeneous (main fast + K CVS helpers) vs
  homogeneous (all fast) at equal threads/clock, plus a king-safety-suite gate
  (our danger-suite EPD) where the transfer should show up strongest.
- **Channel B stability** — must be gated behind eval-kind tagging.
- **Requires a CVS-NNUE worth transferring** — this exploration is moot until
  the CVS net beats *something*. It is the *payoff* architecture for the CVS
  eval, not a substitute for building it.

## Verdict

Architecturally sound and a genuinely good fit: it repurposes low-value
redundant SMP work into a geometry-attention channel, keeps the main line fast,
and the propagation has a provably-safe first version (TT move hints). It is
**downstream of the CVS-NNUE build** — worth prototyping the moment we have a
CVS net that wins its own gate. Ship order: CVS-NNUE → Channel-A heterogeneous
SMP → measure on king-safety suite → cautiously open Channel B.
