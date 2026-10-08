//! Socket-free Service and Control test support.
use super::state::ServiceState;
use crate::request::RequestProxyState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::http::{Method, Response, header};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt as _;
use serde_json::Value;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub(super) const VERSIONED_COMPONENT_COUNT: usize =
    crate::component::ComponentKind::ALL.len() - crate::component::ComponentKind::STATUSLINES.len();

pub(super) fn test_state(root: &Path) -> ServiceState {
    let host_home = root.join("host-home");
    fs::create_dir(&host_home).unwrap();
    let shutdown = CancellationToken::new();
    let request = RequestProxyState::new_with_reporter(root, shutdown.clone(), None).unwrap();
    let management = crate::management::Management::new(
        Arc::new(root.to_path_buf()),
        Arc::new(host_home),
        Arc::new("aibox:test".to_string()),
        request.inspection(),
        Arc::new(crate::testutil::FixtureLatestProvider::empty()),
    );
    ServiceState::new(
        "127.0.0.1:9923".parse().unwrap(),
        "test-csrf".to_string(),
        shutdown,
        request,
        management,
    )
}

pub(super) fn request(method: Method, path: &str, peer: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:9923")
        .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()))
        .body(Body::empty())
        .unwrap()
}

pub(super) fn json_request(path: &str, body: impl Into<Body>) -> Request<Body> {
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

pub(super) async fn response_json(response: Response<Body>) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[derive(Clone, Default)]
pub(super) struct PendingLatestControl {
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) started: Arc<Notify>,
    pub(super) release: Arc<Notify>,
}

impl PendingLatestControl {
    pub(super) async fn wait_until_all_started(&self) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.calls.load(Ordering::SeqCst) < VERSIONED_COMPONENT_COUNT {
                self.started.notified().await;
            }
        })
        .await
        .expect("all Latest Release fixture requests should start");
    }

    pub(super) fn release(&self) {
        self.release.notify_waiters();
    }
}

pub(super) struct PendingLatestProvider {
    pub(super) control: PendingLatestControl,
}

impl crate::component::LatestProvider for PendingLatestProvider {
    fn fetch(
        &self,
        kind: crate::component::ComponentKind,
    ) -> BoxFuture<'static, crate::component::LatestResult> {
        let control = self.control.clone();
        Box::pin(async move {
            control.calls.fetch_add(1, Ordering::SeqCst);
            control.started.notify_one();
            control.release.notified().await;
            crate::component::LatestResult::Unavailable {
                source: kind.name(),
                error: "fixture unavailable".to_string(),
            }
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum FilesystemEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
    Other,
}

pub(super) fn filesystem_snapshot(root: &Path) -> Vec<(PathBuf, FilesystemEntry)> {
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
