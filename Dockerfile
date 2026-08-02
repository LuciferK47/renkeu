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

# Assemble the runtime filesystem while a shell and ldd still exist.
#
# Deriving the library set from the binary rather than hand-listing packages
# means a new transitive dependency cannot be silently missed, and nothing
# unused rides along: `debian:bookworm-slim` plus the runtime packages cost
# 222 MB to carry a program whose real closure is ~61 MB. Most of the rest was
# a userland this daemon never calls, plus the whole curl dependency tree,
# which existed only to run HEALTHCHECK.
RUN set -eux; \
    mkdir -p /rootfs/usr/local/bin /rootfs/etc/ssl/certs /rootfs/tmp; \
    cp /usr/local/bin/latex-ai /rootfs/usr/local/bin/; \
    # The ELF interpreter and every DT_NEEDED library, kept at the paths ld.so
    # searches. -L because several are symlinks to versioned real files.
    ldd /usr/local/bin/latex-ai | grep -oE '/[^ ]+\.so[^ ]*' | sort -u \
      | while read -r lib; do \
          mkdir -p "/rootfs$(dirname "$lib")"; \
          cp -L "$lib" "/rootfs$(dirname "$lib")/"; \
        done; \
    # glibc dlopens its NSS modules at call time, so they are invisible to ldd.
    # Without them getaddrinfo cannot resolve anything and the very first
    # compile fails to fetch the Tectonic bundle — a failure that would appear
    # only on a cold cache, never in a warm-volume smoke test.
    for nss in libnss_files.so.2 libnss_dns.so.2 libresolv.so.2; do \
        cp -L "/lib/x86_64-linux-gnu/$nss" /rootfs/lib/x86_64-linux-gnu/; \
    done; \
    printf 'hosts: files dns\npasswd: files\ngroup: files\n' > /rootfs/etc/nsswitch.conf; \
    cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/; \
    # A real account, so anything resolving uid 1000 gets a name and a home
    # instead of an error.
    printf 'latexai:x:1000:1000::/home/latexai:/sbin/nologin\n' > /rootfs/etc/passwd; \
    printf 'latexai:x:1000:\n' > /rootfs/etc/group; \
    # Volume mount points owned by the runtime user: Docker initialises a named
    # volume from the image path and makes it root-owned when that path is
    # absent, which the unprivileged process then cannot write. Tectonic's
    # cache-root creation is an unwrap and the release profile sets
    # panic="abort", so that surfaced as the daemon dying outright.
    mkdir -p /rootfs/home/latexai/.cache/Tectonic \
             /rootfs/home/latexai/.latex-ai \
             /rootfs/home/latexai/workspace; \
    chown -R 1000:1000 /rootfs/home/latexai; \
    chmod 1777 /rootfs/tmp

FROM scratch
COPY --from=builder /rootfs /

USER 1000:1000
WORKDIR /home/latexai
# HOME is unset on scratch, and the Tectonic cache root is derived from it.
# OpenSSL's compiled-in CA path does not exist here either, so point at the
# bundle explicitly: without it the bundle download fails TLS verification.
ENV HOME=/home/latexai \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

EXPOSE 3000
# Exec form, and the binary probes itself — there is no shell and no curl here.
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/latex-ai", "healthcheck", "--port", "3000"]
# 0.0.0.0 inside the container: Docker forwards a published port to the
# container's interface address, so a loopback-only bind would be unreachable
# from the host even though the in-container healthcheck passed. Access is
# restricted by the host-side mapping in docker-compose.yml (127.0.0.1:3000).
CMD ["/usr/local/bin/latex-ai", "serve", "--port", "3000", "--host", "0.0.0.0"]
