#!/usr/bin/env bash
# Runs in the shared image with only the Tenant Home mounted. Preserve Cargo
# user state while installing or replacing the selected stable rustup toolchain.
set -euo pipefail

requested=${1:-}
if [[ -n $requested && ! $requested =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "invalid stable Rust version: $requested" >&2
    exit 2
fi

export CARGO_HOME=$HOME/.cargo
export RUSTUP_HOME=$HOME/.rustup
rustup=$CARGO_HOME/bin/rustup
if [[ ! -x $rustup ]]; then
    bootstrap=$(mktemp)
    trap 'rm -f "$bootstrap"' EXIT
    curl -fsSL https://sh.rustup.rs -o "$bootstrap"
    sh "$bootstrap" -y --no-modify-path --profile minimal --default-toolchain none
fi

installed_stable_alias=0
version=$requested
if [[ -z $version ]]; then
    "$rustup" toolchain install stable --profile minimal
    version=$("$rustup" run stable rustc --version | awk '{print $2}')
    installed_stable_alias=1
fi
if [[ ! $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "rustup returned an invalid stable Rust version: $version" >&2
    exit 1
fi

current_toolchain=$("$rustup" show active-toolchain 2>/dev/null | awk '{print $1}' || true)
if [[ ${current_toolchain%%-*} == "$version" ]] \
    && "$rustup" run "$current_toolchain" rustc --version >/dev/null 2>&1; then
    if [[ $installed_stable_alias == 1 ]]; then
        "$rustup" toolchain uninstall stable
    fi
    echo "Rust $version is already installed; skipping"
    exit 0
fi
if [[ -n $current_toolchain ]] \
    && "$rustup" toolchain list | sed 's/ (.*//' | grep -Fxq "$current_toolchain"; then
    "$rustup" toolchain uninstall "$current_toolchain"
fi

"$rustup" toolchain install "$version" --profile minimal
"$rustup" default "$version"
if [[ $installed_stable_alias == 1 ]]; then
    "$rustup" toolchain uninstall stable
fi
"$CARGO_HOME/bin/rustc" --version
