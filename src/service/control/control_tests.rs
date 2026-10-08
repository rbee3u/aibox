use super::*;
use axum::response::IntoResponse as _;
use base64::Engine as _;

#[test]
fn application_error_categories_have_stable_http_statuses() {
    let cases = [
        (ApplicationErrorKind::InvalidInput, StatusCode::BAD_REQUEST),
        (ApplicationErrorKind::NotFound, StatusCode::NOT_FOUND),
        (ApplicationErrorKind::Conflict, StatusCode::CONFLICT),
        (
            ApplicationErrorKind::InputTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (ApplicationErrorKind::Busy, StatusCode::CONFLICT),
        (
            ApplicationErrorKind::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (kind, expected) in cases {
        let error = crate::application_error::application_error(kind, "domain failure")
            .context("Control operation failed");
        let response = ControlError::from(error).into_response();
        assert_eq!(response.status(), expected, "{kind:?}");
    }
}

#[test]
fn unclassified_domain_errors_remain_bad_requests() {
    let response = ControlError::from(anyhow::anyhow!("invalid selector")).into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn every_control_error_carries_the_message_as_a_plain_string() {
    use http_body_util::BodyExt as _;

    for (label, status, message) in [
        ("domain error", StatusCode::CONFLICT, "tenant is protected"),
        (
            "request read error",
            StatusCode::NOT_FOUND,
            "no such request",
        ),
    ] {
        let response = ControlError::new(status, message).into_response();
        assert_eq!(response.status(), status, "{label}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, serde_json::json!({"error": message}), "{label}");
        assert_eq!(
            body["error"].as_str(),
            Some(message),
            "{label}: the Console reads `error` as a string"
        );
    }
}

use crate::service::router;
use crate::service::testutil::*;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tower::ServiceExt as _;
#[tokio::test]
async fn component_update_check_is_shared_partial_and_socket_free() {
    use crate::component::LatestResult;
    use crate::testutil::FixtureLatestProvider;
    use std::collections::BTreeMap;

    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state
        .management
        .components
        .set_latest_provider(Arc::new(FixtureLatestProvider {
            results: BTreeMap::from([
                (
                    "node".to_string(),
                    LatestResult::Available {
                        version: "24.19.0".to_string(),
                        newest: Some("26.8.2".to_string()),
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
            .find(|entry| entry["kind"] == "node")
            .unwrap()["newest"],
        "26.8.2"
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
    assert!(
        state
            .management
            .operations
            .current(None)
            .operation
            .is_none()
    );
    assert!(state.management.tenants.hold_mutation_for_test().is_ok());
}

#[tokio::test]
async fn explicit_component_check_joins_prefetch_and_publishes_its_failed_batch() {
    let root = tempfile::tempdir().unwrap();
    let control = PendingLatestControl::default();
    let mut state = test_state(root.path());
    state
        .management
        .components
        .set_latest_provider(Arc::new(PendingLatestProvider {
            control: control.clone(),
        }));
    let app = router(state.clone());
    let prefetch_state = state.clone();
    let observed_state = state.clone();
    let release = control.clone();

    let ((), response, ()) = tokio::join!(
        biased;
        async move { prefetch_state.management.components.prefetch().await },
        async move {
            app.oneshot(json_request("/_aibox/api/components/latest/check", "{}"))
                .await
                .unwrap()
        },
        async move {
            release.wait_until_all_started().await;
            assert_eq!(observed_state.management.components.latest().await, None);
            release.release();
        },
    );

    assert_eq!(response.status(), StatusCode::OK);
    let checked = response_json(response).await;
    assert!(
        checked["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| { entry["state"] == "unavailable" })
    );
    assert_eq!(
        control.calls.load(Ordering::SeqCst),
        VERSIONED_COMPONENT_COUNT
    );
    assert_eq!(
        checked,
        serde_json::to_value(state.management.components.latest().await.unwrap()).unwrap()
    );
}

#[tokio::test]
async fn concurrent_explicit_component_checks_share_one_batch() {
    let root = tempfile::tempdir().unwrap();
    let control = PendingLatestControl::default();
    let mut state = test_state(root.path());
    let previous = state.management.components.check_latest().await;
    state
        .management
        .components
        .set_latest_provider(Arc::new(PendingLatestProvider {
            control: control.clone(),
        }));
    let app = router(state.clone());
    let observed_state = state;
    let release = control.clone();

    let (first, second, ()) = tokio::join!(
        biased;
        app.clone().oneshot(json_request(
            "/_aibox/api/components/latest/check",
            "{}",
        )),
        app.oneshot(json_request(
            "/_aibox/api/components/latest/check",
            "{}",
        )),
        async move {
            release.wait_until_all_started().await;
            assert_eq!(
                observed_state.management.components.latest().await,
                Some(previous)
            );
            release.release();
        },
    );
    let first = first.unwrap();
    let second = second.unwrap();

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(response_json(first).await, response_json(second).await);
    assert_eq!(
        control.calls.load(Ordering::SeqCst),
        VERSIONED_COMPONENT_COUNT
    );
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
    assert!(first_body.contains("src=\"/_aibox/ui/assets/index.js\""));
    assert!(first_body.contains("href=\"/_aibox/ui/assets/style.css\""));

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
    let shutdown = state.shutdown_token();
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
async fn every_management_mutation_returns_conflict_without_filesystem_changes_when_busy() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let state = test_state(root.path());
    let _guard = state.management.tenants.hold_mutation_for_test().unwrap();
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
        .management
        .operations
        .start_for_test("build image", move |_context| {
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
    state.management.operations.wait_until_idle().await;
    assert_eq!(
        state
            .management
            .operations
            .current(None)
            .operation
            .unwrap()
            .state,
        crate::management::OperationState::Succeeded
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
    let host_codex = root.path().join("host-home").join(".codex");
    let host_claude = root.path().join("host-home").join(".claude");
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
    let state = test_state(root.path());
    let app = super::router()
        .fallback(|| async { StatusCode::IM_A_TEAPOT })
        .with_state(state.clone());

    for endpoint in super::routes::ENDPOINTS {
        let method: Method = endpoint.method.parse().unwrap();
        let path = endpoint
            .path
            .replace("{*path}", "configs")
            .replace("{id}", "not-an-id");
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(&path)
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
            StatusCode::IM_A_TEAPOT,
            "unregistered {method} {path}"
        );
        assert_ne!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
    }

    let unknown = super::router()
        .with_state(state)
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

#[tokio::test]
async fn config_comparison_endpoint_is_read_only_and_scoped_to_last_application() {
    let root = tempfile::tempdir().unwrap();
    let app = router(test_state(root.path()));
    for path in ["create", "apply"] {
        let response = app
            .clone()
            .oneshot(json_request(
                &format!("/_aibox/api/configs/{path}"),
                serde_json::json!({"tenant":"managed:work", "agent":"codex", "config":"source"})
                    .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let before = filesystem_snapshot(root.path());
    let response = app
        .clone()
        .oneshot(json_request(
            "/_aibox/api/configs/compare",
            serde_json::json!({
                "tenant":"managed:work", "agent":"codex", "current":true, "config":null, "files":[]
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["source"], "source");
    assert_eq!(body["incomplete"], false);
    assert_eq!(body["files"][0]["differences"], serde_json::json!([]));
    assert_eq!(before, filesystem_snapshot(root.path()));
    let response = app.oneshot(json_request("/_aibox/api/configs/compare", serde_json::json!({
        "tenant":"managed:work", "agent":"codex", "current":false, "config":"other", "files":[]
    }).to_string())).await.unwrap();
    assert!(!response.status().is_success());
    assert_eq!(before, filesystem_snapshot(root.path()));
}
