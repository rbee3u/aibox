#!/bin/bash
set -euo pipefail

requested="${1:-}"
installer="$(mktemp)"
cleanup() {
    rm -f "$installer"
}
trap cleanup EXIT INT TERM

export PATH="$HOME/.local/bin:${PATH:-/usr/local/bin:/usr/bin:/bin}"
export DISABLE_AUTOUPDATER=1

curl -fsSL https://claude.ai/install.sh -o "$installer"
if [ -n "$requested" ]; then
    bash -x "$installer" "$requested"
else
    bash -x "$installer"
fi
