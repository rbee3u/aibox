# Mutable runtimes and Coding Agents are installed into each Managed Tenant
# Home as Components; this image provides their shared system substrate.
#
# That substrate includes the shared fonts and the ABI libraries a headless
# Chromium links against; it still links X11 without needing a display server.
# The browser binary itself stays versioned and Tenant-local, so it is never
# installed or pinned here. Firefox and WebKit need further libraries that stay
# host-owned.

FROM debian:bookworm

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        autoconf \
        automake \
        bind9-dnsutils \
        build-essential \
        bzip2 \
        ca-certificates \
        cmake \
        curl \
        file \
        fontconfig \
        fonts-liberation \
        fonts-noto-color-emoji \
        fonts-wqy-zenhei \
        gawk \
        gdb \
        git \
        git-lfs \
        htop \
        iproute2 \
        iputils-ping \
        jq \
        less \
        libasound2 \
        libatk-bridge2.0-0 \
        libatk1.0-0 \
        libatspi2.0-0 \
        libcairo2 \
        libcups2 \
        libdbus-1-3 \
        libdrm2 \
        libgbm1 \
        libglib2.0-0 \
        libnspr4 \
        libnss3 \
        libpango-1.0-0 \
        libssl-dev \
        libtool \
        libx11-6 \
        libxcb1 \
        libxcomposite1 \
        libxdamage1 \
        libxext6 \
        libxfixes3 \
        libxkbcommon0 \
        libxrandr2 \
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
        zstd && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 aibox && \
    useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash aibox

ENV HOME=/home/aibox LANG=C.UTF-8
WORKDIR /workspace
USER aibox

# AIBox injects the Tenant-local Coding Agent or Debug Shell at runtime.
