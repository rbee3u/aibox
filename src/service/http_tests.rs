use super::*;
use crate::service::testutil::*;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;
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
    assert!(
        overview_body["host_home"]
            .as_str()
            .is_some_and(|home| home.ends_with("host-home")),
        "{overview_body}"
    );

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
