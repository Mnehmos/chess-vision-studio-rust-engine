#!/usr/bin/env python3
"""Canonical SPRT match runner (issue #5, CLASSICAL_EVAL_EXPERIMENT.md Phase 6).

A Sequential Probability Ratio Test decides — with declared error rates — whether a candidate
is stronger than the frozen baseline, streaming game results and stopping the moment the
log-likelihood ratio crosses a boundary (or a game cap is hit). Per INV-1, PROMOTE is valid
ONLY when the UPPER boundary is crossed under the declared hypothesis; this runner is the thing
that produces that evidence.

Division of labour (same shape as benchmarks/de/probe_p_search.py):
  * the statistics + sequential decision core is pure and injectable (a `results` iterable of
    candidate-POV scores), so it is unit-tested WITHOUT any binary;
  * the CLI wires in a real result source — a cutechess-cli / match JSONL stream — and emits a
    record that validates against benchmarks/schemas/sprt-result.schema.json and passes
    benchmarks/scripts/lint_promotion.py (the promotion gate).

Model: the classic BayesElo trinomial LLR (Michel Van den Bergh / Fishtest). The draw rate is a
nuisance parameter estimated from the data (draw_elo) and held fixed while only the strength elo
is swung between H0 (elo0) and H1 (elo1). This matches the wins/losses/draws record schema.

INV-2 / #6: for a fixed-node control, pass tc="nodes:<N>" so the record ties to the cold,
single-thread, fixed-node diagnostic interface.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

WIN, DRAW, LOSS = 1.0, 0.5, 0.0


# ── SPRT statistics core (pure) ──────────────────────────────────────────────


def sprt_bounds(alpha: float, beta: float) -> tuple[float, float]:
    """Wald's A/B boundaries in LLR space. Must match lint_promotion's derivation exactly:
    lowerBound = log(beta/(1-alpha)), upperBound = log((1-beta)/alpha)."""
    lower = math.log(beta / (1.0 - alpha))
    upper = math.log((1.0 - beta) / alpha)
    return lower, upper


def _proba_to_bayeselo(pwin: float, ploss: float) -> tuple[float, float]:
    """Empirical (elo, draw_elo) from the win/loss fractions under the BayesElo model."""
    elo = 200.0 * math.log10(pwin / ploss * (1.0 - ploss) / (1.0 - pwin))
    draw_elo = 200.0 * math.log10((1.0 - ploss) / ploss * (1.0 - pwin) / pwin)
    return elo, draw_elo


def _bayeselo_to_proba(elo: float, draw_elo: float) -> tuple[float, float, float]:
    """Win/draw/loss probabilities for a strength elo at a fixed draw_elo."""
    pwin = 1.0 / (1.0 + 10.0 ** ((-elo + draw_elo) / 400.0))
    ploss = 1.0 / (1.0 + 10.0 ** ((elo + draw_elo) / 400.0))
    pdraw = 1.0 - pwin - ploss
    return pwin, pdraw, ploss


def llr_trinomial(wins: int, draws: int, losses: int, elo0: float, elo1: float) -> float:
    """BayesElo trinomial log-likelihood ratio of H1(elo1) vs H0(elo0).

    Returns 0.0 until all three outcomes have occurred (the MLE draw_elo is undefined when any
    of wins/draws/losses is zero) — the standard "keep sampling" behaviour, never a crash.
    """
    n = wins + draws + losses
    if n == 0 or wins == 0 or draws == 0 or losses == 0:
        return 0.0
    pwin, pdraw, ploss = wins / n, draws / n, losses / n
    _, draw_elo = _proba_to_bayeselo(pwin, ploss)
    w0, d0, l0 = _bayeselo_to_proba(elo0, draw_elo)
    w1, d1, l1 = _bayeselo_to_proba(elo1, draw_elo)
    return (
        wins * math.log(w1 / w0)
        + draws * math.log(d1 / d0)
        + losses * math.log(l1 / l0)
    )


def derive_boundary(llr: float, lower: float, upper: float) -> str:
    """Identical semantics to lint_promotion.derive_boundary."""
    if llr >= upper:
        return "upper"
    if llr <= lower:
        return "lower"
    return "none"


def decision_for(boundary: str) -> str:
    return {"upper": "promote", "lower": "reject", "none": "hold_for_more_data"}[boundary]


def elo_point_estimate(wins: int, draws: int, losses: int) -> float | None:
    """Logistic Elo point estimate from the score (diagnostic only — NOT promotion evidence)."""
    n = wins + draws + losses
    if n == 0:
        return None
    score = (wins + 0.5 * draws) / n
    if score <= 0.0 or score >= 1.0:
        return None
    return -400.0 * math.log10(1.0 / score - 1.0)


# ── sequential driver ────────────────────────────────────────────────────────


def run_sprt(
    results,
    elo0: float,
    elo1: float,
    alpha: float,
    beta: float,
    max_games: int | None = None,
):
    """Stream candidate-POV scores (1.0 win / 0.5 draw / 0.0 loss), recompute the LLR after each
    game, and stop at the first boundary crossing or when `results`/`max_games` is exhausted.

    Returns a dict with the running counts, the stop LLR, the bounds, the crossed boundary, and
    the terminal decision. Raises ValueError on an out-of-range hypothesis (elo1 must exceed
    elo0; alpha,beta in (0,0.5)) so a malformed test cannot silently "promote".
    """
    if not elo1 > elo0:
        raise ValueError(f"elo1 ({elo1}) must exceed elo0 ({elo0})")
    for name, x in (("alpha", alpha), ("beta", beta)):
        if not 0.0 < x < 0.5:
            raise ValueError(f"{name} ({x}) must be in (0, 0.5)")

    lower, upper = sprt_bounds(alpha, beta)
    wins = draws = losses = 0
    llr = 0.0
    boundary = "none"
    for score in results:
        if score == WIN:
            wins += 1
        elif score == LOSS:
            losses += 1
        elif score == DRAW:
            draws += 1
        else:
            raise ValueError(f"result score must be 1.0/0.5/0.0, got {score!r}")
        llr = llr_trinomial(wins, draws, losses, elo0, elo1)
        boundary = derive_boundary(llr, lower, upper)
        if boundary != "none":
            break
        if max_games is not None and wins + draws + losses >= max_games:
            break

    return {
        "wins": wins,
        "draws": draws,
        "losses": losses,
        "games": wins + draws + losses,
        "llr": llr,
        "lowerBound": lower,
        "upperBound": upper,
        "boundary": boundary,
        "decision": decision_for(boundary),
        "eloPointEstimate": elo_point_estimate(wins, draws, losses),
    }


def build_record(
    run: dict,
    *,
    experiment_id: str,
    baseline_id: str,
    candidate_id: str,
    elo0: float,
    elo1: float,
    alpha: float,
    beta: float,
    provenance: dict,
    pgn_sha256: str | None = None,
) -> dict:
    """Assemble a record matching benchmarks/schemas/sprt-result.schema.json. Rounds the LLR/
    bounds to 4 dp so they stay within lint_promotion's BOUND_TOL of the alpha/beta formulas."""
    rec = {
        "schemaVersion": 1,
        "experimentId": experiment_id,
        "baselineId": baseline_id,
        "candidateId": candidate_id,
        "elo0": elo0,
        "elo1": elo1,
        "alpha": alpha,
        "beta": beta,
        "games": run["games"],
        "wins": run["wins"],
        "losses": run["losses"],
        "draws": run["draws"],
        "llr": round(run["llr"], 4),
        "lowerBound": round(run["lowerBound"], 4),
        "upperBound": round(run["upperBound"], 4),
        "boundary": run["boundary"],
        "decision": run["decision"],
        "pgnSha256": pgn_sha256,
        "provenance": provenance,
    }
    return rec


