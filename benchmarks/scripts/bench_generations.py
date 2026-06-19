#!/usr/bin/env python3
"""Canonical cross-generation benchmark entry point.

Examples:
  python benchmarks/scripts/bench_generations.py list
  python benchmarks/scripts/bench_generations.py validate
  python benchmarks/scripts/bench_generations.py smoke
  python benchmarks/scripts/bench_generations.py speed --times 50,250
"""

import argparse
import json
import os
import statistics
import sys

import benchlib as B


def engine_ids(registry, requested):
    if not requested:
        return [row['id'] for row in registry['engines']]
    ids = [item.strip() for item in requested.split(',') if item.strip()]
    known = {row['id'] for row in registry['engines']}
    missing = [item for item in ids if item not in known]
    if missing:
        raise SystemExit(f"unknown engine ids: {', '.join(missing)}")
    return ids


def validate_cfg(cfg):
    paths = {
        'serveExe': cfg['exe'],
        'uciExe': cfg.get('uci_exe'),
        'mainNet': cfg.get('net'),
        'helperNet': cfg.get('helper_net'),
        'baseWeights': cfg.get('base_weights'),
        'rung2Weights': cfg.get('rung2_weights'),
    }
    missing = [f'{name}={path}' for name, path in paths.items() if path and not os.path.exists(path)]
    return missing


def list_engines(registry):
    print(f"{'id':42s} {'status':20s} {'profile':25s} architecture")
    for row in registry['engines']:
        print(
            f"{row['id']:42s} {row['status']:20s} "
            f"{row['searchProfile']:25s} {row['architecture']}"
        )


def validate_engines(registry, ids):
    ok = True
    records = []
    for engine_id in ids:
        cfg = B.registered_engine(engine_id, registry)
        missing = validate_cfg(cfg)
        records.append({
            'engine_id': engine_id,
            'valid': not missing,
            'missing': missing,
            'provenance': B.provenance(cfg),
        })
        print(f"{'PASS' if not missing else 'FAIL'} {engine_id}")
        for item in missing:
            print(f"  missing: {item}")
        ok &= not missing
    B.write_result('generation-validation', {
        'registry': registry['_path'],
        'records': records,
        'pass': ok,
    })
    return ok


def smoke(registry, ids, depth):
    start = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1'
    rows = {}
    ok = True
    for engine_id in ids:
        cfg = B.registered_engine(engine_id, registry, depth=depth)
        missing = validate_cfg(cfg)
        if missing:
            rows[engine_id] = {'pass': False, 'missing': missing}
            print(f'FAIL {engine_id}: missing artifacts')
            ok = False
            continue
        engine = None
        try:
            engine = B.Engine(cfg)
            result = engine.search_depth(start)
            identity_reply = engine._ask('{"cmd":"identity"}')
            identity = (
                identity_reply
                if identity_reply.get('engine') == 'cvs-bitboard-core'
                else None
            )
            expected_options = cfg.get('expected_search_options', {})
            actual_options = identity.get('options', {}) if identity else {}
            option_mismatches = {
                key: {'expected': expected, 'actual': actual_options.get(key)}
                for key, expected in expected_options.items()
                if identity is not None and actual_options.get(key) != expected
            }
            passed = (
                bool(result.get('uci'))
                and not result.get('error')
                and not option_mismatches
            )
            rows[engine_id] = {
                'pass': passed,
                'uci': result.get('uci'),
                'scoreCp': result.get('scoreCp'),
                'depth': result.get('depth'),
                'nodes': result.get('nodes'),
                'timeMs': result.get('timeMs'),
                'command': engine.command,
                'effective_identity': identity,
                'identity_supported': identity is not None,
                'option_mismatches': option_mismatches,
                'provenance': B.provenance(cfg),
            }
            print(
                f"{'PASS' if passed else 'FAIL'} {engine_id}: "
                f"{result.get('uci')} d{result.get('depth')} "
                f"n{result.get('nodes')} {result.get('timeMs')}ms"
            )
            ok &= passed
        except Exception as error:
            rows[engine_id] = {'pass': False, 'error': str(error)}
            print(f'FAIL {engine_id}: {error}')
            ok = False
        finally:
            if engine:
                engine.close()
    B.write_result('generation-smoke', {
        'registry': registry['_path'],
        'depth': depth,
        'rows': rows,
        'pass': ok,
    })
    return ok


