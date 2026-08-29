//! Compose the exported Tenant Environment for Runs and Debug Shells.

use crate::agent::AgentInvocation;
use crate::component::InstalledComponentSnapshot;
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

/// Start login Bash, compose the Tenant Environment, and execute `final_command`.
pub(crate) fn build_command(
    final_command: &[OsString],
    components: InstalledComponentSnapshot,
) -> Vec<OsString> {
    build_command_for_home(final_command, OsStr::new(CONTAINER_HOME), components)
}

/// Compose login Bash and the Tenant Environment around one native Coding
/// Agent invocation.
pub(crate) fn build_agent_command(
    invocation: &AgentInvocation,
    components: InstalledComponentSnapshot,
) -> Vec<OsString> {
    build_command(invocation.command(), components)
}

pub(crate) fn build_command_for_home(
    final_command: &[OsString],
    home: &OsStr,
    components: InstalledComponentSnapshot,
) -> Vec<OsString> {
    let mut command = vec![
        OsString::from("/bin/bash"),
        OsString::from("--login"),
        OsString::from("-c"),
        OsString::from(BOOTSTRAP),
        OsString::from("aibox-tenant-environment"),
        home.to_os_string(),
        component_flag(components.node()),
        component_flag(components.claude()),
        component_flag(components.python()),
        component_flag(components.rust()),
        component_flag(components.go()),
    ];
    command.extend(final_command.iter().cloned());
    command
}

fn component_flag(installed: bool) -> OsString {
    OsString::from(if installed { "1" } else { "0" })
}

