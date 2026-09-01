use super::*;
use axum::body::Body;
use axum::http::Request;
use base64::Engine as _;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;

pub(crate) fn test_state(root: &Path) -> ServiceState {
    let host_home = root.join("host-home");
    fs::create_dir(&host_home).unwrap();
    let shutdown = CancellationToken::new();
    ServiceState::new(
        root.to_path_buf(),
        host_home,
        "aibox:test".to_string(),
        "127.0.0.1:9923".parse().unwrap(),
        "test-csrf".to_string(),
        RequestProxyState::new_with_reporter(root, shutdown, None).unwrap(),
        Arc::new(crate::testutil::FixtureLatestProvider::empty()),
    )
}

fn request(method: Method, path: &str, peer: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:9923")
        .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()))
        .body(Body::empty())
        .unwrap()
}

fn json_request(path: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(body.into())
        .unwrap()
}

async fn response_json(response: Response<Body>) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[derive(Debug, Eq, PartialEq)]
enum FilesystemEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
    Other,
}

fn filesystem_snapshot(root: &Path) -> Vec<(PathBuf, FilesystemEntry)> {
    fn visit(root: &Path, directory: &Path, entries: &mut Vec<(PathBuf, FilesystemEntry)>) {
        let mut children = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        children.sort();
        for path in children {
            let metadata = fs::symlink_metadata(&path).unwrap();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let kind = if metadata.file_type().is_symlink() {
                FilesystemEntry::Symlink(fs::read_link(&path).unwrap())
            } else if metadata.is_dir() {
                FilesystemEntry::Directory
            } else if metadata.is_file() {
                FilesystemEntry::File(fs::read(&path).unwrap())
            } else {
                FilesystemEntry::Other
            };
            entries.push((relative, kind));
            if metadata.is_dir() {
                visit(root, &path, entries);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

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
    let request_shutdown = request_state.shutdown_token();
    request_state.spawn_response_task(async move {
        request_shutdown.cancelled().await;
        finished.store(true, Ordering::SeqCst);
    });
    state
        .start_management_operation("shutdown test", |context| {
            while !context.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok("stopped".to_string())
        })
        .unwrap();

    let (_signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    let listener_shutdown = request_state.shutdown_token();
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
        state.operation_snapshot().unwrap().state,
        crate::service::operation::OperationState::Cancelled
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
    assert!(state.request().shutdown_token().is_cancelled());
}

#[test]
fn shutdown_exit_codes_preserve_interrupt_and_terminate_policy() {
    assert_eq!(signal_exit(ShutdownReason::Interrupt), 130);
    assert_eq!(signal_exit(ShutdownReason::Terminate), 143);
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

#[test]
fn loopback_hosts_accept_names_ipv4_and_bracketed_ipv6_only() {
    for accepted in [
        "localhost",
        "localhost:9923",
        "127.0.0.1:9923",
        "[::1]:9923",
    ] {
        assert!(loopback_host(accepted), "{accepted}");
    }
    for rejected in [
        "example.test",
        "0.0.0.0:9923",
        "[::]:9923",
        "localhost.example",
    ] {
        assert!(!loopback_host(rejected), "{rejected}");
    }
}

#[tokio::test]
async fn management_routes_require_loopback_and_reserve_the_aibox_namespace() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));

    let remote = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/bootstrap",
            "192.0.2.10:5000",
        ))
        .await
        .unwrap();
    assert_eq!(remote.status(), StatusCode::FORBIDDEN);

    let missing = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/not-a-route",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(
        root.path()
            .join("requests")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );

    let removed_request_api = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/requests/api/records")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from("{}"))
        .unwrap();
    let removed = app.clone().oneshot(removed_request_api).await.unwrap();
    assert_eq!(removed.status(), StatusCode::NOT_FOUND);

    for legacy_path in [
        "/_aibox/requests/api/records",
        "/_aibox/requests/app.css",
        "/_aibox/requests/app.js",
    ] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, legacy_path, "127.0.0.1:5000"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{legacy_path}");
    }

    let requests = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/requests",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(requests.status(), StatusCode::OK);
    let body = requests.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["requests"], serde_json::json!([]));
    assert!(body.get("records").is_none());

    let overview = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/overview",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(overview.status(), StatusCode::OK);
    let overview_body: Value =
        serde_json::from_slice(&overview.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(overview_body.get("requests").is_none());
    assert!(overview_body.get("service").is_some());

    let bootstrap = app
        .oneshot(request(
            Method::GET,
            "/_aibox/api/bootstrap",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(bootstrap.status(), StatusCode::OK);
    assert!(
        bootstrap
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY)
    );
    let body = bootstrap.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["listen"], "127.0.0.1:9923");
}

