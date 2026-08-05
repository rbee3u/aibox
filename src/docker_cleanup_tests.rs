use super::*;
use crate::testutil::EnvGuard;

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
        .arg("creds::tests::ignored_hup_helper_process")
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
        .arg("creds::tests::signal_helper_process")
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
