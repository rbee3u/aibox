//! Shared Request state plus the socket-free legacy router used by API and
//! proxy tests. The foreground listener and combined routing live in
//! [`crate::service`].
//!
//! The proxy is global rather than Tenant-owned and never starts Docker; see
//! `docs/adr/0008-global-trusted-request-service.md`.

use crate::request_console::RequestConsole;
use crate::request_store::RequestStore;
use anyhow::Result;
use std::path::Path;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[cfg(test)]
use crate::{request_proxy, request_web};
#[cfg(test)]
use axum::Router;
#[cfg(test)]
use axum::extract::State;
#[cfg(test)]
use axum::routing::{get, post};
#[cfg(test)]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
#[cfg(test)]
use tokio::net::TcpListener;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: RequestStore,
    pub(crate) shutdown: CancellationToken,
    pub(crate) response_tasks: TaskTracker,
    pub(crate) allow_private_upstream: bool,
}

impl AppState {
    #[cfg(test)]
    pub(crate) fn new(root: &Path, shutdown: CancellationToken) -> Result<Self> {
        Self::new_with_console(root, shutdown, None)
    }

    pub(crate) fn new_with_console(
        root: &Path,
        shutdown: CancellationToken,
        console: Option<RequestConsole>,
    ) -> Result<Self> {
        Ok(Self {
            store: RequestStore::open_with_console(root, console)?,
            shutdown,
            response_tasks: TaskTracker::new(),
            allow_private_upstream: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: &Path) -> Result<Self> {
        let mut state = Self::new(root, CancellationToken::new())?;
        state.allow_private_upstream = true;
        Ok(state)
    }
}

#[cfg(test)]
fn request_viewer_url(listen: SocketAddr) -> String {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}/", SocketAddr::new(ip, listen.port()))
}

#[cfg(test)]
fn router(state: AppState) -> Router {
    let request_viewer = Router::new()
        .route("/", get(request_web::index))
        .route("/_aibox/requests/app.css", get(request_web::css))
        .route("/_aibox/requests/app.js", get(request_web::js))
        .route(
            "/_aibox/requests/api/records",
            get(request_web::list_records),
        )
        .route(
            "/_aibox/requests/api/records/delete",
            post(request_web::delete_records),
        )
        .route(
            "/_aibox/requests/api/records/{id}",
            get(request_web::record_detail),
        )
        .route(
            "/_aibox/requests/api/records/{id}/request-body",
            get(request_web::request_body),
        )
        .route(
            "/_aibox/requests/api/records/{id}/response-body",
            get(request_web::response_body),
        )
        .route(
            "/_aibox/requests/api/records/{id}/request-body-decoded",
            get(request_web::decoded_request_body),
        )
        .route(
            "/_aibox/requests/api/records/{id}/response-body-decoded",
            get(request_web::decoded_response_body),
        )
        .route(
            "/_aibox/requests/api/records/{id}/response-event-timings",
            get(request_web::response_event_timings),
        )
        .route("/_aibox/requests/{*path}", get(request_web::not_found));
    Router::new()
        .merge(request_viewer)
        .fallback(proxy_fallback)
        .with_state(state)
}

#[cfg(test)]
async fn proxy_fallback(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    request_proxy::handle(state, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_store::{Outcome, StoredRecord};
    use axum::body::Body;
    use axum::http::{HeaderValue, Request, Response, StatusCode, header};
    use axum::routing::{get, post};
    use bytes::Bytes;
    use http_body_util::BodyExt as _;
    use std::time::Duration;
    use tower::ServiceExt as _;

    #[test]
    fn request_viewer_url_maps_wildcards_to_clickable_loopback_addresses() {
        for (listen, expected) in [
            ("127.0.0.1:9923", "http://127.0.0.1:9923/"),
            ("0.0.0.0:8080", "http://127.0.0.1:8080/"),
            ("[::]:9923", "http://[::1]:9923/"),
            ("192.0.2.10:9923", "http://192.0.2.10:9923/"),
        ] {
            assert_eq!(request_viewer_url(listen.parse().unwrap()), expected);
        }
    }

    #[tokio::test]
    async fn embedded_ui_and_api_are_served_in_memory_without_request_guards() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path()).unwrap();
        let service = router(state.clone());

        for (path, content_type) in [
            ("/", "text/html; charset=utf-8"),
            ("/_aibox/requests/app.css", "text/css; charset=utf-8"),
            (
                "/_aibox/requests/app.js",
                "application/javascript; charset=utf-8",
            ),
            (
                "/_aibox/requests/api/records",
                "application/json; charset=utf-8",
            ),
        ] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                content_type,
                "{path}"
            );
            let body = response.into_body().collect().await.unwrap().to_bytes();
            if path == "/" {
                let html = String::from_utf8(body.to_vec()).unwrap();
                assert!(!html.contains("__AIBOX_CSRF__"));
                assert!(!html.contains("aibox-csrf"));
                assert!(html.contains("/_aibox/ui/app.css"));
                assert!(html.contains("/_aibox/ui/app.js"));
            }
        }

        let unconstrained_request = Request::builder()
            .uri("/")
            .header(header::HOST, "arbitrary.example")
            .header(header::ORIGIN, "http://arbitrary.example")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let response = service.oneshot(unconstrained_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    async fn echo(request: Request<Body>) -> Response<Body> {
        let body = request.into_body().collect().await.unwrap().to_bytes();
        let mut response = Response::new(Body::from(body));
        response
            .headers_mut()
            .append("x-upstream-repeat", HeaderValue::from_static("one"));
        response
            .headers_mut()
            .append("x-upstream-repeat", HeaderValue::from_static("two"));
        response
    }

    async fn redirect() -> Response<Body> {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::FOUND;
        response
            .headers_mut()
            .insert(header::LOCATION, HeaderValue::from_static("/v1/echo"));
        response
    }

    async fn test_servers(
        root: &Path,
    ) -> (
        AppState,
        SocketAddr,
        SocketAddr,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = upstream_listener.local_addr().unwrap();
        let upstream_router = Router::new()
            .route("/v1/echo", post(echo))
            .route("/v1/redirect", get(redirect));
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream_listener, upstream_router)
                .await
                .unwrap();
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let state = AppState::for_test(root).unwrap();
        let proxy_router = router(state.clone());
        let proxy_task = tokio::spawn(async move {
            axum::serve(
                proxy_listener,
                proxy_router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (state, upstream, proxy_address, upstream_task, proxy_task)
    }

    async fn wait_for_terminal(state: &AppState) -> StoredRecord {
        for _ in 0..100 {
            let records = state.store.scan().unwrap();
            if let Some(record) = records.into_iter().next()
                && record.result.is_some()
            {
                return record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("Request Record did not reach a terminal state");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "optional real TCP smoke test; requires a network-permitted environment"]
    async fn reqwest_tcp_smoke_preserves_bytes_headers_query_and_redirect_policy() {
        let root = tempfile::tempdir().unwrap();
        let (state, upstream, proxy_address, upstream_task, proxy_task) =
            test_servers(root.path()).await;
        let target = format!("http://{upstream}/v1/echo?tag=one&tag=&tag=two");
        let proxy_url = format!("http://{proxy_address}/{target}");
        let mut headers = axum::http::HeaderMap::new();
        headers.append("x-client-repeat", HeaderValue::from_static("one"));
        headers.append("x-client-repeat", HeaderValue::from_static("two"));
        let raw = Bytes::from_static(b"request\0\xffbody");
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let response = client
            .post(proxy_url)
            .headers(headers)
            .body(raw.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get_all("x-upstream-repeat")
                .iter()
                .count(),
            2
        );
        assert_eq!(response.bytes().await.unwrap(), raw);

        let record = wait_for_terminal(&state).await;
        assert_eq!(
            record.request.upstream_url.as_deref(),
            Some(target.as_str())
        );
        assert_eq!(
            record
                .request
                .headers
                .iter()
                .filter(|header| header.name == "x-client-repeat")
                .count(),
            2
        );
        assert_eq!(
            std::fs::read(record.directory.join("request.body")).unwrap(),
            raw
        );
        assert_eq!(
            std::fs::read(record.directory.join("response.body")).unwrap(),
            raw
        );
        assert_eq!(record.result.unwrap().outcome, Outcome::Completed);

        let redirect_url = format!("http://{proxy_address}/http://{upstream}/v1/redirect");
        assert_eq!(
            client.get(redirect_url).send().await.unwrap().status(),
            StatusCode::FOUND
        );
        state.shutdown.cancel();
        state.response_tasks.close();
        state.response_tasks.wait().await;
        upstream_task.abort();
        proxy_task.abort();
    }
}
