#!/usr/bin/env bash
set -euo pipefail

target=${1:-}
if [[ -z $target ]]; then
    manifest=$(mktemp)
    trap 'rm -f "$manifest"' EXIT
    curl -fsSL https://static.rust-lang.org/dist/channel-rust-stable.toml -o "$manifest"
    target=$(python3 - "$manifest" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["pkg"]["rust"]["version"].split()[0])
PY
    )
fi

if [[ ! $target =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "invalid stable Rust version: $target" >&2
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

old=$(
    python3 - "$RUSTUP_HOME/settings.toml" <<'PY'
import pathlib
import sys
import tomllib

path = pathlib.Path(sys.argv[1])
if path.is_file():
    print(tomllib.loads(path.read_text()).get("default_toolchain", ""))
PY
)
if [[ ${old%%-*} == "$target" && -x $CARGO_HOME/bin/rustc ]]; then
    echo "Rust $target is already installed; skipping"
    exit 0
fi
if [[ -n $old ]] && "$rustup" toolchain list | sed 's/ (.*//' | grep -Fxq "$old"; then
    "$rustup" toolchain uninstall "$old"
fi

"$rustup" toolchain install "$target" --profile minimal
"$rustup" default "$target"
"$CARGO_HOME/bin/rustc" --version
