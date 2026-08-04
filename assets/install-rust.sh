#!/usr/bin/env bash
# Runs in the shared image with only the Tenant Home mounted. Preserve Cargo
# user state while installing or replacing the selected stable rustup toolchain.
set -euo pipefail

version=${1:-}
if [[ -z $version ]]; then
    manifest=$(mktemp)
    trap 'rm -f "$manifest"' EXIT
    curl -fsSL https://static.rust-lang.org/dist/channel-rust-stable.toml -o "$manifest"
    version=$(python3 - "$manifest" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["pkg"]["rust"]["version"].split()[0])
PY
    )
fi

if [[ ! $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "invalid stable Rust version: $version" >&2
    exit 2
fi

export CARGO_HOME=$HOME/.cargo
export RUSTUP_HOME=$HOME/.rustup
rustup=$CARGO_HOME/bin/rustup
if [[ ! -x $rustup ]]; then
    bootstrap=$(mktemp)
    trap 'rm -f "${manifest:-}" "$bootstrap"' EXIT
    curl -fsSL https://sh.rustup.rs -o "$bootstrap"
    sh "$bootstrap" -y --no-modify-path --profile minimal --default-toolchain none
fi

current_toolchain=$(
    python3 - "$RUSTUP_HOME/settings.toml" <<'PY'
import pathlib
import sys
import tomllib

path = pathlib.Path(sys.argv[1])
if path.is_file():
    print(tomllib.loads(path.read_text()).get("default_toolchain", ""))
PY
)
if [[ ${current_toolchain%%-*} == "$version" ]] \
    && "$rustup" run "$current_toolchain" rustc --version >/dev/null 2>&1; then
    echo "Rust $version is already installed; skipping"
    exit 0
fi
if [[ -n $current_toolchain ]] \
    && "$rustup" toolchain list | sed 's/ (.*//' | grep -Fxq "$current_toolchain"; then
    "$rustup" toolchain uninstall "$current_toolchain"
fi

"$rustup" toolchain install "$version" --profile minimal
"$rustup" default "$version"
"$CARGO_HOME/bin/rustc" --version
