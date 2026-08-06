use super::*;
use crate::testutil::EnvGuard;
use std::fs;
use std::path::Path;

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

struct CidfileGuard;

impl Drop for CidfileGuard {
    fn drop(&mut self) {
        clear_child();
    }
}

fn stable_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("aibox-test.")
        .tempdir()
        .unwrap()
}

#[cfg(unix)]
const SIGNAL_HELPER_DIR: &str = "AIBOX_TEST_SIGNAL_HELPER_DIR";
#[cfg(unix)]
const IGNORED_HUP_HELPER_DIR: &str = "AIBOX_TEST_IGNORED_HUP_HELPER_DIR";

#[cfg(unix)]
#[test]
fn ignored_hup_helper_process() {
    let Some(dir) = std::env::var_os(IGNORED_HUP_HELPER_DIR) else {
        return;
    };
    let dir = PathBuf::from(dir);

    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_IGN;
        libc::sigemptyset(&mut action.sa_mask);
        let rc = libc::sigaction(signal_hook::consts::SIGHUP, &action, std::ptr::null_mut());
        assert_eq!(
            rc,
            0,
            "set SIGHUP to SIG_IGN: {}",
            std::io::Error::last_os_error()
        );
    }

    assert!(signal_is_ignored(signal_hook::consts::SIGHUP));
    install_signal_handler().unwrap();
    assert!(
        signal_is_ignored(signal_hook::consts::SIGHUP),
        "installing cleanup handlers must not un-ignore inherited SIGHUP"
    );

    unsafe {
        libc::raise(signal_hook::consts::SIGHUP);
    }
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(dir.join("survived"), "survived\n").unwrap();
}

#[cfg(unix)]
#[test]
fn ignored_sighup_stays_ignored_when_handlers_are_installed() {
    // The helper inherits the parent environment. Keep it from observing a
    // parallel test's temporary PATH or Docker fixture variables.
    let _env_lock = crate::test_env_lock();
    let scratch = stable_tempdir();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("docker::tests::ignored_hup_helper_process")
        .env(IGNORED_HUP_HELPER_DIR, scratch.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert!(
        status.success(),
        "helper should survive ignored SIGHUP, got {status:?}"
    );
    assert!(
        scratch.path().join("survived").exists(),
        "helper did not continue after raising ignored SIGHUP"
    );
}

// Arm the same daemon-side cleanup handle as a real run, report ready, and
// block until the parent delivers the signal under test.
#[cfg(unix)]
#[test]
fn signal_helper_process() {
    let Some(dir) = std::env::var_os(SIGNAL_HELPER_DIR) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let cid_path = dir.join("cid");
    std::fs::write(&cid_path, "signal-container\n").unwrap();
    // Installs the watcher thread and marks the run active, exactly as
    // `docker::run` does before spawning the Docker CLI.
    set_cidfile(&cid_path).unwrap();

    std::fs::write(dir.join("ready"), "ready\n").unwrap();
    // Park until the watcher exits us.
    std::thread::sleep(Duration::from_secs(60));
}

// Run the helper with a stubbed Docker, wait for its cidfile, then signal it.
#[cfg(unix)]
fn run_signal_helper(sig: i32) -> (tempfile::TempDir, std::process::ExitStatus) {
    let scratch = stable_tempdir();
    let fake_docker = scratch.path().join("bin");
    std::fs::create_dir_all(&fake_docker).unwrap();
    write_signal_fake_docker(&fake_docker);
    let ready = scratch.path().join("ready");

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("docker::tests::signal_helper_process")
        .env(SIGNAL_HELPER_DIR, scratch.path())
        .env("PATH", &fake_docker)
        .env("AIBOX_FAKE_DOCKER_LOG", scratch.path().join("docker.log"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let started = Instant::now();
    while !ready.exists() {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("signal helper never armed its cidfile");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
    let rsig = match sig {
        s if s == signal_hook::consts::SIGINT => rustix::process::Signal::Int,
        s if s == signal_hook::consts::SIGHUP => rustix::process::Signal::Hup,
        _ => rustix::process::Signal::Term,
    };
    rustix::process::kill_process(pid, rsig).unwrap();

    let status = child.wait().unwrap();
    (scratch, status)
}

// Exercise the fatal-signal path in a subprocess because unwinding and
// `Drop` cannot cover it.
#[cfg(unix)]
#[test]
fn fatal_signal_stops_the_container() {
    use std::os::unix::process::ExitStatusExt;

    // Each helper inherits most of this process's environment. Serialize
    // it with tests that temporarily install Docker stubs.
    let _env_lock = crate::test_env_lock();
    for sig in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        let (scratch, status) = run_signal_helper(sig);

        // The container is stopped through the daemon — the only route that
        // works when the Docker CLI has a TTY and does not proxy signals.
        let log = std::fs::read_to_string(scratch.path().join("docker.log")).unwrap_or_default();
        assert!(
            log.contains("signal-container"),
            "sig {sig}: container was not stopped via the daemon; docker log:\n{log}"
        );

        // Death still looks like the signal to the caller's shell, whether
        // by re-raise or the 128+n fallback.
        let died_of_signal = status.signal() == Some(sig);
        assert!(
            died_of_signal || status.code() == Some(128 + sig),
            "sig {sig}: exit status must reflect the signal, got {status:?}"
        );
    }
}

#[cfg(unix)]
fn write_signal_fake_docker(dir: &Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ "$1" = "kill" ] && [ -n "$AIBOX_FAKE_DOCKER_KILL_START_DELAY" ]; then
    sleep "$AIBOX_FAKE_DOCKER_KILL_START_DELAY"
fi
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        printf 'false\n'
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
    );
}

// Report a running container for a configurable number of inspections so
// tests can reach the grace and escalation paths.
#[cfg(unix)]
fn write_running_container_docker(dir: &Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        want="$AIBOX_FAKE_DOCKER_RUNNING_INSPECTS"
        if [ "$want" = "always" ]; then
            printf 'true\n'
            exit 0
        fi
        seen=0
        if [ -f "$AIBOX_FAKE_DOCKER_INSPECT_COUNT" ]; then
            seen=$(cat "$AIBOX_FAKE_DOCKER_INSPECT_COUNT")
        fi
        seen=$((seen + 1))
        printf '%s' "$seen" > "$AIBOX_FAKE_DOCKER_INSPECT_COUNT"
        if [ "$seen" -le "${want:-0}" ]; then
            printf 'true\n'
        else
            printf 'false\n'
        fi
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
    );
}

