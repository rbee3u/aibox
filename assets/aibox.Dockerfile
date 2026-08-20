# aibox.Dockerfile
# Shared development runtime for aibox. It installs both OpenAI Codex and
# Claude Code into one image; the Rust wrapper selects which binary to run.
#
# Build with `aibox build` or from Console Overview.

FROM debian:bookworm-slim

# Fail RUN layers when either side of a pipeline fails.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Populated automatically by buildx (amd64/arm64/...); declaring it here injects
# it into this stage. Falls back to dpkg for a plain `docker build`, where it's
# empty. Used by the Node layer below to pick the right arch tarball.
ARG TARGETARCH

# Create the runtime identity before installing tools so every Tenant Home
# path can derive from one stable home. Build layers remain root-owned;
# the final USER directive switches only the running container.
RUN groupadd --gid 1000 aibox \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash aibox
ENV HOME=/home/aibox

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
# Installed under /usr/local so Node and the global agent CLIs are image-owned
# rather than persisted in a Tenant Home; upgrade them by rebuilding the image.
ARG NODE_VERSION=v24.19.0
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

# Rust is installed on demand with rustup into the mounted Tenant Home. Keep
# its binaries available in non-login shells without making Rust image-owned.
ENV PATH=$HOME/.cargo/bin:$PATH

# Go is installed on demand from an official archive into the mounted Tenant
# Home. Keep the SDK, installed commands, and module/build caches Tenant-local.
ENV GOROOT=$HOME/.goroot
ENV GOPATH=$HOME/.gopath
ENV PATH=$GOROOT/bin:$PATH
ENV PATH=$GOPATH/bin:$PATH

# --- Agent CLIs --------------------------------------------------------------
# Both CLIs live in the same immutable image. Upgrade by changing the pinned
# versions and rebuilding, not by self-updating inside a Tenant Home.
ARG CODEX_VERSION=0.148.0
ARG CLAUDE_CODE_VERSION=2.1.235
RUN set -eux; \
    export HOME=/tmp/aibox-build-home; \
    export npm_config_cache=/tmp/npm-cache; \
    mkdir -p "$HOME"; \
    npm install -g \
        @openai/codex@${CODEX_VERSION} \
        @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}; \
    codex --version; \
    claude --version; \
    npm cache clean --force; \
    rm -rf "$HOME" "$npm_config_cache"
ENV DISABLE_AUTOUPDATER=1

# Debian's /etc/profile resets PATH for login shells. Agent command execution
# can go through a shell, so mirror the image PATH there too.
RUN printf "export PATH=%s\n" "$PATH" > /etc/profile.d/aibox-path.sh

WORKDIR /workspace
USER aibox

# No ENTRYPOINT: the Rust wrapper passes either `codex ...` or `claude ...`.
