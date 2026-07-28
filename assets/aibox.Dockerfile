# aibox.Dockerfile
# Shared development runtime for aibox. It installs both OpenAI Codex and
# Claude Code into one image; the Rust wrapper selects which binary to run.
#
# Build:
#   aibox build

FROM debian:bookworm-slim

# Fail RUN layers when either side of a pipeline fails.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Populated automatically by buildx (amd64/arm64/...); declaring it here injects
# it into this stage. Falls back to dpkg for a plain `docker build`, where it's
# empty. Used by the Node and Go layers below to pick the right arch tarball.
ARG TARGETARCH

# Base system: VCS, TLS roots, fetch/extract tools, a native compiler (for cgo,
# Rust crates, and node native modules), plus the common development and
# diagnostic commands an agent needs while investigating a project.
RUN apt-get update && apt-get install -y --no-install-recommends \
        bind9-dnsutils \
        build-essential \
        bzip2 \
        ca-certificates \
        cmake \
        curl \
        file \
        gawk \
        gdb \
        git \
        htop \
        iproute2 \
        iputils-ping \
        jq \
        less \
        libssl-dev \
        lsof \
        netcat-openbsd \
        ninja-build \
        openssh-client \
        openssl \
        patch \
        pkg-config \
        procps \
        psmisc \
        ripgrep \
        rsync \
        shellcheck \
        socat \
        sqlite3 \
        strace \
        tree \
        unzip \
        vim-tiny \
        wget \
        xxd \
        xz-utils \
        zip \
        zstd \
    && rm -rf /var/lib/apt/lists/*

# --- Python ------------------------------------------------------------------
# System interpreter, pip, and venv from apt, plus uv (Astral's fast installer
# and resolver). UV_UNMANAGED_INSTALL points the install at /usr/local/bin
# (root-owned, already on PATH) and, because it marks the install unmanaged,
# also blocks shell/env edits and disables `uv self update` - right for an
# immutable image you upgrade by rebuilding.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        python3 \
        python3-pip \
        python3-venv; \
    rm -rf /var/lib/apt/lists/*; \
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
    version="${NODE_VERSION}"; \
    [ -n "$version" ]; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) arch=x64 ;; \
        arm64) arch=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/${version}/node-${version}-linux-${arch}.tar.xz" -o /tmp/node.tar.xz; \
    tar -C /usr/local --strip-components=1 -xJf /tmp/node.tar.xz; \
    rm /tmp/node.tar.xz; \
    node --version; \
    npm --version

# --- Rust -------------------------------------------------------------------
# Pinned by default so cached builds stay stable. Change RUST_VERSION here when
# you intentionally want to upgrade Rust.
ARG RUST_VERSION=1.88.0
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
RUN set -eux; \
    version="${RUST_VERSION}"; \
    [ -n "$version" ]; \
    curl -fsSL https://sh.rustup.rs | sh -s -- \
        -y \
        --no-modify-path \
        --profile default \
        --default-toolchain "$version"; \
    chmod -R a+rwX "$RUSTUP_HOME" "$CARGO_HOME"; \
    "$CARGO_HOME/bin/rustc" --version; \
    "$CARGO_HOME/bin/cargo" --version; \
    "$CARGO_HOME/bin/rustup" --version

ENV PATH=/usr/local/cargo/bin:$PATH

# --- Go ----------------------------------------------------------------------
# Pinned by default so cached builds stay stable. Change GO_VERSION here when
# you intentionally want to upgrade Go.
ARG GO_VERSION=1.26.5
RUN set -eux; \
    version="${GO_VERSION}"; \
    [ -n "$version" ]; \
    case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) arch=amd64 ;; \
        arm64) arch=arm64 ;; \
        *) echo "unsupported arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://go.dev/dl/go${version}.linux-${arch}.tar.gz" -o /tmp/go.tgz; \
    tar -C /usr/local -xzf /tmp/go.tgz; \
    rm /tmp/go.tgz; \
    /usr/local/go/bin/go version

# GOPATH lives in the mounted home => module cache persists per profile.
ENV GOPATH=/home/aibox/go
ENV PATH=/usr/local/go/bin:$PATH
ENV PATH=/home/aibox/go/bin:$PATH

# --- Agent CLIs --------------------------------------------------------------
# Both CLIs live in the same immutable image. Upgrade by changing the pinned
# versions and rebuilding, not by self-updating inside a profile.
ARG CODEX_VERSION=0.145.0
ARG CLAUDE_CODE_VERSION=2.1.220
RUN npm install -g \
        @openai/codex@${CODEX_VERSION} \
        @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION} \
    && codex --version \
    && claude --version \
    && npm cache clean --force
ENV DISABLE_AUTOUPDATER=1

# Recreate a predictable non-root user at uid/gid 1000 so the mounted home has
# a stable path.
RUN groupadd --gid 1000 aibox \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash aibox

ENV HOME=/home/aibox
# Debian's /etc/profile resets PATH for login shells. Agent command execution
# can go through a shell, so mirror the image PATH there too.
RUN printf "export PATH=%s\n" "$PATH" > /etc/profile.d/aibox-path.sh

WORKDIR /work
USER aibox

# No ENTRYPOINT: the Rust wrapper passes either `codex ...` or `claude ...`.
