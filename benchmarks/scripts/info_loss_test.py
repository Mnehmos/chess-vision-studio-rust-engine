#!/usr/bin/env python3
"""Decisive Information-Loss Test for CVS Geometry.

Groups positions from the Gen 9 dataset by their sorted active geometry feature IDs
to analyze collisions, within-group CP/WDL variance, and best-move differences.
"""

import os
import sys
import glob
import json
import numpy as np
import chess
import chess.engine
import argparse
from collections import defaultdict

STOCKFISH_PATH = r"F:\tools\stockfish\stockfish\stockfish-windows-x86-64-avx2.exe"

def main():
    parser = argparse.ArgumentParser(description="Run CVS geometry information-loss test")
    parser.add_argument('--data-dir', default='training/gen9/gen9-cvs')
    parser.add_argument('--limit', type=int, default=50000, help='Max positions to process')
    parser.add_argument('--sf-depth', type=int, default=24, help='Stockfish depth for best move check')
    parser.add_argument('--sf-limit-groups', type=int, default=100, help='Max colliding groups to query SF for best moves')
    args = parser.parse_args()

    print("Loading positions...")
    files = sorted(glob.glob(os.path.join(args.data_dir, "*.jsonl")))
    if not files:
        print(f"No jsonl files found in {args.data_dir}")
        return

    positions = []
    loaded = 0
    for fpath in files:
        if loaded >= args.limit:
            break
        with open(fpath, encoding='utf-8') as fd:
            for line in fd:
                if loaded >= args.limit:
                    break
                try:
                    j = json.loads(line)
                    # Use features list as the geometry representation
                    features = tuple(sorted(j['features']))
                    positions.append({
                        'fen': j['fen'],
                        'cp': j.get('cp', 0),
                        'res': j.get('res', 0.5),
                        'features': features
                    })
                    loaded += 1
                except Exception:
                    continue

    print(f"Loaded {loaded} positions.")
    if loaded == 0:
        return

    # Group by geometry hash
    groups = defaultdict(list)
    for pos in positions:
        groups[pos['features']].append(pos)

    total_groups = len(groups)
    colliding_groups = {k: v for k, v in groups.items() if len(v) > 1}
    num_colliding_groups = len(colliding_groups)
    colliding_positions = sum(len(v) for v in colliding_groups.values())

    print("\n--- Geometry Hash Grouping ---")
    print(f"Total Unique Geometry Hashes: {total_groups}")
    print(f"Colliding Hashes (group size > 1): {num_colliding_groups}")
    print(f"Total Positions in Collisions: {colliding_positions} ({colliding_positions/loaded*100:.2f}%)")

    # Analyze evaluation variance for colliding groups
    cp_variances = []
    res_variances = []
    diff_eval_groups = 0  # groups where max(cp) - min(cp) > 50cp
    diff_wdl_groups = 0   # groups where max(res) != min(res)

    for geom, pos_list in colliding_groups.items():
        cps = [p['cp'] for p in pos_list]
        ress = [p['res'] for p in pos_list]
        
        cp_var = np.var(cps)
        res_var = np.var(ress)
        
        cp_variances.append(cp_var)
        res_variances.append(res_var)

        if max(cps) - min(cps) > 50:
            diff_eval_groups += 1
        if max(ress) != min(ress):
            diff_wdl_groups += 1

    mean_cp_var = np.mean(cp_variances) if cp_variances else 0.0
    mean_res_var = np.mean(res_variances) if res_variances else 0.0
    max_cp_var = np.max(cp_variances) if cp_variances else 0.0
    max_res_var = np.max(res_variances) if res_variances else 0.0

    print("\n--- Evaluation Variance (Colliding Groups) ---")
    print(f"Mean CP Variance: {mean_cp_var:.2f}")
    print(f"Max CP Variance: {max_cp_var:.2f}")
    print(f"Mean WDL/Result Variance: {mean_res_var:.4f}")
    print(f"Max WDL/Result Variance: {max_res_var:.4f}")
    print(f"Groups with different evaluations (>50 cp diff): {diff_eval_groups} ({diff_eval_groups/num_colliding_groups*100:.2f}% of colliding)")
    print(f"Groups with different WDL/Results: {diff_wdl_groups} ({diff_wdl_groups/num_colliding_groups*100:.2f}% of colliding)")

    # Run Stockfish check for best moves if Stockfish executable is found
    sf_active = os.path.exists(STOCKFISH_PATH)
    if sf_active and num_colliding_groups > 0:
        print(f"\nStockfish found at {STOCKFISH_PATH}. Analyzing best moves for first {min(args.sf_limit_groups, num_colliding_groups)} colliding groups...")
        try:
            engine = chess.engine.SimpleEngine.popen_uci(STOCKFISH_PATH)
            
            diff_move_groups = 0
            processed_groups = 0
            
            for geom, pos_list in list(colliding_groups.items())[:args.sf_limit_groups]:
                best_moves = []
                for pos in pos_list:
                    board = chess.Board(pos['fen'])
                    result = engine.play(board, chess.engine.Limit(depth=args.sf_depth))
                    best_moves.append(result.move.uci() if result.move else "None")
                
                # Check if all best moves in the group are identical
                if len(set(best_moves)) > 1:
                    diff_move_groups += 1
                processed_groups += 1
                
            engine.quit()
            print(f"\n--- Stockfish Best Move Variance ({processed_groups} groups analyzed) ---")
            print(f"Groups with different best moves: {diff_move_groups} ({diff_move_groups/processed_groups*100:.2f}%)")
            
        except Exception as e:
            print(f"Error running Stockfish: {e}")
    else:
        if not sf_active:
            print(f"\nStockfish executable not found at {STOCKFISH_PATH}. Skipping best move analysis.")

if __name__ == '__main__':
    main()