# ── result-source parsing (real runner) ──────────────────────────────────────


def score_from_result_row(row: dict) -> float:
    """Normalise one match-result row to a candidate-POV score.

    Accepts either an explicit {"score": 1.0|0.5|0.0} (already candidate-POV) or a cutechess-cli
    style {"result": "1-0"|"0-1"|"1/2-1/2", "candidateColor": "white"|"black"}.
    """
    if "score" in row:
        s = float(row["score"])
        if s not in (WIN, DRAW, LOSS):
            raise ValueError(f"score must be 1.0/0.5/0.0, got {s}")
        return s
    result = row["result"]
    if result in ("1/2-1/2", "1/2", "draw"):
        return DRAW
    color = row["candidateColor"].lower()
    if color not in ("white", "black"):
        raise ValueError(f"candidateColor must be white/black, got {color!r}")
    white_won = result in ("1-0", "1", "white")
    black_won = result in ("0-1", "0", "black")
    if not (white_won or black_won):
        raise ValueError(f"unrecognised result {result!r}")
    candidate_won = (white_won and color == "white") or (black_won and color == "black")
    return WIN if candidate_won else LOSS


def _iter_result_scores(path: Path):
    """Yield candidate-POV scores from a JSONL match-result stream (one row per game)."""
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            yield score_from_result_row(json.loads(line))


