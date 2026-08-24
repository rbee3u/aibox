# aibox.Dockerfile
# Shared base runtime for aibox. Mutable runtimes and Coding Agent executables
# are installed into each Managed Tenant Home as Components.
#
# Build from Console Overview.

FROM debian:bookworm-slim

# Fail RUN layers when either side of a pipeline fails.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Create the runtime identity before installing tools so every Tenant Home
# path can derive from one stable home. Build layers remain root-owned;
# the final USER directive switches only the running container.
RUN groupadd --gid 1000 aibox \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash aibox
ENV HOME=/home/aibox

# Base system: VCS, TLS roots, fetch/extract tools, a native compiler (for cgo,
# Rust crates, Python sdists, and node native modules), plus the common
# development and diagnostic commands an agent needs while investigating a
# project. Some diagnostics may pull in libpython as an ABI dependency, but no
# application Python command is installed.
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

WORKDIR /workspace
USER aibox

# No ENTRYPOINT: the Rust wrapper starts a login Bash that loads the Tenant
# environment and then executes the selected Tenant-local Coding Agent.
