# Branching model (gitflow)

Long-lived branches:

| branch | holds | who moves it |
|---|---|---|
| `master` | released engine history; every commit is a gated, integrated change | merges from `develop` (or a hotfix) |
| `develop` | the integration branch; where finished work lands first | merges from `feature/*` |

Short-lived branches, all named by prefix:

| prefix | branches from | merges into | for |
|---|---|---|---|
| `feature/*` | `develop` | `develop` | any engine, benchmark, tooling or docs change |
| `release/*` | `develop` | `develop` + `master` | freezing a set of gated changes for release |
| `hotfix/*` | `master` | `develop` + `master` | a defect in released history that cannot wait |
| `experiment/*` | `master` or `develop` | nothing | isolated measurement runs; never merged as-is |

## Rules

1. **One change per feature branch**, matching the one-variable-per-gate discipline in
   `benchmarks/README.md`. A branch that bundles a search change with an eval change
   cannot be gated, so it cannot be merged.
2. **Gate before merge.** A `feature/*` branch merges into `develop` only with its
   decision record attached and `benchmarks/scripts/lint_promotion.py` clean:
   - strength change (INV-1) → SPRT record with `boundary: "upper"`
   - performance-only change (INV-2) → perf record with `decision: "accept_performance"`
   Analysis-only and held candidates stay on their branch.
3. **Re-measure after a rebase.** The baseline in a decision record is the commit the
   change is integrated onto. If `develop` moves under a feature branch, rebase and
   re-run its gate against the new baseline before merging — a record measured against
   an older baseline is stale evidence, not a promotion.
4. **`experiment/*` is a laboratory, not a queue.** Experiment branches carry raw
   artifacts and are kept for provenance. Work that survives its gate is re-applied on a
   `feature/*` branch cut from current `develop`.
5. **Never commit directly to `master` or `develop`.** Both move only by merge.

## Worktrees

Measurement runs need a clean tree while the main checkout stays in use, so feature and
experiment branches are usually checked out as worktrees:

```powershell
git worktree add ..\chess-vision-studio-rust-engine-<topic> -b feature/<topic> develop
git worktree list
git worktree remove ..\chess-vision-studio-rust-engine-<topic>
```

Build a worktree into its own `--target-dir` so a candidate binary is never confused with
the baseline binary, and record both executables' sha256 in the decision record.