/// Build the shared environment wrapper around an interactive or stdin-driven Bash.
pub(crate) fn build_debug_command(components: InstalledComponentSnapshot) -> Vec<OsString> {
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
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[cfg(unix)]
    #[test]
    fn profile_values_win_and_existing_component_paths_are_appended() {
        let home = tempfile::tempdir().unwrap();
        let existing_user_bin = home.path().join("user-bin");
        let existing_local_bin = home.path().join(".local/bin");
        let existing_uv_bin = home.path().join("custom-python-bin");
        let existing_cargo_bin = home.path().join("custom-cargo/bin");
        for path in [
            &existing_user_bin,
            &existing_local_bin,
            &existing_uv_bin,
            &existing_cargo_bin,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            home.path().join(".bash_profile"),
            format!(
                "export PROFILE_VALUE=profile\n\
export BASHRC_VALUE=\n\
export HOME=/ignored\n\
export PATH='{user}:already::{local}:{user}'\n\
export CARGO_HOME='{cargo}'\n\
export RUSTUP_HOME=\n\
export UV_PYTHON_BIN_DIR='{uv}'\n\
export UV_MANAGED_PYTHON=0\n\
export UV_NO_MANAGED_PYTHON=1\n\
export UV_PYTHON_DOWNLOADS=automatic\n\
export DISABLE_AUTOUPDATER=0\n",
                user = existing_user_bin.display(),
                local = existing_local_bin.display(),
                cargo = home.path().join("custom-cargo").display(),
                uv = existing_uv_bin.display(),
            ),
        )
        .unwrap();
        fs::write(home.path().join(".bashrc"), b"export BASHRC_VALUE=bashrc\n").unwrap();

        let probe = home.path().join("probe");
        fs::write(
            &probe,
            br#"#!/bin/bash
printf '%s\n' "$HOME"
printf '%s|%s|%s|%s|%s|%s|%s\n' "$PROFILE_VALUE" "$BASHRC_VALUE" "$RUSTUP_HOME" "$DISABLE_AUTOUPDATER" "$UV_MANAGED_PYTHON" "$UV_NO_MANAGED_PYTHON" "$UV_PYTHON_DOWNLOADS"
printf '%s\n' "$PATH"
printf '%s\n' "$1"
"#,
        )
        .unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let final_command = [
            probe.into_os_string(),
            OsString::from("argument with spaces"),
        ];
        let command = build_command_for_home(
            &final_command,
            home.path().as_os_str(),
            InstalledComponentSnapshot::default(),
        );
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("PROFILE_VALUE")
            .env_remove("BASHRC_VALUE")
            .env_remove("CARGO_HOME")
            .env_remove("RUSTUP_HOME")
            .env_remove("GOROOT")
            .env_remove("GOPATH")
            .env_remove("NPM_CONFIG_PREFIX")
            .env_remove("UV_PYTHON_INSTALL_DIR")
            .env_remove("UV_PYTHON_BIN_DIR")
            .env_remove("DISABLE_AUTOUPDATER")
            .env_remove("UV_MANAGED_PYTHON")
            .env_remove("UV_NO_MANAGED_PYTHON")
            .env_remove("UV_PYTHON_DOWNLOADS")
            .output()
            .unwrap();

        assert!(output.status.success(), "{:?}", output);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let mut lines = stdout.lines();
        assert_eq!(lines.next(), Some(home.path().to_str().unwrap()));
        assert_eq!(lines.next(), Some("profile|||0|0|1|automatic"));
        let expected_path = format!(
            "{user}:already::{local}:{user}:{uv}:{cargo}",
            user = existing_user_bin.display(),
            local = existing_local_bin.display(),
            uv = existing_uv_bin.display(),
            cargo = existing_cargo_bin.display(),
        );
        assert_eq!(lines.next(), Some(expected_path.as_str()));
        assert_eq!(lines.next(), Some("argument with spaces"));
    }

    #[cfg(unix)]
    #[test]
    fn missing_values_get_defaults_while_explicit_empty_values_remain_empty() {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join(".bash_profile"),
            b"export CARGO_HOME=\nexport UV_PYTHON_BIN_DIR=\n",
        )
        .unwrap();
        let probe = home.path().join("probe");
        fs::write(
            &probe,
            br#"#!/bin/bash
printf '%s|%s|%s|%s|%s|%s\n' "$CARGO_HOME" "$RUSTUP_HOME" "$UV_PYTHON_BIN_DIR" "$UV_MANAGED_PYTHON" "$UV_PYTHON_DOWNLOADS" "$DISABLE_AUTOUPDATER"
"#,
        )
        .unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let command = build_command_for_home(
            &[probe.into_os_string()],
            home.path().as_os_str(),
            InstalledComponentSnapshot::for_tests(true, true, true, true, true),
        );
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("CARGO_HOME")
            .env_remove("RUSTUP_HOME")
            .env_remove("UV_PYTHON_BIN_DIR")
            .env_remove("UV_MANAGED_PYTHON")
            .env_remove("UV_NO_MANAGED_PYTHON")
            .env_remove("UV_PYTHON_DOWNLOADS")
            .env_remove("DISABLE_AUTOUPDATER")
            .output()
            .unwrap();

        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            format!("|{}/.rustup||1|manual|1", home.path().display()),
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_python_defaults_only_when_both_uv_switches_are_unset() {
        for (profile, expected) in [
            ("", "x:1|:"),
            ("export UV_MANAGED_PYTHON=\n", "x:|:"),
            ("export UV_NO_MANAGED_PYTHON=\n", ":|x:"),
            (
                "export UV_MANAGED_PYTHON=managed\nexport UV_NO_MANAGED_PYTHON=unmanaged\n",
                "x:managed|x:unmanaged",
            ),
        ] {
            let home = tempfile::tempdir().unwrap();
            fs::write(home.path().join(".bash_profile"), profile).unwrap();
            let probe = home.path().join("probe");
            fs::write(
                &probe,
                br#"#!/bin/bash
printf '%s:%s|%s:%s\n' "${UV_MANAGED_PYTHON+x}" "${UV_MANAGED_PYTHON-}" "${UV_NO_MANAGED_PYTHON+x}" "${UV_NO_MANAGED_PYTHON-}"
"#,
            )
            .unwrap();
            fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

            let command = build_command_for_home(
                &[probe.into_os_string()],
                home.path().as_os_str(),
                InstalledComponentSnapshot::for_tests(false, false, true, false, false),
            );
            let output = Command::new(&command[0])
                .args(&command[1..])
                .env("HOME", home.path())
                .env("SHELL", "/bin/bash")
                .env_remove("UV_MANAGED_PYTHON")
                .env_remove("UV_NO_MANAGED_PYTHON")
                .output()
                .unwrap();

            assert!(output.status.success(), "{profile:?}: {output:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim_end(),
                expected,
                "{profile:?}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn path_candidates_are_inserted_before_the_system_anchor_in_stable_order() {
        let home = tempfile::tempdir().unwrap();
        let uv = home.path().join("uv-bin");
        let npm = home.path().join("npm-prefix");
        let cargo = home.path().join("cargo-home");
        let goroot = home.path().join("go-root");
        let gopath = home.path().join("go-path");
        for path in [
            home.path().join(".local/bin"),
            uv.clone(),
            home.path().join(".node/current/bin"),
            npm.join("bin"),
            cargo.join("bin"),
            goroot.join("bin"),
            gopath.join("bin"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            home.path().join(".bash_profile"),
            format!(
                "export PATH='before::before:/usr/local/bin:/usr/bin:/bin'\n\
export UV_PYTHON_BIN_DIR='{uv}'\n\
export NPM_CONFIG_PREFIX='{npm}'\n\
export CARGO_HOME='{cargo}'\n\
export GOROOT='{goroot}'\n\
export GOPATH='{gopath}'\n",
                uv = uv.display(),
                npm = npm.display(),
                cargo = cargo.display(),
                goroot = goroot.display(),
                gopath = gopath.display(),
            ),
        )
        .unwrap();
        let probe = home.path().join("probe");
        fs::write(&probe, b"#!/bin/bash\nprintf '%s\\n' \"$PATH\"\n").unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let command = build_command_for_home(
            &[probe.into_os_string()],
            home.path().as_os_str(),
            InstalledComponentSnapshot::default(),
        );
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("UV_PYTHON_BIN_DIR")
            .env_remove("NPM_CONFIG_PREFIX")
            .env_remove("CARGO_HOME")
            .env_remove("GOROOT")
            .env_remove("GOPATH")
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            format!(
                "before::before:{local}:{uv}:{node}:{npm}:{cargo}:{goroot}:{gopath}:/usr/local/bin:/usr/bin:/bin",
                local = home.path().join(".local/bin").display(),
                uv = uv.display(),
                node = home.path().join(".node/current/bin").display(),
                npm = npm.join("bin").display(),
                cargo = cargo.join("bin").display(),
                goroot = goroot.join("bin").display(),
                gopath = gopath.join("bin").display(),
            ),
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_candidates_use_the_last_exact_system_anchor() {
        let home = tempfile::tempdir().unwrap();
        let local = home.path().join(".local/bin");
        fs::create_dir_all(&local).unwrap();
        fs::write(
            home.path().join(".bash_profile"),
            b"export PATH='user:/usr/local/bin:middle:/usr/local/bin:/usr/bin'\n",
        )
        .unwrap();
        let probe = home.path().join("probe");
        fs::write(&probe, b"#!/bin/bash\nprintf '%s\\n' \"$PATH\"\n").unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let command = build_command_for_home(
            &[probe.into_os_string()],
            home.path().as_os_str(),
            InstalledComponentSnapshot::default(),
        );
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("UV_PYTHON_BIN_DIR")
            .env_remove("NPM_CONFIG_PREFIX")
            .env_remove("CARGO_HOME")
            .env_remove("GOROOT")
            .env_remove("GOPATH")
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            format!(
                "user:/usr/local/bin:middle:{local}:/usr/local/bin:/usr/bin",
                local = local.display(),
            ),
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_path_candidates_keep_their_profile_position() {
        let home = tempfile::tempdir().unwrap();
        let local = home.path().join(".local/bin");
        let uv = home.path().join("uv-bin");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&uv).unwrap();
        fs::write(
            home.path().join(".bash_profile"),
            format!(
                "export PATH='user:/usr/local/bin:/usr/bin:{local}'\n\
export UV_PYTHON_BIN_DIR='{uv}'\n",
                local = local.display(),
                uv = uv.display(),
            ),
        )
        .unwrap();
        let probe = home.path().join("probe");
        fs::write(&probe, b"#!/bin/bash\nprintf '%s\\n' \"$PATH\"\n").unwrap();
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

        let command = build_command_for_home(
            &[probe.into_os_string()],
            home.path().as_os_str(),
            InstalledComponentSnapshot::default(),
        );
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("NPM_CONFIG_PREFIX")
            .env_remove("CARGO_HOME")
            .env_remove("GOROOT")
            .env_remove("GOPATH")
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            format!(
                "user:{uv}:/usr/local/bin:/usr/bin:{local}",
                uv = uv.display(),
                local = local.display(),
            ),
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_candidates_append_when_the_exact_anchor_is_absent() {
        for profile in [
            "export PATH='user:/usr/local/bin/:/opt/usr/local/bin'\n",
            "export PATH=\n",
            "unset PATH\n",
        ] {
            let home = tempfile::tempdir().unwrap();
            let local = home.path().join(".local/bin");
            fs::create_dir_all(&local).unwrap();
            fs::write(home.path().join(".bash_profile"), profile).unwrap();
            let probe = home.path().join("probe");
            fs::write(&probe, b"#!/bin/bash\nprintf '%s\\n' \"$PATH\"\n").unwrap();
            fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

            let command = build_command_for_home(
                &[probe.into_os_string()],
                home.path().as_os_str(),
                InstalledComponentSnapshot::default(),
            );
            let output = Command::new(&command[0])
                .args(&command[1..])
                .env("HOME", home.path())
                .env("SHELL", "/bin/bash")
                .env_remove("UV_PYTHON_BIN_DIR")
                .env_remove("NPM_CONFIG_PREFIX")
                .env_remove("CARGO_HOME")
                .env_remove("GOROOT")
                .env_remove("GOPATH")
                .output()
                .unwrap();

            assert!(output.status.success(), "{profile:?}: {output:?}");
            let expected = match profile {
                "export PATH='user:/usr/local/bin/:/opt/usr/local/bin'\n" => {
                    format!(
                        "user:/usr/local/bin/:/opt/usr/local/bin:{}",
                        local.display()
                    )
                }
                "export PATH=\n" => format!(":{}", local.display()),
                "unset PATH\n" => local.display().to_string(),
                _ => unreachable!(),
            };
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim_end(),
                expected,
                "{profile:?}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn defaults_are_injected_only_for_the_installed_owning_component() {
        const MANAGED_NAMES: &[&str] = &[
            "CARGO_HOME",
            "DISABLE_AUTOUPDATER",
            "GOPATH",
            "GOROOT",
            "NPM_CONFIG_PREFIX",
            "RUSTUP_HOME",
            "UV_MANAGED_PYTHON",
            "UV_NO_MANAGED_PYTHON",
            "UV_PYTHON_BIN_DIR",
            "UV_PYTHON_DOWNLOADS",
            "UV_PYTHON_INSTALL_DIR",
        ];
        let cases: [(InstalledComponentSnapshot, &[&str]); 6] = [
            (InstalledComponentSnapshot::default(), &[]),
            (
                InstalledComponentSnapshot::for_tests(true, false, false, false, false),
                &["NPM_CONFIG_PREFIX"],
            ),
            (
                InstalledComponentSnapshot::for_tests(false, true, false, false, false),
                &["DISABLE_AUTOUPDATER"],
            ),
            (
                InstalledComponentSnapshot::for_tests(false, false, true, false, false),
                &[
                    "UV_MANAGED_PYTHON",
                    "UV_PYTHON_BIN_DIR",
                    "UV_PYTHON_DOWNLOADS",
                    "UV_PYTHON_INSTALL_DIR",
                ],
            ),
            (
                InstalledComponentSnapshot::for_tests(false, false, false, true, false),
                &["CARGO_HOME", "RUSTUP_HOME"],
            ),
            (
                InstalledComponentSnapshot::for_tests(false, false, false, false, true),
                &["GOPATH", "GOROOT"],
            ),
        ];

        for (components, expected_names) in cases {
            let home = tempfile::tempdir().unwrap();
            fs::write(home.path().join(".bash_profile"), b"").unwrap();
            let command = build_command_for_home(
                &[OsString::from("/usr/bin/env")],
                home.path().as_os_str(),
                components,
            );
            let mut process = Command::new(&command[0]);
            process
                .args(&command[1..])
                .env("HOME", home.path())
                .env("SHELL", "/bin/bash");
            for name in MANAGED_NAMES {
                process.env_remove(name);
            }
            let output = process.output().unwrap();
            assert!(output.status.success(), "{components:?}: {output:?}");
            let environment: BTreeMap<_, _> = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            let actual_names: Vec<_> = MANAGED_NAMES
                .iter()
                .copied()
                .filter(|name| environment.contains_key(*name))
                .collect();
            assert_eq!(actual_names, expected_names, "{components:?}");

            for name in expected_names.iter().copied() {
                let expected = match name {
                    "CARGO_HOME" => home.path().join(".cargo").display().to_string(),
                    "DISABLE_AUTOUPDATER" | "UV_MANAGED_PYTHON" => "1".to_string(),
                    "GOPATH" => home.path().join(".gopath").display().to_string(),
                    "GOROOT" => home.path().join(".goroot").display().to_string(),
                    "NPM_CONFIG_PREFIX" => home.path().join(".npm-global").display().to_string(),
                    "RUSTUP_HOME" => home.path().join(".rustup").display().to_string(),
                    "UV_PYTHON_BIN_DIR" => home.path().join(".python/bin").display().to_string(),
                    "UV_PYTHON_DOWNLOADS" => "manual".to_string(),
                    "UV_PYTHON_INSTALL_DIR" => home
                        .path()
                        .join(".python/cpython/releases")
                        .display()
                        .to_string(),
                    _ => unreachable!("unexpected managed variable {name}"),
                };
                assert_eq!(environment.get(name), Some(&expected), "{components:?}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn preserved_user_tool_paths_do_not_require_a_component_or_export_defaults() {
        for (profile, expected_marker, expect_path) in [
            ("export PATH=base\n", "", true),
            ("export PATH=base\nexport CARGO_HOME=\n", "x", false),
        ] {
            let home = tempfile::tempdir().unwrap();
            fs::create_dir_all(home.path().join(".cargo/bin")).unwrap();
            fs::write(home.path().join(".bash_profile"), profile).unwrap();
            let probe = home.path().join("probe");
            fs::write(
                &probe,
                b"#!/bin/bash\nprintf '%s|%s|%s\\n' \"$PATH\" \"${CARGO_HOME+x}\" \"${CARGO_HOME-}\"\n",
            )
            .unwrap();
            fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

            let command = build_command_for_home(
                &[probe.into_os_string()],
                home.path().as_os_str(),
                InstalledComponentSnapshot::default(),
            );
            let mut process = Command::new(&command[0]);
            process
                .args(&command[1..])
                .env("HOME", home.path())
                .env("SHELL", "/bin/bash");
            for name in [
                "CARGO_HOME",
                "RUSTUP_HOME",
                "GOROOT",
                "GOPATH",
                "NPM_CONFIG_PREFIX",
                "UV_PYTHON_INSTALL_DIR",
                "UV_PYTHON_BIN_DIR",
                "UV_MANAGED_PYTHON",
                "UV_NO_MANAGED_PYTHON",
                "UV_PYTHON_DOWNLOADS",
                "DISABLE_AUTOUPDATER",
            ] {
                process.env_remove(name);
            }
            let output = process.output().unwrap();

            assert!(output.status.success(), "{profile:?}: {output:?}");
            let expected_path = if expect_path {
                format!("base:{}", home.path().join(".cargo/bin").display())
            } else {
                "base".to_string()
            };
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim_end(),
                format!("{expected_path}|{expected_marker}|"),
                "{profile:?}",
            );
        }
    }
}
