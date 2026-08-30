use super::*;
use crate::request::store::{Outcome, StoredRequest};
use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, StatusCode, header};
use axum::routing::{get, post};
use bytes::Bytes;
use http_body_util::BodyExt as _;
use std::time::Duration;

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
    RequestProxyState,
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
    let state = RequestProxyState::for_test(root).unwrap();
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

async fn wait_for_terminal(state: &RequestProxyState) -> StoredRequest {
    for _ in 0..100 {
        let requests = state.inspection().store().scan().unwrap();
        if let Some(request) = requests.into_iter().next()
            && request.result.is_some()
        {
            return request;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("Request did not reach a terminal state");
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

    let request = wait_for_terminal(&state).await;
    assert_eq!(
        request.request.upstream_url.as_deref(),
        Some(target.as_str())
    );
    assert_eq!(
        request
            .request
            .headers
            .iter()
            .filter(|header| header.name == "x-client-repeat")
            .count(),
        2
    );
    assert_eq!(
        std::fs::read(request.directory.join("request.body")).unwrap(),
        raw
    );
    assert_eq!(
        std::fs::read(request.directory.join("response.body")).unwrap(),
        raw
    );
    assert_eq!(request.result.unwrap().outcome, Outcome::Completed);

    let redirect_url = format!("http://{proxy_address}/http://{upstream}/v1/redirect");
    assert_eq!(
        client.get(redirect_url).send().await.unwrap().status(),
        StatusCode::FOUND
    );
    state.begin_shutdown();
    state.wait_for_response_tasks().await;
    upstream_task.abort();
    proxy_task.abort();
}
