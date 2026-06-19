import json
import glob
import sys
import time
import argparse
import os

import numpy as np
import torch
import torch.nn as nn

REGISTRY_VERSION = 1
CORE_REGISTRY_HASH = "58cb4e1e461a607d"
FULL_REGISTRY_HASH = "25c15688f9f4ebba"

parser = argparse.ArgumentParser()
parser.add_argument('--data-dir', default='training/gen9/gen9-cvs')
parser.add_argument('--epochs', type=int, default=30)
parser.add_argument('--out', default='target/cvs-residual.json')
parser.add_argument('--ps-hidden', type=int, default=256)
parser.add_argument('--cvs-hidden', type=int, default=32)
parser.add_argument('--cvs-dim', type=int, default=104) # 104 for core, 168 for full
parser.add_argument('--rows', type=int, default=100000000)
parser.add_argument('--lr', type=float, default=1e-3)
args = parser.parse_args()

K = 256.0
LAMBDA = 0.6
BATCH = 16384
DEVICE = 'cuda' if torch.cuda.is_available() else 'cpu'
PS_INPUTS = 768

PIECE_IDX = {'P': 0, 'N': 1, 'B': 2, 'R': 3, 'Q': 4, 'K': 5,
             'p': 6, 'n': 7, 'b': 8, 'r': 9, 'q': 10, 'k': 11}

def encode_ps(fen):
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
    return idx, white

def load():
    ps_feats, cvs_feats, cps, ress = [], [], [], []
    t0 = time.time()
    files = sorted(glob.glob(os.path.join(args.data_dir, "*.jsonl")))
    
    loaded = 0
    for fpath in files:
        if loaded >= args.rows:
            break
        with open(fpath, encoding='utf8') as fd:
            for line in fd:
                if loaded >= args.rows:
                    break
                try:
                    j = json.loads(line)
                    ps_idx, white = encode_ps(j['fen'])
                    
                    cp = j['cp'] if white else -j['cp']
                    res = j['res'] if white else 1.0 - j['res']
                    
                    cvs_idx = []
                    for i in j['features']:
                        if not white:
                            fam, within = divmod(i, 8)
                            side, bucket = divmod(within, 4)
                            i = fam * 8 + (1 - side) * 4 + bucket
                        cvs_idx.append(i)
                        
                    ps_feats.append(ps_idx)
                    cvs_feats.append(cvs_idx)
                    cps.append(cp)
                    ress.append(res)
                    loaded += 1
                except Exception:
                    continue

    print(f'loaded {loaded} rows in {time.time()-t0:.0f}s', flush=True)
    
    maxlen_ps = max((len(f) for f in ps_feats), default=0)
    maxlen_cvs = max((len(f) for f in cvs_feats), default=0)
    
    F_ps = np.full((loaded, maxlen_ps), PS_INPUTS, dtype=np.int64)
    F_cvs = np.full((loaded, maxlen_cvs), args.cvs_dim, dtype=np.int64)
    
    for i, idx in enumerate(ps_feats):
        F_ps[i, :len(idx)] = idx
    for i, idx in enumerate(cvs_feats):
        F_cvs[i, :len(idx)] = idx
        
    return F_ps, F_cvs, np.array(cps, dtype=np.float32), np.array(ress, dtype=np.float32)

class ResidualNet(nn.Module):
    def __init__(self):
        super().__init__()
        self.ps_embed = nn.EmbeddingBag(PS_INPUTS + 1, args.ps_hidden, mode='sum', padding_idx=PS_INPUTS)
        self.ps_b1 = nn.Parameter(torch.zeros(args.ps_hidden))
        self.ps_out = nn.Linear(args.ps_hidden, 1)

        self.cvs_embed = nn.EmbeddingBag(args.cvs_dim + 1, args.cvs_hidden, mode='sum', padding_idx=args.cvs_dim)
        self.cvs_b1 = nn.Parameter(torch.zeros(args.cvs_hidden))
        self.cvs_out = nn.Linear(args.cvs_hidden, 1)

        nn.init.normal_(self.ps_embed.weight, std=0.05)
        self.ps_embed.weight.data[PS_INPUTS].zero_()
        
        # Initialize cvs branch near zero so it acts as a residual
        nn.init.normal_(self.cvs_embed.weight, std=0.001)
        self.cvs_embed.weight.data[args.cvs_dim].zero_()
        nn.init.constant_(self.cvs_out.weight, 0.0)
        nn.init.constant_(self.cvs_out.bias, 0.0)

    def forward(self, ps_f, cvs_f):
        ps_h = torch.clamp(self.ps_embed(ps_f) + self.ps_b1, 0.0, 1.0)
        ps_score = self.ps_out(ps_h).squeeze(-1)
        
        cvs_h = torch.clamp(self.cvs_embed(cvs_f) + self.cvs_b1, 0.0, 1.0)
        cvs_score = self.cvs_out(cvs_h).squeeze(-1)
        
        return (ps_score + cvs_score) * 400.0

