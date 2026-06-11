# Gate 0 — Identity / Smoke. Prove we are testing the intended artifact.
#
#   python bench_identity.py [--exe PATH] [--net PATH] [--no-futility]
#
# Pass: engine boots, answers the serve protocol, the expected net loads
# (loud-fail conventions verified by a sane eval reply), no crash.
import json
import sys

import benchlib as B


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


cfg = B.engine_cfg(
    name=arg('--name', 'candidate' if '--exe' in sys.argv else 'baseline'),
    exe=arg('--exe'), net=arg('--net'),
    futility=False if '--no-futility' in sys.argv else None,
    depth=4,
)
prov = B.provenance(cfg)
print(json.dumps(prov, indent=1))

ok = True
try:
    e = B.Engine(cfg)
    r = e.search_depth(B.BASELINE and 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1')
    boot = r.get('uci') is not None and r.get('error') is None
    print(f'boot+search: {"PASS" if boot else "FAIL"}  (uci={r.get("uci")} scoreCp={r.get("scoreCp")})')
    ok &= boot
    # eval probe confirms the net actually loaded (nnueStmCp present)
    ev = e._ask('eval rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1')
    net_ok = 'nnueStmCp' in ev if cfg.get('net') else True
    print(f'net loaded:  {"PASS" if net_ok else "FAIL"}  (eval={ev})')
    ok &= net_ok
    e.close()
except Exception as ex:
    print(f'FAIL: {ex}')
    ok = False

B.write_result('gate0-identity', {'provenance': prov, 'pass': ok})
print(f'GATE 0: {"PASS" if ok else "FAIL"}')
sys.exit(0 if ok else 1)
