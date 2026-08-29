#!/usr/bin/env bash
# Install one AIBox-owned uv + CPython generation without relying on a system
# Python. The current generation is published only after every health check.
set -euo pipefail

requested=${1:-}
if [[ -n $requested && ! $requested =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "invalid stable Python version: $requested" >&2
    exit 2
fi
if [[ -n $requested && $requested != 3.* ]]; then
    echo "unsupported CPython major version: $requested" >&2
    exit 2
fi

case "$(dpkg --print-architecture)" in
    amd64)
        python_arch=x86_64
        platform=x86_64-unknown-linux-gnu
        ;;
    arm64)
        python_arch=aarch64
        platform=aarch64-unknown-linux-gnu
        ;;
    *)
        echo "unsupported Python architecture" >&2
        exit 1
        ;;
esac

root=$HOME/.python
uv_releases=$root/uv/releases
python_releases=$root/cpython/releases
generations=$root/generations
python_bin=$root/bin
local_bin=$HOME/.local/bin

ensure_real_dir() {
    if [[ -L $1 || (-e $1 && ! -d $1) ]]; then
        echo "unsafe Python Component directory: $1" >&2
        exit 1
    fi
    mkdir -p -- "$1"
}

ensure_real_dir "$HOME/.local"
ensure_real_dir "$local_bin"
ensure_real_dir "$root"
ensure_real_dir "$root/uv"
ensure_real_dir "$uv_releases"
ensure_real_dir "$root/cpython"
ensure_real_dir "$python_releases"
ensure_real_dir "$generations"
ensure_real_dir "$python_bin"

owned_launcher_target() {
    local path=$1 name=$2 target expected actual
    [[ ! -e $path && ! -L $path ]] && return 0
    if [[ $name == python || $name == python3 || $name =~ ^python3\.[0-9]+$ ]]; then
        if [[ -L $path ]]; then
            target=$(readlink -- "$path")
            case "$target" in
                "$HOME/.python/current/bin/$name" | "/home/aibox/.python/current/bin/$name")
                    return 0
                    ;;
            esac
        fi
        if [[ -f $path && ! -L $path && -x $path ]]; then
            expected=$(printf '%s\n' \
                '#!/usr/bin/env bash' \
                'set -euo pipefail' \
                "exec \"\$HOME/.python/current/bin/$name\" \"\$@\"")
            actual=$(cat -- "$path")
            [[ $actual == "$expected" ]] && return 0
        fi
        return 1
    fi
    [[ -L $path ]] || return 1
    target=$(readlink -- "$path")
    case "$target" in
        "$HOME/.python/"* | /home/aibox/.python/*) return 0 ;;
        *) return 1 ;;
    esac
}

for name in uv uvx python python3 pip pip3; do
    if ! owned_launcher_target "$local_bin/$name" "$name"; then
        echo "refusing to replace unmanaged launcher: $local_bin/$name" >&2
        exit 1
    fi
done
shopt -s nullglob
for launcher in "$local_bin"/python3.*; do
    name=${launcher##*/}
    if [[ $name =~ ^python3\.[0-9]+$ ]] && ! owned_launcher_target "$launcher" "$name"; then
        echo "refusing to replace unmanaged launcher: $launcher" >&2
        exit 1
    fi
done
shopt -u nullglob

if [[ -e $root/current || -L $root/current ]]; then
    if [[ ! -L $root/current ]]; then
        echo "Python current generation is not an AIBox symlink" >&2
        exit 1
    fi
    case "$(readlink -- "$root/current")" in
        generations/* | "$HOME/.python/generations/"* | /home/aibox/.python/generations/*) ;;
        *)
            echo "Python current generation escapes the AIBox generation collection" >&2
            exit 1
            ;;
    esac
fi

staging=$(mktemp -d "$root/.staging.XXXXXX")
generation=
generation_stage=
published=0
cleanup() {
    rm -rf -- "$staging"
    if [[ -n $generation_stage ]]; then
        rm -rf -- "$generation_stage"
    fi
    if [[ $published == 0 && -n $generation ]]; then
        rm -rf -- "$generation"
    fi
}
trap cleanup EXIT

uv_installer=$staging/uv-installer.sh
curl -LsSf https://astral.sh/uv/install.sh -o "$uv_installer"
env \
    UV_UNMANAGED_INSTALL="$staging/uv" \
    UV_NO_MODIFY_PATH=1 \
    sh -x "$uv_installer"
test -x "$staging/uv/uv"
test -x "$staging/uv/uvx"
uv_version=$("$staging/uv/uv" --version | awk '{print $2}')
if [[ ! $uv_version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "official uv installer returned an unsupported version: $uv_version" >&2
    exit 1
fi

uv_release=$uv_releases/v$uv_version
if [[ -e $uv_release ]]; then
    if [[ -L $uv_release || ! -d $uv_release \
        || ! -x $uv_release/uv || ! -x $uv_release/uvx ]]; then
        echo "existing uv release is incomplete or unsafe: $uv_release" >&2
        exit 1
    fi
else
    mv -- "$staging/uv" "$uv_release"
fi
uv=$uv_release/uv
"$uv" --version
"$uv_release/uvx" --version

export UV_PYTHON_INSTALL_DIR=$python_releases
export UV_PYTHON_BIN_DIR=$python_bin
export UV_MANAGED_PYTHON=1
export UV_PYTHON_DOWNLOADS=manual
export UV_NO_CONFIG=1

request=${requested:-cpython@3}
"$uv" python install --default --managed-python "$request"
candidate_python=$(
    "$uv" python find \
        --managed-python \
        --no-project \
        --no-python-downloads \
        --resolve-links \
        "$request"
)
case "$candidate_python" in
    "$python_releases"/*/bin/python*) ;;
    *)
        echo "uv selected a Python outside the Component release collection: $candidate_python" >&2
        exit 1
        ;;
