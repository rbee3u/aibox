#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Output};

fn write_fake_docker(bin: &Path) {
    let path = bin.join("docker");
    std::fs::write(
        &path,
        r#"#!/bin/sh
printf 'ARGS:' >> "$AIBOX_FAKE_DOCKER_LOG"
for arg in "$@"; do printf ' <%s>' "$arg" >> "$AIBOX_FAKE_DOCKER_LOG"; done
printf '\n' >> "$AIBOX_FAKE_DOCKER_LOG"
if [ "$1" = image ] && [ "$2" = inspect ]; then printf 'sha256:fake\n'; exit 0; fi
if [ "$1" = inspect ]; then printf 'false\n'; exit 0; fi
if [ "$1" = run ]; then
    shift
    while [ "$#" -gt 0 ]; do
        if [ "$1" = --cidfile ]; then printf 'fake-container\n' > "$2"; exit 0; fi
        shift
    done
fi
exit 99
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn command(home: &Path, root: &Path, bin: &Path) -> Command {
    let mut path = OsString::from(bin);
    path.push(":/usr/bin:/bin");
    let mut command = Command::new(env!("CARGO_BIN_EXE_aibox"));
    command
        .env_clear()
        .env("HOME", home)
        .env("AIBOX_ROOT", root)
        .env("PATH", path);
    command
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seed_codex_component(root: &Path, tenant: &str) {
    let home = root.join("tenants").join(tenant);
    let standalone = home.join(".codex/packages/standalone");
    let release = standalone.join("releases/1.2.3-x86_64-unknown-linux-musl");
    std::fs::create_dir_all(release.join("bin")).unwrap();
    std::fs::write(release.join("bin/codex"), b"fake codex\n").unwrap();
    std::fs::set_permissions(
        release.join("bin/codex"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    symlink(
        "/home/aibox/.codex/packages/standalone/releases/1.2.3-x86_64-unknown-linux-musl",
        standalone.join("current"),
    )
    .unwrap();
    symlink(
        "/home/aibox/.codex/packages/standalone/current/bin/codex",
        home.join(".local/bin/codex"),
    )
    .unwrap();
}

#[test]
fn process_environment_runs_with_fixed_runtime_image() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    let home = scratch.path().join("home");
    let bin = scratch.path().join("bin");
    let workspace = scratch.path().join("workspace");
    let log = scratch.path().join("docker.log");
    for directory in [&root, &home, &bin, &workspace] {
        std::fs::create_dir(directory).unwrap();
    }
    write_fake_docker(&bin);
    seed_codex_component(&root, "env-wired");
    let output = command(&home, &root, &bin)
        .current_dir(&workspace)
        .env("AIBOX_FAKE_DOCKER_LOG", &log)
        .args(["run", "--tenant", "env-wired", "--", "exec", "probe"])
        .output()
        .unwrap();
    assert_success(&output);
    let log = std::fs::read_to_string(log).unwrap();
    assert!(
        log.contains("<aibox:latest> </bin/bash> <--login> <-c>"),
        "{log}"
    );
    assert!(
        log.contains(
            "<aibox-tenant-environment> </home/aibox> <0> <0> <0> <0> <0> </home/aibox/.local/bin/codex> <exec> <probe>"
        ),
        "{log}"
    );
    assert!(root.join("tenants/env-wired/.codex").is_dir());
    assert!(!home.join(".aibox").exists());
}