#[tokio::test]
async fn component_update_check_is_shared_partial_and_socket_free() {
    use crate::component::LatestResult;
    use crate::testutil::FixtureLatestProvider;
    use std::collections::BTreeMap;

    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.set_latest_provider(Arc::new(FixtureLatestProvider {
        results: BTreeMap::from([
            (
                "node".to_string(),
                LatestResult::Available {
                    version: "24.19.0".to_string(),
                    source: "nodejs.org",
                },
            ),
            (
                "codex".to_string(),
                LatestResult::Unavailable {
                    source: "chatgpt.com",
                    error: "fixture unavailable".to_string(),
                },
            ),
        ]),
    }));
    let app = router(state.clone());

    let initial = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/components/latest",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(initial.status(), StatusCode::OK);
    let initial = initial.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&initial).unwrap(),
        Value::Null
    );

    let checked = app
        .clone()
        .oneshot(json_request("/_aibox/api/components/latest/check", "{}"))
        .await
        .unwrap();
    assert_eq!(checked.status(), StatusCode::OK);
    let checked = checked.into_body().collect().await.unwrap().to_bytes();
    let checked: Value = serde_json::from_slice(&checked).unwrap();
    assert!(checked["checked_at"].as_str().is_some());
    assert_eq!(checked["entries"].as_array().unwrap().len(), 6);
    assert!(checked["entries"].as_array().unwrap().iter().all(|entry| {
        entry["kind"] != "claude-statusline" && entry["kind"] != "codex-statusline"
    }));
    assert_eq!(
        checked["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["kind"] == "node")
            .unwrap()["version"],
        "24.19.0"
    );
    assert_eq!(
        checked["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["kind"] == "codex")
            .unwrap()["state"],
        "unavailable"
    );

    let shared = app
        .oneshot(request(
            Method::GET,
            "/_aibox/api/components/latest",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    let shared = shared.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(serde_json::from_slice::<Value>(&shared).unwrap(), checked);
    assert!(!root.path().join("tenants").exists());
    assert!(state.operation_snapshot().is_none());
    assert!(state.begin_management_mutation().is_ok());
}

#[tokio::test]
async fn console_page_authorizes_its_code_mirror_styles_with_a_fresh_nonce() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));

    let load_page = || {
        app.clone()
            .oneshot(request(Method::GET, "/_aibox/ui/configs", "127.0.0.1:5000"))
    };
    let first = load_page().await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_policy = first
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let first_body = first.into_body().collect().await.unwrap().to_bytes();
    let first_body = std::str::from_utf8(&first_body).unwrap();
    let prefix = r#"<meta name="aibox-csp-nonce" content=""#;
    let first_nonce = first_body
        .split_once(prefix)
        .and_then(|(_, suffix)| suffix.split_once('"'))
        .map(|(nonce, _)| nonce)
        .unwrap();

    assert!(!first_nonce.is_empty());
    assert!(first_policy.contains(&format!("style-src 'self' 'nonce-{first_nonce}'")));
    assert!(!first_policy.contains("'unsafe-inline'"));
    assert!(!first_body.contains("__AIBOX_CSP_NONCE__"));

    let second = load_page().await.unwrap();
    let second_body = second.into_body().collect().await.unwrap().to_bytes();
    let second_body = std::str::from_utf8(&second_body).unwrap();
    let second_nonce = second_body
        .split_once(prefix)
        .and_then(|(_, suffix)| suffix.split_once('"'))
        .map(|(nonce, _)| nonce)
        .unwrap();
    assert_ne!(first_nonce, second_nonce);
}

