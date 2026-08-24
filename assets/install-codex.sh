#!/bin/bash
set -euo pipefail

requested="${1:-}"
installer="$(mktemp)"
cleanup() {
    rm -f "$installer"
}
trap cleanup EXIT INT TERM

export CODEX_NON_INTERACTIVE=1
export CODEX_INSTALL_DIR="$HOME/.local/bin"
export PATH="$CODEX_INSTALL_DIR:${PATH:-/usr/local/bin:/usr/bin:/bin}"

curl -fsSL https://chatgpt.com/codex/install.sh -o "$installer"
if [ -n "$requested" ]; then
    sh "$installer" --release "$requested"
else
    sh "$installer"
fi
