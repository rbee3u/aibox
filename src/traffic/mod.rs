mod proxy;
mod store;
mod web;

use crate::cli::TrafficArgs;
use anyhow::{bail, Context, Result};
use axum::extract::State;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use socket2::{Domain, Protocol, Socket, Type};
use std::future::IntoFuture as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[derive(Clone)]
struct AppState {
    store: store::TrafficStore,
    csrf: String,
    port: u16,
    shutdown: CancellationToken,
    response_tasks: TaskTracker,
    allow_private_upstream: bool,
}

impl AppState {
    fn new(root: &Path, port: u16, shutdown: CancellationToken) -> Result<Self> {
        Ok(Self {
            store: store::TrafficStore::open(root)?,
            csrf: uuid::Uuid::new_v4().to_string(),
            port,
            shutdown,
            response_tasks: TaskTracker::new(),
            allow_private_upstream: false,
        })
    }

    #[cfg(test)]
    fn for_test(root: &Path, port: u16) -> Result<Self> {
        let mut state = Self::new(root, port, CancellationToken::new())?;
        state.allow_private_upstream = true;
        Ok(state)
    }
}

/// Run the foreground host-side Traffic Proxy and management viewer.
pub(crate) fn dispatch(args: &TrafficArgs) -> Result<i32> {
    validate_listener_scope(args)?;
    let root = crate::tenant::aibox_root()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("aibox-traffic")
        .build()
        .context("create Traffic Proxy async runtime")?;
    runtime.block_on(serve(args.listen, &root))?;
    Ok(0)
}

fn validate_listener_scope(args: &TrafficArgs) -> Result<()> {
    if !args.listen.ip().is_loopback() && !args.allow_remote {
        bail!(
            "non-loopback Traffic listener {} requires --allow-remote",
            args.listen
        );
    }
    Ok(())
}

async fn serve(listen: SocketAddr, root: &Path) -> Result<()> {
    let shutdown = CancellationToken::new();
    let state = AppState::new(root, listen.port(), shutdown.clone())?;
    let listeners = bind_listeners(listen)?;
    let router = router(state.clone());

    eprintln!(">> Traffic Proxy listening on http://{listen}");
    eprintln!(
        ">> Traffic viewer: http://127.0.0.1:{}/ (raw records: {})",
        listen.port(),
        state.store.root_display()
    );

    let signal_shutdown = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        signal_shutdown.cancel();
    });

    let mut servers = JoinSet::new();
    for listener in listeners {
        let service = router
            .clone()
            .into_make_service_with_connect_info::<SocketAddr>();
        let listener_shutdown = shutdown.clone();
        servers.spawn(
            axum::serve(listener, service)
                .with_graceful_shutdown(listener_shutdown.cancelled_owned())
                .into_future(),
        );
    }
    let mut first_error = None;
    while let Some(result) = servers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                first_error.get_or_insert_with(|| anyhow::Error::new(error));
                shutdown.cancel();
            }
            Err(error) => {
                first_error.get_or_insert_with(|| anyhow::Error::new(error));
                shutdown.cancel();
            }
        }
    }
    signal_task.abort();
    await_response_tasks(&state).await;
    if let Some(error) = first_error {
        return Err(error).context("serve Traffic Proxy");
    }
    Ok(())
}

fn router(state: AppState) -> Router {
    let management = Router::new()
        .route("/", get(web::index))
        .route("/_aibox/traffic/app.css", get(web::css))
        .route("/_aibox/traffic/app.js", get(web::js))
        .route("/_aibox/traffic/api/records", get(web::list_records))
        .route(
            "/_aibox/traffic/api/records/delete",
            post(web::delete_records),
        )
        .route(
            "/_aibox/traffic/api/records/delete-all",
            post(web::delete_all),
        )
        .route("/_aibox/traffic/api/records/{id}", get(web::record_detail))
        .route(
            "/_aibox/traffic/api/records/{id}/request-body",
            get(web::request_body),
        )
        .route(
            "/_aibox/traffic/api/records/{id}/response-body",
            get(web::response_body),
        )
        .route("/_aibox/traffic/{*path}", get(web::not_found))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            web::security_middleware,
        ));
    Router::new()
        .merge(management)
        .fallback(proxy_fallback)
        .with_state(state)
}

