//! Compose the exported Tenant Environment for Runs and Debug Shells.

use crate::agent::AgentInvocation;
use std::ffi::{OsStr, OsString};

pub(crate) const CONTAINER_HOME: &str = "/home/aibox";

const BOOTSTRAP: &str = r#"HOME=$1
export HOME
shift
aibox_node_installed=$1
aibox_claude_installed=$2
aibox_python_installed=$3
aibox_rust_installed=$4
aibox_go_installed=$5
shift 5

if [[ $aibox_node_installed == 1 && -z ${NPM_CONFIG_PREFIX+x} ]]; then
    NPM_CONFIG_PREFIX=$HOME/.npm-global
fi
if [[ $aibox_claude_installed == 1 && -z ${DISABLE_AUTOUPDATER+x} ]]; then
    DISABLE_AUTOUPDATER=1
fi
if [[ $aibox_python_installed == 1 ]]; then
    if [[ -z ${UV_PYTHON_INSTALL_DIR+x} ]]; then
        UV_PYTHON_INSTALL_DIR=$HOME/.python/cpython/releases
    fi
    if [[ -z ${UV_PYTHON_BIN_DIR+x} ]]; then UV_PYTHON_BIN_DIR=$HOME/.python/bin; fi
    if [[ -z ${UV_MANAGED_PYTHON+x} && -z ${UV_NO_MANAGED_PYTHON+x} ]]; then
        UV_MANAGED_PYTHON=1
    fi
    if [[ -z ${UV_PYTHON_DOWNLOADS+x} ]]; then UV_PYTHON_DOWNLOADS=manual; fi
fi
if [[ $aibox_rust_installed == 1 ]]; then
    if [[ -z ${CARGO_HOME+x} ]]; then CARGO_HOME=$HOME/.cargo; fi
    if [[ -z ${RUSTUP_HOME+x} ]]; then RUSTUP_HOME=$HOME/.rustup; fi
fi
if [[ $aibox_go_installed == 1 ]]; then
    if [[ -z ${GOROOT+x} ]]; then GOROOT=$HOME/.goroot; fi
    if [[ -z ${GOPATH+x} ]]; then GOPATH=$HOME/.gopath; fi
fi

aibox_export_if_set() {
    local name
    for name in "$@"; do
        if declare -p "$name" >/dev/null 2>&1; then export "$name"; fi
    done
}
aibox_export_if_set CARGO_HOME RUSTUP_HOME GOROOT GOPATH NPM_CONFIG_PREFIX
aibox_export_if_set UV_PYTHON_INSTALL_DIR UV_PYTHON_BIN_DIR
aibox_export_if_set UV_MANAGED_PYTHON UV_NO_MANAGED_PYTHON UV_PYTHON_DOWNLOADS
aibox_export_if_set DISABLE_AUTOUPDATER
unset -f aibox_export_if_set

aibox_add_path() {
    local wanted=$1 remaining segment more padded before_anchor
    [[ -n $wanted && $wanted != *:* && -d $wanted ]] || return
    if [[ -n ${PATH+x} ]]; then
        remaining=$PATH
        while :; do
            if [[ $remaining == *:* ]]; then
                segment=${remaining%%:*}
                remaining=${remaining#*:}
                more=1
            else
                segment=$remaining
                more=
            fi
            [[ $segment != "$wanted" ]] || return
            [[ -n $more ]] || break
        done
        padded=:$PATH:
        before_anchor=${padded%:/usr/local/bin:*}
        if [[ $before_anchor != "$padded" ]]; then
            padded="${before_anchor}:$wanted${padded#"$before_anchor"}"
            PATH=${padded#:}
            PATH=${PATH%:}
        else
            PATH="${PATH}:$wanted"
        fi
    else
        PATH=$wanted
    fi
    export PATH
}

aibox_add_path_from_variable() {
    local name=$1 fallback=$2 suffix=$3 value
    if declare -p "$name" >/dev/null 2>&1; then
        value=${!name}
        [[ -n $value ]] || return
    else
        value=$fallback
    fi
    aibox_add_path "$value$suffix"
}

aibox_add_path "$HOME/.local/bin"
aibox_add_path_from_variable UV_PYTHON_BIN_DIR "$HOME/.python/bin" ''
aibox_add_path "$HOME/.node/current/bin"
aibox_add_path_from_variable NPM_CONFIG_PREFIX "$HOME/.npm-global" /bin
aibox_add_path_from_variable CARGO_HOME "$HOME/.cargo" /bin
aibox_add_path_from_variable GOROOT "$HOME/.goroot" /bin
aibox_add_path_from_variable GOPATH "$HOME/.gopath" /bin
unset -f aibox_add_path aibox_add_path_from_variable
unset aibox_node_installed aibox_claude_installed aibox_python_installed
unset aibox_rust_installed aibox_go_installed

exec "$@""#;

/// Healthy Components that contribute defaults to a Tenant Environment.
///
/// Construct this with a struct literal so each capability is named at the call
/// site; five positional booleans would be indistinguishable to the compiler.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TenantEnvironmentCapabilities {
    pub(crate) node: bool,
    pub(crate) claude: bool,
    pub(crate) python: bool,
    pub(crate) rust: bool,
    pub(crate) go: bool,
}

/// Start login Bash, compose the Tenant Environment, and execute `final_command`.
pub(crate) fn build_command(
    final_command: &[OsString],
    components: TenantEnvironmentCapabilities,
) -> Vec<OsString> {
    build_command_for_home(final_command, OsStr::new(CONTAINER_HOME), components)
}

/// Compose login Bash and the Tenant Environment around one native Coding
/// Agent invocation.
pub(crate) fn build_agent_command(
    invocation: &AgentInvocation,
    components: TenantEnvironmentCapabilities,
) -> Vec<OsString> {
    build_command(invocation.command(), components)
}

pub(crate) fn build_command_for_home(
    final_command: &[OsString],
    home: &OsStr,
    components: TenantEnvironmentCapabilities,
) -> Vec<OsString> {
    let mut command = vec![
        OsString::from("/bin/bash"),
        OsString::from("--login"),
        OsString::from("-c"),
        OsString::from(BOOTSTRAP),
        OsString::from("aibox-tenant-environment"),
        home.to_os_string(),
        component_flag(components.node),
        component_flag(components.claude),
        component_flag(components.python),
        component_flag(components.rust),
        component_flag(components.go),
    ];
    command.extend(final_command.iter().cloned());
    command
}

fn component_flag(installed: bool) -> OsString {
    OsString::from(if installed { "1" } else { "0" })
}

/// Build the shared environment wrapper around an interactive or stdin-driven Bash.
pub(crate) fn build_debug_command(components: TenantEnvironmentCapabilities) -> Vec<OsString> {
    build_command(
        &[
            OsString::from("/bin/bash"),
            OsString::from("--noprofile"),
            OsString::from("--norc"),
        ],
        components,
    )
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
