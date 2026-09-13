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
    def __init__(self, hidden: int, b1_init: float = 0.0, out_init: float = 0.05,
                 embed_init: float = 0.05, head2: int = 0):
        super().__init__()
        self.embed = nn.Embedding(PS_INPUTS + 1, hidden, padding_idx=PS_INPUTS)
        # Symmetry-breaking init: with a zeroed embedding every input feature is
        # identical and the net can only ever learn a constant (observed: a net
        # that output ~0 regardless of position). Matches the proven recipe.
        nn.init.normal_(self.embed.weight, std=embed_init)
        self.b1 = nn.Parameter(torch.full((hidden,), b1_init))
        self.center = False
        # Optional second head layer: features -> h1 -> h2 -> out. Extra capacity is what
        # a LINEAR centipawn target needs to rival the compressed target's accuracy.
        self.head2 = head2
        self.hidden_dim = hidden
        if head2:
            self.h2w = nn.Parameter(torch.randn(head2, hidden) * (1.0 / hidden ** 0.5))
            self.h2b = nn.Parameter(torch.zeros(head2))
            self.h2out = nn.Parameter(torch.randn(1, head2) * (1.0 / head2 ** 0.5))
            self.h2out_b = nn.Parameter(torch.zeros(1))
        self.out = nn.Linear(hidden, 1)
        nn.init.normal_(self.out.weight, std=out_init)
        nn.init.zeros_(self.out.bias)

    def forward(self, idx):
        acc = self.embed(idx).sum(dim=1) + self.b1
        h = acc.clamp(0.0, 1.0)
        if self.center:
            h = h - 0.5
        if self.head2:
            z = torch.nn.functional.linear(h, self.h2w, self.h2b).clamp(0.0, 1.0)
            return torch.nn.functional.linear(z, self.h2out, self.h2out_b).squeeze(-1)
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
    ap.add_argument("--huber-delta", type=float, default=0.4,
                    help="Huber delta in target units. Large values are effectively MSE, "
                         "which is what the proven pipeline used: a small delta caps the "
                         "gradient on large residuals and the net never fits the extremes")
    ap.add_argument("--target-mode", choices=("sigmoid", "sigmoid-mid", "linear"), default="sigmoid",
                    help="sigmoid: target = sigmoid(cp / --sigmoid-k), the proven recipe for "
                         "this [0,1]-clamped hidden layer (compressive; undo with --nnue-cal). "
                         "linear: target = cp / --target-scale (needs a differently-scaled "
                         "architecture to be trainable from a cold start).")
    ap.add_argument("--sigmoid-k", type=float, default=256.0)
    ap.add_argument("--target-scale", type=float, default=400.0,
                    help="train target = cp / target-scale; exported as outputScaleCp. "
                         "The hidden layer clamps to [0,1], so the net's raw output range is "
                         "O(1) -- a LINEAR target at this scale keeps centipawns linear "
                         "(the old sigmoid(cp/256) target is what compressed the output).")
    ap.add_argument("--embed-init", type=float, default=0.05,
                    help="input embedding std. Small values make every hidden unit nearly "
                         "identical across positions, so the output layer receives no "
                         "learning signal and the net sits at the target mean (observed with "
                         "linear targets). Larger values start the layer position-sensitive.")
    ap.add_argument("--b1-init", type=float, default=0.0,
                    help="hidden bias init; ~0.5 keeps clamp(acc,0,1) units responsive at "
                         "t=0, which is what a linear (unbounded-range) target needs")
    ap.add_argument("--out-init", type=float, default=0.05, help="std for the output layer init")
    ap.add_argument("--init-from", default=None,
                    help="warm-start the model from an exported net (first layer + head), "
                         "then optionally freeze the feature layer (--freeze-features) to fit "
                         "only a linear output head. A linear cp target has zero gradient at "
                         "init (the output already equals the target mean); fitting the head "
                         "on top of good frozen features is a well-posed regression instead.")
    ap.add_argument("--feature-lr-scale", type=float, default=1.0,
                    help="feature-layer LR as a fraction of the head's. Joint fine-tuning of a "
                         "well-fit head with a shared LR destabilises it (observed: 114 -> 262 "
                         "MAE); the head needs to move ~10x faster than the features it reads")
    ap.add_argument("--head2", type=int, default=0,
                    help="second head layer width (0 = the shipped single-layer head). A linear "
                         "centipawn target needs more head capacity than 256->1 to match a "
                         "compressed target's accuracy; this tests that in the trainer before "
                         "any engine inference change")
    ap.add_argument("--freeze-features", action="store_true",
                    help="train only the output layer (w2/b2)")
    ap.add_argument("--center-hidden", action="store_true",
                    help="use h = clamp(acc,0,1) - 0.5 (signed hidden features). The export "
                         "folds -0.5*sum(w2) into b2, so the engine's CURRENT inference "
                         "reproduces it exactly -- no engine change needed. Rationale: with "
                         "unsigned features the output layer sees no position-varying signal "
                         "while units sit at the clamp floor, which is what makes a linear "
                         "centipawn target collapse to the mean.")
    ap.add_argument("--label-field", default="cp", choices=("cp", "cpStatic"),
                    help="which corpus field is the training label (cpStatic = SF's static eval)")
    ap.add_argument("--stride", type=int, default=1,
                    help="take every Nth row (subsample a multi-million-row corpus in place)")
    ap.add_argument("--files", default=None,
                    help="glob of jsonl corpus files (default: read --data as one file)")
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)

    t0 = time.time()
    import glob as _glob
    import math as _math
    import numpy as _np

    PAD = PS_INPUTS  # embedding index for padding (zeroed)

    files = sorted(_glob.glob(a.files)) if a.files else [a.data]
    rows = []  # list of (features: list[int], target: float)
    kept = seen = 0
    for f in files:
        with open(f, encoding="utf-8") as fd:
            for line in fd:
                seen += 1
                if a.stride > 1 and seen % a.stride:
                    continue
                j = json.loads(line)
                cp = j.get(a.label_field)
                if cp is None or abs(cp) > a.clamp:
                    continue
                if j["fen"].split()[1] == "b":
                    cp = -cp
                if a.target_mode == "sigmoid-mid":
                    target = 2.56 * (1.0 / (1.0 + _math.exp(-cp / a.sigmoid_k)) - 0.5)
                elif a.target_mode == "sigmoid":
                    target = 1.0 / (1.0 + _math.exp(-cp / a.sigmoid_k))
                else:
                    target = cp / a.target_scale
                feat = encode_ps(j["fen"])
                if len(feat) > 32:
                    continue  # malformed FEN with >32 pieces
                rows.append((feat, target))
                kept += 1

    print(f"kept {kept} of {seen} rows in {time.time()-t0:.0f}s")

    N = len(rows)
    idx = torch.full((N, 32), PAD, dtype=torch.long)
    target = torch.zeros(N, dtype=torch.float32)
    for i, (feat, tgt) in enumerate(rows):
        for j, v in enumerate(feat):
            idx[i, j] = v
        target[i] = tgt
    del rows
    print(f"built dense tensor [{N}, 32] in {time.time()-t0:.0f}s")

    dev = 'cuda' if torch.cuda.is_available() else 'cpu'
    rng = np.random.default_rng(a.seed)
    ho_mask = torch.from_numpy(rng.random(N) < a.holdout_frac)
    tr_mask = ~ho_mask

    net = RawNet(a.hidden, a.b1_init, a.out_init, a.embed_init, a.head2).to(dev)
    if a.init_from:
        src = json.load(open(a.init_from, encoding="utf-8"))
        with torch.no_grad():
            net.embed.weight[:PS_INPUTS].copy_(
                torch.tensor(src["w1"], dtype=torch.float32).reshape(PS_INPUTS, a.hidden))
            net.embed.weight[PS_INPUTS:].zero_()
            net.b1.copy_(torch.tensor(src["b1"], dtype=torch.float32))
            net.out.weight.copy_(torch.tensor(src["w2"], dtype=torch.float32).unsqueeze(0))
            net.out.bias.fill_(float(src["b2"]))
        if a.head2:
            h2 = a.head2
            if h2 != a.hidden:
                raise SystemExit("--head2 must equal --hidden for the identity init")
            with torch.no_grad():
                net.h2w.zero_(); net.h2w[:, :a.hidden] = torch.eye(h2, a.hidden)
                net.h2b.zero_()
                net.h2out.zero_(); net.h2out[0, :a.hidden] = net.out.weight[0]
                net.h2out_b.fill_(float(net.out.bias[0]))
        print(f"warm-started from {a.init_from} (hidden {src.get('hidden')})")
    if a.freeze_features:
        for q in (net.embed.weight, net.b1):
            q.requires_grad_(False)
        print("feature layer frozen: fitting the output head only")
    net.center = a.center_hidden

    feat_params = [net.embed.weight, net.b1]
    head_params = [net.out.weight, net.out.bias]
    groups = [{"params": head_params, "lr": a.lr}]
    if not a.freeze_features:
        groups.append({"params": feat_params, "lr": a.lr * a.feature_lr_scale})
    opt = torch.optim.Adam(groups, foreach=False)
    lossf = nn.HuberLoss(delta=a.huber_delta)

    tr_idx = idx[tr_mask].to(dev)
    tr_y = target[tr_mask].to(dev)
    ho_idx = idx[ho_mask].to(dev)
    ho_y = target[ho_mask].to(dev)
    print(f"device {dev}  train {tr_y.numel()}  holdout {ho_y.numel()}")

    def evaluate(ix, yy):
        net.eval()
        errs = []
        with torch.no_grad():
            for s in range(0, ix.shape[0], 65536):
                e = min(s + 65536, ix.shape[0])
                pred = net(ix[s:e])
                errs.append((pred.cpu() - yy[s:e].cpu()).abs() * a.target_scale)
        net.train()
        return torch.cat(errs).mean().item()

    nb = max(1, tr_idx.shape[0] // a.batch)
    for ep in range(1, a.epochs + 1):
        perm = torch.randperm(tr_idx.shape[0])
        tot = 0.0
        for b in range(nb):
            sel = perm[b * a.batch:(b + 1) * a.batch]
            batch_idx = tr_idx[sel]
            pred = net(batch_idx)
            loss = lossf(pred, tr_y[sel])
            opt.zero_grad(); loss.backward(); opt.step()
            tot += loss.item()
        if ep % 2 == 0 or ep == a.epochs:
            print(f"epoch {ep:3d} | train huber {tot/nb:.5f} | holdout MAE {evaluate(ho_idx, ho_y):6.1f}cp "
                  f"({time.time()-t0:.0f}s)", flush=True)

    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    w1 = net.embed.weight.detach().cpu().numpy()[:PS_INPUTS]
    json.dump({
        "modelKind": "nnue", "arch": f"{PS_INPUTS}x{a.hidden}cReLU-1(cp)",
        "psInputs": PS_INPUTS, "hidden": a.hidden, "outputScaleCp": float(a.target_scale),
        "w1": [[round(float(v), 6) for v in row] for row in w1],
        "b1": [round(float(v), 6) for v in net.b1.detach().cpu().numpy()],
        "w2": [round(float(v), 6) for v in net.out.weight.detach().cpu().numpy()[0]],
        "b2": float(net.out.bias.detach().cpu().numpy()[0]
                     - (0.5 * sum(net.out.weight.detach().cpu().numpy()[0]) if a.center_hidden else 0.0)),
    }, open(out, "w", encoding="utf-8"), separators=(",", ":"))
    print(f"wrote {out}")
    return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
