import json
import glob
import sys
import time
import argparse
import os

import numpy as np
import torch
import torch.nn as nn

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from source_split_guard import seed_everything  # noqa: E402  (#13 deterministic seeding)

parser = argparse.ArgumentParser()
parser.add_argument('--data-dir', default='training/gen9/gen9-cvs')
parser.add_argument('--epochs', type=int, default=30)
parser.add_argument('--out-dir', default='target-cvs')
parser.add_argument('--ps-hidden', type=int, default=256)
parser.add_argument('--cvs-hidden', type=int, default=32)
parser.add_argument('--cvs-dim', type=int, default=104) # 104 for core
parser.add_argument('--rows', type=int, default=100000000)
parser.add_argument('--lr', type=float, default=1e-3)
parser.add_argument('--seed', type=int, default=0)           # #13: deterministic, reproducible
parser.add_argument('--deterministic', action='store_true')  # also pin cuDNN determinism
parser.add_argument('--allow-unsafe-row-split', action='store_true',
                    help='dev-only: accept the source-leaking %% 50 row holdout; NON-PROMOTABLE')
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
    F_flat = np.full((loaded, maxlen_ps + maxlen_cvs), PS_INPUTS + args.cvs_dim, dtype=np.int64)
    
    for i in range(loaded):
        ps_l = len(ps_feats[i])
        cvs_l = len(cvs_feats[i])
        F_ps[i, :ps_l] = ps_feats[i]
        F_cvs[i, :cvs_l] = cvs_feats[i]
        
        # Flat features: PS features, then CVS features shifted by PS_INPUTS
        F_flat[i, :ps_l] = ps_feats[i]
        F_flat[i, ps_l:ps_l+cvs_l] = [x + PS_INPUTS for x in cvs_feats[i]]
        
    return F_ps, F_cvs, F_flat, np.array(cps, dtype=np.float32), np.array(ress, dtype=np.float32)

class RawNet(nn.Module):
    def __init__(self):
        super().__init__()
        self.embed = nn.EmbeddingBag(PS_INPUTS + 1, args.ps_hidden, mode='sum', padding_idx=PS_INPUTS)
        self.b1 = nn.Parameter(torch.zeros(args.ps_hidden))
        self.out = nn.Linear(args.ps_hidden, 1)
        nn.init.normal_(self.embed.weight, std=0.05)
        self.embed.weight.data[PS_INPUTS].zero_()

    def forward(self, ps_f, cvs_f, flat_f):
        h = torch.clamp(self.embed(ps_f) + self.b1, 0.0, 1.0)
        return self.out(h).squeeze(-1) * 400.0

class FlatCvsNet(nn.Module):
    def __init__(self):
        super().__init__()
        tot_inputs = PS_INPUTS + args.cvs_dim
        self.embed = nn.EmbeddingBag(tot_inputs + 1, args.ps_hidden, mode='sum', padding_idx=tot_inputs)
        self.b1 = nn.Parameter(torch.zeros(args.ps_hidden))
        self.out = nn.Linear(args.ps_hidden, 1)
        nn.init.normal_(self.embed.weight, std=0.05)
        self.embed.weight.data[tot_inputs].zero_()

    def forward(self, ps_f, cvs_f, flat_f):
        h = torch.clamp(self.embed(flat_f) + self.b1, 0.0, 1.0)
        return self.out(h).squeeze(-1) * 400.0

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
        
        nn.init.normal_(self.cvs_embed.weight, std=0.001)
        self.cvs_embed.weight.data[args.cvs_dim].zero_()
        nn.init.constant_(self.cvs_out.weight, 0.0)
        nn.init.constant_(self.cvs_out.bias, 0.0)

    def forward(self, ps_f, cvs_f, flat_f):
        ps_h = torch.clamp(self.ps_embed(ps_f) + self.ps_b1, 0.0, 1.0)
        ps_score = self.ps_out(ps_h).squeeze(-1)
        
        cvs_h = torch.clamp(self.cvs_embed(cvs_f) + self.cvs_b1, 0.0, 1.0)
        cvs_score = self.cvs_out(cvs_h).squeeze(-1)
        
        return (ps_score + cvs_score) * 400.0

