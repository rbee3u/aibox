# Mutable runtimes and Coding Agents are installed into each Managed Tenant
# Home as Components; this image provides their shared system substrate.

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
        gawk \
        gdb \
        git \
        git-lfs \
        htop \
        iproute2 \
        iputils-ping \
        jq \
        less \
        libssl-dev \
        libtool \
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
