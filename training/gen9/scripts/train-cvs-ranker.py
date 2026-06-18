#!/usr/bin/env python3
"""Train Quiet-Hybrid-B Ranker using Pairwise RankNet Loss in PyTorch."""

import argparse
import json
import os
import random
import time
from pathlib import Path
import numpy as np
import torch
import torch.nn as nn

parser = argparse.ArgumentParser()
parser.add_argument('--data', default='training/gen9/gen9-cvs-deltas.jsonl')
parser.add_argument('--epochs', type=int, default=15)
parser.add_argument('--out', default='target-cvs/matrix-ranker.json')
parser.add_argument('--cvs-hidden', type=int, default=32)
parser.add_argument('--lr', type=float, default=1e-3)
parser.add_argument('--batch-size', type=int, default=4096)
parser.add_argument('--pairs-per-pos', type=int, default=10)
args = parser.parse_args()

DEVICE = 'cuda' if torch.cuda.is_available() else 'cpu'
REGISTRY_HASH = "25c15688f9f4ebba"


def load_dataset(fpath, max_positions=100000):
    positions = []
    t0 = time.time()
    print(f"Loading dataset from {fpath}...")
    with open(fpath, 'r', encoding='utf-8') as f:
        for line in f:
            if not line.strip():
                continue
            try:
                data = json.loads(line)
                moves = data["moves"]
                if len(moves) < 2:
                    continue
                positions.append(moves)
                if len(positions) >= max_positions:
                    break
            except Exception:
                continue
    print(f"Loaded {len(positions)} positions in {time.time()-t0:.1f}s")
    return positions


def make_pairs(positions, pairs_per_pos=10):
    pairs = []
    for moves in positions:
        n_moves = len(moves)
        valid_pairs = []
        for i in range(n_moves):
            for j in range(n_moves):
                if moves[i]["rawScore"] > moves[j]["rawScore"]:
                    valid_pairs.append((
                        moves[i]["sparse"],
                        moves[i]["dense"],
                        moves[j]["sparse"],
                        moves[j]["dense"]
                    ))
        if len(valid_pairs) > pairs_per_pos:
            valid_pairs = random.sample(valid_pairs, pairs_per_pos)
        pairs.extend(valid_pairs)
    return pairs


def collate_fn(batch):
    max_len = max(max(len(x[0]), len(x[2])) for x in batch)
    
    sparse_i = np.full((len(batch), max_len), 504, dtype=np.int64)
    dense_i = np.zeros((len(batch), 32), dtype=np.float32)
    
    sparse_j = np.full((len(batch), max_len), 504, dtype=np.int64)
    dense_j = np.zeros((len(batch), 32), dtype=np.float32)
    
    for idx, (sp_i, de_i, sp_j, de_j) in enumerate(batch):
        sparse_i[idx, :len(sp_i)] = sp_i
        dense_i[idx] = de_i
        sparse_j[idx, :len(sp_j)] = sp_j
        dense_j[idx] = de_j
        
    return (
        torch.tensor(sparse_i, dtype=torch.long),
        torch.tensor(dense_i, dtype=torch.float32),
        torch.tensor(sparse_j, dtype=torch.long),
        torch.tensor(dense_j, dtype=torch.float32)
    )


class QuietHybridBRanker(nn.Module):
    def __init__(self, cvs_hidden):
        super().__init__()
        self.cvs_hidden = cvs_hidden
        self.cvs_embed = nn.EmbeddingBag(504 + 1, cvs_hidden, mode='sum', padding_idx=504)
        self.cvs_b1 = nn.Parameter(torch.zeros(cvs_hidden))
        
        self.ranker_w1 = nn.Linear(cvs_hidden + 32, 32)
        self.ranker_w2 = nn.Linear(32, 1)
        
        # Initialization
        nn.init.normal_(self.cvs_embed.weight, std=0.001)
        self.cvs_embed.weight.data[504].zero_()
        
        nn.init.normal_(self.ranker_w1.weight, std=0.05)
        nn.init.constant_(self.ranker_w1.bias, 0.0)
        
        nn.init.normal_(self.ranker_w2.weight, std=0.05)
        nn.init.constant_(self.ranker_w2.bias, 0.0)

    def forward(self, sparse_idx, dense_features):
        sparse_h = torch.clamp(self.cvs_embed(sparse_idx) + self.cvs_b1, 0.0, 1.0)
        combined = torch.cat([sparse_h, dense_features], dim=-1)
        fc1_h = torch.clamp(self.ranker_w1(combined), 0.0, 1.0)
        logit = self.ranker_w2(fc1_h).squeeze(-1)
        return logit


