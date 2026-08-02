# syntax=docker/dockerfile:1
# LaTeX-AI server. Single-binary Rust daemon; Tectonic is the only heavy
# external asset (fetched to ~/.cache/Tectonic at first compile).
#
# This image is the daemon alone. The SPA is served by Vite in development and
# bundled into the Tauri shell for the desktop app (README "Desktop app"), so
# there is no frontend build stage here.

FROM rust:1-slim-bookworm AS builder
# g++ is not optional: tectonic_engine_xetex compiles vendored C *and* C++
# sources, and its build script aborts on "failed to find tool c++". The slim
# rust image ships no C/C++ toolchain, only the Rust one.
RUN apt-get update && apt-get install -y --no-install-recommends \
    g++ \
    pkg-config libfontconfig1-dev libgraphite2-dev libharfbuzz-dev \
    libicu-dev libfreetype6-dev libssl-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
# Build against the checked-in .sqlx/ data so the query! macros do not need a
# live database at image-build time (same as CI).
ENV SQLX_OFFLINE=true
# Cache mounts, not layers. COPY . . invalidates this step on any source change,
# so without them every edit rebuilt all ~460 dependency crates from scratch —
# about an hour, most of it Tectonic's vendored C/C++ engines. The registry and
# target caches survive that invalidation, leaving only changed crates plus the
# final LTO link.
#
# A cache mount is not part of the image, so the binary has to be copied out
# inside the same RUN. That also stops the multi-GB target/ from being committed
# into the builder layer, which was itself minutes of the old build.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release -p latex-ai-server \
    && cp target/release/latex-ai /usr/local/bin/latex-ai

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libfontconfig1 libgraphite2-3 libharfbuzz0b \
    libicu72 libfreetype6 libssl3 zlib1g \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/latex-ai /usr/local/bin/latex-ai

RUN useradd -m -u 1000 latexai
# Create the volume mount points *before* declaring them, owned by the runtime
# user. Docker initialises a fresh named volume from the image path; when that
# path does not exist it makes it root-owned, and the unprivileged process then
# cannot write. Tectonic's cache-root creation is an unwrap, and the release
# profile sets panic="abort", so that surfaced as the whole daemon dying on the
# first compile rather than as one failed request.
RUN mkdir -p /home/latexai/.cache/Tectonic /home/latexai/.latex-ai /home/latexai/workspace \
    && chown -R latexai:latexai /home/latexai
USER latexai
WORKDIR /home/latexai

EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -fsS http://localhost:3000/health || exit 1
# 0.0.0.0 inside the container: Docker forwards a published port to the
# container's interface address, so a loopback-only bind would be unreachable
# from the host even though the in-container healthcheck passed. Access is
# restricted by the host-side mapping in docker-compose.yml (127.0.0.1:3000).
CMD ["latex-ai", "serve", "--port", "3000", "--host", "0.0.0.0"]
