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
        TenantEnvironmentCapabilities::default(),
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
        TenantEnvironmentCapabilities {
            node: true,
            claude: true,
            python: true,
            rust: true,
            go: true,
        },
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
            TenantEnvironmentCapabilities {
                python: true,
                ..Default::default()
            },
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
        TenantEnvironmentCapabilities::default(),
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
        TenantEnvironmentCapabilities::default(),
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
        TenantEnvironmentCapabilities::default(),
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
            TenantEnvironmentCapabilities::default(),
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
    let cases: [(TenantEnvironmentCapabilities, &[&str]); 6] = [
        (TenantEnvironmentCapabilities::default(), &[]),
        (
            TenantEnvironmentCapabilities {
                node: true,
                ..Default::default()
            },
            &["NPM_CONFIG_PREFIX"],
        ),
        (
            TenantEnvironmentCapabilities {
                claude: true,
                ..Default::default()
            },
            &["DISABLE_AUTOUPDATER"],
        ),
        (
            TenantEnvironmentCapabilities {
                python: true,
                ..Default::default()
            },
            &[
                "UV_MANAGED_PYTHON",
                "UV_PYTHON_BIN_DIR",
                "UV_PYTHON_DOWNLOADS",
                "UV_PYTHON_INSTALL_DIR",
            ],
        ),
        (
            TenantEnvironmentCapabilities {
                rust: true,
                ..Default::default()
            },
            &["CARGO_HOME", "RUSTUP_HOME"],
        ),
        (
            TenantEnvironmentCapabilities {
                go: true,
                ..Default::default()
            },
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
            TenantEnvironmentCapabilities::default(),
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