def main():
    print(f'seed: {seed_everything(args.seed, deterministic=args.deterministic)}', flush=True)
    if not args.allow_unsafe_row_split:
        raise SystemExit(
            'REFUSED: this gen9 trainer slice uses a single unsplit corpus with a row-level (% 50) '
            'holdout that LEAKS sibling positions from the same game across train/holdout — its '
            'artifact is NOT promotion-eligible. Pass --allow-unsafe-row-split for a clearly '
            'NON-PROMOTABLE dev run, or use the verified --train/--validation split slice '
            '(pre-flight gate: python training/gen9/scripts/verify-split.py).')
    F_ps, F_cvs, F_flat, cps, ress = load()
    n = len(F_ps)
    if n == 0:
        print("No data loaded!")
        return

    hold = (np.arange(n) % 50) == 7
    tr_idx = np.flatnonzero(~hold)
    ho_idx = torch.from_numpy(np.flatnonzero(hold))
    
    Ft_ps = torch.from_numpy(F_ps)
    Ft_cvs = torch.from_numpy(F_cvs)
    Ft_flat = torch.from_numpy(F_flat)
    target = LAMBDA * torch.sigmoid(torch.from_numpy(cps) / K) + (1 - LAMBDA) * torch.from_numpy(ress)

    nets = {
        'raw': RawNet().to(DEVICE),
        'flat': FlatCvsNet().to(DEVICE),
        'res': ResidualNet().to(DEVICE)
    }
    opts = {k: torch.optim.Adam(v.parameters(), lr=args.lr) for k, v in nets.items()}

    def holdout_loss(name):
        net = nets[name]
        net.eval()
        with torch.no_grad():
            losses = []
            for c in range(0, len(ho_idx), BATCH):
                b = ho_idx[c:c + BATCH]
                pred = torch.sigmoid(net(Ft_ps[b].to(DEVICE), Ft_cvs[b].to(DEVICE), Ft_flat[b].to(DEVICE)) / K)
                losses.append(((pred - target[b].to(DEVICE)) ** 2).mean().item())
        net.train()
        return float(np.mean(losses))

    print(f'train {len(tr_idx)} / holdout {len(ho_idx)}  device={DEVICE}', flush=True)
    for ep in range(args.epochs):
        np.random.shuffle(tr_idx)
        t0 = time.time()
        
        tot = {k: 0 for k in nets}
        nb = 0
        for c in range(0, len(tr_idx), BATCH):
            b = torch.from_numpy(tr_idx[c:c + BATCH])
            t_b = target[b].to(DEVICE)
            
            fps_b = Ft_ps[b].to(DEVICE)
            fcvs_b = Ft_cvs[b].to(DEVICE)
            fflat_b = Ft_flat[b].to(DEVICE)
            
            for name, net in nets.items():
                pred = torch.sigmoid(net(fps_b, fcvs_b, fflat_b) / K)
                loss = ((pred - t_b) ** 2).mean()
                opts[name].zero_grad()
                loss.backward()
                opts[name].step()
                tot[name] += loss.item()
                
            nb += 1
            
        print(f'epoch {ep:2d} ({time.time()-t0:.0f}s)', flush=True)
        for name in nets:
            print(f'  {name:4s} | trn: {tot[name]/nb:.6f} | val: {holdout_loss(name):.6f}')

    os.makedirs(args.out_dir, exist_ok=True)
    
    # Export raw
    net = nets['raw']
    w1 = net.embed.weight.detach().cpu().numpy()[:PS_INPUTS]
    json.dump({
        'modelKind': 'nnue', 'arch': f'{PS_INPUTS}x{args.ps_hidden}cReLU-1',
        'psInputs': PS_INPUTS, 'hidden': args.ps_hidden, 'outputScaleCp': 400.0,
        'w1': [[round(float(v), 6) for v in row] for row in w1],
        'b1': [round(float(v), 6) for v in net.b1.detach().cpu().numpy()],
        'w2': [round(float(v), 6) for v in net.out.weight.detach().cpu().numpy()[0]],
        'b2': float(net.out.bias.detach().cpu().numpy()[0]),
    }, open(f'{args.out_dir}/matrix-raw.json', 'w'))

    # Export flat cvs
    net = nets['flat']
    tot_inputs = PS_INPUTS + args.cvs_dim
    w1 = net.embed.weight.detach().cpu().numpy()[:tot_inputs]
    json.dump({
        'modelKind': 'cvs_nnue', 'arch': f'{tot_inputs}x{args.ps_hidden}cReLU-1',
        'psInputs': PS_INPUTS, 'cvsDim': args.cvs_dim, 'hidden': args.ps_hidden, 'outputScaleCp': 400.0,
        'w1': [[round(float(v), 6) for v in row] for row in w1],
        'b1': [round(float(v), 6) for v in net.b1.detach().cpu().numpy()],
        'w2': [round(float(v), 6) for v in net.out.weight.detach().cpu().numpy()[0]],
        'b2': float(net.out.bias.detach().cpu().numpy()[0]),
        'cvsStmRelative': True,
    }, open(f'{args.out_dir}/matrix-flat.json', 'w'))

    # Export residual cvs
    net = nets['res']
    ps_w1 = net.ps_embed.weight.detach().cpu().numpy()[:PS_INPUTS]
    cvs_w1 = net.cvs_embed.weight.detach().cpu().numpy()[:args.cvs_dim]
    json.dump({
        'modelKind': 'cvs_residual_nnue', 
        'arch': f'PS({PS_INPUTS}x{args.ps_hidden}cReLU) + CVS({args.cvs_dim}x{args.cvs_hidden}cReLU)',
        'psInputs': PS_INPUTS, 'psHidden': args.ps_hidden,
        'cvsDim': args.cvs_dim, 'cvsHidden': args.cvs_hidden,
        'outputScaleCp': 400.0,
        'ps_w1': [[round(float(v), 6) for v in row] for row in ps_w1],
        'ps_b1': [round(float(v), 6) for v in net.ps_b1.detach().cpu().numpy()],
        'ps_w2': [round(float(v), 6) for v in net.ps_out.weight.detach().cpu().numpy()[0]],
        'cvs_w1': [[round(float(v), 6) for v in row] for row in cvs_w1],
        'cvs_b1': [round(float(v), 6) for v in net.cvs_b1.detach().cpu().numpy()],
        'cvs_w2': [round(float(v), 6) for v in net.cvs_out.weight.detach().cpu().numpy()[0]],
        'b2': float(net.ps_out.bias.detach().cpu().numpy()[0] + net.cvs_out.bias.detach().cpu().numpy()[0]),
        'cvsStmRelative': True,
    }, open(f'{args.out_dir}/matrix-residual.json', 'w'))
    print(f'wrote matrix models to {args.out_dir}/')

if __name__ == '__main__':
    main()
