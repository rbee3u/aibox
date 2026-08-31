use super::super::api_error;
use crate::request::{BodyContentCoding, RequestInspection, RequestProxyState, body_reader};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use bytes::Bytes;
use serde::Deserialize;
use std::io::Read as _;
use tokio::io::AsyncReadExt as _;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct BodyQuery {
    #[serde(default)]
    pub(super) offset: u64,
}

pub(crate) async fn request_body(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(state.inspection(), &id, false, query.offset).await
}

pub(crate) async fn response_body(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(state.inspection(), &id, true, query.offset).await
}

pub(crate) async fn decoded_request_body(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(state.inspection(), &id, false).await
}

pub(crate) async fn decoded_response_body(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(state.inspection(), &id, true).await
}

pub(super) async fn body_response(
    inspection: RequestInspection,
    id: &str,
    response: bool,
    offset: u64,
) -> Response<Body> {
    let id = id.to_string();
    let opened =
        tokio::task::spawn_blocking(move || inspection.open_body(&id, response, offset)).await;
    let (file, length) = match opened {
        Ok(Ok(value)) => value,
        Ok(Err(error)) if error.to_string().contains("exceeds current length") => {
            return api_error(StatusCode::RANGE_NOT_SATISFIABLE, &error.to_string());
        }
        Ok(Err(error)) => return api_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("open Request body: {error}"),
            );
        }
    };
    let remaining = length - offset;
    let file = tokio::fs::File::from_std(file).take(remaining);
    let stream = ReaderStream::new(file);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&remaining.to_string()).expect("body length is a valid header"),
    );
    response.headers_mut().insert(
        "x-aibox-request-next-offset",
        HeaderValue::from_str(&length.to_string()).expect("body offset is a valid header"),
    );
    response
}

pub(super) async fn decoded_body_response(
    inspection: RequestInspection,
    id: &str,
    response: bool,
) -> Response<Body> {
    let lookup = inspection.clone();
    let lookup_id = id.to_string();
    let request = match tokio::task::spawn_blocking(move || lookup.find(&lookup_id)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => return api_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("read Request for body decoding: {error}"),
            );
        }
    };
    let completed = if response {
        request
            .summary
            .timing
            .upstream_response_body_completed_at_ns
            .is_some()
    } else {
        request
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .is_some()
    };
    if request.active && !completed {
        return api_error(
            StatusCode::CONFLICT,
            if response {
                "the response body is still being recorded"
            } else {
                "the request body is still being recorded"
            },
        );
    }
    let headers = if response {
        request
            .response
            .as_ref()
            .map(|metadata| metadata.headers.as_slice())
            .unwrap_or_default()
    } else {
        &request.request.headers
    };
    let coding = match inspection.body_content_coding(headers) {
        Ok(coding) => coding,
        Err(error) => return api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, &error.to_string()),
    };
    let opened =
        tokio::task::spawn_blocking(move || inspection.open_request_body(&request, response, 0))
            .await;
    let (file, length) = match opened {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return api_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("open Request body for decoding: {error}"),
            );
        }
    };
    let (body, length) = match coding {
        BodyContentCoding::Identity => {
            let file = tokio::fs::File::from_std(file).take(length);
            (Body::from_stream(ReaderStream::new(file)), Some(length))
        }
        BodyContentCoding::Zstd | BodyContentCoding::Gzip => (encoded_body(file, coding), None),
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Some(length) = length {
        response.headers_mut().insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).expect("body length is a valid header"),
        );
    }
    response
}

fn encoded_body(file: std::fs::File, coding: BodyContentCoding) -> Body {
    const CHUNK_SIZE: usize = 64 * 1024;
    const CHANNEL_CAPACITY: usize = 4;
    let (sender, receiver) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);
    tokio::task::spawn_blocking(move || {
        let mut decoder = match body_reader(file, coding) {
            Ok(decoder) => decoder,
            Err(error) => {
                let _ = sender.blocking_send(Err(std::io::Error::other(error.to_string())));
                return;
            }
        };
        let mut buffer = vec![0u8; CHUNK_SIZE];
        loop {
            match decoder.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => {
                    if sender
                        .blocking_send(Ok(Bytes::copy_from_slice(&buffer[..read])))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    Body::from_stream(ReceiverStream::new(receiver))
}
