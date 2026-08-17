//! Foreground aibox Service, protected management routes, and Request fallback.

use crate::cli::ServeArgs;
use crate::operation::OperationManager;
use crate::request::AppState as RequestState;
use crate::{config, control_web, request_proxy, request_web, tenant};
use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, FromRef, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use fs2::FileExt as _;
use socket2::{Domain, Protocol, Socket, Type};
use std::fs;
use std::future::IntoFuture as _;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct ServiceState {
    pub(crate) root: Arc<PathBuf>,
    pub(crate) host_home: Arc<PathBuf>,
    pub(crate) image: Arc<String>,
    pub(crate) listen: SocketAddr,
    pub(crate) started: Instant,
    pub(crate) csrf: Arc<String>,
    pub(crate) request: RequestState,
    pub(crate) operations: OperationManager,
    pub(crate) mutation: Arc<Mutex<()>>,
    pub(crate) auth_propagation: Arc<std::sync::Mutex<Option<PendingAuthPropagation>>>,
}

pub(crate) struct PendingAuthPropagation {
    pub(crate) id: String,
    pub(crate) plan: config::AuthPropagationPlan,
}

impl FromRef<ServiceState> for RequestState {
    fn from_ref(state: &ServiceState) -> Self {
        state.request.clone()
    }
}

#[derive(Debug)]
struct ServiceLock {
    _file: fs::File,
}

#[derive(Clone, Copy)]
enum ShutdownReason {
    Interrupt,
    Terminate,
}

pub(crate) fn dispatch(args: &ServeArgs) -> Result<i32> {
    if args.listen.port() == 0 {
        bail!("aibox Service listener port must not be 0");
    }
    let root = tenant::aibox_root()?;
    let host_home = tenant::host_home()?;
    let image_override = crate::env_override("AIBOX_IMAGE")?;
    let image = crate::image_for(image_override.as_deref())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("aibox-service")
        .build()
        .context("create aibox Service async runtime")?;
    runtime.block_on(serve(args.listen, root, host_home, image))
}

async fn serve(
    listen: SocketAddr,
    root: PathBuf,
    host_home: PathBuf,
    image: String,
) -> Result<i32> {
    let _lock = acquire_service_lock(&root)?;
    let shutdown = CancellationToken::new();
    let request = RequestState::new_with_console(
        &root,
        shutdown.clone(),
        Some(crate::request_console::RequestConsole::new()),
    )?;
    let state = ServiceState {
        root: Arc::new(root),
        host_home: Arc::new(host_home),
        image: Arc::new(image),
        listen,
        started: Instant::now(),
        csrf: Arc::new(Uuid::new_v4().to_string()),
        request,
        operations: OperationManager::new(),
        mutation: Arc::new(Mutex::new(())),
        auth_propagation: Arc::new(std::sync::Mutex::new(None)),
    };
    let listener =
        bind_listener(listen).with_context(|| format!("bind aibox Service listener {listen}"))?;
    let router = router(state.clone());
    println!("{}", startup_banner(listen));

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    let signal_task = tokio::spawn(signal_loop(signal_tx));
    let server_shutdown = shutdown.clone();
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(server_shutdown.cancelled_owned())
    .into_future();
    tokio::pin!(server);

    let reason = tokio::select! {
        result = &mut server => {
            signal_task.abort();
            result.context("serve aibox Service")?;
            None
        }
        reason = signal_rx.recv() => reason,
    };
    let Some(reason) = reason else {
        return Ok(0);
    };
    shutdown.cancel();
    state.operations.cancel_current();

    let forced = tokio::select! {
        result = &mut server => {
            result.context("shut down aibox Service listener")?;
            false
        }
        second = signal_rx.recv() => second.is_some(),
    };
    if forced {
        signal_task.abort();
        return Ok(signal_exit(reason));
    }
    state.request.response_tasks.close();
    let wait_cleanup = async {
        state.request.response_tasks.wait().await;
        while state.operations.is_running() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let forced = tokio::select! {
        _ = wait_cleanup => false,
        second = signal_rx.recv() => second.is_some(),
    };
    signal_task.abort();
    if forced {
        Ok(signal_exit(reason))
    } else {
        Ok(match reason {
            ShutdownReason::Interrupt => 0,
            ShutdownReason::Terminate => 143,
        })
    }
}

fn router(state: ServiceState) -> Router {
    let protected = Router::new()
        .route("/", get(root_redirect))
        .merge(control_web::router())
        .merge(request_viewer_routes())
        .route(
            "/_aibox",
            get(management_not_found).post(management_not_found),
        )
        .route(
            "/_aibox/{*path}",
            get(management_not_found).post(management_not_found),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            management_guard,
        ));
    Router::new()
        .merge(protected)
        .fallback(proxy_fallback)
        .with_state(state)
}

fn request_viewer_routes() -> Router<ServiceState> {
    Router::new()
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
            get(request_web::record_detail).post(management_not_found),
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
        .route("/_aibox/requests/{*path}", get(request_web::not_found))
}

async fn root_redirect() -> Redirect {
    Redirect::temporary("/_aibox/ui/overview")
}

async fn management_not_found() -> Response<Body> {
    plain_error(StatusCode::NOT_FOUND, "aibox management route not found")
}

