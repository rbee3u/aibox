# aibox-base.Dockerfile
# Shared development runtime for aibox agent images. Agent-specific images build
# FROM this local image and only add the agent CLI plus its user/home setup.
#
# Build:
#   aibox build
#   aibox build codex
#   aibox build claude

FROM debian:bookworm

# Resolve curl|jq pipelines correctly (fail the layer if either side fails).
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Populated automatically by buildx (amd64/arm64/...); declaring it here injects
# it into this stage. Falls back to dpkg for a plain `docker build`, where it's
# empty. Used by the Node and Go layers below to pick the right arch tarball.
ARG TARGETARCH

# Base system: VCS, TLS roots, fetch/extract tools, a native compiler (for cgo,
# Rust crates, and node native modules), jq, and the handful of CLIs coding
# agents shell out to.
RUN apt-get update && apt-get install -y --no-install-recommends \
        git \
        curl \
        ca-certificates \
        less \
        jq \
        xz-utils \
        ripgrep \
        openssh-client \
        build-essential \
    && rm -rf /var/lib/apt/lists/*

# --- Python ------------------------------------------------------------------
# System interpreter, pip, and venv from apt, plus uv (Astral's fast installer
# and resolver). UV_UNMANAGED_INSTALL points the install at /usr/local/bin
# (root-owned, already on PATH) and, because it marks the install unmanaged,
# also blocks shell/env edits and disables `uv self update` - right for an
# immutable image you upgrade by rebuilding.
RUN apt-get update && apt-get install -y --no-install-recommends \
        python3 \
        python3-pip \
        python3-venv \
    && rm -rf /var/lib/apt/lists/*
RUN set -eux; \
    curl -LsSf https://astral.sh/uv/install.sh \
        | env UV_UNMANAGED_INSTALL=/usr/local/bin sh; \
    /usr/local/bin/uv --version; \
    /usr/local/bin/uvx --version

# --- Node.js -----------------------------------------------------------------
# Pinned by default so cached builds stay stable. Change NODE_VERSION here when
# you intentionally want to upgrade Node.
# Installed under /usr/local (root-owned) so `npm -g` never touches the mounted
# home and avoids permission surprises.
ARG NODE_VERSION=v24.4.0
RUN set -eux; \
    v="${NODE_VERSION}"; \
    [ -n "$v" ]; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) a=x64 ;; \
        arm64) a=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/${v}/node-${v}-linux-${a}.tar.xz" -o /tmp/node.tar.xz; \
    tar -C /usr/local --strip-components=1 -xJf /tmp/node.tar.xz; \
    rm /tmp/node.tar.xz; \
    node --version; npm --version

# --- Rust -------------------------------------------------------------------
# Pinned by default so cached builds stay stable. Change RUST_VERSION here when
# you intentionally want to upgrade Rust.
ARG RUST_VERSION=1.88.0
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN set -eux; \
    v="${RUST_VERSION}"; \
    [ -n "$v" ]; \
    curl -fsSL https://sh.rustup.rs | sh -s -- \
        -y \
        --no-modify-path \
        --profile default \
        --default-toolchain "$v"; \
    chmod -R a+rwX "$RUSTUP_HOME" "$CARGO_HOME"; \
    "$CARGO_HOME/bin/rustc" --version; \
    "$CARGO_HOME/bin/cargo" --version; \
    "$CARGO_HOME/bin/rustup" --version

# --- Go ----------------------------------------------------------------------
# Pinned by default so cached builds stay stable. Change GO_VERSION here when
# you intentionally want to upgrade Go.
ARG GO_VERSION=1.26.5
RUN set -eux; \
    v="${GO_VERSION}"; \
    [ -n "$v" ]; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) a=amd64 ;; \
        arm64) a=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://go.dev/dl/go${v}.linux-${a}.tar.gz" -o /tmp/go.tgz; \
    tar -C /usr/local -xzf /tmp/go.tgz; \
    rm /tmp/go.tgz; \
    /usr/local/go/bin/go version

ENV PATH=/usr/local/cargo/bin:/usr/local/go/bin:$PATH

# --- Extra shared toolchains -------------------------------------------------
# This is the slot to grow the shared image. Uncomment / add what projects need;
# each is its own layer, so adding one only rebuilds from here down. Keep
# language runtimes root-owned under /usr/local so mounted homes stay clean.
#
# Java (Debian OpenJDK):
#   RUN apt-get update && apt-get install -y --no-install-recommends \
#         default-jdk maven \
#     && rm -rf /var/lib/apt/lists/*
