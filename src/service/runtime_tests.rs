use super::*;
use crate::service::testutil::*;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
#[test]
fn startup_banner_reports_the_listener_and_clickable_console_url() {
    for (listen, expected) in [
        (
            "127.0.0.1:9923",
            "Listening on 127.0.0.1:9923 · Console http://127.0.0.1:9923/_aibox/ui/overview",
        ),
        (
            "0.0.0.0:8080",
            "Listening on 0.0.0.0:8080 · Console http://127.0.0.1:8080/_aibox/ui/overview",
        ),
        (
            "[::]:9923",
            "Listening on [::]:9923 · Console http://[::1]:9923/_aibox/ui/overview",
        ),
        (
            "[::1]:8080",
            "Listening on [::1]:8080 · Console http://[::1]:8080/_aibox/ui/overview",
        ),
    ] {
        assert_eq!(
            startup_banner(listen.parse().unwrap()),
            expected,
            "{listen}"
        );
    }
}

#[tokio::test]
async fn shutdown_coordinator_drains_request_tasks_and_management_operation() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = tempfile::tempdir().unwrap();
    let state = test_state(root.path());
    let request_state = state.request();
    let request_task_finished = Arc::new(AtomicBool::new(false));
    let finished = request_task_finished.clone();
    let request_shutdown = state.shutdown_token();
    request_state.spawn_response_task(async move {
        request_shutdown.cancelled().await;
        finished.store(true, Ordering::SeqCst);
    });
    state
        .management
        .operations
        .start_for_test("shutdown test", |context| {
            while !context.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok("stopped".to_string())
        })
        .unwrap();

    let (_signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    let listener_shutdown = state.shutdown_token();
    let server = async move {
        listener_shutdown.cancelled().await;
        Ok(())
    };
    tokio::pin!(server);

    let code = coordinate_shutdown(
        ShutdownReason::Interrupt,
        &state,
        &mut signal_rx,
        server.as_mut(),
    )
    .await
    .unwrap();

    assert_eq!(code, 0);
    assert!(request_task_finished.load(Ordering::SeqCst));
    assert_eq!(
        state
            .management
            .operations
            .current(None)
            .operation
            .unwrap()
            .state,
        crate::management::OperationState::Cancelled
    );
}

#[tokio::test]
async fn shutdown_coordinator_uses_the_first_signal_for_forced_exit() {
    let root = tempfile::tempdir().unwrap();
    let state = test_state(root.path());
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    signal_tx.send(ShutdownReason::Terminate).unwrap();
    let server = std::future::pending::<io::Result<()>>();
    tokio::pin!(server);

    let code = coordinate_shutdown(
        ShutdownReason::Interrupt,
        &state,
        &mut signal_rx,
        server.as_mut(),
    )
    .await
    .unwrap();

    assert_eq!(code, 130);
    assert!(state.shutdown_token().is_cancelled());
}

#[test]
fn shutdown_exit_codes_preserve_interrupt_and_terminate_policy() {
    assert_eq!(signal_exit(ShutdownReason::Interrupt), 130);
    assert_eq!(signal_exit(ShutdownReason::Terminate), 143);
}

#[tokio::test]
async fn service_task_scope_cancels_and_releases_its_workers() {
    let shutdown = CancellationToken::new();
    let worker = || {
        let (finished, dropped) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _finished = finished;
            std::future::pending::<()>().await;
        });
        (handle, dropped)
    };
    let (component_updates, first) = worker();
    let (request_compaction, second) = worker();
    let (signals, third) = worker();
    drop(ServiceTasks {
        shutdown: shutdown.clone(),
        component_updates,
        request_compaction,
        signals,
    });
    assert!(shutdown.is_cancelled());
    for receiver in [first, second, third] {
        assert!(receiver.await.is_err());
    }
}

#[tokio::test]
async fn shutdown_keeps_terminate_status_and_listener_failure_context() {
    for fail in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = test_state(root.path());
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let server = async move {
            if fail {
                Err(io::Error::other("listener fixture"))
            } else {
                Ok(())
            }
        };
        tokio::pin!(server);
        let result =
            coordinate_shutdown(ShutdownReason::Terminate, &state, &mut rx, server.as_mut()).await;
        if fail {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("shut down AIBox Service listener"));
            assert!(error.contains("listener fixture"));
        } else {
            assert_eq!(result.unwrap(), 143);
        }
    }
}

