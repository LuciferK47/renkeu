# LaTeX-AI server (M1). Single-binary Rust daemon; Tectonic is the only heavy
# external asset (fetched to ~/.cache/Tectonic at first compile).
# ponytail: daemon only — frontend bundling + Tauri shell land at M5.

FROM rust:1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libfontconfig1-dev libgraphite2-dev libharfbuzz-dev \
    libicu-dev libfreetype6-dev libssl-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release -p latex-ai-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libfontconfig1 libgraphite2-3 libharfbuzz0b \
    libicu72 libfreetype6 libssl3 zlib1g \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/latex-ai /usr/local/bin/latex-ai

RUN useradd -m -u 1000 latexai
USER latexai
WORKDIR /home/latexai

EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -fsS http://localhost:3000/health || exit 1
CMD ["latex-ai", "serve", "--port", "3000"]
