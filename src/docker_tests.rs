use super::*;
use crate::testutil::EnvGuard;
use std::fs;
use std::path::Path;

fn write_fake_docker(dir: &Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_IMAGE_LOG" ] && [ "$1" = "image" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_IMAGE_LOG"
fi
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    case "$AIBOX_FAKE_DOCKER_IMAGE_MODE" in
        exists)
            printf 'sha256:fake-image\n'
            exit 0
            ;;
        missing-localized)
            printf 'image not found: %s\n' "${5:-}" >&2
            exit 1
            ;;
        missing-empty)
            exit 1
            ;;
        list-exists-tagged|tagless-repository-match)
            exit 1
            ;;
        daemon-error)
            printf 'Cannot connect to the Docker daemon\n' >&2
            exit 1
            ;;
        *)
            exit 97
            ;;
    esac
fi
if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
    case "$AIBOX_FAKE_DOCKER_IMAGE_MODE" in
        exists)
            printf 'sha256:fake-image\n'
            exit 0
            ;;
        missing-localized|missing-empty)
            exit 0
            ;;
        list-exists-tagged)
            if [ "${5:-}" = "repo/name:tag" ]; then
                printf 'sha256:fake-image\n'
            fi
            exit 0
            ;;
        tagless-repository-match)
            if [ "${5:-}" = "repo/name" ]; then
                printf 'sha256:fake-image\n'
            fi
            exit 0
            ;;
        daemon-error)
            printf 'Cannot connect to the Docker daemon\n' >&2
            exit 1
            ;;
        *)
            exit 96
            ;;
    esac
fi
if [ "$1" = "container" ] && [ "$2" = "ls" ]; then
    # Normal fixture runs model an already-removed --rm container.
    exit 0
fi
if { [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-failure" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; } && [ "$1" = "inspect" ]; then
    if [ -e "$AIBOX_FAKE_DOCKER_STOPPED" ]; then
        printf 'false\n'
    else
        printf 'true\n'
    fi
    exit 0
fi
if { [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-failure" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; } && [ "$1" = "kill" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
    if [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] && [ "$2" = "--signal" ]; then
        exit 0
    fi
    if [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; then
        exit 39
    fi
    : > "$AIBOX_FAKE_DOCKER_STOPPED"
    exit 0
fi
if [ "$1" != "run" ]; then
    exit 99
fi
if [ -n "$AIBOX_FAKE_DOCKER_RUN_LOG" ]; then
    printf 'run%s\n' "$(for a in "$@"; do printf ' <%s>' "$a"; done)" >> "$AIBOX_FAKE_DOCKER_RUN_LOG"
fi
shift
cid=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--cidfile" ]; then
        cid="$2"
        shift 2
    else
        shift
    fi
done
case "$AIBOX_FAKE_DOCKER_MODE" in
    forwards-args)
        printf 'fake-container\n' > "$cid"
        exit 0
        ;;
    delayed-cid)
        sleep 0.2
        if [ -n "$AIBOX_CALLBACK_MARKER" ] && [ -e "$AIBOX_CALLBACK_MARKER" ]; then
            printf 'early\n' > "$AIBOX_EARLY_MARKER"
        fi
        printf 'fake-container\n' > "$cid"
        sleep 0.05
        exit 0
        ;;
    slow-cid)
        sleep 1.2
        if [ -n "$AIBOX_CALLBACK_MARKER" ] && [ -e "$AIBOX_CALLBACK_MARKER" ]; then
            printf 'early\n' > "$AIBOX_EARLY_MARKER"
        fi
        printf 'fake-container\n' > "$cid"
        sleep 0.05
        exit 0
        ;;
    no-cid)
        exit 23
        ;;
    slow-no-cid)
        sleep 1.2
        exit 24
        ;;
    lingering|lingering-stubborn|lingering-kill-failure)
        printf 'fake-container\n' > "$cid"
        exit 0
        ;;
    lingering-failure)
        printf 'fake-container\n' > "$cid"
        exit 47
        ;;
    *)
        exit 98
        ;;
esac
"#,
    );
}

fn write_fake_build_docker(dir: &Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ "$1" != "build" ]; then
    exit 99
fi
if [ "$AIBOX_FAKE_DOCKER_BUILD_MODE" = "exit-early" ]; then
    exit 23
fi
log="$AIBOX_FAKE_DOCKER_BUILD_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
    last="$arg"
done
printf '\n' >> "$log"
if [ ! -d "$last" ]; then
    printf 'context is not a directory: %s\n' "$last" >&2
    exit 98