#[tokio::test]
async fn second_signal_can_skip_response_task_drain() {
    let root = tempfile::tempdir().unwrap();
    let state = test_state(root.path());
    let request = state.request();
    let (release, blocked) = tokio::sync::oneshot::channel::<()>();
    request.spawn_response_task(async move {
        let _ = blocked.await;
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (listener_done, wait_listener) = tokio::sync::oneshot::channel();
    let server = async move {
        listener_done.send(()).unwrap();
        Ok(())
    };
    tokio::pin!(server);
    let (result, ()) = tokio::join!(
        coordinate_shutdown(ShutdownReason::Terminate, &state, &mut rx, server.as_mut()),
        async move {
            wait_listener.await.unwrap();
            tx.send(ShutdownReason::Interrupt).unwrap();
        },
    );
    assert_eq!(result.unwrap(), 143);
    release.send(()).unwrap();
    request.wait_for_response_tasks().await;
}

#[test]
fn service_lock_is_exclusive_per_root() {
    let root = tempfile::tempdir().unwrap();
    let first = acquire_service_lock(root.path()).unwrap();
    let error = acquire_service_lock(root.path()).unwrap_err().to_string();
    assert!(error.contains("another AIBox Service"), "{error}");
    drop(first);
    acquire_service_lock(root.path()).unwrap();
}

#[test]
fn service_preparation_creates_and_repairs_the_default_managed_tenant() {
    let root = tempfile::tempdir().unwrap();

    ensure_default_managed_tenant(root.path()).unwrap();
    let home = root.path().join("tenants/default");
    assert!(home.join(".gitconfig").is_file());
    assert!(home.join(".codex").is_dir());
    assert!(home.join(".claude").is_dir());

    fs::write(home.join("preserved"), b"user state").unwrap();
    fs::write(home.join(".codex/config.toml"), b"model = \"preserved\"\n").unwrap();
    fs::create_dir(home.join(".codex/sessions")).unwrap();
    fs::write(
        home.join(".codex/sessions/transcript.jsonl"),
        b"session state",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home.join("preserved"), fs::Permissions::from_mode(0o640)).unwrap();
    }
    fs::remove_dir(home.join(".claude")).unwrap();
    ensure_default_managed_tenant(root.path()).unwrap();

    assert_eq!(fs::read(home.join("preserved")).unwrap(), b"user state");
    assert_eq!(
        fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
        "model = \"preserved\"\n"
    );
    assert_eq!(
        fs::read(home.join(".codex/sessions/transcript.jsonl")).unwrap(),
        b"session state"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(home.join("preserved"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    assert!(home.join(".claude").is_dir());
}

#[test]
fn service_preparation_rejects_an_unsafe_default_managed_tenant() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tenants")).unwrap();
    fs::write(root.path().join("tenants/default"), b"not a directory").unwrap();

    let error = ensure_default_managed_tenant(root.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Default Managed Tenant"), "{error}");
}

#[test]
fn service_preparation_rejects_an_unsafe_default_agent_state() {
    let root = tempfile::tempdir().unwrap();
    ensure_default_managed_tenant(root.path()).unwrap();
    let codex = root.path().join("tenants/default/.codex");
    fs::remove_dir(&codex).unwrap();
    fs::write(&codex, b"not a directory").unwrap();

    let error = ensure_default_managed_tenant(root.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Default Managed Tenant"), "{error}");
}

#[cfg(unix)]
#[test]
fn service_preparation_rejects_a_symlinked_default_managed_tenant() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tenants")).unwrap();
    symlink(outside.path(), root.path().join("tenants/default")).unwrap();

    let error = ensure_default_managed_tenant(root.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Default Managed Tenant"), "{error}");
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn shutdown_cancels_component_update_prefetch_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let control = PendingLatestControl::default();
    let mut state = test_state(root.path());
    state
        .management
        .components
        .set_latest_provider(Arc::new(PendingLatestProvider {
            control: control.clone(),
        }));
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();
    let wait = control.clone();

    tokio::join!(
        biased;
        prefetch_component_updates(state.clone(), shutdown),
        async move {
            wait.wait_until_all_started().await;
            cancel.cancel();
        },
    );

    assert_eq!(
        control.calls.load(Ordering::SeqCst),
        VERSIONED_COMPONENT_COUNT
    );
    assert_eq!(state.management.components.latest().await, None);
}
