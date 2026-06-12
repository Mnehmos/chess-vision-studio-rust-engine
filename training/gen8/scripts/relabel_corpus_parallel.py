#!/usr/bin/env python3
"""Portable parallel SF-relabel launcher — built to saturate a FRESH MACHINE.

Shards a corpus, spawns one single-threaded Stockfish worker per core (resumable),
and merges the labeled shards back. Designed to ship to a rented spot box: set
--sf to that box's Stockfish binary and --workers to its physical core count.

  python relabel_corpus_parallel.py CORPUS.jsonl OUT.jsonl \
      --depth 20 --workers 96 --sf /usr/bin/stockfish

Throughput math (measured ~2.4s/pos/core at d20, ~0.7s at d16):
  4.17M positions @ d20:  ~10M core-sec  -> 96 cores ~29h | 16 cores ~7.3d
  4.17M positions @ d16:  ~2.9M core-sec -> 96 cores ~8.5h | 16 cores ~2.1d
So d20 wants a rented 64-128 vCPU spot box (a few $ at spot pricing); d16 is
the local sweet spot. Resumable: re-run after a kill and it continues.

This wraps sf-relabel-worker.py (one SF process per shard). Workers run
detached; the script polls shard line-counts and reports progress, then merges.
"""
import glob
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORKER = os.path.join(HERE, '..', '..', '..', '..', 'chess-vision-studio',
                      'arena', 'sf-relabel-worker.py')


def arg(flag, dflt=None):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else dflt


def main():
    corpus, out = sys.argv[1], sys.argv[2]
    depth = arg('--depth', '20')
    workers = int(arg('--workers', str(os.cpu_count() or 8)))
    sf = arg('--sf')  # passed through via env to the worker if set
    worker = arg('--worker', WORKER)

    # 1. shard
    base = out + '.shard'
    rows = [l for l in open(corpus, encoding='utf8')]
    total = len(rows)
    print(f'{total} rows -> {workers} shards', flush=True)
    shard_paths = []
    for w in range(workers):
        p = f'{base}{w:03d}.jsonl'
        with open(p, 'w', encoding='utf8') as fh:
            fh.writelines(rows[w::workers])
        shard_paths.append(p)
    del rows

    # 2. spawn one resumable worker per shard
    env = dict(os.environ)
    if sf:
        env['CVS_SF'] = sf  # sf-relabel-worker honors CVS_SF if present
    procs = []
    for p in shard_paths:
        lp = p.replace('.shard', '.labeled')
        procs.append(subprocess.Popen(
            [sys.executable, worker, p, lp, depth], env=env))
    labeled_paths = [p.replace('.shard', '.labeled') for p in shard_paths]

    # 3. progress poll until all workers exit
    while any(pr.poll() is None for pr in procs):
        done = sum(sum(1 for _ in open(lp, encoding='utf8'))
                   for lp in labeled_paths if os.path.exists(lp))
        print(f'  {done}/{total} labeled ({done * 100 // total}%)', flush=True)
        time.sleep(60)

    # 4. merge
    with open(out, 'w', encoding='utf8') as wf:
        for lp in labeled_paths:
            if os.path.exists(lp):
                for line in open(lp, encoding='utf8'):
                    wf.write(line)
    done = sum(1 for _ in open(out, encoding='utf8'))
    print(f'merged {done}/{total} -> {out}', flush=True)


if __name__ == '__main__':
    main()
