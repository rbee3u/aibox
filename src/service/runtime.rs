//! Foreground Service assembly, startup preparation, and supervised shutdown.

use super::router;
use super::state::ServiceState;
use crate::component::{LatestProvider, OfficialLatestProvider};
use crate::request::{REQUEST_GROUP_COMPACT_INTERVAL, RequestProxyState, RequestReporter};
use crate::tenant;
use anyhow::{Context, Result, bail};
use socket2::{Domain, Protocol, Socket, Type};
use std::fs;
use std::future::Future;
use std::future::IntoFuture as _;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug)]
struct ServiceLock {
    _file: fs::File,
}

/// Service-owned best-effort workers. Request response tasks and Management
/// Operations retain their own drain policies in `coordinate_shutdown`.
struct ServiceTasks {
    shutdown: CancellationToken,
    component_updates: tokio::task::JoinHandle<()>,
    request_compaction: tokio::task::JoinHandle<()>,
    signals: tokio::task::JoinHandle<()>,
}

impl Drop for ServiceTasks {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.component_updates.abort();
        self.request_compaction.abort();
        self.signals.abort();
        // Aborting an async worker does not interrupt an already-running
        // spawn_blocking compaction. Runtime teardown keeps its existing policy.
    }
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
    crate::foundation::platform::raise_nofile_limit();
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
    let management = crate::management::Management::new(
        Arc::new(root),
        Arc::new(host_home),
        Arc::new(image),
        request.inspection(),
        latest_provider,
    );
    let state = ServiceState::new(
        listen,
        Uuid::new_v4().to_string(),
        shutdown.clone(),
        request,
        management,
    );
    let listener =
        bind_listener(listen).with_context(|| format!("bind AIBox Service listener {listen}"))?;
    let router = router(state.clone());
    println!("{}", startup_banner(listen));

    let component_update_shutdown = shutdown.clone();
    let component_update_state = state.clone();
    let component_updates = tokio::spawn(prefetch_component_updates(
        component_update_state,
        component_update_shutdown,
    ));

    let compact_shutdown = shutdown.clone();
    let compact_state = state.request();
    let request_compaction =
        tokio::spawn(request_group_compact_loop(compact_state, compact_shutdown));

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel();
    let _tasks = ServiceTasks {
        shutdown: shutdown.clone(),
        component_updates,
        request_compaction,
        signals: tokio::spawn(signal_loop(signal_tx)),
    };
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
            result.context("serve AIBox Service")?;
            None
        }
        reason = signal_rx.recv() => reason,
    };
    let Some(reason) = reason else {
        return Ok(0);
    };
    coordinate_shutdown(reason, &state, &mut signal_rx, server.as_mut()).await
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
    let shutdown = state.shutdown_token();
    let operations = state.management.operations.clone();
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

async fn prefetch_component_updates(state: ServiceState, shutdown: CancellationToken) {
    tokio::select! {
        biased;
        () = shutdown.cancelled() => {}
        () = state.management.components.prefetch() => {}
    }
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
    file.try_lock().with_context(|| {
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
#[path = "runtime_tests.rs"]
mod tests;
