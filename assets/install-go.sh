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
IFS=$'\t' read -r version filename sha256 < <(
    jq -er --arg requested "$requested" --arg arch "$arch" '
        (if $requested == ""
         then [.[] | select(.stable == true)][0]
         else [.[] | select(.version == ("go" + $requested))][0]
         end) as $release
        | if $release == null
          then error("requested stable Go version was not found")
          else $release
          end
        | ([.files[]
            | select(.os == "linux" and .arch == $arch and .kind == "archive")][0]) as $file
        | if $file == null
          then error("no official Go archive for this architecture")
          else [(.version | ltrimstr("go")), $file.filename, $file.sha256] | @tsv
          end
    ' "$metadata"
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
