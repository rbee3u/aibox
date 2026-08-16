//! The `aibox traffic` command: listeners, routing, and graceful shutdown.
//!
//! One Axum router serves two unrelated surfaces on the same port: the
//! Traffic Viewer at `/` with its assets and JSON API under
//! `/_aibox/traffic/`, and a catch-all fallback that proxies everything else.
//! Both surfaces are available through the single socket selected by
//! `--listen`.
//!
//! The proxy is global rather than Tenant-owned and never starts Docker; see
//! `docs/adr/0008-global-trusted-traffic-service.md`.

use crate::cli::TrafficArgs;
use crate::tenant;
use crate::traffic_console::{ShutdownReason, TrafficConsole};
use crate::traffic_proxy;
use crate::traffic_store::TrafficStore;
use crate::traffic_web;
use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use socket2::{Domain, Protocol, Socket, Type};
use std::future::IntoFuture as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: TrafficStore,
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
        console: Option<TrafficConsole>,
    ) -> Result<Self> {
        Ok(Self {
            store: TrafficStore::open_with_console(root, console)?,
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

/// Run the foreground host-side Traffic Proxy and Traffic Viewer.
pub(crate) fn dispatch(args: &TrafficArgs) -> Result<i32> {
    validate_listener(args)?;
    let root = tenant::aibox_root()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("aibox-traffic")
        .build()
        .context("create Traffic Proxy async runtime")?;
    runtime.block_on(serve(args.listen, &root))
}

fn validate_listener(args: &TrafficArgs) -> Result<()> {
    if args.listen.port() == 0 {
        bail!("Traffic listener port must not be 0");
    }
    Ok(())
}

async fn serve(listen: SocketAddr, root: &Path) -> Result<i32> {
    let shutdown = CancellationToken::new();
    let console = TrafficConsole::new();
    let state = AppState::new_with_console(root, shutdown.clone(), Some(console.clone()))?;
    let listener =
        bind_listener(listen).with_context(|| format!("bind Traffic listener {listen}"))?;
    let router = router(state.clone());

    console.startup(&listen.to_string(), &traffic_viewer_url(listen));

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    let signal_task = tokio::spawn(async move {
        signal_loop(signal_tx).await;
    });

    let mut servers = JoinSet::new();
    let listener_shutdown = shutdown.clone();
    servers.spawn(
        axum::serve(listener, router)
            .with_graceful_shutdown(listener_shutdown.cancelled_owned())
            .into_future(),
    );
    let mut first_error = None;
    let mut reason = None;
    while !servers.is_empty() {
        tokio::select! {
            result = servers.join_next() => {
                let Some(result) = result else { break };
                if let Some(error) = server_error(result) {
                    first_error.get_or_insert(error);
                    shutdown.cancel();
                    break;
                }
            }
            received = signal_rx.recv() => {
                let Some(received) = received else { break };
                reason = Some(received);
                console.begin_shutdown(received, state.store.active_count());
                shutdown.cancel();
                break;
            }
        }
    }
    while !servers.is_empty() {
        tokio::select! {
            result = servers.join_next() => {
                let Some(result) = result else { break };
                if let Some(error) = server_error(result) {
                    first_error.get_or_insert(error);
                }
            }
            received = signal_rx.recv(), if reason.is_some() => {
                if let Some(received) = received {
                    console.forced_shutdown(received);
                    signal_task.abort();
                    return Ok(received.forced_exit_code());
                }
            }
        }
    }
    if let Some(error) = first_error {
        signal_task.abort();
        await_response_tasks(&state).await;
        return Err(error).context("serve Traffic Proxy");
    }
    state.response_tasks.close();
    tokio::select! {
        _ = state.response_tasks.wait() => {
            signal_task.abort();
            let code = reason.map_or(0, ShutdownReason::completion_exit_code);
            if reason.is_some() {
                console.stopped(code);
            }
            Ok(code)
        }
        received = signal_rx.recv(), if reason.is_some() => {
            let received = received.unwrap_or_else(|| reason.expect("shutdown reason exists"));
            console.forced_shutdown(received);
            signal_task.abort();
            Ok(received.forced_exit_code())
        }
    }
}

fn server_error(
    result: Result<Result<(), std::io::Error>, tokio::task::JoinError>,
) -> Option<anyhow::Error> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(anyhow::Error::new(error)),
        Err(error) => Some(anyhow::Error::new(error)),
    }
}

fn traffic_viewer_url(listen: SocketAddr) -> String {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}/", SocketAddr::new(ip, listen.port()))
}

