#!/usr/bin/env python3
import json
import os
import subprocess
import argparse
import sys
from pathlib import Path

LICHESS_DATA_PATH = 'f:/Github/chess-vision-studio/arena/out/lichess-dataset.jsonl'
SELFPLAY_DATA_PATH = 'F:/tools/gen9-disagreement-seeds.jsonl'
BASE_TRAIN_PATH = 'F:/tools/gen9-train.jsonl'

def load_fens(path):
    fens = {}
    if not os.path.exists(path):
        return fens
    with open(path, encoding='utf8') as f:
        for idx, line in enumerate(f):
            if not line.strip():
                continue
            try:
                data = json.loads(line)
                fen = data['fen']
                fens[fen] = data
            except Exception:
                pass
    return fens

def normalize_row(fen, row):
    mate = row.get('mate', row.get('sfMate'))
    cp = row.get('cp', row.get('sfCp'))
    if mate is not None:
        cp = 10000 if mate > 0 else -10000
    if cp is None:
        return None
    return {
        'fen': fen,
        'cp': cp,
        'res': row.get('res', 0.5)
    }

def main():
    parser = argparse.ArgumentParser(description="Gen 9 RSI Loop pipeline")
    parser.add_argument('--dry-run', action='store_true', help="Print actions without executing commands")
    parser.add_argument('--skip-training', action='store_true', help="Skip PyTorch training stage")
    parser.add_argument('--epochs', type=int, default=10, help="Number of training epochs (default: 10 for quick loop)")
    parser.add_argument('--base-data', default=BASE_TRAIN_PATH)
    parser.add_argument('--lichess-data', default=LICHESS_DATA_PATH)
    parser.add_argument('--selfplay-data', default=SELFPLAY_DATA_PATH)
    parser.add_argument('--data-dir', default='training/gen9/gen9-cvs')
    parser.add_argument('--out-dir', default='target-cvs')
    parser.add_argument('--engine', default='target/release/analyze.exe')
    args = parser.parse_args()

    print("=== Gen 9 RSI Loop Orchestrator ===")

    # 1. Loading existing base dataset
    print(f"Loading base training data from {args.base_data}...")
    base_data = load_fens(args.base_data)
    print(f"Loaded {len(base_data)} positions from base.")
    seen_fens = set(base_data)

    # 2. Loading new Lichess bot review data
    new_rows = []
    if os.path.exists(args.lichess_data):
        print(f"Loading new Lichess bot reviewed positions from {args.lichess_data}...")
        lichess_data = load_fens(args.lichess_data)
        print(f"Loaded {len(lichess_data)} positions from Lichess reviews.")
        for fen, row in lichess_data.items():
            if fen not in seen_fens:
                row_fmt = normalize_row(fen, row)
                if row_fmt is None:
                    continue
                new_rows.append(row_fmt)
                seen_fens.add(fen)
    else:
        print(f"No Lichess bot reviewed data found at {args.lichess_data} yet.")

    # 3. Loading self-play disagreement data
    if os.path.exists(args.selfplay_data):
        print(f"Loading self-play disagreement positions from {args.selfplay_data}...")
        selfplay_data = load_fens(args.selfplay_data)
        print(f"Loaded {len(selfplay_data)} positions from self-play.")
        for fen, row in selfplay_data.items():
            if fen not in seen_fens:
                row_fmt = normalize_row(fen, row)
                if row_fmt is None:
                    continue
                new_rows.append(row_fmt)
                seen_fens.add(fen)
    else:
        print(f"No self-play disagreement data found at {args.selfplay_data} yet.")

    if not new_rows:
        print("No new unique positions found to add to the training corpus. RSI loop is already up to date!")
        # We can still proceed to train if requested, but let's exit if dry-run
        if args.dry_run:
            return
    else:
        print(f"Adding {len(new_rows)} new unique positions to the training corpus...")
        if not args.dry_run:
            Path(args.base_data).parent.mkdir(parents=True, exist_ok=True)
            with open(args.base_data, 'a', encoding='utf8') as out:
                for row in new_rows:
                    out.write(json.dumps(row) + '\n')
            print("Successfully appended new positions to base training data.")
        else:
            print(f"[Dry Run] Would append {len(new_rows)} rows to {BASE_TRAIN_PATH}")

    # 4. Run data preparation command
    prep_cmd = [
        sys.executable,
        'training/gen9/scripts/prepare_cvs_data.py',
        '--input', args.base_data,
        '--out-dir', args.data_dir,
        '--engine', args.engine
    ]
    print(f"Running data preparation: {' '.join(prep_cmd)}")
    if not args.dry_run:
        try:
            subprocess.run(prep_cmd, check=True)
            print("Data preparation complete.")
        except subprocess.CalledProcessError as e:
            print(f"Error: Data preparation failed: {e}")
            sys.exit(1)
    else:
        print("[Dry Run] Skipped data preparation execution.")

    # 5. Run Pytorch training stage
    if args.skip_training:
        print("Skipping PyTorch training stage as requested.")
        return

    train_cmd = [
        sys.executable,
        'training/gen9/scripts/train_matrix.py',
        '--epochs', str(args.epochs),
        '--data-dir', args.data_dir,
        '--out-dir', args.out_dir
    ]
    print(f"Running PyTorch training: {' '.join(train_cmd)}")
    if not args.dry_run:
        try:
            subprocess.run(train_cmd, check=True)
            print("PyTorch training complete. Updated models written to target-cvs/")
        except subprocess.CalledProcessError as e:
            print(f"Error: Training failed: {e}")
            sys.exit(1)
    else:
        print("[Dry Run] Skipped PyTorch training execution.")

if __name__ == '__main__':
    main()
