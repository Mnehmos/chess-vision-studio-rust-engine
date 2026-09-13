# NNUE Scaling Classes

Each evaluator is identified by `{param_class}-v{version}`:
- **param_class** = the hidden-layer width (roughly, the parameter count)
- **version** = the iteration within that class (v1 = first attempt)

When we move to a new parameter class, we go back to v1. The old class's final net is
frozen at its last version. This is the LLM convention: GPT-3 and GPT-4 are different
classes, each with their own iteration history.

Example: the current shipped net is `200k-v10` — the 10th iteration of the 200K
parameter class (768→256→1). The next class starts as `400k-v1` — a fresh 394K
parameter net (768→512→1), trained from scratch.

## The Classes

| class | hidden | params | data at 40:1 | nps (est.) | engine support | status |
|---|---:|---:|---:|---:|---|---|
| **200k** | 256 | 197K | 8M | ~700K | ✅ current | **200k-v10 shipped** |
| **400k** | 512 | 394K | 16M | ~350K | ✅ (hidden ≤ 512) | 🔜 next: 400k-v1 |
| **800k** | 1024 | 788K | 32M | ~175K | ✅ (hidden ≤ 512) | |
| **1.2M** | 1536 | 1.18M | 47M | ~120K | needs engine change | |
| **1.6M** | 2048 | 1.58M | 63M | ~90K | needs engine change | |
| **3.2M** | 4096 | 3.15M | 126M | ~45K | needs SF-scale training | |

## Version Naming

```
200k-v1   first 200K net ever trained
200k-v2   second iteration (more data, better hyperparams)
...
200k-v10  current shipped net (the incumbent)

400k-v1   first 400K net (fresh start, new class)
400k-v2   second iteration
...
800k-v1   first 800K net (fresh class)
```

## Data Requirements

The 40:1 ratio is the measured sweet spot for this architecture (7.9M / 197K params).
Below ~20:1 the net underfits; above ~60:1 returns diminish. The Lichess eval DB
provides ~98M quiet d16+ positions — enough to hit 40:1 for any class up to 2.4M params.

## Scaling Protocol

1. Create a fresh net at the new parameter count (400k-v1, 800k-v1, etc.)
2. Train on the largest available dataset
3. Score on the eval instrument (MAE + r vs Stockfish's static eval)
4. Compare to the incumbent (200k-v10)
5. If the instrument shows improvement, gate with fixed-node SPRT
6. If the gate passes, promote and ship

Each class starts from v1. Warm-starting from the previous class is optional but
sometimes helps (the feature transformer patterns transfer even across widths).