def speed(registry, ids, times, threads, repeats):
    canonical = json.load(open(os.path.join(B.SUITES, 'canonical.json'), encoding='utf8'))
    positions = [
        row for row in canonical['positions']
        if row['name'] in canonical['ladder_subset']
    ]
    table = {}
    for engine_id in ids:
        for thread_count in threads:
            cfg = B.registered_engine(
                engine_id, registry, depth=30, threads=thread_count
            )
            missing = validate_cfg(cfg)
            if missing:
                print(f'SKIP {engine_id}: missing artifacts')
                table[f'{engine_id}|{thread_count}T'] = {'missing': missing}
                continue
            engine = B.Engine(cfg)
            for budget in times:
                depth_runs = []
                mnps_runs = []
                elapsed_runs = []
                for _ in range(repeats):
                    depths = []
                    mnps = []
                    elapsed = []
                    for position in positions:
                        result = engine.search_time(position['fen'], budget)
                        actual_ms = max(1, result.get('timeMs') or budget)
                        depths.append(result.get('depth', 0))
                        mnps.append(result.get('nodes', 0) / actual_ms / 1000)
                        elapsed.append(actual_ms)
                    depth_runs.append(statistics.mean(depths))
                    mnps_runs.append(statistics.mean(mnps))
                    elapsed_runs.append(statistics.mean(elapsed))
                table[f'{engine_id}|{thread_count}T|{budget}ms'] = {
                    'median_avg_depth': statistics.median(depth_runs),
                    'depth_stdev': statistics.pstdev(depth_runs),
                    'median_avg_mnps': statistics.median(mnps_runs),
                    'mnps_stdev': statistics.pstdev(mnps_runs),
                    'median_elapsed_ms': statistics.median(elapsed_runs),
                    'repeats': repeats,
                }
            engine.close()

    print(f"{'engine':42s} {'thr':>3s} " + ' '.join(f'{ms:>17d}ms' for ms in times))
    for engine_id in ids:
        for thread_count in threads:
            cells = []
            for budget in times:
                row = table.get(f'{engine_id}|{thread_count}T|{budget}ms')
                cells.append(
                    'missing' if not row or row.get('missing')
                    else f"d{row['median_avg_depth']:.1f}/{row['median_avg_mnps']:.2f}M"
                )
            print(
                f'{engine_id:42s} {thread_count:>3d} '
                + ' '.join(f'{cell:>19s}' for cell in cells)
            )
    B.write_result('generation-speed', {
        'registry': registry['_path'],
        'suite': {
            'name': 'canonical-ladder-subset',
            'hash': B.sha256(os.path.join(B.SUITES, 'canonical.json')),
            'positions': [row['name'] for row in positions],
        },
        'times_ms': times,
        'threads': threads,
        'repeats': repeats,
        'table': table,
        'provenance': [
            B.provenance(B.registered_engine(engine_id, registry))
            for engine_id in ids
        ],
    })


def percentile(values, fraction):
    if not values:
        return None
    index = min(len(values) - 1, int(fraction * len(values)))
    return values[index]