async fn proxy_fallback(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    proxy::handle(state, request).await
}

fn bind_listeners(requested: SocketAddr) -> Result<Vec<TcpListener>> {
    let mut listeners =
        vec![bind_listener(requested)
            .with_context(|| format!("bind Traffic listener {requested}"))?];
    if needs_canonical_loopback(requested.ip()) {
        let canonical = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), requested.port());
        listeners.push(bind_listener(canonical).with_context(|| {
            format!("bind loopback Traffic management listener {canonical} alongside {requested}")
        })?);
    }
    Ok(listeners)
}

fn needs_canonical_loopback(address: IpAddr) -> bool {
    address != IpAddr::V4(Ipv4Addr::LOCALHOST) && address != IpAddr::V4(Ipv4Addr::UNSPECIFIED)
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

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{header, HeaderValue, Request, Response, StatusCode};
    use axum::routing::{get, post};
    use bytes::Bytes;
    use http_body_util::BodyExt as _;
    use std::time::Duration;
    use tower::ServiceExt as _;

    #[test]
    fn canonical_loopback_binding_is_added_only_when_needed() {
        assert!(!needs_canonical_loopback("127.0.0.1".parse().unwrap()));
        assert!(!needs_canonical_loopback("0.0.0.0".parse().unwrap()));
        assert!(needs_canonical_loopback("192.0.2.10".parse().unwrap()));
        assert!(needs_canonical_loopback("::".parse().unwrap()));
        assert!(needs_canonical_loopback("::1".parse().unwrap()));
    }

    #[test]
    fn non_loopback_listener_requires_explicit_remote_permission() {
        let denied = TrafficArgs {
            listen: "0.0.0.0:9923".parse().unwrap(),
            allow_remote: false,
        };
        assert!(validate_listener_scope(&denied).is_err());
        let allowed = TrafficArgs {
            allow_remote: true,
            ..denied
        };
        validate_listener_scope(&allowed).unwrap();
    }

    #[tokio::test]
    async fn embedded_ui_and_api_are_served_in_memory_with_management_guards() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path(), 9923).unwrap();
        let service = router(state.clone());
        let peer = ConnectInfo("127.0.0.1:40000".parse::<SocketAddr>().unwrap());

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
            let mut request = Request::builder()
                .uri(path)
                .header(header::HOST, "127.0.0.1:9923")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                content_type,
                "{path}"
            );
            assert_eq!(
                response.headers().get(header::CACHE_CONTROL).unwrap(),
                "no-store",
                "{path}"
            );
            assert!(response.headers().contains_key("content-security-policy"));
            assert_eq!(
                response.headers().get("x-content-type-options").unwrap(),
                "nosniff"
            );
            let body = response.into_body().collect().await.unwrap().to_bytes();
            if path == "/" {
                let html = String::from_utf8(body.to_vec()).unwrap();
                assert!(html.contains(&state.csrf));
                assert!(html.contains("/_aibox/traffic/app.css"));
                assert!(html.contains("/_aibox/traffic/app.js"));
            }
        }

        let mut remote_request = Request::builder()
            .uri("/")
            .header(header::HOST, "127.0.0.1:9923")
            .body(Body::empty())
            .unwrap();
        remote_request.extensions_mut().insert(ConnectInfo(
            "192.0.2.1:40000".parse::<SocketAddr>().unwrap(),
        ));
        let response = service.oneshot(remote_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
        let state = AppState::for_test(root, proxy_address.port()).unwrap();
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

    async fn wait_for_terminal(state: &AppState) -> store::StoredRecord {
        for _ in 0..100 {
            let records = state.store.scan().unwrap();
            if let Some(record) = records.into_iter().next() {
                if record.result.is_some() {
                    return record;
                }
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
        assert_eq!(record.result.unwrap().outcome, store::Outcome::Completed);

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
