# Self-play + deep-label training data generator for Chess Vision Studio.
#
# Pipeline per batch: our engine plays NNUE-backed self-play games ->
# Stockfish labels every quiet position at depth 20 -> our engine attaches
# CVS geometry feature IDs -> one immutable shard on /data.
#
# Deploy:  railway up  (or docker build + run locally)
#
# The container runs until the game target is reached. Output shards are
# written to /data/training-*.jsonl (mount a Railway volume at /data).

FROM rust:1.96-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin selfplay --bin analyze

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y python3 python3-pip curl && \
    pip3 install --break-system-packages chess zstandard && \
    curl -sL -o /tmp/sf.tar \
      "https://github.com/official-stockfish/Stockfish/releases/download/sf_17.1/stockfish-ubuntu-x86-64-avx2.tar" && \
    tar -xf /tmp/sf.tar -C /tmp && \
    install -m 0755 /tmp/stockfish/stockfish-ubuntu-x86-64-avx2 /usr/local/bin/stockfish && \
    rm -rf /tmp/sf.tar /tmp/stockfish && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/selfplay /app/selfplay
COPY --from=builder /build/target/release/analyze /app/analyze
COPY --from=builder /build/src/facts /app/src/facts
COPY nets/matrix-raw.json /app/matrix-raw.json
COPY nets/matrix-residual.json /app/matrix-residual.json
COPY nets/eval-cal.json /app/eval-cal.json
COPY training/gen10/build_corpus.py /app/build_corpus.py
COPY training/gen10/run_generation.py /app/run_generation.py

WORKDIR /app
ENTRYPOINT ["python3", "run_generation.py"]