def main(argv) -> int:
    ap = argparse.ArgumentParser(description="Canonical SPRT match runner (#5).")
    ap.add_argument("--results", required=True, help="JSONL of per-game results (candidate POV)")
    ap.add_argument("--elo0", type=float, default=0.0)
    ap.add_argument("--elo1", type=float, default=5.0)
    ap.add_argument("--alpha", type=float, default=0.05)
    ap.add_argument("--beta", type=float, default=0.05)
    ap.add_argument("--max-games", type=int, default=None)
    ap.add_argument("--experiment-id", required=True)
    ap.add_argument("--baseline-id", required=True)
    ap.add_argument("--candidate-id", required=True)
    ap.add_argument("--engine-sha", default="unknown")
    ap.add_argument("--net-sha", default="unknown")
    ap.add_argument("--cand-args", default="", help="space-separated candidate engine args")
    ap.add_argument("--base-args", default="", help="space-separated baseline engine args")
    ap.add_argument("--tc", default="nodes:0", help="time control, or 'nodes:<N>' for #6")
    ap.add_argument("--nodes", type=int, default=None)
    ap.add_argument("--threads", type=int, default=1)
    ap.add_argument("--openings-sha", default=None)
    ap.add_argument("--out", default=None, help="write the record here (else stdout)")
    args = ap.parse_args(argv)

    run = run_sprt(
        _iter_result_scores(Path(args.results)),
        elo0=args.elo0,
        elo1=args.elo1,
        alpha=args.alpha,
        beta=args.beta,
        max_games=args.max_games,
    )
    provenance = {
        "engineSha": args.engine_sha,
        "netSha": args.net_sha,
        "candArgs": args.cand_args.split() if args.cand_args else [],
        "baseArgs": args.base_args.split() if args.base_args else [],
        "tc": args.tc,
        "nodes": args.nodes,
        "threads": args.threads,
        "openingsSha": args.openings_sha,
    }
    rec = build_record(
        run,
        experiment_id=args.experiment_id,
        baseline_id=args.baseline_id,
        candidate_id=args.candidate_id,
        elo0=args.elo0,
        elo1=args.elo1,
        alpha=args.alpha,
        beta=args.beta,
        provenance=provenance,
    )
    text = json.dumps(rec, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    else:
        print(text)
    # Human-readable summary to stderr so stdout stays a clean record.
    print(
        f"SPRT {rec['decision']} (boundary={rec['boundary']}) "
        f"W-L-D {rec['wins']}-{rec['losses']}-{rec['draws']} "
        f"LLR {rec['llr']:.3f} in [{rec['lowerBound']:.3f}, {rec['upperBound']:.3f}]",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