fi
printf 'STDIN:' >> "$log"
cat >> "$log"
printf '\nEND\n' >> "$log"
"#,
    );
}

#[test]
fn embedded_dockerfile_does_not_require_build_context() {
    for line in DOCKERFILE.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let instruction = trimmed
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        assert!(
            !matches!(instruction.as_str(), "COPY" | "ADD"),
            "aibox Dockerfile must not read from a build context: {line:?}"
        );
    }
}

#[test]
fn build_image_uses_stdin_empty_context_and_cache_flags() {
    let _env_lock = crate::test_env_lock();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("docker-build.log");
    write_fake_build_docker(dir.path());
    let _path = EnvGuard::prepend_path(dir.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_LOG", log.as_os_str());

    build_image("FROM scratch\n", "test/cached:latest", BuildCache::Cached).unwrap();
    build_image("RUN true\n", "test/nocache:latest", BuildCache::NoCache).unwrap();
    build_image("RUN false\n", "test/pull:latest", BuildCache::NoCachePull).unwrap();

    let log = fs::read_to_string(log).unwrap();
    assert!(
        log.contains("ARGS: <build> <-f> <-> <-t> <test/cached:latest> <"),
        "cached build should pass Dockerfile through stdin with only -f/-t: {log}"
    );
    assert!(
        log.contains("ARGS: <build> <--no-cache> <-f> <-> <-t> <test/nocache:latest> <"),
        "no-cache build should add only --no-cache: {log}"
    );
    assert!(
        log.contains("ARGS: <build> <--no-cache> <--pull> <-f> <-> <-t> <test/pull:latest> <"),
        "forced base build should add --no-cache and --pull: {log}"
    );
    assert!(log.contains("STDIN:FROM scratch\n"), "{log}");
    assert!(log.contains("STDIN:RUN true\n"), "{log}");
    assert!(log.contains("STDIN:RUN false\n"), "{log}");
}

#[test]
fn build_image_reports_docker_status_when_child_exits_early() {
    let _env_lock = crate::test_env_lock();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("docker-build.log");
    write_fake_build_docker(dir.path());
    let _path = EnvGuard::prepend_path(dir.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_LOG", log.as_os_str());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_MODE", "exit-early");

    let err = build_image("RUN true\n", "test/fails:latest", BuildCache::Cached)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("docker build failed"),
        "docker status should be reported instead of masking it with stdin write errors: {err}"
    );
}

#[test]
fn image_exists_uses_exact_image_inspect() {
    let _env_lock = crate::test_env_lock();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("image.log");
    write_fake_docker(dir.path());
    let _path = EnvGuard::prepend_path(dir.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_LOG", log.as_os_str());

    {
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "exists");
        assert!(image_exists("repo/name:tag").unwrap());
        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("image inspect --format {{.Id}} repo/name:tag"));
        assert!(
            !calls.contains("image ls"),
            "a successful exact inspect must not fall back to a repository listing: {calls}"
        );
    }

    {
        fs::write(&log, "").unwrap();
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing-localized");
        assert!(!image_exists("repo/name:tag").unwrap());
        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("image inspect --format {{.Id}} repo/name:tag"));
        assert!(
            calls.contains("image ls --quiet --no-trunc repo/name:tag"),
            "a failed inspect must use an exact tag in the fallback lookup: {calls}"
        );
    }

    {
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing-empty");
        assert!(!image_exists("repo/name:tag").unwrap());
    }

    {
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "list-exists-tagged");
        assert!(image_exists("repo/name:tag").unwrap());
    }

    {
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "tagless-repository-match");
        assert!(
            !image_exists("repo/name").unwrap(),
            "tagless lookup must query repo/name:latest, not broad repo/name"
        );
    }

    {
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "daemon-error");
        let err = image_exists("repo/name:tag").unwrap_err().to_string();
        assert!(err.contains("docker image inspect failed"), "{err}");
        assert!(err.contains("docker image ls failed"), "{err}");
        assert!(err.contains("Cannot connect"), "{err}");
    }
}

#[test]
fn image_ref_for_exact_ls_adds_latest_only_when_tagless() {
    assert_eq!(image_ref_for_exact_ls("busybox"), "busybox:latest");
    assert_eq!(image_ref_for_exact_ls("repo/name"), "repo/name:latest");
    assert_eq!(
        image_ref_for_exact_ls("registry.example:5000/repo/name"),
        "registry.example:5000/repo/name:latest"
    );
    assert_eq!(image_ref_for_exact_ls("repo/name:dev"), "repo/name:dev");
    assert_eq!(
        image_ref_for_exact_ls("repo/name@sha256:abc"),
        "repo/name@sha256:abc"
    );
}