def main():
    positions = load_dataset(args.data)
    if not positions:
        print("Error: No data loaded.")
        return

    # Train/Val split at position level
    random.seed(42)
    random.shuffle(positions)
    
    split = int(len(positions) * 0.9)
    train_positions = positions[:split]
    val_positions = positions[split:]
    
    train_pairs = make_pairs(train_positions, args.pairs_per_pos)
    val_pairs = make_pairs(val_positions, args.pairs_per_pos)
    
    print(f"Generated {len(train_pairs)} train pairs, {len(val_pairs)} val pairs.")
    
    net = QuietHybridBRanker(args.cvs_hidden).to(DEVICE)
    optimizer = torch.optim.Adam(net.parameters(), lr=args.lr)

    def evaluate_loss(pairs):
        net.eval()
        total_loss = 0.0
        nb = 0
        with torch.no_grad():
            for c in range(0, len(pairs), args.batch_size):
                batch = pairs[c:c + args.batch_size]
                sp_i, de_i, sp_j, de_j = collate_fn(batch)
                
                logit_i = net(sp_i.to(DEVICE), de_i.to(DEVICE))
                logit_j = net(sp_j.to(DEVICE), de_j.to(DEVICE))
                
                # Pairwise cross entropy
                loss = torch.log(1.0 + torch.exp(-(logit_i - logit_j))).mean()
                total_loss += loss.item()
                nb += 1
        net.train()
        return total_loss / max(nb, 1)

    print(f"Training on {DEVICE}...")
    for epoch in range(args.epochs):
        t0 = time.time()
        random.shuffle(train_pairs)
        
        total_loss = 0.0
        nb = 0
        for c in range(0, len(train_pairs), args.batch_size):
            batch = train_pairs[c:c + args.batch_size]
            sp_i, de_i, sp_j, de_j = collate_fn(batch)
            
            logit_i = net(sp_i.to(DEVICE), de_i.to(DEVICE))
            logit_j = net(sp_j.to(DEVICE), de_j.to(DEVICE))
            
            loss = torch.log(1.0 + torch.exp(-(logit_i - logit_j))).mean()
            
            optimizer.zero_grad()
            loss.backward()
            optimizer.step()
            
            total_loss += loss.item()
            nb += 1
            
        train_l = total_loss / max(nb, 1)
        val_l = evaluate_loss(val_pairs)
        print(f"Epoch {epoch:2d} | Train Loss: {train_l:.6f} | Val Loss: {val_l:.6f} | Time: {time.time()-t0:.1f}s")

    # Export weights
    print(f"Exporting model to {args.out}...")
    cvs_w1 = net.cvs_embed.weight.detach().cpu().numpy()[:504]
    cvs_b1 = net.cvs_b1.detach().cpu().numpy()
    
    ranker_w1 = net.ranker_w1.weight.detach().cpu().numpy()
    ranker_b1 = net.ranker_w1.bias.detach().cpu().numpy()
    
    ranker_w2 = net.ranker_w2.weight.detach().cpu().numpy()[0]
    b2 = float(net.ranker_w2.bias.detach().cpu().numpy()[0])
    
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    
    json.dump({
        "modelKind": "cvs_ranker_b",
        "registryHash": REGISTRY_HASH,
        "anchorSchemaVersion": 1,
        "featureCount": 168,
        "sparseInputCount": 504,
        "trainingCommit": "quiet-hybrid-b-initial",
        "datasetManifestHash": "manifest-hash",
        "rankerTemperature": 1.0,
        "rankerMaxBonus": 4000,
        "outputScaleCp": 400.0,
        "cvsHidden": args.cvs_hidden,
        "cvs_w1": [[round(float(v), 6) for v in row] for row in cvs_w1],
        "cvs_b1": [round(float(v), 6) for v in cvs_b1],
        "ranker_w1": [[round(float(v), 6) for v in row] for row in ranker_w1],
        "ranker_b1": [round(float(v), 6) for v in ranker_b1],
        "ranker_w2": [round(float(v), 6) for v in ranker_w2],
        "b2": b2,
    }, out_path.open("w", encoding="utf-8"))
    print("Done!")


if __name__ == "__main__":
    main()
