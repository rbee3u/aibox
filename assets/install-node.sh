#!/bin/bash
set -euo pipefail

requested="${1:-}"
case "$requested" in
    "") ;;
    *[!0-9.]* | .* | *..* | *.)
        echo "invalid Node.js version: $requested (expected X.Y.Z)" >&2
        exit 2
        ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) arch=x64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *)
        echo "unsupported Node.js architecture: $(uname -m)" >&2
        exit 2
        ;;
esac

if [ -n "$requested" ]; then
    version="v$requested"
else
    version="$(
        curl -fsSL https://nodejs.org/dist/index.json |
            jq -r '[.[] | select(.version | test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))][0].version'
    )"
fi

if ! [[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "failed to resolve a stable Node.js version" >&2
    exit 1
fi

root="$HOME/.node"
releases="$root/releases"
release="$releases/$version"
archive="node-$version-linux-$arch.tar.xz"
base_url="https://nodejs.org/dist/$version"

mkdir -p "$releases"
tmp="$(mktemp -d "$root/.install.XXXXXX")"
stage="$releases/.staging.$version.$$"
cleanup() {
    rm -rf "$tmp" "$stage"
}
trap cleanup EXIT INT TERM

curl -fsSL "$base_url/SHASUMS256.txt" -o "$tmp/SHASUMS256.txt"
expected="$(awk -v archive="$archive" '$2 == archive { print $1; exit }' "$tmp/SHASUMS256.txt")"
if ! [[ "$expected" =~ ^[0-9a-f]{64}$ ]]; then
    echo "Node.js checksum is unavailable for $archive" >&2
    exit 1
fi

curl -fsSL "$base_url/$archive" -o "$tmp/$archive"
actual="$(sha256sum "$tmp/$archive" | awk '{ print $1 }')"
if [ "$actual" != "$expected" ]; then
    echo "Node.js checksum mismatch for $archive" >&2
    exit 1
fi

mkdir "$stage"
tar -xJf "$tmp/$archive" -C "$stage" --strip-components=1
test -x "$stage/bin/node"
test -e "$stage/bin/npm"

if [ -e "$release" ] || [ -L "$release" ]; then
    rm -rf "$release"
fi
mv "$stage" "$release"

next="$root/.current.$$"
ln -s "releases/$version" "$next"
mv -Tf "$next" "$root/current"

printf 'Node.js %s installed successfully.\n' "${version#v}"
