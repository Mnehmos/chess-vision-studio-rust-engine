#!/usr/bin/env python3
"""Prepare sharded CVS training dataset for Gen 9.

Processes the raw training JSONL file, groups positions by game sequence,
shards them, extracts CVS core/full feature IDs using the Rust analyzer binary,
stores the features as bitsets (3 x uint64), and writes metadata files.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


REGISTRY_VERSION = 1
REGISTRY_HASH_FULL = "25c15688f9f4ebba"
REGISTRY_HASH_CORE = "58cb4e1e461a607d"


def key_for(row: dict) -> str:
    """Gets sequence/game key for sequence-level splitting."""
    if "splitKey" in row:
        return row["splitKey"]
    if "sourceKey" in row:
        return row["sourceKey"]
    has_any = any(k in row for k in ("sourceBucket", "sourceFile", "gameId"))
    if has_any:
        return "|".join(
            str(row.get(k, ""))
            for k in ("sourceBucket", "sourceFile", "gameId")
        )
    return row["fen"]


def compute_bitset(ids: list[int]) -> list[int]:
    """Encodes a list of active feature IDs into a 3 x uint64 bitset."""
    bitset = [0, 0, 0]
    for id_val in ids:
        word = id_val // 64
        bit = id_val % 64
        if 0 <= word < 3:
            bitset[word] |= (1 << bit)
    return bitset


def run_batch_extraction(
    fens: list[str], engine_path: Path, mode: str
) -> list[list[int] | None]:
    """Runs target engine binary in batch mode to extract CVS feature IDs."""
    results: list[list[int] | None] = []
    
    # Process in chunks of 20,000 to prevent OS/memory limit issues
    chunk_size = 20000
    for i in range(0, len(fens), chunk_size):
        chunk_fens = fens[i:i+chunk_size]
        # Write chunk FENs to a temp file
        with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".fens", encoding="utf-8") as tmp:
            tmp_name = tmp.name
            for fen in chunk_fens:
                tmp.write(fen + "\n")
                
        try:
            # Run analyze.exe
            flag = "--cvs-ids" if mode == "full" else "--cvs-core-ids"
            cmd = [str(engine_path), "--fens", tmp_name, flag, "--depth", "1"]
            
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                check=True,
                encoding="utf-8"
            )
            
            for line in proc.stdout.splitlines():
                line = line.strip()
                if not line:
                    results.append([])
                elif line == "ERR":
                    results.append(None)
                else:
                    try:
                        ids = [int(x) for x in line.split(",")]
                        results.append(ids)
                    except ValueError:
                        results.append(None)
                        
        except subprocess.CalledProcessError:
            # Engine crashed on this chunk. Let's process FENs one by one to isolate and skip the crasher.
            print(f"Warning: Engine crashed on a batch of {len(chunk_fens)} FENs. Isolating crasher...")
            for idx, fen in enumerate(chunk_fens):
                with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".fens", encoding="utf-8") as single_tmp:
                    single_name = single_tmp.name
                    single_tmp.write(fen + "\n")
                try:
                    single_proc = subprocess.run(
                        [str(engine_path), "--fens", single_name, flag, "--depth", "1"],
                        capture_output=True,
                        text=True,
                        check=True,
                        encoding="utf-8"
                    )
                    out_line = single_proc.stdout.strip()
                    if not out_line:
                        results.append([])
                    elif out_line == "ERR":
                        results.append(None)
                    else:
                        try:
                            ids = [int(x) for x in out_line.split(",")]
                            results.append(ids)
                        except ValueError:
                            results.append(None)
                except subprocess.CalledProcessError:
                    print(f"Error: Engine crashed on FEN: {fen}")
                    results.append(None)
                finally:
                    if os.path.exists(single_name):
                        os.remove(single_name)
        finally:
            if os.path.exists(tmp_name):
                os.remove(tmp_name)
                
        # Pad results if engine didn't output one line per FEN for this chunk
        expected_len = i + len(chunk_fens)
        while len(results) < expected_len:
            results.append(None)
            
    return results


def get_file_sha256(path: Path) -> str:
    """Computes SHA-256 checksum of a file."""
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def process_shard(
    shard_idx: int,
    rows: list[dict],
    out_dir: Path,
    engine_path: Path,
    mode: str,
    registry_hash: str,
    engine_hash: str
) -> None:
    """Processes a single shard: extracts features, writes jsonl and metadata."""
    shard_name = f"shard-{shard_idx:05d}"
    jsonl_path = out_dir / f"{shard_name}.jsonl"
    meta_path = out_dir / f"{shard_name}.meta.json"
    
    print(f"Processing {shard_name} ({len(rows)} positions)...")
    
    # Extract FENs
    fens = [r["fen"] for r in rows]
    
    # Run batch extraction
    extracted = run_batch_extraction(fens, engine_path, mode)
    
    # Combine and save
    valid_rows = []
    for idx, row in enumerate(rows):
        res_ids = extracted[idx]
        if res_ids is None:
            # Skip invalid positions
            continue
            
        row_out = {
            "fen": row["fen"],
            "res": row.get("res", 0.5),
        }
        
        # Get target score
        if "sfCp" in row and row["sfCp"] is not None:
            row_out["cp"] = row["sfCp"]
        elif "cp" in row:
            row_out["cp"] = row["cp"]
        else:
            row_out["cp"] = 0
            
        row_out["features"] = res_ids
        row_out["features_bitset"] = compute_bitset(res_ids)
        valid_rows.append(row_out)
        
    # Write jsonl
    with jsonl_path.open("w", encoding="utf-8", newline="\n") as out:
        for r in valid_rows:
            out.write(json.dumps(r, separators=(",", ":")) + "\n")
            
    # Write metadata
    jsonl_checksum = get_file_sha256(jsonl_path)
    meta = {
        "shard_index": shard_idx,
        "position_count": len(valid_rows),
        "registry_version": REGISTRY_VERSION,
        "registry_hash": registry_hash,
        "analyzer_binary_hash": engine_hash,
        "checksum": jsonl_checksum,
        "feature_mode": mode
    }
    
    with meta_path.open("w", encoding="utf-8", newline="\n") as out:
        json.dump(meta, out, indent=2)
        out.write("\n")
        
    print(f"Finished {shard_name}: wrote {len(valid_rows)} valid positions.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Prepare sharded CVS training dataset for Gen 9")
    parser.add_argument("--input", default="F:/tools/gen9-train.jsonl", help="Path to input jsonl file")
    parser.add_argument("--out-dir", default="training/gen9/gen9-cvs", help="Path to output directory")
    parser.add_argument("--engine", default="target-cvs/release/analyze.exe", help="Path to engine analyzer binary")
    parser.add_argument("--shard-size", type=int, default=200000, help="Number of positions per shard")
    parser.add_argument("--feature-mode", choices=["core", "full"], default="core", help="CVS feature mode")
    parser.add_argument("--resume", action="store_true", help="Resume and skip existing shards")
    parser.add_argument("--start-shard", type=int, default=0, help="First shard index to process")
    parser.add_argument("--end-shard", type=int, default=9999, help="Last shard index to process")
    parser.add_argument("--verify-only", action="store_true", help="Only verify existing shards without creating new ones")
    
    args = parser.parse_args()
    
    input_path = Path(args.input)
    if not input_path.exists():
        print(f"Error: input file {input_path} does not exist.")
        sys.exit(1)
        
    engine_path = Path(args.engine)
    if not engine_path.exists():
        print(f"Error: engine binary {engine_path} does not exist.")
        sys.exit(1)
        
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    
    engine_hash = get_file_sha256(engine_path)
    registry_hash = REGISTRY_HASH_CORE if args.feature_mode == "core" else REGISTRY_HASH_FULL
    
    if args.verify_only:
        print("Running in verify-only mode...")
        meta_files = sorted(out_dir.glob("shard-*.meta.json"))
        if not meta_files:
            print("No shards found to verify.")
            return
            
        corrupt = 0
        for meta_path in meta_files:
            with meta_path.open(encoding="utf-8") as f:
                meta = json.load(f)
            jsonl_path = meta_path.with_suffix(".jsonl")
            if not jsonl_path.exists():
                print(f"Error: {jsonl_path} is missing for {meta_path.name}")
                corrupt += 1
                continue
                
            checksum = get_file_sha256(jsonl_path)
            if checksum != meta["checksum"]:
                print(f"Checksum mismatch for {jsonl_path.name}! Expected: {meta['checksum']}, Got: {checksum}")
                corrupt += 1
            else:
                print(f"{jsonl_path.name} is verified (checksum matches).")
                
        if corrupt > 0:
            print(f"Verification FAILED: found {corrupt} corrupt/missing shards.")
            sys.exit(1)
        else:
            print("Verification PASSED: all shards are intact.")
            return
            
    print("Reading input file and grouping by sequence...")
    # Group by key to maintain sequence splits
    groups: dict[str, list[dict]] = {}
    with input_path.open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            row = json.loads(line)
            key = key_for(row)
            if key not in groups:
                groups[key] = []
            groups[key].append(row)
            
    # Deterministic shuffle of groups (using SHA256-based sorting for consistency without external seed issues)
    sorted_keys = sorted(groups.keys(), key=lambda k: hashlib.sha256(k.encode("utf-8")).hexdigest())
    
    # Assign groups to shards
    shards: list[list[dict]] = []
    current_shard: list[dict] = []
    
    for key in sorted_keys:
        group_rows = groups[key]
        if len(current_shard) + len(group_rows) > args.shard_size and current_shard:
            shards.append(current_shard)
            current_shard = []
        current_shard.extend(group_rows)
        
    if current_shard:
        shards.append(current_shard)
        
    print(f"Partitioned {sum(len(s) for s in shards)} positions into {len(shards)} sequence-aligned shards.")
    
    # Process shards
    for idx, shard_rows in enumerate(shards):
        if idx < args.start_shard or idx > args.end_shard:
            continue
            
        meta_path = out_dir / f"shard-{idx:05d}.meta.json"
        if args.resume and meta_path.exists():
            print(f"Shard {idx:05d} already exists. Skipping...")
            continue
            
        process_shard(
            idx,
            shard_rows,
            out_dir,
            engine_path,
            args.feature_mode,
            registry_hash,
            engine_hash
        )


if __name__ == "__main__":
    main()
