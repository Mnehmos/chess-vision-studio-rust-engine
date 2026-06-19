# Engine Generation And Naming Standard

## Canonical Identity

An engine identity is four independent parts:

1. Engine binary and commit.
2. Main evaluation model.
3. Optional helper or ranker model.
4. Search profile and resource budget.

The canonical name is:

```text
g<generation>.<eval-family>.<training-or-variant>[+<helper>]@<search-profile>
```

Examples:

```text
g7.raw-h256.sf-d12.v3@legacy-gen7-futility
g8.raw-h256.mixed-d12-d20.v2@champion-2026-06-12
g9.hybrid-a.raw-plus-residual@champion-2026-06-12
```

Names such as `Gen9`, `Hybrid A`, `champion`, or `best` are display labels, not
benchmark identities.

## Registry

`benchmarks/engines.json` is the source of truth for benchmarkable generations.
Each entry records its artifacts, architecture, lifecycle state, and search
profile. Paths support `${NAME:-fallback}`. Common overrides are:

```text
CVS_BASELINES_DIR
CVS_APP_DIR
```

## Model Metadata

Every newly trained model must include:

```text
modelKind
arch
trainingCommit
datasetManifestHash
rows
epochs
```

CVS geometry models additionally require:

```text
registryVersion
registryHash
cvsDim or featureCount
cvsStmRelative
```

Models without training and dataset provenance are development artifacts, not
promotion candidates.

## Lifecycle Labels

- `frozen-baseline`: immutable rollback and control.
- `historical-champion`: previously promoted and reproducible.
- `live-candidate`: deployed experimentally but not fully promoted.
- `candidate`: eligible for benchmark gates.
- `experimental`: architecture research with no strength claim.
- `audit-control`: intentionally captures a questionable or default stack.
- `rejected`: retained only for regression history.

## Result Naming

Benchmark results use:

```text
YYYYMMDD-HHMMSS-<gate-or-matrix>.json
```

Every result must contain artifact hashes, model metadata, search profile,
suite hash, hardware, toolchain, threads, and exact budget.

The registry's `effectiveOptions` is the expected profile contract. New
binaries must also return `{"cmd":"identity"}` with the effective runtime
options. A benchmark is invalid when the expected and reported options differ.
Historical binaries that predate the identity command are marked
`identity_supported: false`; their command line and frozen hashes remain the
source of truth.

Stockfish review and child-scoring commands default to depth 24. Lower depths
are allowed only as explicit diagnostic overrides and must remain recorded in
the result payload.