#[test]
fn cidfile_has_id_requires_a_non_empty_container_id() {
    let dir = tempfile::tempdir().unwrap();
    let cid = dir.path().join("cid");

    assert!(!cidfile_has_id(&cid), "missing cidfile is not a create");
    fs::write(&cid, "").unwrap();
    assert!(!cidfile_has_id(&cid), "empty cidfile is not a create");
    fs::write(&cid, " \n\t").unwrap();
    assert!(
        !cidfile_has_id(&cid),
        "whitespace-only cidfile does not identify a created container"
    );
    fs::write(&cid, "fake-container\n").unwrap();
    assert!(cidfile_has_id(&cid));
}

#[test]
fn run_spawn_failure_does_not_call_container_created_callback() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let callback_marker = dir.path().join("callback");
    let _path = EnvGuard::set("PATH", dir.path().as_os_str());

    let err = run(&[], "image:tag", &[], || {
        fs::write(&callback_marker, "called\n").unwrap();
    })
    .unwrap_err()
    .to_string();

    assert!(err.contains("spawn docker run"), "{err}");
    assert!(
        !callback_marker.exists(),
        "spawn failure means no container exists, so the callback must not run"
    );
}

#[test]
fn run_forwards_run_args_image_and_cmd_in_order() {
    // The Filesystem Sandbox configuration lives in `run_args`
    // (--cap-drop ALL, --security-opt no-new-privileges, --user, every bind
    // mount). `run` must deliver them to `docker run` unchanged, with the
    // image after the run args and the command after the image. A dropped
    // run_args or a swapped image/cmd would silently strip the isolation
    // the tool exists to enforce, so assert the whole assembled line, not
    // just --cidfile.
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let run_log = dir.path().join("run.log");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "forwards-args");
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_RUN_LOG", run_log.as_os_str());

    let run_args = vec![
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
    ];
    let cmd = vec![OsString::from("exec"), OsString::from("--flag")];
    let code = run(&run_args, IMAGE, &cmd, || {}).unwrap();

    assert_eq!(code, 0);
    let log = fs::read_to_string(&run_log).unwrap();
    // --cidfile is inserted first (before run_args) so its own tests keep
    // working; assert the rest follows it in order and the image sits
    // between the run args and the command.
    assert!(
        log.contains(
            "<--cap-drop> <ALL> <--security-opt> <no-new-privileges> \
                 <aibox:latest> <exec> <--flag>"
        ),
        "run must forward run_args, then image, then cmd, in order: {log}"
    );
}

#[test]
fn run_callback_waits_until_cidfile_has_container_id() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let callback_marker = dir.path().join("callback");
    let early_marker = dir.path().join("early");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "delayed-cid");
    let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());
    let _early = EnvGuard::set("AIBOX_EARLY_MARKER", early_marker.as_os_str());

    let code = run(&[], "image:tag", &[], || {
        fs::write(&callback_marker, "called\n").unwrap();
    })
    .unwrap();

    assert_eq!(code, 0);
    assert!(
        callback_marker.exists(),
        "callback runs after the cidfile is populated"
    );
    assert!(
        !early_marker.exists(),
        "callback must not run before Docker records a container id"
    );
}

#[test]
fn run_callback_still_runs_when_cidfile_appears_after_initial_wait() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let callback_marker = dir.path().join("callback");
    let early_marker = dir.path().join("early");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "slow-cid");
    let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());
    let _early = EnvGuard::set("AIBOX_EARLY_MARKER", early_marker.as_os_str());

    let code = run(&[], "image:tag", &[], || {
        fs::write(&callback_marker, "called\n").unwrap();
    })
    .unwrap();

    assert_eq!(code, 0);
    assert!(
        callback_marker.exists(),
        "callback runs once the delayed cidfile is populated"
    );
    assert!(
        !early_marker.exists(),
        "callback must not run before the delayed container id exists"
    );
}

#[test]
fn run_cleans_up_the_container_and_registry_when_callback_panics() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let stopped_marker = dir.path().join("stopped");
    let docker_log = dir.path().join("docker.log");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering");
    let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run(&[], "image:tag", &[], || panic!("callback failed"));
    }));

    assert!(panic.is_err());
    assert!(
        stopped_marker.exists(),
        "unwinding out of the callback must still stop the created container"
    );
    assert!(
        fs::read_to_string(&docker_log)
            .unwrap()
            .contains("kill fake-container"),
        "callback cleanup must use the registered cidfile"
    );

    let _next_mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "forwards-args");
    assert_eq!(run(&[], "image:tag", &[], || {}).unwrap(), 0);
}

