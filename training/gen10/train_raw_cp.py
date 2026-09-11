#!/usr/bin/env python3
"""Gen10 raw evaluator: 768 piece-square -> cReLU hidden -> cp, trained on cp directly.

Why this exists next to training/gen9/scripts/train_matrix.py: that trainer's target is

    target = 0.6 * sigmoid(cp / 256) + 0.4 * game_result      (outputScaleCp = 400)

which is a WDL-flavoured, sigmoid-compressed objective. Learning it makes the net's
raw output a *compressed, scale-wrong* estimate of centipawns: measured slope 0.49
against Stockfish's static eval, i.e. every centipawn threshold in the search was
applied at roughly double severity until the --nnue-cal calibration curve undid it.

This trainer predicts centipawns directly (Huber loss on cp/100, outputScaleCp=100),
so the engine's margins mean what they say without a calibration curve.

Data is built by training/gen10/build_corpus.py (quiet, deduped, d24-labelled,
stability-filtered).

Run:
  python training/gen10/train_raw_cp.py --data training/gen10/corpus/d24.jsonl \
      --out target-cvs/gen10-raw-cp.json
"""
from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn

PIECE_IDX = {'P': 0, 'N': 1, 'B': 2, 'R': 3, 'Q': 4, 'K': 5,
             'p': 6, 'n': 7, 'b': 8, 'r': 9, 'q': 10, 'k': 11}
PS_INPUTS = 768


def encode_ps(fen: str) -> list[int]:
    """Side-to-move perspective piece-square indices (the engine's input order):
    black to move mirrors vertically and swaps colours."""
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
    return idx


class RawNet(nn.Module):
    def __init__(self, hidden: int):
        super().__init__()
        self.embed = nn.EmbeddingBag(PS_INPUTS, hidden, mode='sum', include_last_offset=False)
        nn.init.zeros_(self.embed.weight)
        self.b1 = nn.Parameter(torch.zeros(hidden))
        self.out = nn.Linear(hidden, 1)

    def forward(self, idx, offsets):
        acc = self.embed(idx, offsets) + self.b1
        h = acc.clamp(0.0, 1.0)
        return self.out(h).squeeze(-1)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", default="training/gen10/corpus/d24.jsonl")
    ap.add_argument("--out", default="target-cvs/gen10-raw-cp.json")
    ap.add_argument("--hidden", type=int, default=256)
    ap.add_argument("--epochs", type=int, default=24)
    ap.add_argument("--batch", type=int, default=16384)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--holdout-frac", type=float, default=0.02)
    ap.add_argument("--clamp", type=int, default=1500)
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)

    t0 = time.time()
    fens, cps = [], []
    with open(a.data, encoding="utf-8") as fd:
        for line in fd:
            j = json.loads(line)
            fens.append(j["fen"])
            cps.append(max(-a.clamp, min(a.clamp, j["cp"])))
    n = len(fens)
    print(f"loaded {n} rows in {time.time()-t0:.0f}s")

    dev = 'cuda' if torch.cuda.is_available() else 'cpu'
    idx_list, off_list = [], []
    off = 0
    for f in fens:
        ii = encode_ps(f)
        idx_list.extend(ii)
        off_list.append(off)
        off += len(ii)
    idx = torch.tensor(idx_list, dtype=torch.long)
    offsets = torch.tensor(off_list, dtype=torch.long)
    target = torch.tensor([c / 100.0 for c in cps], dtype=torch.float32)

    rng = np.random.default_rng(a.seed)
    ho = rng.random(n) < a.holdout_frac
    ho_t = torch.tensor(ho)
    tr_t = ~ho_t

    net = RawNet(a.hidden).to(dev)
    opt = torch.optim.Adam(net.parameters(), lr=a.lr)
    lossf = nn.HuberLoss(delta=1.5)

    def idx_for(mask):
        return offsets[mask].to(dev), idx.to(dev), target[mask].to(dev)

    tr_off, tr_idx, tr_y = idx_for(tr_t)
    ho_off, ho_idx, ho_y = idx_for(ho_t)
    print(f"device {dev}  train {tr_y.numel()}  holdout {ho_y.numel()}")

    def evaluate(off_, idx_, y_):
        net.eval()
        errs = []
        with torch.no_grad():
            for s in range(0, len(off_), 65536):
                e = min(s + 65536, len(off_))
                off_b = off_[s:e].cpu()
                cnt = (off_b[1:] - off_b[:-1]) if e - s > 1 else None
                # rebuild a contiguous index slice for this batch
                lo = off_[s].item()
                hi = (off_[e].item() if e < len(off_) else idx_.numel())
                pred = net(idx_.cpu()[lo:hi].to(dev), off_b.to(dev) - lo)
                errs.append((pred.cpu() - y_[s:e].cpu()).abs() * 100.0)
        net.train()
        return torch.cat(errs).mean().item()

    nb = max(1, len(tr_off) // a.batch)
    for ep in range(1, a.epochs + 1):
        perm = torch.randperm(len(tr_off))
        tot = 0.0
        for b in range(nb):
            sel = perm[b * a.batch:(b + 1) * a.batch]
            batch_off = tr_off[sel].to(dev)
            lo = batch_off[0].item()
            hi = (tr_off[sel[-1]].item() + 1) if sel[-1].item() + 1 < len(tr_off) else idx.numel()
            pred = net(tr_idx[lo:hi].to(dev), batch_off - lo)
            loss = lossf(pred, tr_y[sel].to(dev))
            opt.zero_grad(); loss.backward(); opt.step()
            tot += loss.item()
        if ep % 2 == 0 or ep == a.epochs:
            print(f"epoch {ep:3d} | train huber {tot/nb:.5f} | holdout MAE {evaluate(ho_off, ho_idx, ho_y):6.1f}cp "
                  f"({time.time()-t0:.0f}s)", flush=True)

    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    w1 = net.embed.weight.detach().cpu().numpy()
    json.dump({
        "modelKind": "nnue", "arch": f"{PS_INPUTS}x{a.hidden}cReLU-1(cp)",
        "psInputs": PS_INPUTS, "hidden": a.hidden, "outputScaleCp": 100.0,
        "w1": [[round(float(v), 6) for v in row] for row in w1],
        "b1": [round(float(v), 6) for v in net.b1.detach().cpu().numpy()],
        "w2": [round(float(v), 6) for v in net.out.weight.detach().cpu().numpy()[0]],
        "b2": float(net.out.bias.detach().cpu().numpy()[0]),
    }, open(out, "w", encoding="utf-8"), separators=(",", ":"))
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