def decision(registry, ids, suite_name, limit, movetime, decision_depth, sf_depth):
    suite = B.load_suite(suite_name)
    if not suite['oracle']:
        raise SystemExit(f'{suite_name} has no saved oracle moves')
    count = min(limit or len(suite['fens']), len(suite['fens']))
    fens = suite['fens'][:count]
    oracle = suite['oracle'][:count]
    danger = (suite['danger'] or [False] * len(suite['fens']))[:count]
    stockfish = B.Stockfish(depth=sf_depth)
    rows = {}

    for engine_id in ids:
        cfg = B.registered_engine(
            engine_id, registry, depth=decision_depth or 30, threads=1
        )
        missing = validate_cfg(cfg)
        if missing:
            rows[engine_id] = {'missing': missing}
            print(f'SKIP {engine_id}: missing artifacts')
            continue
        engine = B.Engine(cfg)
        moves = [
            (
                engine.search_depth(fen)
                if decision_depth
                else engine.search_time(fen, movetime)
            ).get('uci')
            for fen in fens
        ]
        engine.close()
        losses = []
        for fen, expected, move in zip(fens, oracle, moves):
            expected_cp = stockfish.child_cp(fen, expected)
            candidate_cp = stockfish.child_cp(fen, move) if move else None
            losses.append(
                None if expected_cp is None or candidate_cp is None
                else max(0, expected_cp - candidate_cp)
            )
        valid = sorted(value for value in losses if value is not None)
        danger_values = [
            losses[index] for index in range(count)
            if danger[index] and losses[index] is not None
        ]
        quiet_values = [
            losses[index] for index in range(count)
            if not danger[index] and losses[index] is not None
        ]
        stats = {
            'positions': count,
            'scored': len(valid),
            'match_pct': 100 * sum(
                move == expected for move, expected in zip(moves, oracle)
            ) / count,
            'avg_cp': statistics.mean(valid) if valid else None,
            'median_cp': statistics.median(valid) if valid else None,
            'p90_cp': percentile(valid, 0.90),
            'p95_cp': percentile(valid, 0.95),
            'bl100_pct': 100 * sum(value >= 100 for value in valid) / len(valid)
            if valid else None,
            'bl200_pct': 100 * sum(value >= 200 for value in valid) / len(valid)
            if valid else None,
            'danger_avg_cp': statistics.mean(danger_values) if danger_values else None,
            'quiet_avg_cp': statistics.mean(quiet_values) if quiet_values else None,
        }
        rows[engine_id] = {
            'stats': stats,
            'moves': moves,
            'losses': losses,
            'provenance': B.provenance(cfg),
        }
        print(
            f"{engine_id:42s} match {stats['match_pct']:5.1f}% "
            f"avg {stats['avg_cp']:6.1f} p90 {stats['p90_cp']:4} "
            f"bl200 {stats['bl200_pct']:4.1f}%"
        )
    stockfish.close()
    B.write_result('generation-decision', {
        'registry': registry['_path'],
        'suite': {
            'name': suite_name,
            'hash': suite['hash'],
            'positions': count,
        },
        'engine_budget': {
            'movetime_ms': None if decision_depth else movetime,
            'depth': decision_depth or None,
            'threads': 1,
        },
        'oracle': {'stockfish_depth': sf_depth},
        'results': rows,
    })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        'command', choices=('list', 'validate', 'smoke', 'speed', 'decision')
    )
    parser.add_argument('--registry', default=None)
    parser.add_argument('--engines', default='')
    parser.add_argument('--depth', type=int, default=2)
    parser.add_argument('--times', default='50,250')
    parser.add_argument('--threads', default='1')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--suite', default='suite-fresh-100')
    parser.add_argument('--limit', type=int, default=20)
    parser.add_argument('--movetime', type=int, default=100)
    parser.add_argument('--decision-depth', type=int, default=0)
    parser.add_argument(
        '--sf-depth',
        type=int,
        default=B.DEFAULT_STOCKFISH_REVIEW_DEPTH,
    )
    args = parser.parse_args()

    registry = B.load_engine_registry(args.registry)
    ids = engine_ids(registry, args.engines)
    if args.command == 'list':
        list_engines(registry)
        return
    if args.command == 'validate':
        raise SystemExit(0 if validate_engines(registry, ids) else 1)
    if args.command == 'smoke':
        raise SystemExit(0 if smoke(registry, ids, args.depth) else 1)
    if args.command == 'decision':
        decision(
            registry,
            ids,
            args.suite,
            args.limit,
            args.movetime,
            args.decision_depth,
            args.sf_depth,
        )
        return
    speed(
        registry,
        ids,
        [int(value) for value in args.times.split(',')],
        [int(value) for value in args.threads.split(',')],
        args.repeats,
    )


if __name__ == '__main__':
    main()
