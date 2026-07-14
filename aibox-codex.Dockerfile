# aibox-codex.Dockerfile
# Packages OpenAI Codex CLI plus a general dev toolchain into a Debian image, so
# high-risk projects can be run inside a container that IS the sandbox boundary.
#
# Codex ships its own OS sandbox (Seatbelt on macOS, Landlock+seccomp on Linux).
# We don't rely on it: aibox-codex launches with the sandbox bypassed, because
# the container is the boundary. See the aibox-codex script for the flags.
#
# Build (latest stable Node + Go + Rust, resolved at build time):
#   docker build -f aibox-codex.Dockerfile -t aibox-codex:latest .
#   (or just run  aibox-codex --build)
#
# Pin specific versions instead:
#   docker build -f aibox-codex.Dockerfile \
#       --build-arg GO_VERSION=go1.26.0 \
#       --build-arg NODE_VERSION=v24.4.0 \
#       --build-arg RUST_VERSION=1.88.0 \
#       -t aibox-codex:latest .

FROM debian:bookworm-slim

# Resolve curl|jq pipelines correctly (fail the layer if either side fails).
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Populated automatically by buildx (amd64/arm64/…); declaring it here injects
# it into this stage. Falls back to dpkg for a plain `docker build`, where it's
# empty. Used by the Node and Go layers below to pick the right arch tarball.
ARG TARGETARCH

# Base system: VCS, TLS roots, fetch/extract tools, a native compiler (for cgo,
# Rust crates, and node native modules), jq (to resolve "latest" versions), and
# the handful of CLIs a coding agent shells out to.
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

# --- Node.js -----------------------------------------------------------------
# Empty NODE_VERSION => latest LTS resolved from the official dist index.
# Installed under /usr/local (root-owned) so `npm -g` never touches the mounted
# home and avoids permission surprises.
ARG NODE_VERSION=
RUN set -eux; \
    v="${NODE_VERSION}"; \
    if [ -z "$v" ]; then \
        v="$(curl -fsSL https://nodejs.org/dist/index.json \
             | jq -r '[.[] | select(.lts != false)][0].version')"; \
    fi; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) a=x64 ;; \
        arm64) a=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/${v}/node-${v}-linux-${a}.tar.xz" -o /tmp/node.tar.xz; \
    tar -C /usr/local --strip-components=1 -xJf /tmp/node.tar.xz; \
    rm /tmp/node.tar.xz; \
    node --version; npm --version

# --- Go ----------------------------------------------------------------------
# Empty GO_VERSION => latest stable resolved from go.dev. Accepts "go1.26.0"
# or "1.26.0" when pinning.
ARG GO_VERSION=
RUN set -eux; \
    v="${GO_VERSION}"; \
    if [ -z "$v" ]; then \
        v="$(curl -fsSL 'https://go.dev/dl/?mode=json' | jq -r '.[0].version')"; \
    fi; \
    case "$v" in go*) ;; *) v="go$v" ;; esac; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) a=amd64 ;; \
        arm64) a=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://go.dev/dl/${v}.linux-${a}.tar.gz" -o /tmp/go.tgz; \
    tar -C /usr/local -xzf /tmp/go.tgz; \
    rm /tmp/go.tgz; \
    /usr/local/go/bin/go version

# --- Rust -------------------------------------------------------------------
# Empty/default RUST_VERSION => latest stable resolved by rustup at build time.
# Accepts rustup toolchain names like stable, nightly, or 1.88.0 when pinning.
ARG RUST_VERSION=stable
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN set -eux; \
    v="${RUST_VERSION:-stable}"; \
    curl -fsSL https://sh.rustup.rs | sh -s -- \
        -y \
        --no-modify-path \
        --profile default \
        --default-toolchain "$v"; \
    chmod -R a+rwX "$RUSTUP_HOME" "$CARGO_HOME"; \
    "$CARGO_HOME/bin/rustc" --version; \
    "$CARGO_HOME/bin/cargo" --version; \
    "$CARGO_HOME/bin/rustup" --version

# --- Extra toolchains (add per project, then `aibox-codex --build`) ----------
# This is the slot to grow the image. Uncomment / add what a project needs;
# each is its own layer, so adding one only rebuilds from here down. Keep
# language runtimes root-owned under /usr/local so the mounted home stays clean.
#
# Java (Temurin JDK from Debian's adoptium-free packaging):
#   RUN apt-get update && apt-get install -y --no-install-recommends \
#         default-jdk maven \
#     && rm -rf /var/lib/apt/lists/*
#
# Python (system interpreter + venv/pip):
#   RUN apt-get update && apt-get install -y --no-install-recommends \
#         python3 python3-pip python3-venv \
#     && rm -rf /var/lib/apt/lists/*

# --- Codex CLI ---------------------------------------------------------------
# The npm package delivers a per-platform native binary via optionalDependencies;
# `npm -g` fetches the right one for this image's arch.
RUN npm install -g @openai/codex \
    && npm cache clean --force

# Recreate a predictable non-root user at uid/gid 1000 so the mounted home has
# a stable path.
RUN groupadd --gid 1000 codex \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash codex

ENV HOME=/home/codex
# Codex keeps all its state (config.toml, auth.json, sessions, history) under
# CODEX_HOME. Point it inside the mounted home so it persists per profile.
ENV CODEX_HOME=/home/codex/.codex
# GOPATH lives in the mounted home => module cache persists per profile.
ENV GOPATH=/home/codex/go
ENV PATH=/usr/local/cargo/bin:/usr/local/go/bin:/home/codex/go/bin:$PATH

WORKDIR /work
USER codex

# Image is immutable; upgrade by rebuilding (aibox-codex --build), not in place.
ENTRYPOINT ["codex"]