#[test]
fn wait_failure_keeps_registered_run_cleanup_armed() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let cid_dir = tempfile::tempdir().unwrap();
    set_cidfile(&cid_dir.path().join("cid")).unwrap();
    let child = Command::new("sh").args(["-c", "sleep 10"]).spawn().unwrap();
    set_child(child.id());
    let mut run = RegisteredRun::new(child);

    let error = run
        .finish_after_wait(Err(anyhow::anyhow!("synthetic wait failure")))
        .unwrap_err()
        .to_string();

    assert!(error.contains("wait for docker run"), "{error}");
    assert!(!run.finished, "the drop guard must remain armed");
}

#[test]
fn run_does_not_call_callback_when_child_exits_before_cidfile() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let callback_marker = dir.path().join("callback");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "no-cid");
    let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());

    let code = run(&[], "image:tag", &[], || {
        fs::write(&callback_marker, "called\n").unwrap();
    })
    .unwrap();

    assert_eq!(code, 23);
    assert!(
        !callback_marker.exists(),
        "no container id means the created callback must not run"
    );
}

#[test]
fn run_does_not_call_callback_when_delayed_child_exits_without_cidfile() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let callback_marker = dir.path().join("callback");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "slow-no-cid");
    let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());

    let code = run(&[], "image:tag", &[], || {
        fs::write(&callback_marker, "called\n").unwrap();
    })
    .unwrap();

    assert_eq!(code, 24);
    assert!(
        !callback_marker.exists(),
        "callback must remain uncalled after the initial create wait times out"
    );
}

#[test]
fn run_immediately_kills_a_lingering_container_and_returns_nonzero() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let stopped_marker = dir.path().join("stopped");
    let docker_log = dir.path().join("docker.log");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-stubborn");
    let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

    let started = Instant::now();
    let code = run(&[], "image:tag", &[], || {}).unwrap();

    assert_eq!(
        code, 1,
        "a client-side zero exit is not a successful agent run when its container lingered"
    );
    assert!(stopped_marker.exists());
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "orphan cleanup must not wait through the ten-second graceful signal path"
    );
    let log = fs::read_to_string(docker_log).unwrap();
    assert!(log.contains("kill fake-container"), "{log}");
    assert!(!log.contains("--signal"), "{log}");
}

#[test]
fn run_kills_a_lingering_container_even_when_client_failed() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let stopped_marker = dir.path().join("stopped");
    let docker_log = dir.path().join("docker.log");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-failure");
    let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

    let code = run(&[], "image:tag", &[], || {}).unwrap();

    assert_eq!(
        code, 47,
        "the Docker client's failure code must be preserved after orphan cleanup"
    );
    assert!(
        stopped_marker.exists(),
        "an orphan must be killed even when docker run itself exits non-zero"
    );
    let log = fs::read_to_string(docker_log).unwrap();
    assert!(log.contains("kill fake-container"), "{log}");
    assert!(!log.contains("--signal"), "{log}");
}

#[test]
fn run_reports_failure_when_lingering_container_kill_fails() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let dir = tempfile::tempdir().unwrap();
    write_fake_docker(dir.path());
    let stopped_marker = dir.path().join("stopped");
    let docker_log = dir.path().join("docker.log");
    let _path = EnvGuard::prepend_path(dir.path());
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-kill-failure");
    let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

    let code = run(&[], "image:tag", &[], || {}).unwrap();

    assert_eq!(
        code, 1,
        "a zero-exit Docker client must not make an uncleaned orphan look successful"
    );
    assert!(
        !stopped_marker.exists(),
        "the fixture must prove the daemon-side kill failed"
    );
    let log = fs::read_to_string(docker_log).unwrap();
    assert!(log.contains("kill fake-container"), "{log}");
    assert!(!log.contains("--signal"), "{log}");
}

#[test]
fn exit_code_maps_child_exit_and_signal_statuses() {
    let _env_lock = crate::test_env_lock();
    let exited = Command::new("sh").args(["-c", "exit 37"]).status().unwrap();
    assert_eq!(exit_code(exited), 37);

    let signaled = Command::new("sh")
        .args(["-c", "kill -TERM $$"])
        .status()
        .unwrap();
    assert_eq!(exit_code(signaled), 128 + signal_hook::consts::SIGTERM);
}