// SIGNAL_COUNT is process-global, so tests that fake a signal restore it.
struct SignalCountGuard(usize);

impl SignalCountGuard {
    fn set(value: usize) -> Self {
        let old = SIGNAL_COUNT.swap(value, Ordering::SeqCst);
        SignalCountGuard(old)
    }
}

impl Drop for SignalCountGuard {
    fn drop(&mut self) {
        SIGNAL_COUNT.store(self.0, Ordering::SeqCst);
    }
}

#[cfg(unix)]
#[test]
fn stop_container_id_waits_out_a_container_that_stops_on_the_signal() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let scratch = stable_tempdir();
    let log_path = scratch.path().join("docker.log");
    let count_path = scratch.path().join("inspects");
    let fake_docker = stable_tempdir();
    write_running_container_docker(fake_docker.path());
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    let _count = EnvGuard::set("AIBOX_FAKE_DOCKER_INSPECT_COUNT", count_path.as_os_str());
    // Running on the first inspect, stopped on the next: the container took
    // the signal, just not instantly.
    let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "1");

    let started = Instant::now();
    stop_container_id(signal_hook::consts::SIGTERM, "graceful-container");

    assert!(
        started.elapsed() < CONTAINER_GRACE,
        "a container that stops during the grace window must not wait it out"
    );
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("kill --signal TERM graceful-container"),
        "the graceful signal goes through the daemon: {log}"
    );
    assert!(
        !log.lines().any(|l| l == "kill graceful-container"),
        "a container that exited on the signal must not also be SIGKILLed:\n{log}"
    );
}

#[cfg(unix)]
#[test]
fn stop_container_id_escalates_to_sigkill_on_a_second_signal() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let scratch = stable_tempdir();
    let log_path = scratch.path().join("docker.log");
    let count_path = scratch.path().join("inspects");
    let fake_docker = stable_tempdir();
    write_running_container_docker(fake_docker.path());
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    let _count = EnvGuard::set("AIBOX_FAKE_DOCKER_INSPECT_COUNT", count_path.as_os_str());
    let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "always");
    // A second delivered signal (Ctrl-C again, or a supervisor re-kill).
    let _signals = SignalCountGuard::set(2);

    let started = Instant::now();
    stop_container_id(signal_hook::consts::SIGINT, "stubborn-container");

    assert!(
        started.elapsed() < CONTAINER_GRACE,
        "a second signal must skip the rest of the grace wait"
    );
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("kill --signal INT stubborn-container"),
        "SIGINT maps to the container's INT: {log}"
    );
    assert!(
        log.lines().any(|l| l == "kill stubborn-container"),
        "a container that ignored the signal must be SIGKILLed:\n{log}"
    );
}

