#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write_fake_docker(bin: &Path) -> PathBuf {
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
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

fn clean_command(home: &Path, root: &Path, bin: &Path) -> Command {
    let mut path = OsString::from(bin);
    path.push(":/usr/bin:/bin");
    let mut command = Command::new(env!("CARGO_BIN_EXE_aibox"));
    command
        .env_clear()
        .env("HOME", home)
        .env("AIBOX_ROOT", root)
        .env("PATH", path)
        .env("LC_ALL", "C");
    command
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn process_environment_wires_root_home_image_and_docker_into_a_run() {
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

    let output = clean_command(&home, &root, &bin)
        .current_dir(&workspace)
        .env("AIBOX_IMAGE", "registry.example/aibox:test")
        .env("AIBOX_FAKE_DOCKER_LOG", &log)
        .args(["run", "--tenant", "env-wired", "--", "exec", "probe"])
        .output()
        .unwrap();

    assert_success(&output);
    let log = std::fs::read_to_string(log).unwrap();
    assert!(
        log.contains("<registry.example/aibox:test> <codex> <exec> <probe>"),
        "{log}"
    );
    assert!(root.join("tenants/env-wired/.codex").is_dir());
    assert!(!home.join(".aibox").exists());
}

#[test]
fn completion_environment_protocol_reads_the_explicit_root_and_index() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    let home = scratch.path().join("home");
    let bin = scratch.path().join("bin");
    std::fs::create_dir_all(root.join("tenants/work")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&bin).unwrap();

    let output = clean_command(&home, &root, &bin)
        .env("AIBOX_COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "3")
        .args(["--", "aibox", "run", "--tenant", ""])
        .output()
        .unwrap();

    assert_success(&output);
    let candidates: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(candidates, ["default", "work"]);
}
