#!/usr/bin/env bash
# Runs in the shared image with only the Tenant Home mounted. Verify the
# official archive before replacing .goroot, and leave .gopath untouched.
set -euo pipefail

requested=${1:-}
if [[ -n $requested && ! $requested =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "invalid stable Go version: $requested" >&2
    exit 2
fi

case "$(dpkg --print-architecture)" in
    amd64) arch=amd64 ;;
    arm64) arch=arm64 ;;
    *) echo "unsupported Go architecture" >&2; exit 1 ;;
esac

metadata=$(mktemp)
archive=$(mktemp)
extract=$(mktemp -d)
trap 'rm -f "$metadata" "$archive"; rm -rf "$extract"' EXIT
curl -fsSL 'https://go.dev/dl/?mode=json&include=all' -o "$metadata"
read -r version filename sha256 < <(
    python3 - "$metadata" "$requested" "$arch" <<'PY'
import json
import pathlib
import sys

releases = json.loads(pathlib.Path(sys.argv[1]).read_text())
requested = sys.argv[2]
arch = sys.argv[3]
release = next(
    (item for item in releases if item["version"] == f"go{requested}"),
    None,
) if requested else next((item for item in releases if item.get("stable")), None)
if release is None:
    raise SystemExit("requested stable Go version was not found")
archive_file = next(
    (
        item
        for item in release["files"]
        if item["os"] == "linux"
        and item["arch"] == arch
        and item["kind"] == "archive"
    ),
    None,
)
if archive_file is None:
    raise SystemExit("no official Go archive for this architecture")
print(
    release["version"].removeprefix("go"),
    archive_file["filename"],
    archive_file["sha256"],
)
PY
)

if [[ -x $HOME/.goroot/bin/go \
    && -f $HOME/.goroot/VERSION \
    && $(head -n 1 "$HOME/.goroot/VERSION") == "go$version" ]]; then
    echo "Go $version is already installed; skipping"
    exit 0
fi

curl -fsSL "https://go.dev/dl/$filename" -o "$archive"
printf '%s  %s\n' "$sha256" "$archive" | sha256sum -c -
tar -C "$extract" -xzf "$archive"
test -x "$extract/go/bin/go"

rm -rf -- "$HOME/.goroot"
mv "$extract/go" "$HOME/.goroot"
"$HOME/.goroot/bin/go" version
echo "installed Go $version"