#[cfg(unix)]
#[test]
fn stop_container_id_forwards_each_signal_under_its_own_name() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let fake_docker = stable_tempdir();
    write_signal_fake_docker(fake_docker.path());
    let _path = EnvGuard::prepend_path(fake_docker.path());

    for (sig, name) in [
        (signal_hook::consts::SIGINT, "INT"),
        (signal_hook::consts::SIGHUP, "HUP"),
        (signal_hook::consts::SIGTERM, "TERM"),
    ] {
        let scratch = stable_tempdir();
        let log_path = scratch.path().join("docker.log");
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());

        stop_container_id(sig, "named-container");

        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.contains(&format!("kill --signal {name} named-container")),
            "sig {sig} must reach the container as {name}:\n{log}"
        );
    }
}

#[cfg(unix)]
#[test]
fn signal_child_forwards_each_supported_signal_to_the_registered_process() {
    use std::os::unix::process::ExitStatusExt;

    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;

    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGTERM,
    ] {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .unwrap();
        set_child(child.id());

        signal_child(signal);

        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("signal {signal} did not terminate the registered child");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            status.signal(),
            Some(signal),
            "the Docker CLI child must receive the wrapper's original signal"
        );
    }
}

#[test]
fn watcher_commands_are_bounded() {
    // The `/bin/sh` helpers below resolve `sleep` through `$PATH`, so this
    // must hold the env lock even though it installs no guard of its own: a
    // parallel test that replaces `$PATH` with a stub directory would make
    // `sleep` unresolvable and turn an expected timeout into a fast exit 127.
    let _env_lock = crate::test_env_lock();

    // Worst case is the late-cidfile path in `stop_active_run`: the first
    // bounded wait fails (CIDFILE_WAIT), then after signalling the child the
    // longer late wait succeeds (LATE_CIDFILE_WAIT), then `stop_container_id`
    // runs its full graceful/escalating cleanup.
    let container_state_timeout = DOCKER_INSPECT_TIMEOUT + DOCKER_INSPECT_TIMEOUT;
    let worst_case_stop_container_id = DOCKER_KILL_TIMEOUT
        + container_state_timeout
        + CONTAINER_GRACE
        + CONTAINER_POLL_INTERVAL
        + container_state_timeout
        + DOCKER_KILL_TIMEOUT;
    let worst_case_signal_cleanup = CIDFILE_WAIT + LATE_CIDFILE_WAIT + worst_case_stop_container_id;
    assert!(
        SIGNAL_FINISH_WAIT > worst_case_signal_cleanup,
        "the main thread must not exit before the watcher can finish its bounded cleanup"
    );

    assert!(matches!(
        command_quiet("/bin/sh", &["-c", "printf ok"], Duration::from_secs(1)),
        CommandOutcome::Succeeded(out) if out == "ok"
    ));

    let inherited_stdout_started = Instant::now();
    assert!(matches!(
        command_quiet(
            "/bin/sh",
            &["-c", "sleep 5 & printf ok"],
            Duration::from_secs(1)
        ),
        CommandOutcome::Succeeded(out) if out == "ok"
    ));
    assert!(
        inherited_stdout_started.elapsed() < Duration::from_secs(2),
        "an inherited stdout handle must not outlive the command timeout"
    );

    // A fast non-zero exit is a definitive failure, distinct from a timeout.
    assert!(matches!(
        command_quiet("/bin/sh", &["-c", "exit 1"], Duration::from_secs(1)),
        CommandOutcome::Failed
    ));

    let oversized_output = format!(
        "dd if=/dev/zero bs={} count=1 2>/dev/null",
        COMMAND_OUTPUT_LIMIT + 1
    );
    assert!(
        matches!(
            command_quiet(
                "/bin/sh",
                &["-c", &oversized_output],
                Duration::from_secs(1)
            ),
            CommandOutcome::Unfinished
        ),
        "watcher helpers must not retain unbounded command output"
    );

    let started = Instant::now();
    let out = command_quiet("/bin/sh", &["-c", "sleep 5"], Duration::from_millis(50));

    assert!(
        matches!(out, CommandOutcome::Unfinished),
        "timed-out command should be treated as best-effort failure"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timeout should not wait for the child script to finish"
    );
}

