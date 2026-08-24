# shellcheck shell=bash
# Managed by aibox. User shell customization belongs in ~/.bash_profile or ~/.bashrc.

: "${CARGO_HOME:=$HOME/.cargo}"
: "${RUSTUP_HOME:=$HOME/.rustup}"
: "${GOROOT:=$HOME/.goroot}"
: "${GOPATH:=$HOME/.gopath}"
: "${NPM_CONFIG_PREFIX:=$HOME/.npm-global}"
: "${UV_PYTHON_INSTALL_DIR:=$HOME/.python/cpython/releases}"
: "${UV_PYTHON_BIN_DIR:=$HOME/.python/bin}"

export CARGO_HOME RUSTUP_HOME GOROOT GOPATH NPM_CONFIG_PREFIX
export UV_PYTHON_INSTALL_DIR UV_PYTHON_BIN_DIR

aibox_prepend_path() {
    local wanted=$1 remaining=${PATH-} rebuilt='' segment more rebuilt_set=''
    if [[ -z ${PATH-} ]]; then
        PATH=$wanted
        return
    fi
    while :; do
        if [[ $remaining == *:* ]]; then
            segment=${remaining%%:*}
            remaining=${remaining#*:}
            more=1
        else
            segment=$remaining
            more=
        fi
        if [[ $segment != "$wanted" ]]; then
            rebuilt="${rebuilt}${rebuilt_set:+:}${segment}"
            rebuilt_set=1
        fi
        [[ -n $more ]] || break
    done
    PATH="$wanted${rebuilt_set:+:$rebuilt}"
}

# Prepend in reverse order so aibox-owned launchers win while the relative
# ordering and empty entries of the user's original PATH remain unchanged.
aibox_prepend_path "$GOPATH/bin"
aibox_prepend_path "$GOROOT/bin"
aibox_prepend_path "$CARGO_HOME/bin"
aibox_prepend_path "$NPM_CONFIG_PREFIX/bin"
aibox_prepend_path "$HOME/.node/current/bin"
aibox_prepend_path "$UV_PYTHON_BIN_DIR"
aibox_prepend_path "$HOME/.local/bin"
unset -f aibox_prepend_path

export PATH
export DISABLE_AUTOUPDATER=1
unset UV_NO_MANAGED_PYTHON
export UV_MANAGED_PYTHON=1
export UV_PYTHON_DOWNLOADS=manual
