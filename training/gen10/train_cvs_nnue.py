#!/usr/bin/env python3
"""CVS-NNUE trainer for the Railway d20 self-play corpus.

Input representation: 768 piece-square features (stm perspective) PLUS the
168 CVS registry-v1 geometry ids (offset +768, stm-relative side flip) —
the differentiator representation the raw 768 net cannot see.

Corpus rows (one JSONL, any number of shard files):
    {"fen", "cp" (white POV, SF d20), "res" (white POV), "features": [raw ids]}

Teacher blend (same convention as the gen9 CVS line):
    target = LAMBDA * sigmoid(cp / K) + (1 - LAMBDA) * res
Loss: MSE on sigmoid(pred / K). Holdout: every 50th row.

    python train_cvs_nnue.py corpus/*.jsonl --hidden 256 --epochs 30 --out net.json
"""
import argparse
import glob
import json
import subprocess
import time

import numpy as np
import torch
import torch.nn as nn

PS_INPUTS = 768
CVS_DIM = 168
INPUTS = PS_INPUTS + CVS_DIM
PIECE_IDX = {'P': 0, 'N': 1, 'B': 2, 'R': 3, 'Q': 4, 'K': 5,
             'p': 6, 'n': 7, 'b': 8, 'r': 9, 'q': 10, 'k': 11}
START_FEN = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1'


def registry_info(engine):
    """Ask the engine for the live CVS registry version/hash via serve."""
    p = subprocess.Popen([engine, '--serve', '--depth', '1'],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True)
    p.stdin.write(f'cvs {START_FEN}\n')
    p.stdin.flush()
    j = json.loads(p.stdout.readline())
    p.kill()
    assert j['inputDim'] == CVS_DIM, f"registry dim {j['inputDim']} != {CVS_DIM}"
    return j['registryVersion'], j['registryHash']


def encode_ps(fen, cp_white, res_white):
    """768 piece-square indices (stm perspective) + stm-POV targets."""
    board, stm = fen.split(' ')[0], fen.split(' ')[1]
    white = stm == 'w'
    idx = []
    sq = 56
    for ch in board:
        if ch == '/':
            sq -= 16
        elif ch.isdigit():
            sq += int(ch)
        else:
            p = PIECE_IDX[ch]
            s = sq
            if not white:
                p = (p + 6) % 12
                s = sq ^ 56
            idx.append(p * 64 + s)
            sq += 1
    cp = cp_white if white else -cp_white
    res = res_white if white else 1.0 - res_white
    return idx, cp, res


def load(paths, max_rows):
    feats, cps, ress = [], [], []
    t0 = time.time()
    for path in paths:
        with open(path, encoding='utf-8') as fd:
            for line in fd:
                if len(feats) >= max_rows:
                    break
                try:
                    j = json.loads(line)
                    idx, cp, res = encode_ps(j['fen'], j['cp'], j['res'])
                except Exception:
                    continue
                # stm-relative geometry: PS half is stm-mirrored, so the CVS
                # side bit flips too when black is to move.
                white = j['fen'].split(' ')[1] == 'w'
                for x in j.get('features') or []:
                    i = int(x)
                    if not white:
                        fam, within = divmod(i, 8)
                        side, bucket = divmod(within, 4)
                        i = fam * 8 + (1 - side) * 4 + bucket
                    idx.append(PS_INPUTS + i)
                feats.append(idx)
                cps.append(cp)
                ress.append(res)
        if len(feats) >= max_rows:
            break
    n = len(feats)
    maxlen = max(len(f) for f in feats)
    F = np.full((n, maxlen), INPUTS, dtype=np.int64)  # pad = INPUTS
    for i, idx in enumerate(feats):
        F[i, :len(idx)] = idx
    print(f'loaded {n} rows (maxlen {maxlen}) in {time.time()-t0:.0f}s', flush=True)
    return F, np.array(cps, dtype=np.float32), np.array(ress, dtype=np.float32)


