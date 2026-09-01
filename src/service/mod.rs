//! Foreground AIBox Service, protected management routes, and Request fallback.

mod control;
mod coordination;
mod operation;
mod state;

use crate::component::{LatestProvider, OfficialLatestProvider};
use crate::request::{
    REQUEST_GROUP_COMPACT_INTERVAL, RequestProxyState, RequestReporter, handle_proxy,
};
use crate::service::coordination::OperationCoordinator;
use crate::service::state::{ConsoleCspNonce, ServiceState};
use crate::tenant;
use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use fs2::FileExt as _;
use socket2::{Domain, Protocol, Socket, Type};
use std::fs;
use std::future::Future;
use std::future::IntoFuture as _;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug)]
struct ServiceLock {
    _file: fs::File,
}

#[derive(Clone, Copy)]
enum ShutdownReason {
    Interrupt,
    Terminate,
}

pub(crate) struct ConsoleCommand {
    pub(crate) listen: SocketAddr,
}

pub(crate) fn dispatch(command: ConsoleCommand) -> Result<i32> {
    if command.listen.port() == 0 {
        bail!("AIBox Service listener port must not be 0");
    }
    let root = tenant::aibox_root()?;
    let host_home = tenant::host_home()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("aibox-service")
        .build()
        .context("create AIBox Service async runtime")?;
    runtime.block_on(serve(
        command.listen,
        root,
        host_home,
        crate::docker::IMAGE.to_string(),
    ))
}

async fn serve(
    listen: SocketAddr,
    root: PathBuf,
    host_home: PathBuf,
    image: String,
) -> Result<i32> {
    let _lock = acquire_service_lock(&root)?;
    ensure_default_managed_tenant(&root)?;
    let shutdown = CancellationToken::new();
    let request = RequestProxyState::new_with_reporter(
        &root,
        shutdown.clone(),
        Some(RequestReporter::new()),
    )?;
    let latest_provider: Arc<dyn LatestProvider> = Arc::new(OfficialLatestProvider::new()?);
    let state = ServiceState::new(
        root,
        host_home,
        image,
        listen,
        Uuid::new_v4().to_string(),
        request,
        latest_provider,
    );
    let listener =
        bind_listener(listen).with_context(|| format!("bind AIBox Service listener {listen}"))?;
    let router = router(state.clone());
    println!("{}", startup_banner(listen));

    let compact_shutdown = shutdown.clone();
    let compact_state = state.request();
    tokio::spawn(request_group_compact_loop(compact_state, compact_shutdown));

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
            result.context("serve AIBox Service")?;
            None
        }
        reason = signal_rx.recv() => reason,
    };
    let Some(reason) = reason else {
        return Ok(0);
    };
    let result = coordinate_shutdown(reason, &state, &mut signal_rx, server.as_mut()).await;
    signal_task.abort();
    result
}

async fn coordinate_shutdown<F>(
    reason: ShutdownReason,
    state: &ServiceState,
    signal_rx: &mut mpsc::UnboundedReceiver<ShutdownReason>,
    mut server: Pin<&mut F>,
) -> Result<i32>
where
    F: Future<Output = io::Result<()>>,
{
    let shutdown = state.request().shutdown_token();
    let operations = OperationCoordinator::new(state.clone());
    shutdown.cancel();
    operations.cancel_current();

    let forced = tokio::select! {
        result = server.as_mut() => {
            result.context("shut down AIBox Service listener")?;
            false
        }
        second = signal_rx.recv() => second.is_some(),
    };
    if forced {
        return Ok(signal_exit(reason));
    }
    state.request().begin_shutdown();
    let wait_cleanup = async {
        state.request().wait_for_response_tasks().await;
        operations.wait_until_idle().await;
    };
    let forced = tokio::select! {
        _ = wait_cleanup => false,
        second = signal_rx.recv() => second.is_some(),
    };
    if forced {
        Ok(signal_exit(reason))
    } else {
        Ok(match reason {
            ShutdownReason::Interrupt => 0,
            ShutdownReason::Terminate => 143,
        })
    }
}

fn ensure_default_managed_tenant(root: &Path) -> Result<()> {
    tenant::ManagedTenant::resolve(root, tenant::DEFAULT_TENANT_NAME)?
        .ensure_initialized()
        .context("create or repair Default Managed Tenant")
}

/// Wait one compact interval, then compact at most one Request Group, until shutdown.
async fn request_group_compact_loop(state: RequestProxyState, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = tokio::time::sleep(REQUEST_GROUP_COMPACT_INTERVAL) => {}
        }
        if shutdown.is_cancelled() {
            break;
        }
        let state = state.clone();
        let _ = tokio::task::spawn_blocking(move || state.compact_once()).await;
    }
}

fn router(state: ServiceState) -> Router {
    let protected = Router::new()
        .route("/", get(root_redirect))
        .merge(control::router())
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

async fn root_redirect() -> Redirect {
    Redirect::temporary("/_aibox/ui/overview")
}

async fn management_not_found() -> Response<Body> {
    plain_error(StatusCode::NOT_FOUND, "AIBox management route not found")
}

async fn proxy_fallback(State(state): State<ServiceState>, request: Request) -> Response<Body> {
    handle_proxy(state.request(), request).await
}

async fn management_guard(
    State(state): State<ServiceState>,
    mut request: Request,
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
        if csrf != Some(state.csrf_token()) {
            return plain_error(StatusCode::FORBIDDEN, "invalid CSRF token");
        }
    }
    let csp_nonce = ConsoleCspNonce::new();
    request.extensions_mut().insert(csp_nonce.clone());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'nonce-{}'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
            csp_nonce.as_str()
        ))
        .expect("generated CSP nonce produces a valid header"),
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
    crate::foundation::safe_fs::ensure_real_dir(root, "AIBox Root")?;
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
        .with_context(|| format!("open AIBox Service lock {}", path.display()))?;
    if !file.metadata()?.file_type().is_file() {
        bail!(
            "AIBox Service lock is not a regular file: {}",
            path.display()
        );
    }
    file.try_lock_exclusive().with_context(|| {
        format!(
            "another AIBox Service already manages Root {}",
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
#[path = "service_tests.rs"]
mod tests;
