#!/usr/bin/env python3
"""Search-switch registry: every search toggle/value flag, its source default, and its gate
evidence — generated from source so it cannot drift the way hand-written flag tables do.

Sources of truth:
  src/search/types.rs            SearchOptions fields, Default values, `with_cli_flags` parsing
  benchmarks/results/**/sprt*.json  gate records (fixtures/ excluded)

Outputs (both committed):
  benchmarks/search-switches.json   machine-readable registry (lab switch-matrix columns)
  docs/SEARCH_SWITCHES.md           the same, as a table

  python benchmarks/scripts/build_switch_registry.py          # regenerate both
  python benchmarks/scripts/build_switch_registry.py --check  # exit 1 if either is stale

Evidence states follow the lab protocol (chess-vision-studio-lab docs/RESEARCH_PROTOCOL.md):
promote -> SUPPORTED, reject -> REJECTED, hold_for_more_data -> INCONCLUSIVE, no record ->
PROPOSED. The latest record per switch (by result-directory date, then games) is `current`;
earlier records are kept as `history`, never dropped.
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TYPES_RS = REPO / "src" / "search" / "types.rs"
RESULTS = REPO / "benchmarks" / "results"
OUT_JSON = REPO / "benchmarks" / "search-switches.json"
OUT_MD = REPO / "docs" / "SEARCH_SWITCHES.md"

LAB_STATE = {"promote": "SUPPORTED", "reject": "REJECTED", "hold_for_more_data": "INCONCLUSIVE"}

# Gate slugs that do not match a flag name one-to-one.
GATE_ALIASES = {
    "kingact": ["--king-activity"],
    "sfprune": ["--sfprune"],
    "--sfprune --sfnull": ["--sfprune", "--sfnull"],
    "lmrdiv175": ["--lmr-div"], "lmrdiv20": ["--lmr-div"], "lmrdiv25": ["--lmr-div"], "lmrdiv275": ["--lmr-div"],
}

# Non-SearchOptions engine arguments that define an engine identity. Listed, not parsed:
# they live in src/bin/{analyze,uci}.rs argument handling.
ENGINE_ARGS = [
    ("--nnue FILE", "eval", "main NNUE net"),
    ("--nnue-cal FILE", "eval", "piecewise eval output calibration (gate: nnuecal-gate-20260911, promote)"),
    ("--helper-nnue FILE", "eval", "residual/ranker helper net (root quiet ordering only)"),
    ("--base FILE / --rung2 FILE", "eval", "handcrafted value weights / rung2 tactical weights"),
    ("--quant-eval", "eval", "quantized NNUE inference path"),
    ("--syzygy DIR", "resource", "tablebase directory (the --syzygy toggle is inert without it)"),
    ("--book FILE", "resource", "polyglot book (the --book-enabled toggle is inert without it)"),
    ("--threads N", "resource", "search threads (gates run 1)"),
    ("--cvs-helpers N", "resource", "specialist-lane helper threads"),
    ("--lane NAME", "resource", "specialist lane: fast|king|see|tactics|defender|quietdef|pawn"),
    ("--depth N / --movetime MS / nodeBudget", "budget", "search budget"),
    ("--smarttime", "budget", "soft/hard clock split for timed searches"),
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_types(src: str) -> list[dict]:
    struct = re.search(r"pub struct SearchOptions \{(.*?)\n\}", src, re.S).group(1)
    docs: dict[str, str] = {}
    pending: list[str] = []
    for line in struct.splitlines():
        s = line.strip()
        if s.startswith("///"):
            pending.append(s[3:].strip())
            continue
        m = re.match(r"pub (\w+):", s)
        if m:
            if pending:
                docs[m.group(1)] = " ".join(pending)
            pending = []
        elif s and not s.startswith("//"):
            pending = []

    default_body = re.search(r"impl Default for SearchOptions \{.*?SearchOptions \{(.*?)\n\s*\}\n\s*\}", src, re.S).group(1)
    defaults = {m.group(1): m.group(2).strip() for m in re.finditer(r"^\s*(\w+):\s*([^,\n]+),", default_body, re.M)}

    cli = re.search(r"pub fn with_cli_flags\(.*?\n    \}\n", src, re.S).group(0)
    switches = []
    for m in re.finditer(r"self\.(\w+)\s*=\s*toggle\(\s*\"(--[\w-]+)\",\s*\"(--[\w-]+)\",", cli, re.S):
        field, on, off = m.groups()
        switches.append({"id": on[2:], "kind": "toggle", "field": field, "on": on, "off": off,
                         "default": defaults.get(field) == "true" if field in defaults else None})
    for m in re.finditer(r"num\(\"(--[\w-]+)\"\)\s*\{\s*self\.(\w+)", cli):
        flag, field = m.groups()
        switches.append({"id": flag[2:], "kind": "value", "field": field, "flag": flag, "default": defaults.get(field)})
    for m in re.finditer(r"a == \"(--[\w-]+)\"\).*?self\.(\w+)\s*=", cli, re.S):
        flag, field = m.groups()
        if not any(s.get("flag") == flag for s in switches):
            switches.append({"id": flag[2:], "kind": "value", "field": field, "flag": flag, "default": defaults.get(field)})
    for s in switches:
        if s["field"] in docs:
            s["doc"] = docs[s["field"]]
    return sorted(switches, key=lambda s: (s["kind"], s["id"]))


def gate_records() -> list[dict]:
    out = []
    for f in sorted(glob.glob(str(RESULTS / "**" / "sprt*.json"), recursive=True)):
        rel = Path(f).relative_to(REPO).as_posix()
        if "/fixtures/" in rel:
            continue
        try:
            d = json.loads(Path(f).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        cand = str(d.get("candidateId") or "")
        slug = cand.split("+", 1)[1] if "+" in cand else Path(f).parent.name
        slug = re.sub(r"-regate$", "", slug)
        date = re.search(r"(\d{8})", rel)
        out.append({
            "record": rel, "slug": slug, "date": date.group(1) if date else None,
            "games": d.get("games"), "llr": d.get("llr"), "boundary": d.get("boundary"),
            "decision": d.get("decision"), "labState": LAB_STATE.get(d.get("decision"), "INCONCLUSIVE"),
            "elo0": d.get("elo0"), "elo1": d.get("elo1"),
        })
    return out


def attach_evidence(switches: list[dict], records: list[dict]) -> list[dict]:
    flag_of = {}
    for s in switches:
        flag_of[s.get("on") or s.get("flag")] = s
    unmatched = []
    for r in records:
        flags = GATE_ALIASES.get(r["slug"], ["--" + r["slug"]])
        hit = False
        for fl in flags:
            if fl in flag_of:
                flag_of[fl].setdefault("_records", []).append(r)
                hit = True
        if not hit:
            unmatched.append(r)
    for s in switches:
        recs = sorted(s.pop("_records", []), key=lambda r: (r["date"] or "", r["games"] or 0, r["record"]))
        if recs and s["kind"] == "value":
            # a value switch is swept (one record per tested value), so no record is "current"
            s["evidence"] = {"sweep": recs}
            s["labState"] = "SUPPORTED" if any(r["decision"] == "promote" for r in recs) else (
                "REJECTED" if all(r["decision"] == "reject" for r in recs) else "INCONCLUSIVE")
        elif recs:
            s["evidence"] = {"current": recs[-1], "history": recs[:-1][::-1]}
            s["labState"] = recs[-1]["labState"]
        else:
            s["labState"] = "PROPOSED"
        review = []
        if s["kind"] == "toggle" and recs:
            cur = recs[-1]
            if s["default"] and cur["decision"] == "reject":
                review.append("default ON but latest gate REJECTED")
            if s["default"] is False and cur["decision"] == "promote":
                review.append("latest gate PROMOTED but default OFF")
            if s["default"] and cur["decision"] != "promote":
                review.append("default ON without an upper-bound record (retained/perf/correctness change)")
        if review:
            s["review"] = review
    return unmatched


def build() -> dict:
    src = TYPES_RS.read_text(encoding="utf-8")
    switches = parse_types(src)
    records = gate_records()
    unmatched = attach_evidence(switches, records)
    return {
        "schemaVersion": 1,
        "generator": "benchmarks/scripts/build_switch_registry.py",
        "sources": {"typesRsSha256": sha256(TYPES_RS), "gateRecords": len(records)},
        "labStateMapping": LAB_STATE | {"(no record)": "PROPOSED"},
        "switches": switches,
        "engineArgs": [{"arg": a, "category": c, "meaning": m} for a, c, m in ENGINE_ARGS],
        "unmatchedGateRecords": unmatched,
    }


def render_md(reg: dict) -> str:
    L = ["# Search switches", "",
         "Generated by `benchmarks/scripts/build_switch_registry.py` from `src/search/types.rs` and",
         f"{reg['sources']['gateRecords']} gate records under `benchmarks/results/`. Do not edit by hand; run the",
         "script (CI/review: `--check`). Machine-readable copy: `benchmarks/search-switches.json`.", "",
         "Evidence state is the lab protocol's: SUPPORTED = SPRT upper bound, REJECTED = lower bound,",
         "INCONCLUSIVE = no boundary, PROPOSED = never gated. Only the latest record per switch is shown;",
         "earlier records stay in the JSON `history`.", "",
         "## Toggles", "", "| switch | on / off | default | state | latest gate (games, LLR) | review |", "|---|---|---|---|---|---|"]
    for s in reg["switches"]:
        if s["kind"] != "toggle":
            continue
        cur = (s.get("evidence") or {}).get("current")
        gate = f"`{Path(cur['record']).parent.as_posix().removeprefix('benchmarks/results/')}` ({cur['games']}, {cur['llr']:+.3f})" if cur else "—"
        default = {True: "on", False: "off", None: "?"}[s["default"]]
        L.append(f"| `{s['field']}` | `{s['on']}` / `{s['off']}` | {default} | {s['labState']} | {gate} | {'; '.join(s.get('review', []))} |")
    L += ["", "## Value switches", "", "| flag | field | default | state | sweep records (gate: LLR) |", "|---|---|---|---|---|"]
    for s in reg["switches"]:
        if s["kind"] != "value":
            continue
        sweep = (s.get("evidence") or {}).get("sweep") or []
        gate = ", ".join(f"`{Path(r['record']).parent.name}`: {r['llr']:+.3f}" for r in sweep) or "—"
        L.append(f"| `{s['flag']}` | `{s['field']}` | `{s['default']}` | {s['labState']} | {gate} |")
    L += ["", "## Engine identity arguments (not SearchOptions)", "", "| argument | category | meaning |", "|---|---|---|"]
    L += [f"| `{a['arg']}` | {a['category']} | {a['meaning']} |" for a in reg["engineArgs"]]
    if reg["unmatchedGateRecords"]:
        L += ["", "## Gate records not tied to a single switch", "", "| record | games | LLR | state |", "|---|---:|---:|---|"]
        L += [f"| `{r['record']}` | {r['games']} | {r['llr']:+.3f} | {r['labState']} |" for r in reg["unmatchedGateRecords"]]
    return "\n".join(L) + "\n"


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if the committed outputs are stale")
    a = ap.parse_args(argv)
    reg = build()
    js = json.dumps(reg, indent=2) + "\n"
    md = render_md(reg)
    if a.check:
        # compare modulo line endings: .gitattributes may check the markdown out as CRLF
        stale = [p for p, want in ((OUT_JSON, js), (OUT_MD, md))
                 if not p.exists() or p.read_text(encoding="utf-8").replace("\r\n", "\n") != want]
        for p in stale:
            print(f"stale: {p.relative_to(REPO).as_posix()} (run build_switch_registry.py)")
        return 1 if stale else 0
    OUT_JSON.write_text(js, encoding="utf-8", newline="\n")
    OUT_MD.write_text(md, encoding="utf-8", newline="\n")
    toggles = sum(s["kind"] == "toggle" for s in reg["switches"])
    print(f"{toggles} toggles, {len(reg['switches']) - toggles} value switches, "
          f"{reg['sources']['gateRecords']} gate records ({len(reg['unmatchedGateRecords'])} not switch-specific)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