#[tokio::test]
async fn operation_events_stream_ends_when_service_shuts_down() {
    let root = tempfile::tempdir().unwrap();
    let state = test_state(root.path());
    let shutdown = state.request().shutdown_token();
    let response = router(state)
        .oneshot(request(
            Method::GET,
            "/_aibox/api/operations/events",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();

    shutdown.cancel();
    let body = tokio::time::timeout(Duration::from_secs(1), response.into_body().collect())
        .await
        .expect("Operations event stream must close during Service shutdown")
        .unwrap()
        .to_bytes();
    assert!(
        body.windows(b"event: operation".len())
            .any(|window| window == b"event: operation")
    );
}

#[tokio::test]
async fn management_writes_require_json_same_origin_and_csrf() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));
    let base = || request(Method::POST, "/_aibox/api/tenants", "127.0.0.1:5000");

    assert_eq!(
        app.clone().oneshot(base()).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let wrong_origin = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/api/tenants")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://localhost:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from(r#"{"name":"work"}"#))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(wrong_origin).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let valid = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/api/tenants")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from(r#"{"name":"work"}"#))
        .unwrap();
    let response = app.oneshot(valid).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["created"],
        "work"
    );
}

#[tokio::test]
async fn every_management_mutation_returns_conflict_without_filesystem_changes_when_busy() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let state = test_state(root.path());
    let _guard = state.begin_management_mutation().unwrap();
    let app = router(state);
    let before = filesystem_snapshot(root.path());
    let cases = [
        (
            "create Tenant",
            "/_aibox/api/tenants",
            serde_json::json!({"name": "busy-created"}),
        ),
        (
            "delete Tenant",
            "/_aibox/api/tenants/delete",
            serde_json::json!({
                "names": ["work"],
                "all": false,
                "confirmation": "work"
            }),
        ),
        (
            "install runtime Component",
            "/_aibox/api/components/install",
            serde_json::json!({
                "tenant": "managed:work",
                "component": "codex",
                "version": null
            }),
        ),
        (
            "install statusline Component",
            "/_aibox/api/components/install",
            serde_json::json!({
                "tenant": "managed:work",
                "component": "codex-statusline",
                "version": null
            }),
        ),
        (
            "remove Component",
            "/_aibox/api/components/remove",
            serde_json::json!({
                "tenant": "managed:work",
                "component": "codex-statusline",
                "version": null
            }),
        ),
        (
            "create Named Config",
            "/_aibox/api/configs/create",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "config": "busy"
            }),
        ),
        (
            "save Current Config",
            "/_aibox/api/configs/save",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "current": true,
                "config": null,
                "file": "config.toml",
                "revision": "unused-while-busy",
                "content_base64": ""
            }),
        ),
        (
            "apply Named Config",
            "/_aibox/api/configs/apply",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "config": "busy"
            }),
        ),
        (
            "delete Named Config",
            "/_aibox/api/configs/delete",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "configs": ["busy"],
                "all": false,
                "confirmation": "busy"
            }),
        ),
        (
            "execute Credential Propagation",
            "/_aibox/api/configs/propagate-auth/execute",
            serde_json::json!({"plan_id": "busy-plan"}),
        ),
        (
            "delete Session",
            "/_aibox/api/sessions/delete",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "ids": ["11111111-1111-1111-1111-111111111111"],
                "all": false,
                "confirmation": "111111111111"
            }),
        ),
        (
            "delete Request",
            "/_aibox/api/requests/delete",
            serde_json::json!({"ids": ["11111111-1111-1111-1111-111111111111"]}),
        ),
    ];

    for (label, path, body) in cases {
        let response = app
            .clone()
            .oneshot(json_request(path, body.to_string()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{label}");
        let body = response_json(response).await;
        assert_eq!(
            body["error"], "another management mutation is running",
            "{label}"
        );
        assert_eq!(filesystem_snapshot(root.path()), before, "{label}");
    }
}

#[tokio::test]
async fn runtime_image_operation_does_not_take_the_management_mutation_lock() {
    let root = tempfile::tempdir().unwrap();
    let state = test_state(root.path());
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    state
        .start_management_operation("build image", move |_context| {
            started_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
            Ok("built image".to_string())
        })
        .unwrap();
    started_rx.recv().unwrap();

    let response = router(state.clone())
        .oneshot(json_request(
            "/_aibox/api/tenants",
            serde_json::json!({"name": "during-build"}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(root.path().join("tenants/during-build").is_dir());

    finish_tx.send(()).unwrap();
    OperationCoordinator::new(state.clone())
        .wait_until_idle()
        .await;
    assert_eq!(
        state.operation_snapshot().unwrap().state,
        crate::service::operation::OperationState::Succeeded
    );
}

#[tokio::test]
async fn control_api_protects_the_default_managed_tenant_from_explicit_and_all_deletion() {
    let root = tempfile::tempdir().unwrap();
    for name in ["default", "work"] {
        crate::tenant::ManagedTenant::resolve(root.path(), name)
            .unwrap()
            .ensure_initialized()
            .unwrap();
    }
    let app = router(test_state(root.path()));
    let request = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/api/tenants/delete")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from(
            r#"{"names":["default"],"all":false,"confirmation":"default"}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(root.path().join("tenants/default").is_dir());

    let request = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/api/tenants/delete")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from(
            r#"{"names":[],"all":true,"confirmation":"delete all tenants"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(root.path().join("tenants/default").is_dir());
    assert!(!root.path().join("tenants/work").exists());
}

#[tokio::test]
async fn topology_and_session_summary_are_read_only_domain_views() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let state = test_state(root.path());
    let host_codex = state.host_home().join(".codex");
    let host_claude = state.host_home().join(".claude");
    let app = router(state);

    let topology = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/topology",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(topology.status(), StatusCode::OK);
    let topology_body = topology.into_body().collect().await.unwrap().to_bytes();
    let topology: Value = serde_json::from_slice(&topology_body).unwrap();
    let tenants = topology["tenants"].as_array().unwrap();
    assert_eq!(tenants.len(), 2);
    assert_eq!(tenants[0]["kind"], "host");
    assert_eq!(tenants[1]["name"], "work");
    assert_eq!(tenants[0]["agents"].as_array().unwrap().len(), 2);
    assert_eq!(
        tenants[0]["agents"][0]["current_config"]["present_files"],
        0
    );

    let summary = app
        .oneshot(request(
            Method::GET,
            "/_aibox/api/sessions/summary?tenant=host&agent=codex",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body = summary.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&summary_body).unwrap(),
        serde_json::json!({"count": 0, "warnings": [], "partial": false})
    );
    assert!(!host_codex.exists());
    assert!(!host_claude.exists());
}

#[tokio::test]
async fn missing_managed_config_scope_is_an_empty_read_only_view() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/configs?tenant=managed%3Amissing&agent=codex",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        serde_json::json!({
            "configs": [],
            "files": ["config.toml", "auth.json"],
            "application": {
                "last_application": null,
                "drift": "untracked"
            },
            "credential_propagation_available": false
        })
    );
    assert!(!root.path().join("tenants/missing").exists());

    for file in ["config.toml", "auth.json"] {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/_aibox/api/configs/reveal")
            .header(header::HOST, "127.0.0.1:9923")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:9923")
            .header("x-aibox-csrf", "test-csrf")
            .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
            .body(Body::from(
                serde_json::json!({
                    "tenant": "managed:missing",
                    "agent": "codex",
                    "current": true,
                    "config": null,
                    "file": file
                })
                .to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{file}");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["file"], file);
        assert_eq!(body["exists"], false);
    }
    assert!(!root.path().join("tenants/missing").exists());
}

#[tokio::test]
async fn named_config_lifecycle_is_socket_free_across_control_and_storage() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));
    let tenant = "managed:work";

    let created = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/create",
            serde_json::json!({"tenant": tenant, "agent": "codex", "config": "daily"}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);

    let listed = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/configs?tenant=managed%3Awork&agent=codex",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = response_json(listed).await;
    assert_eq!(listed["configs"][0]["name"], "daily");
    assert_eq!(listed["configs"][0]["state"], "ready");

    let reveal_body = serde_json::json!({
        "tenant": tenant,
        "agent": "codex",
        "current": false,
        "config": "daily",
        "file": "config.toml"
    });
    let revealed = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/reveal",
            reveal_body.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(revealed.status(), StatusCode::OK);
    let revealed = response_json(revealed).await;

    let saved = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/save",
            serde_json::json!({
                "tenant": tenant,
                "agent": "codex",
                "current": false,
                "config": "daily",
                "file": "config.toml",
                "revision": revealed["revision"],
                "content_base64": revealed["content_base64"]
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);

    let applied = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/apply",
            serde_json::json!({"tenant": tenant, "agent": "codex", "config": "daily"}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(applied.status(), StatusCode::OK);
    assert_eq!(response_json(applied).await["drift"], "clean");
    assert!(
        root.path()
            .join("tenants/work/.codex/config.toml")
            .is_file()
    );
    assert!(root.path().join("tenants/work/.codex/auth.json").is_file());

    let deleted = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/delete",
            serde_json::json!({
                "tenant": tenant,
                "agent": "codex",
                "configs": ["daily"],
                "all": false,
                "confirmation": "daily"
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    assert!(!root.path().join("codex/work/daily").exists());
}

#[tokio::test]
async fn statusline_lifecycle_stays_socket_free_and_never_starts_docker() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));
    let mutation = serde_json::json!({
        "tenant": "managed:work",
        "component": "codex-statusline",
        "version": null
    });

    let installed = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/components/install",
            mutation.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(installed.status(), StatusCode::OK);
    assert_eq!(
        response_json(installed).await["installed"],
        "codex-statusline"
    );

    let listed = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/_aibox/api/components?tenant=managed%3Awork",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    let listed = response_json(listed).await;
    let statusline = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "codex-statusline")
        .unwrap();
    assert_eq!(statusline["status"], "installed");
    assert!(
        root.path()
            .join("tenants/work/.codex/config.toml")
            .is_file()
    );

    let removed = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/components/remove",
            mutation.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(response_json(removed).await["removed"], "codex-statusline");

    let listed = app
        .oneshot(request(
            Method::GET,
            "/_aibox/api/components?tenant=managed%3Awork",
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();
    let listed = response_json(listed).await;
    let statusline = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "codex-statusline")
        .unwrap();
    assert_eq!(statusline["status"], "not-installed");
}