#[test]
fn wait_current_cid_reads_delayed_cidfile() {
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cid");
    *cidfile().lock().unwrap() = Some(path.clone());

    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(path, "abc123\n").unwrap();
    });

    let got = wait_current_cid(Duration::from_secs(1));
    writer.join().unwrap();

    assert_eq!(got.as_deref(), Some("abc123"));
}

#[cfg(unix)]
#[test]
fn stop_active_run_kills_late_cidfile_container() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let scratch = stable_tempdir();
    let cid_path = scratch.path().join("cid");
    let log_path = scratch.path().join("docker.log");
    let fake_docker = stable_tempdir();
    write_signal_fake_docker(fake_docker.path());
    *cidfile().lock().unwrap() = Some(cid_path.clone());
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    let _kill_delay = EnvGuard::set("AIBOX_FAKE_DOCKER_KILL_START_DELAY", "1.2");

    let writer = std::thread::spawn(move || {
        std::thread::sleep(CIDFILE_WAIT + Duration::from_millis(100));
        std::fs::write(cid_path, "late-container\n").unwrap();
    });

    stop_active_run(signal_hook::consts::SIGTERM);
    writer.join().unwrap();

    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("kill --signal TERM late-container"),
        "late cidfile should still trigger daemon-side kill; log:\n{log}"
    );
}

#[cfg(unix)]
#[test]
fn finish_child_kills_a_container_that_outlived_its_detached_client() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let scratch = stable_tempdir();
    let cid_path = scratch.path().join("cid");
    let log_path = scratch.path().join("docker.log");
    let fake_docker = stable_tempdir();
    write_running_container_docker(fake_docker.path());
    std::fs::write(&cid_path, "detached-container\n").unwrap();
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    // Report running forever: the client detached but the container lives on.
    let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "always");
    // Arm the run exactly as `docker::run` does before spawning the client.
    set_cidfile(&cid_path).unwrap();

    let stopped = finish_child();

    assert!(
        stopped,
        "a still-running container after a detached client must be reported killed"
    );
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.lines().any(|l| l == "kill detached-container"),
        "the lingering container must be SIGKILLed by id:\n{log}"
    );
}

#[test]
fn a_second_active_run_registration_is_rejected_without_losing_the_first() {
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let first = stable_tempdir();
    let second = stable_tempdir();
    let first_cid = first.path().join("cid");
    let second_cid = second.path().join("cid");
    std::fs::write(&first_cid, "first-container\n").unwrap();
    std::fs::write(&second_cid, "second-container\n").unwrap();

    set_cidfile(&first_cid).unwrap();
    let error = set_cidfile(&second_cid).unwrap_err().to_string();

    assert!(error.contains("another docker run is already registered"));
    assert_eq!(
        current_cid().as_deref(),
        Some("first-container"),
        "a rejected registration must not overwrite the active run's cleanup handle"
    );
}

#[cfg(unix)]
#[test]
fn finish_child_leaves_a_cleanly_exited_run_alone() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let scratch = stable_tempdir();
    let cid_path = scratch.path().join("cid");
    let log_path = scratch.path().join("docker.log");
    let fake_docker = stable_tempdir();
    // This stub's `inspect` always reports stopped, mimicking a `--rm`
    // container whose id no longer resolves.
    write_signal_fake_docker(fake_docker.path());
    std::fs::write(&cid_path, "gone-container\n").unwrap();
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    set_cidfile(&cid_path).unwrap();

    let started = Instant::now();
    let stopped = finish_child();

    assert!(
        !stopped,
        "a container that already exited must not be reported killed"
    );
    assert!(
        started.elapsed() < CONTAINER_GRACE,
        "a clean exit must not enter the grace wait"
    );
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        !log.lines().any(|l| l.starts_with("kill")),
        "a cleanly exited run must trigger no docker kill:\n{log}"
    );
}