class Net(nn.Module):
    def __init__(self, hidden):
        super().__init__()
        self.embed = nn.EmbeddingBag(INPUTS + 1, hidden, mode='sum', padding_idx=INPUTS)
        self.b1 = nn.Parameter(torch.zeros(hidden))
        self.out = nn.Linear(hidden, 1)
        nn.init.normal_(self.embed.weight, std=0.05)
        self.embed.weight.data[INPUTS].zero_()

    def forward(self, f):
        h = torch.clamp(self.embed(f) + self.b1, 0.0, 1.0)
        return self.out(h).squeeze(-1) * 400.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('corpus', nargs='+')
    ap.add_argument('--rows', type=int, default=100_000_000)
    ap.add_argument('--epochs', type=int, default=30)
    ap.add_argument('--hidden', type=int, default=256)
    ap.add_argument('--out', default='training/gen10/out/cvs-nnue.json')
    ap.add_argument('--engine', default='target/release/analyze.exe')
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--batch', type=int, default=16384)
    ap.add_argument('--lambda-v', dest='lam', type=float, default=0.6)
    ap.add_argument('--k', type=float, default=256.0)
    a = ap.parse_args()

    paths = []
    for pat in a.corpus:
        paths.extend(sorted(glob.glob(pat)))
    reg_ver, reg_hash = registry_info(a.engine)
    print(f'registry v{reg_ver} hash {reg_hash}', flush=True)

    F, cps, ress = load(paths, a.rows)
    n = len(F)
    hold = (np.arange(n) % 50) == 7
    tr_idx = np.flatnonzero(~hold)
    ho_idx = torch.from_numpy(np.flatnonzero(hold))
    Ft = torch.from_numpy(F)
    target = a.lam * torch.sigmoid(torch.from_numpy(cps) / a.k) \
        + (1 - a.lam) * torch.from_numpy(ress)

    device = 'cuda' if torch.cuda.is_available() else 'cpu'
    net = Net(a.hidden).to(device)
    opt = torch.optim.Adam(net.parameters(), lr=a.lr)

    def holdout_loss():
        net.eval()
        with torch.no_grad():
            losses = []
            for c in range(0, len(ho_idx), a.batch):
                b = ho_idx[c:c + a.batch]
                pred = torch.sigmoid(net(Ft[b].to(device)) / a.k)
                losses.append(((pred - target[b].to(device)) ** 2).mean().item())
        net.train()
        return float(np.mean(losses))

    print(f'train {len(tr_idx)} / holdout {len(ho_idx)}  device={device}  '
          f'inputs={INPUTS} hidden={a.hidden}', flush=True)
    for ep in range(a.epochs):
        np.random.shuffle(tr_idx)
        t0 = time.time()
        tot = nb = 0
        for c in range(0, len(tr_idx), a.batch):
            b = torch.from_numpy(tr_idx[c:c + a.batch])
            pred = torch.sigmoid(net(Ft[b].to(device)) / a.k)
            loss = ((pred - target[b].to(device)) ** 2).mean()
            opt.zero_grad()
            loss.backward()
            opt.step()
            tot += loss.item()
            nb += 1
        print(f'epoch {ep:2d}  train {tot/nb:.6f}  holdout {holdout_loss():.6f}  '
              f'({time.time()-t0:.0f}s)', flush=True)

    w1 = net.embed.weight.detach().cpu().numpy()[:INPUTS]
    import os
    os.makedirs(os.path.dirname(a.out) or '.', exist_ok=True)
    json.dump({
        'modelKind': 'cvs_nnue', 'arch': f'{INPUTS}x{a.hidden}cReLU-1',
        'inputCount': INPUTS, 'psInputs': PS_INPUTS, 'cvsDim': CVS_DIM,
        'hidden': a.hidden, 'outputScaleCp': 400.0,
        'registryVersion': reg_ver, 'registryHash': reg_hash,
        'teacher': 'stockfish_d20', 'rows': int(n), 'epochs': a.epochs,
        'k': a.k, 'lambda': a.lam,
        'w1': [[round(float(v), 6) for v in row] for row in w1],
        'b1': [round(float(v), 6) for v in net.b1.detach().cpu().numpy()],
        'w2': [round(float(v), 6) for v in net.out.weight.detach().cpu().numpy()[0]],
        'b2': float(net.out.bias.detach().cpu().numpy()[0]),
        'note': 'stm-POV piece-square + STM-RELATIVE CVS registry-v1 ids (+768); '
                'trained on Railway self-play corpus with SF d20 labels',
        'cvsStmRelative': True,
    }, open(a.out, 'w'))
    print(f'wrote {a.out}', flush=True)


if __name__ == '__main__':
    main()