def main():
    F_ps, F_cvs, cps, ress = load()
    n = len(F_ps)
    if n == 0:
        print("No data loaded!")
        return

    hold = (np.arange(n) % 50) == 7
    tr_idx = np.flatnonzero(~hold)
    ho_idx = torch.from_numpy(np.flatnonzero(hold))
    
    Ft_ps = torch.from_numpy(F_ps)
    Ft_cvs = torch.from_numpy(F_cvs)
    target = LAMBDA * torch.sigmoid(torch.from_numpy(cps) / K) + (1 - LAMBDA) * torch.from_numpy(ress)

    net = ResidualNet().to(DEVICE)
    opt = torch.optim.Adam(net.parameters(), lr=args.lr)

    def holdout_loss():
        net.eval()
        with torch.no_grad():
            losses = []
            for c in range(0, len(ho_idx), BATCH):
                b = ho_idx[c:c + BATCH]
                pred = torch.sigmoid(net(Ft_ps[b].to(DEVICE), Ft_cvs[b].to(DEVICE)) / K)
                losses.append(((pred - target[b].to(DEVICE)) ** 2).mean().item())
        net.train()
        return float(np.mean(losses))

    print(f'train {len(tr_idx)} / holdout {len(ho_idx)}  device={DEVICE}', flush=True)
    for ep in range(args.epochs):
        np.random.shuffle(tr_idx)
        t0 = time.time()
        tot = nb = 0
        for c in range(0, len(tr_idx), BATCH):
            b = torch.from_numpy(tr_idx[c:c + BATCH])
            pred = torch.sigmoid(net(Ft_ps[b].to(DEVICE), Ft_cvs[b].to(DEVICE)) / K)
            loss = ((pred - target[b].to(DEVICE)) ** 2).mean()
            opt.zero_grad()
            loss.backward()
            opt.step()
            tot += loss.item()
            nb += 1
        print(f'epoch {ep:2d}  train {tot/nb:.6f}  holdout {holdout_loss():.6f}  ({time.time()-t0:.0f}s)', flush=True)

    # Export weights
    ps_w1 = net.ps_embed.weight.detach().cpu().numpy()[:PS_INPUTS]
    cvs_w1 = net.cvs_embed.weight.detach().cpu().numpy()[:args.cvs_dim]
    
    out_dir = os.path.dirname(args.out)
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)
        
    json.dump({
        'modelKind': 'cvs_residual_nnue', 
        'registryVersion': REGISTRY_VERSION,
        'registryHash': CORE_REGISTRY_HASH if args.cvs_dim == 104 else FULL_REGISTRY_HASH,
        'arch': f'PS({PS_INPUTS}x{args.ps_hidden}cReLU) + CVS({args.cvs_dim}x{args.cvs_hidden}cReLU)',
        'psInputs': PS_INPUTS, 'psHidden': args.ps_hidden,
        'cvsDim': args.cvs_dim, 'cvsHidden': args.cvs_hidden,
        'outputScaleCp': 400.0,
        'rows': int(n), 'epochs': args.epochs,
        'k': K, 'lambda': LAMBDA,
        'ps_w1': [[round(float(v), 6) for v in row] for row in ps_w1],
        'ps_b1': [round(float(v), 6) for v in net.ps_b1.detach().cpu().numpy()],
        'ps_w2': [round(float(v), 6) for v in net.ps_out.weight.detach().cpu().numpy()[0]],
        'cvs_w1': [[round(float(v), 6) for v in row] for row in cvs_w1],
        'cvs_b1': [round(float(v), 6) for v in net.cvs_b1.detach().cpu().numpy()],
        'cvs_w2': [round(float(v), 6) for v in net.cvs_out.weight.detach().cpu().numpy()[0]],
        'b2': float(net.ps_out.bias.detach().cpu().numpy()[0] + net.cvs_out.bias.detach().cpu().numpy()[0]),
        'cvsStmRelative': True,
    }, open(args.out, 'w'))
    print(f'wrote {args.out}')

if __name__ == '__main__':
    main()