async fn proxy_fallback(State(state): State<ServiceState>, request: Request) -> Response<Body> {
    request_proxy::handle(state.request, request).await
}

async fn management_guard(
    State(state): State<ServiceState>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0);
    if !peer.is_some_and(|peer| peer.ip().is_loopback()) {
        return plain_error(StatusCode::FORBIDDEN, "management access requires loopback");
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !host.is_some_and(loopback_host) {
        return plain_error(
            StatusCode::FORBIDDEN,
            "management Host must resolve to loopback",
        );
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        let content_type = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| value.starts_with("application/json")) {
            return plain_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "expected JSON request");
        }
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok());
        let expected_origin = format!("http://{}", host.expect("host checked above"));
        if origin != Some(expected_origin.as_str()) {
            return plain_error(StatusCode::FORBIDDEN, "request Origin is not same-origin");
        }
        let csrf = request
            .headers()
            .get("x-aibox-csrf")
            .and_then(|value| value.to_str().ok());
        if csrf != Some(state.csrf.as_str()) {
            return plain_error(StatusCode::FORBIDDEN, "invalid CSRF token");
        }
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn loopback_host(value: &str) -> bool {
    let host = value
        .parse::<axum::http::uri::Authority>()
        .ok()
        .map(|authority| {
            authority
                .host()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });
    let Some(host) = host else { return false };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn plain_error(status: StatusCode, message: &str) -> Response<Body> {
    (status, message.to_string()).into_response()
}

fn acquire_service_lock(root: &Path) -> Result<ServiceLock> {
    tenant::ensure_real_dir(root, "aibox root")?;
    let path = root.join(".service.lock");
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(&path)
        .with_context(|| format!("open aibox Service lock {}", path.display()))?;
    if !file.metadata()?.file_type().is_file() {
        bail!(
            "aibox Service lock is not a regular file: {}",
            path.display()
        );
    }
    file.try_lock_exclusive().with_context(|| {
        format!(
            "another aibox Service already manages Root {}",
            root.display()
        )
    })?;
    Ok(ServiceLock { _file: file })
}

fn bind_listener(address: SocketAddr) -> io::Result<TcpListener> {
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

fn console_url(listen: SocketAddr) -> String {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!(
        "http://{}/_aibox/ui/overview",
        SocketAddr::new(ip, listen.port())
    )
}

fn startup_banner(listen: SocketAddr) -> String {
    format!("Listening on {listen} · Console {}", console_url(listen))
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
            return;
        }
    }
    loop {
        if tokio::signal::ctrl_c().await.is_err() || sender.send(ShutdownReason::Interrupt).is_err()
        {
            return;
        }
    }
}

fn signal_exit(reason: ShutdownReason) -> i32 {
    match reason {
        ShutdownReason::Interrupt => 130,
        ShutdownReason::Terminate => 143,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt as _;
    use serde_json::Value;
    use tower::ServiceExt as _;

    fn test_state(root: &Path) -> ServiceState {
        let host_home = root.join("host-home");
        fs::create_dir(&host_home).unwrap();
        let shutdown = CancellationToken::new();
        ServiceState {
            root: Arc::new(root.to_path_buf()),
            host_home: Arc::new(host_home),
            image: Arc::new("aibox:test".to_string()),
            listen: "127.0.0.1:9923".parse().unwrap(),
            started: Instant::now(),
            csrf: Arc::new("test-csrf".to_string()),
            request: RequestState::new_with_console(root, shutdown, None).unwrap(),
            operations: OperationManager::new(),
            mutation: Arc::new(Mutex::new(())),
            auth_propagation: Arc::new(std::sync::Mutex::new(None)),
        }
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

    #[test]
    fn service_lock_is_exclusive_per_root() {
        let root = tempfile::tempdir().unwrap();
        let first = acquire_service_lock(root.path()).unwrap();
        let error = acquire_service_lock(root.path()).unwrap_err().to_string();
        assert!(error.contains("another aibox Service"), "{error}");
        drop(first);
        acquire_service_lock(root.path()).unwrap();
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

        let removed_delete_all = Request::builder()
            .method(Method::POST)
            .uri("/_aibox/requests/api/records/delete-all")
            .header(header::HOST, "127.0.0.1:9923")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:9923")
            .header("x-aibox-csrf", "test-csrf")
            .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()))
            .body(Body::from("{}"))
            .unwrap();
        let removed = app.clone().oneshot(removed_delete_all).await.unwrap();
        assert_eq!(removed.status(), StatusCode::NOT_FOUND);

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
    }

    #[tokio::test]
    async fn operation_events_stream_ends_when_service_shuts_down() {
        let root = tempfile::tempdir().unwrap();
        let state = test_state(root.path());
        let shutdown = state.request.shutdown.clone();
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
            &format!(
                "tenants/work/.codex/sessions/2026/08/17/rollout-selected-{selected_id}.jsonl"
            ),
            &[r#"{"timestamp":"2026-08-17T10:00:00Z","type":"session_meta"}"#],
        );
        let same_scope_unselected = crate::testutil::write_jsonl(
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
            "scope": "managed",
            "tenant": "work",
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
        assert!(same_scope_unselected.exists());
        assert!(other_tenant.exists());
        assert!(other_agent.exists());
    }
}
