#!/usr/bin/env python3
import argparse
import subprocess
import sys

CC = 'F:/tools/cutechess/cutechess-1.3.1-win64/cutechess-cli.exe'
BOOK = 'F:/tools/openings.epd'
SF = 'f:/tools/stockfish/stockfish/stockfish-windows-x86-64-avx2.exe'
CVS = 'target/release/uci.exe'

BASE_W = 'f:/Github/chess-vision-studio/arena/out/value-weights-mixed.json'
RUNG2_W = 'f:/Github/chess-vision-studio/arena/out/rung2-weights-mixed.json'

def main():
    parser = argparse.ArgumentParser(description="Run CVS against native Stockfish Elo Gauntlet")
    parser.add_argument('--elo', type=int, default=2400, help="Stockfish Elo limit (default: 2400)")
    parser.add_argument('--games', type=int, default=40, help="Number of games to play (default: 40)")
    parser.add_argument('--tc', default='10+0.1', help="Time control (default: 10+0.1)")
    parser.add_argument('--conc', type=int, default=4, help="Concurrency / threads (default: 4)")
    parser.add_argument('--net', default='f:/Github/chess-vision-studio/arena/out/gen9-raw-h256-d20.json', help="Path to NNUE net")
    args = parser.parse_args()

    import os
    net_stem = os.path.splitext(os.path.basename(args.net))[0]
    pgn = f'f:/tools/cvs-{net_stem}-vs-sf{args.elo}.pgn'

    cmd = [
        CC,
        '-engine', 'name=CVS', f'cmd={CVS}',
        f'arg=--base', f'arg={BASE_W}',
        f'arg=--rung2', f'arg={RUNG2_W}',
        f'arg=--nnue', f'arg={args.net}',
        'arg=--futility',
        '-engine', 'name=Stockfish', f'cmd={SF}',
        'option.UCI_LimitStrength=true', f'option.UCI_Elo={args.elo}',
        '-each', 'proto=uci', f'tc={args.tc}',
        '-games', str(args.games),
        '-repeat',
        '-concurrency', str(args.conc),
        '-openings', f'file={BOOK}', 'format=epd', 'order=random',
        '-pgnout', pgn
    ]

    print(f"Running Cutechess Gauntlet: CVS vs Stockfish-{args.elo} ELO", file=sys.stderr)
    print(f"Games: {args.games} | TC: {args.tc} | Threads: {args.conc} | PGN: {pgn}", file=sys.stderr)
    print("=" * 60, file=sys.stderr)

    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    
    # Track score
    W = L = D = 0
    for line in p.stdout:
        print(line, end='', flush=True)
        # Parse cutechess scores if present
        if 'Score of CVS vs Stockfish' in line:
            # Score of CVS vs Stockfish: 3 - 2 - 1  [0.583] 6
            try:
                parts = line.split('Score of CVS vs Stockfish:')[1].split()
                W = int(parts[0])
                L = int(parts[2])
                D = int(parts[4])
            except Exception:
                pass

    p.wait()
    total = W + L + D
    if total > 0:
        pct = (W + D/2) / total * 100
        print("\n" + "=" * 60)
        print("FINAL RESULTS:")
        print(f"  CVS vs Stockfish-{args.elo}: +{W} -{L} ={D} ({total} games) {pct:.1f}%")
        print("=" * 60)
    else:
        print("\nNo games completed.")

if __name__ == '__main__':
    main()
