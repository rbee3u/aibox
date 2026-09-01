//! Raw HTTP message vocabulary: recording, hop-by-hop rules, and version names.
//!
//! Everything here reads a message without deciding anything about the Request
//! it belongs to, so both the request and the response side can share it.

use crate::request::model::RecordedHeader;
use axum::http::{HeaderMap, Version, header};
use base64::Engine as _;
use std::collections::HashSet;

pub(super) fn forwarded_headers(headers: &HeaderMap) -> HeaderMap {
    let mut remove: HashSet<String> = [
        "host",
        "connection",
        "proxy-connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .map(str::to_string)
    .into_iter()
    .collect();
    remove.extend(connection_named_headers(headers));
    let mut forwarded = HeaderMap::new();
    for (name, value) in headers {
        if !remove.contains(name.as_str()) {
            forwarded.append(name.clone(), value.clone());
        }
    }
    forwarded
}

pub(super) fn recorded_headers(headers: &HeaderMap) -> Vec<RecordedHeader> {
    let connection_named = connection_named_headers(headers);
    headers
        .iter()
        .filter(|(name, _)| {
            !is_hop_by_hop(name.as_str()) && !connection_named.contains(name.as_str())
        })
        .map(|(name, value)| RecordedHeader {
            name: name.as_str().to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode(value.as_bytes()),
        })
        .collect()
}

pub(super) fn connection_named_headers(headers: &HeaderMap) -> HashSet<String> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
        .filter_map(|token| {
            axum::http::HeaderName::from_bytes(trim_http_ows(token))
                .ok()
                .map(|name| name.as_str().to_string())
        })
        .collect()
}

pub(super) fn trim_http_ows(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
    {
        value = &value[1..];
    }
    while value
        .last()
        .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
    {
        value = &value[..value.len() - 1];
    }
    value
}

pub(super) fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

pub(super) fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        || headers.get_all(header::CONNECTION).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
        })
}

pub(super) fn declared_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

pub(super) fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::CONTENT_TYPE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .any(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub(super) fn version_name(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/unknown",
    }
}
