# aibox-claude.Dockerfile
# Packages Claude Code on top of aibox-base, so high-risk projects can be run
# inside a container that IS the sandbox boundary.
#
# Build:
#   aibox build claude

FROM aibox-base:latest

# --- Claude Code -------------------------------------------------------------
RUN npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force

# Recreate a predictable non-root user at uid/gid 1000 so the mounted home has
# a stable path.
RUN groupadd --gid 1000 claude \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash claude

ENV HOME=/home/claude
# GOPATH lives in the mounted home => module cache persists per profile.
ENV GOPATH=/home/claude/go
ENV PATH=/home/claude/go/bin:$PATH
# Debian's /etc/profile resets PATH for login shells. Agent command execution
# can go through a shell, so mirror the image PATH there too.
RUN printf "%s\n" \
        "# Keep login shells aligned with Docker's ENV PATH." \
        "export PATH=$PATH" \
    > /etc/profile.d/aibox-path.sh
# Image is immutable; update by rebuilding, not self-updating.
ENV DISABLE_AUTOUPDATER=1

WORKDIR /work
USER claude

ENTRYPOINT ["claude"]
