#!/usr/bin/env python3
import json
import random
import sys
import time

random.seed(42)

def main():
    if len(sys.argv) < 4:
        print("Usage: merge_and_shuffle.py <in1.jsonl> <in2.jsonl> <out.jsonl>")
        sys.exit(1)

    in1_path = sys.argv[1]
    in2_path = sys.argv[2]
    out_path = sys.argv[3]

    rows = []
    t0 = time.time()

    print(f"Reading {in1_path}...")
    with open(in1_path, 'r', encoding='utf-8') as f:
        for line in f:
            if line.strip():
                rows.append(line)
    print(f"Loaded {len(rows)} rows from {in1_path}.")

    print(f"Reading {in2_path}...")
    count2 = 0
    with open(in2_path, 'r', encoding='utf-8') as f:
        for line in f:
            if line.strip():
                rows.append(line)
                count2 += 1
    print(f"Loaded {count2} rows from {in2_path}. Total rows: {len(rows)}.")

    print("Shuffling...")
    random.shuffle(rows)

    print(f"Writing to {out_path}...")
    with open(out_path, 'w', encoding='utf-8') as f:
        for row in rows:
            f.write(row)

    print(f"Done in {time.time() - t0:.1f}s.")

if __name__ == '__main__':
    main()