#[tokio::test]
async fn credential_propagation_preview_and_execute_share_one_preflight_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));
    let source = serde_json::json!({
        "auth_mode": "chatgpt",
        "tokens": {"account_id": "same-account"},
        "last_refresh": "2026-08-29T08:00:00Z",
        "marker": "source"
    })
    .to_string();
    let older = serde_json::json!({
        "auth_mode": "chatgpt",
        "tokens": {"account_id": "same-account"},
        "last_refresh": "2026-08-28T08:00:00Z",
        "marker": "older"
    })
    .to_string();

    let host_target = serde_json::json!({
        "tenant": "host",
        "agent": "codex",
        "current": true,
        "config": null,
        "file": "auth.json"
    });
    let host_reveal = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/reveal",
            host_target.to_string(),
        ))
        .await
        .unwrap();
    let host_reveal = response_json(host_reveal).await;
    let host_save = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/save",
            serde_json::json!({
                "tenant": "host",
                "agent": "codex",
                "current": true,
                "config": null,
                "file": "auth.json",
                "revision": host_reveal["revision"],
                "content_base64": base64::engine::general_purpose::STANDARD.encode(&source)
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(host_save.status(), StatusCode::OK);

    let create = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/create",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "config": "older"
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);

    let named_target = serde_json::json!({
        "tenant": "managed:work",
        "agent": "codex",
        "current": false,
        "config": "older",
        "file": "auth.json"
    });
    let named_reveal = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/reveal",
            named_target.to_string(),
        ))
        .await
        .unwrap();
    let named_reveal = response_json(named_reveal).await;
    let named_save = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/save",
            serde_json::json!({
                "tenant": "managed:work",
                "agent": "codex",
                "current": false,
                "config": "older",
                "file": "auth.json",
                "revision": named_reveal["revision"],
                "content_base64": base64::engine::general_purpose::STANDARD.encode(&older)
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(named_save.status(), StatusCode::OK);

    let preview = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/propagate-auth/preview",
            "{}",
        ))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = response_json(preview).await;
    assert_eq!(preview["preview"]["updates"], 1);
    assert_eq!(
        preview["preview"]["entries"][0]["label"],
        "tenant/work/config/older"
    );
    let plan_id = preview["plan_id"].as_str().unwrap();

    let executed = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/propagate-auth/execute",
            serde_json::json!({"plan_id": plan_id}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(executed.status(), StatusCode::OK);
    let executed = response_json(executed).await;
    assert_eq!(executed["entries"][0]["outcome"]["status"], "updated");

    let final_auth = app
        .oneshot(json_request(
            "/_aibox/api/configs/reveal",
            named_target.to_string(),
        ))
        .await
        .unwrap();
    let final_auth = response_json(final_auth).await;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(final_auth["content_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["marker"],
        "source"
    );
}

#[tokio::test]
async fn session_detail_and_evidence_routes_stream_and_validate_snapshots() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let id = "44444444-4444-4444-4444-444444444444";
    let transcript = crate::testutil::write_jsonl(
        root.path(),
        &format!("tenants/work/.codex/sessions/2026/08/20/rollout-detail-{id}.jsonl"),
        &[
            r#"{"timestamp":"2026-08-20T09:00:00Z","type":"session_meta","payload":{"timestamp":"2026-08-20T09:00:00Z"}}"#,
            r#"{"timestamp":"2026-08-20T09:00:01Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"hello"}]}}"#,
            r#"{"timestamp":"2026-08-20T09:00:02Z","type":"event_msg","payload":{"type":"agent_message","message":"hi"}}"#,
        ],
    );
    let app = router(test_state(root.path()));
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/_aibox/api/sessions/detail?tenant=managed%3Awork&agent=codex&id={id}"),
            "127.0.0.1:5000",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/x-ndjson; charset=utf-8"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let frames = body
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frames[0]["type"], "meta");
    assert!(
        frames
            .iter()
            .any(|frame| { frame["type"] == "message" && frame["message"]["role"] == "user" })
    );
    assert!(
        frames
            .iter()
            .any(|frame| { frame["type"] == "message" && frame["message"]["role"] == "assistant" })
    );
    let complete = frames
        .iter()
        .find(|frame| frame["type"] == "complete")
        .unwrap();
    let snapshot = complete["stats"]["snapshot"].as_str().unwrap();

    let evidence = app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!(
                    "/_aibox/api/sessions/evidence?tenant=managed%3Awork&agent=codex&id={id}&entry=line-2&snapshot={snapshot}"
                ),
                "127.0.0.1:5000",
            ))
            .await
            .unwrap();
    assert_eq!(evidence.status(), StatusCode::OK);
    let evidence = evidence.into_body().collect().await.unwrap().to_bytes();
    let evidence = serde_json::from_slice::<Value>(&evidence).unwrap();
    assert_eq!(evidence["entry_id"], "line-2");
    assert!(evidence["content"].as_str().unwrap().contains("hello"));

    fs::write(&transcript, b"changed\n").unwrap();
    let stale = app
            .oneshot(request(
                Method::GET,
                &format!(
                    "/_aibox/api/sessions/evidence?tenant=managed%3Awork&agent=codex&id={id}&entry=line-2&snapshot={snapshot}"
                ),
                "127.0.0.1:5000",
            ))
            .await
            .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn session_delete_api_stays_within_the_selected_tenant_and_agent() {
    let root = tempfile::tempdir().unwrap();
    for name in ["work", "other"] {
        crate::tenant::ManagedTenant::resolve(root.path(), name)
            .unwrap()
            .ensure_initialized()
            .unwrap();
    }
    let selected_id = "11111111-2222-3333-4444-555555555555";
    let kept_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let selected = crate::testutil::write_jsonl(
        root.path(),
        &format!("tenants/work/.codex/sessions/2026/08/17/rollout-selected-{selected_id}.jsonl"),
        &[r#"{"timestamp":"2026-08-17T10:00:00Z","type":"session_meta"}"#],
    );
    let same_tenant_unselected = crate::testutil::write_jsonl(
        root.path(),
        &format!("tenants/work/.codex/sessions/2026/08/17/rollout-kept-{kept_id}.jsonl"),
        &[r#"{"timestamp":"2026-08-17T09:00:00Z","type":"session_meta"}"#],
    );
    let other_tenant = crate::testutil::write_jsonl(
        root.path(),
        &format!("tenants/other/.codex/sessions/2026/08/17/rollout-other-{selected_id}.jsonl"),
        &[r#"{"timestamp":"2026-08-17T08:00:00Z","type":"session_meta"}"#],
    );
    let other_agent = crate::testutil::write_jsonl(
        root.path(),
        &format!("tenants/work/.claude/projects/demo/{selected_id}.jsonl"),
        &[r#"{"timestamp":"2026-08-17T07:00:00Z"}"#],
    );
    let body = serde_json::json!({
        "tenant": "managed:work",
        "agent": "codex",
        "ids": [selected_id],
        "all": false,
        "confirmation": ""
    })
    .to_string();
    let request = Request::builder()
        .method(Method::POST)
        .uri("/_aibox/api/sessions/delete")
        .header(header::HOST, "127.0.0.1:9923")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:9923")
        .header("x-aibox-csrf", "test-csrf")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
        .body(Body::from(body))
        .unwrap();

    let response = router(test_state(root.path()))
        .oneshot(request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        serde_json::json!({"deleted": 1})
    );
    assert!(!selected.exists());
    assert!(same_tenant_unselected.exists());
    assert!(other_tenant.exists());
    assert!(other_agent.exists());
}

#[tokio::test]
async fn control_router_exposes_the_complete_method_and_path_surface() {
    let root = tempfile::tempdir().unwrap();
    let app = control::router().with_state(test_state(root.path()));
    let routes = [
        (Method::GET, "/_aibox/ui"),
        (Method::GET, "/_aibox/ui/app.css"),
        (Method::GET, "/_aibox/ui/app.js"),
        (Method::GET, "/_aibox/ui/configs"),
        (Method::GET, "/_aibox/api/bootstrap"),
        (Method::GET, "/_aibox/api/overview"),
        (Method::GET, "/_aibox/api/topology"),
        (Method::GET, "/_aibox/api/tenants"),
        (Method::POST, "/_aibox/api/tenants"),
        (Method::POST, "/_aibox/api/tenants/delete"),
        (Method::GET, "/_aibox/api/components"),
        (Method::GET, "/_aibox/api/components/latest"),
        (Method::POST, "/_aibox/api/components/latest/check"),
        (Method::POST, "/_aibox/api/components/install"),
        (Method::POST, "/_aibox/api/components/remove"),
        (Method::GET, "/_aibox/api/configs"),
        (Method::POST, "/_aibox/api/configs/create"),
        (Method::POST, "/_aibox/api/configs/reveal"),
        (Method::POST, "/_aibox/api/configs/save"),
        (Method::POST, "/_aibox/api/configs/diagnose"),
        (Method::POST, "/_aibox/api/configs/apply"),
        (Method::POST, "/_aibox/api/configs/delete"),
        (Method::POST, "/_aibox/api/configs/propagate-auth/preview"),
        (Method::POST, "/_aibox/api/configs/propagate-auth/execute"),
        (Method::GET, "/_aibox/api/sessions"),
        (Method::GET, "/_aibox/api/sessions/summary"),
        (Method::GET, "/_aibox/api/sessions/detail"),
        (Method::GET, "/_aibox/api/sessions/evidence"),
        (Method::POST, "/_aibox/api/sessions/delete"),
        (Method::GET, "/_aibox/api/operations/current"),
        (Method::GET, "/_aibox/api/operations/events"),
        (Method::POST, "/_aibox/api/operations/build"),
        (Method::POST, "/_aibox/api/operations/not-an-id/cancel"),
        (Method::GET, "/_aibox/api/requests"),
        (Method::GET, "/_aibox/api/requests/not-an-id"),
        (Method::GET, "/_aibox/api/requests/not-an-id/request-body"),
        (Method::GET, "/_aibox/api/requests/not-an-id/response-body"),
        (
            Method::GET,
            "/_aibox/api/requests/not-an-id/request-body-decoded",
        ),
        (
            Method::GET,
            "/_aibox/api/requests/not-an-id/response-body-decoded",
        ),
        (
            Method::GET,
            "/_aibox/api/requests/not-an-id/response-event-timings",
        ),
    ];

    for (method, path) in routes {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .body(if method == Method::POST {
                        Body::from("{}")
                    } else {
                        Body::empty()
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
    }

    let unknown = app
        .oneshot(
            Request::builder()
                .uri("/_aibox/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}