esac
test -x "$candidate_python"

python_version=$(
    "$candidate_python" -I -c \
        'import sys; print(".".join(map(str, sys.version_info[:3])))'
)
release_level=$(
    "$candidate_python" -I -c 'import sys; print(sys.version_info.releaselevel)'
)
if [[ ! $python_version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ \
    || $release_level != final ]]; then
    echo "uv selected a non-stable CPython release: $python_version ($release_level)" >&2
    exit 1
fi
if [[ -n $requested && $python_version != "$requested" ]]; then
    echo "uv selected Python $python_version instead of requested $requested" >&2
    exit 1
fi

python_release=${candidate_python%/bin/python*}
case "${python_release##*/}" in
    "cpython-$python_version-linux-$python_arch-gnu") ;;
    *)
        echo "uv selected a Python release for a different platform: $python_release" >&2
        exit 1
        ;;
esac

"$candidate_python" -I -c \
    'import bz2, ctypes, lzma, multiprocessing, sqlite3, ssl, venv'

python_minor=${python_version%.*}
generation_name="python-${python_version}__uv-${uv_version}__${platform}__$(date +%s)-$$"
generation=$generations/$generation_name
generation_stage=$(mktemp -d "$generations/.staging.XXXXXX")
"$candidate_python" -I -m venv "$generation_stage"
rm -f -- \
    "$generation_stage/bin/python" \
    "$generation_stage/bin/python3" \
    "$generation_stage/bin/python$python_minor"
ln -s "$uv_release/uv" "$generation_stage/bin/uv"
ln -s "$uv_release/uvx" "$generation_stage/bin/uvx"
ln -s "$candidate_python" "$generation_stage/bin/python"
ln -s "$candidate_python" "$generation_stage/bin/python3"
ln -s "$candidate_python" "$generation_stage/bin/python$python_minor"

for name in pip pip3; do
    install -m 0755 /dev/null "$generation_stage/bin/$name"
    # The quoted expressions intentionally belong to the generated wrapper.
    # shellcheck disable=SC2016
    printf '%s\n' \
        '#!/usr/bin/env bash' \
        'set -euo pipefail' \
        'launcher=$(readlink -f -- "${BASH_SOURCE[0]}")' \
        'bin=${launcher%/*}' \
        'exec "$bin/python" -m pip "$@"' \
        > "$generation_stage/bin/$name"
done

"$generation_stage/bin/uv" --version
"$generation_stage/bin/uvx" --version
"$generation_stage/bin/python" -I -c \
    'import bz2, ctypes, lzma, multiprocessing, sqlite3, ssl, sys, venv; assert sys.prefix != sys.base_prefix'
"$generation_stage/bin/pip" --version
mv -- "$generation_stage" "$generation"

publish_launcher() {
    local name=$1
    local temp=$local_bin/.aibox-python-$name.$$
    ln -s "/home/aibox/.python/current/bin/$name" "$temp"
    mv -Tf -- "$temp" "$local_bin/$name"
}
publish_python_launcher() {
    local name=$1
    local temp=$local_bin/.aibox-python-$name.$$
    install -m 0755 /dev/null "$temp"
    printf '%s\n' \
        '#!/usr/bin/env bash' \
        'set -euo pipefail' \
        "exec \"\$HOME/.python/current/bin/$name\" \"\$@\"" \
        > "$temp"
    mv -Tf -- "$temp" "$local_bin/$name"
}
shopt -s nullglob
for launcher in "$local_bin"/python3.*; do
    name=${launcher##*/}
    if [[ $name =~ ^python3\.[0-9]+$ && $name != "python$python_minor" ]]; then
        rm -f -- "$launcher"
    fi
done
shopt -u nullglob
for name in uv uvx pip pip3; do
    publish_launcher "$name"
done
for name in python python3 "python$python_minor"; do
    publish_python_launcher "$name"
done

current_temp=$root/.current.$$
ln -s "generations/$generation_name" "$current_temp"
mv -Tf -- "$current_temp" "$root/current"
published=1

"$HOME/.local/bin/uv" --version
"$HOME/.local/bin/uvx" --version
"$HOME/.local/bin/python" -I -c \
    'import sys; assert sys.prefix != sys.base_prefix; print("Python", ".".join(map(str, sys.version_info[:3])))'
"$HOME/.local/bin/pip" --version
