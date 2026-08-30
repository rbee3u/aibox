//! Raw header recording and hop-by-hop forwarding rules.

use crate::request::model::RecordedHeader;
use axum::http::{HeaderMap, header};
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
