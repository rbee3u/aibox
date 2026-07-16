# aibox-codex.Dockerfile
# Packages OpenAI Codex CLI on top of aibox-base, so high-risk projects can be
# run inside a container that IS the sandbox boundary.
#
# Codex ships its own OS sandbox (Seatbelt on macOS, Landlock+seccomp on Linux).
# We don't rely on it: aibox-codex launches with the sandbox bypassed, because
# the container is the boundary. See `agent.rs::build_codex` for the flags.
#
# Build:
#   aibox build codex

FROM aibox-base:latest

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
ENV PATH=/home/codex/go/bin:$PATH
# Debian's /etc/profile resets PATH for login shells. Codex snapshots command
# environments through a shell, so mirror the image PATH there too.
RUN printf "%s\n" \
        "# Keep login shells aligned with Docker's ENV PATH." \
        "export PATH=$PATH" \
    > /etc/profile.d/aibox-path.sh

WORKDIR /work
USER codex

# Image is immutable; upgrade by rebuilding (`aibox build codex`), not in place.
ENTRYPOINT ["codex"]
