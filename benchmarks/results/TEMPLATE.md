# Candidate report: <candidate name>

- baseline: snapshot/gen7-acc-futility-2026-06-11 (f07caae)
- one variable changed: <exactly one>
- exact command(s): `<...>`
- environment: <threads / tc or movetime or depth / hash / machine notes>
- artifact SHAs: engine `<sha>` net `<sha>` (futility ON/OFF: <state>)
- suite hash(es): <from results json>

## Results

| gate | result | notes |
|---|---|---|
| 0 identity | PASS/FAIL | |
| 1 parity | identical / N diffs | |
| 2 ladder | <depth/nps deltas> | |
| 3 cp-loss | <avgCP, bl200, danger vs baseline> | |
| 4 same-budget | <vs plain raw depth> | only for helper/clock/ponder features |
| 5 screen | <W-L-D, %, label> | not an Elo proof |
| 6 promotion | <W-L-D, Elo ±err, LOS, SPRT status, pgn/log paths> | label per README |
| 7 bot replay | <hit rates, avgCP, bl200> | only for opponent-clock features |

## Interpretation

<what the numbers mean; what they don't>

## Rollback path

<frozen artifact to repoint to>

## Next gate

<what runs next, or none>

Decision: PROMOTE | REJECT | ACCEPTED WITH NOTE | HOLD FOR MORE DATA | LIVE-DEV ONLY | ANALYSIS MODE ONLY