fn router(state: AppState) -> Router {
    let traffic_viewer = Router::new()
        .route("/", get(traffic_web::index))
        .route("/_aibox/traffic/app.css", get(traffic_web::css))
        .route("/_aibox/traffic/app.js", get(traffic_web::js))
        .route(
            "/_aibox/traffic/api/records",
            get(traffic_web::list_records),
        )
        .route(
            "/_aibox/traffic/api/records/delete",
            post(traffic_web::delete_records),
        )
        .route(
            "/_aibox/traffic/api/records/delete-all",
            post(traffic_web::delete_all),
        )
        .route(
            "/_aibox/traffic/api/records/{id}",
            get(traffic_web::record_detail),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/request-body",
            get(traffic_web::request_body),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/response-body",
            get(traffic_web::response_body),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/request-body-decoded",
            get(traffic_web::decoded_request_body),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/response-body-decoded",
            get(traffic_web::decoded_response_body),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/response-event-timings",
            get(traffic_web::response_event_timings),
        )
        .route("/_aibox/traffic/{*path}", get(traffic_web::not_found));
    Router::new()
        .merge(traffic_viewer)
        .fallback(proxy_fallback)
        .with_state(state)
}

async fn proxy_fallback(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    traffic_proxy::handle(state, request).await
}

fn bind_listener(address: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    if address.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

async fn await_response_tasks(state: &AppState) {
    state.response_tasks.close();
    state.response_tasks.wait().await;
}

async fn signal_loop(sender: mpsc::UnboundedSender<ShutdownReason>) {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            loop {
                let reason = tokio::select! {
                    _ = tokio::signal::ctrl_c() => Some(ShutdownReason::Interrupt),
                    signal = terminate.recv() => signal.map(|_| ShutdownReason::Terminate),
                };
                let Some(reason) = reason else { break };
                if sender.send(reason).is_err() {
                    return;
                }
            }
        }
        loop {
            if tokio::signal::ctrl_c().await.is_err()
                || sender.send(ShutdownReason::Interrupt).is_err()
            {
                return;
            }
        }
    }
    #[cfg(not(unix))]
    loop {
        if tokio::signal::ctrl_c().await.is_err() || sender.send(ShutdownReason::Interrupt).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_store::{Outcome, StoredRecord};
    use axum::body::Body;
    use axum::http::{HeaderValue, Request, Response, StatusCode, header};
    use axum::routing::{get, post};
    use bytes::Bytes;
    use http_body_util::BodyExt as _;
    use std::time::Duration;
    use tower::ServiceExt as _;

    #[test]
    fn traffic_viewer_url_maps_wildcards_to_clickable_loopback_addresses() {
        for (listen, expected) in [
            ("127.0.0.1:9923", "http://127.0.0.1:9923/"),
            ("0.0.0.0:8080", "http://127.0.0.1:8080/"),
            ("[::]:9923", "http://[::1]:9923/"),
            ("192.0.2.10:9923", "http://192.0.2.10:9923/"),
        ] {
            assert_eq!(traffic_viewer_url(listen.parse().unwrap()), expected);
        }
    }

    #[test]
    fn listener_rejects_an_ephemeral_port_the_viewer_cannot_advertise() {
        let args = TrafficArgs {
            listen: "127.0.0.1:0".parse().unwrap(),
        };
        assert_eq!(
            validate_listener(&args).unwrap_err().to_string(),
            "Traffic listener port must not be 0"
        );
    }

    #[tokio::test]
    async fn embedded_ui_and_api_are_served_in_memory_without_request_guards() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path()).unwrap();
        let service = router(state.clone());

        for (path, content_type) in [
            ("/", "text/html; charset=utf-8"),
            ("/_aibox/traffic/app.css", "text/css; charset=utf-8"),
            (
                "/_aibox/traffic/app.js",
                "application/javascript; charset=utf-8",
            ),
            (
                "/_aibox/traffic/api/records",
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
                assert!(html.contains("/_aibox/traffic/app.css"));
                assert!(html.contains("/_aibox/traffic/app.js"));
            }
        }

        let delete_all = Request::builder()
            .method("POST")
            .uri("/_aibox/traffic/api/records/delete-all")
            .body(Body::empty())
            .unwrap();
        let response = service.clone().oneshot(delete_all).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"deleted": 0})
        );

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
        panic!("Traffic Record did not reach a terminal state");
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