#[cfg(unix)]
#[test]
fn finish_child_kills_when_container_state_is_unknown() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let scratch = stable_tempdir();
    let cid_path = scratch.path().join("cid");
    let log_path = scratch.path().join("docker.log");
    let fake_docker = stable_tempdir();
    crate::testutil::write_stub_script(
        fake_docker.path(),
        "docker",
        r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        printf 'unknown\n'
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
    );
    std::fs::write(&cid_path, "uncertain-container\n").unwrap();
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
    set_cidfile(&cid_path).unwrap();

    let stopped = finish_child();

    assert!(
        stopped,
        "an unknown daemon state after client exit must be treated as unclean"
    );
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.lines().any(|line| line == "kill uncertain-container"),
        "unknown state should still trigger a daemon-side kill:\n{log}"
    );
}

#[cfg(unix)]
#[test]
fn finish_child_treats_a_missing_docker_cli_as_unclean_and_resets_the_registry() {
    let _env_lock = crate::test_env_lock();
    let _run_lock = run_registry_test_lock();
    let _guard = CidfileGuard;
    let scratch = stable_tempdir();
    let cid_path = scratch.path().join("cid");
    std::fs::write(&cid_path, "uninspectable-container\n").unwrap();
    let empty_path = stable_tempdir();
    let _path = EnvGuard::set("PATH", empty_path.path().as_os_str());
    set_cidfile(&cid_path).unwrap();

    assert!(
        finish_child(),
        "losing the Docker CLI after a child exit must be treated as an unclean run"
    );
    assert!(
        !finish_child(),
        "finishing the run must clear the process-global registration"
    );
}

#[cfg(unix)]
#[test]
fn failed_inspect_distinguishes_a_missing_container_from_a_daemon_failure() {
    let _env_lock = crate::test_env_lock();
    let fake_docker = stable_tempdir();
    let log_path = fake_docker.path().join("docker.log");
    crate::testutil::write_stub_script(
        fake_docker.path(),
        "docker",
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
case "$1" in
    inspect)
        exit 1
        ;;
    container)
        if [ "$AIBOX_FAKE_DOCKER_STATE" = "missing" ]; then
            exit 0
        fi
        exit 1
        ;;
    *)
        exit 99
        ;;
esac
"#,
    );
    let _path = EnvGuard::prepend_path(fake_docker.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.as_os_str());

    {
        let _state = EnvGuard::set("AIBOX_FAKE_DOCKER_STATE", "missing");
        // `container_state` intentionally treats a scheduling timeout as
        // Unknown. Under a heavily parallel test run, allow a fresh
        // bounded attempt so this test asserts the completed-query
        // contract instead of the host scheduler's timing.
        let mut state = ContainerState::Unknown;
        for _ in 0..3 {
            state = container_state("gone-container");
            if state == ContainerState::Stopped {
                break;
            }
        }
        assert_eq!(
            state,
            ContainerState::Stopped,
            "an exact empty container list confirms that the id is gone"
        );
    }

    {
        let _state = EnvGuard::set("AIBOX_FAKE_DOCKER_STATE", "daemon-error");
        assert_eq!(
            container_state("possibly-running-container"),
            ContainerState::Unknown,
            "two failed daemon queries must not be mistaken for a stopped container"
        );
    }

    let log = std::fs::read_to_string(log_path).unwrap();
    for cid in ["gone-container", "possibly-running-container"] {
        assert!(
            log.lines()
                .any(|line| line == format!("inspect -f {{{{.State.Running}}}} {cid}")),
            "container state must inspect the exact id first:\n{log}"
        );
        assert!(
            log.lines().any(|line| {
                line == format!("container ls --all --quiet --no-trunc --filter id={cid}")
            }),
            "a failed inspect must use one exact, non-truncated id filter:\n{log}"
        );
    }
}

#[test]
fn container_state_parser_distinguishes_running_stopped_unknown() {
    assert_eq!(
        parse_container_state(&CommandOutcome::Succeeded("true\n".into())),
        ContainerState::Running
    );
    assert_eq!(
        parse_container_state(&CommandOutcome::Succeeded("false\n".into())),
        ContainerState::Stopped
    );
    assert_eq!(
        parse_container_state(&CommandOutcome::Succeeded(String::new())),
        ContainerState::Unknown
    );
    assert_eq!(
        parse_container_state(&CommandOutcome::Succeeded("docker error".into())),
        ContainerState::Unknown
    );
    assert_eq!(
        parse_container_state(&CommandOutcome::Failed),
        ContainerState::Unknown
    );
    // A timeout / unspawnable docker is not an answer: stay Unknown so a
    // wedged daemon can't be mistaken for a stopped container.
    assert_eq!(
        parse_container_state(&CommandOutcome::Unfinished),
        ContainerState::Unknown
    );
}
