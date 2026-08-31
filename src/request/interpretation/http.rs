//! HTTP body coding, URL family detection, and recorded header decoding.

use super::wire::RequestEnvelope;
use crate::request::model::{ProtocolFamily, RecordedHeader};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BodyContentCoding {
    Identity,
    Zstd,
    Gzip,
}

impl BodyContentCoding {
    pub(crate) fn is_encoded(self) -> bool {
        !matches!(self, Self::Identity)
    }
}

pub(super) fn parse_request(path: &Path, headers: &[RecordedHeader]) -> Result<RequestEnvelope> {
    let file = crate::foundation::safe_fs::open_real_file(path, "Incoming HTTP Request body")?;
    let coding = body_content_coding(headers)?;
    let mut reader = body_reader(file, coding)?;
    serde_json::from_reader(&mut reader).context("parse request JSON")
}

pub(crate) fn body_reader(file: File, coding: BodyContentCoding) -> Result<Box<dyn Read + Send>> {
    match coding {
        BodyContentCoding::Identity => Ok(Box::new(file)),
        BodyContentCoding::Zstd => {
            let decoder =
                zstd::stream::read::Decoder::new(file).context("create zstd body decoder")?;
            Ok(Box::new(decoder))
        }
        BodyContentCoding::Gzip => Ok(Box::new(flate2::read::GzDecoder::new(file))),
    }
}

pub(crate) fn body_content_coding(headers: &[RecordedHeader]) -> Result<BodyContentCoding> {
    let mut codings = Vec::new();
    for header in headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-encoding"))
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&header.value_base64)
            .context("decode Content-Encoding header")?;
        let value = std::str::from_utf8(&bytes).context("Content-Encoding header is not UTF-8")?;
        codings.extend(
            value
                .split(',')
                .map(str::trim)
                .filter(|coding| !coding.is_empty())
                .map(str::to_ascii_lowercase),
        );
    }
    if codings.is_empty() {
        return Ok(BodyContentCoding::Identity);
    }
    match codings.as_slice() {
        [coding] if coding == "identity" => Ok(BodyContentCoding::Identity),
        [coding] if coding == "zstd" => Ok(BodyContentCoding::Zstd),
        [coding] if coding == "gzip" => Ok(BodyContentCoding::Gzip),
        _ => bail!("unsupported Content-Encoding {:?}", codings.join(", ")),
    }
}

pub(super) fn family_from_url(value: Option<&str>) -> ProtocolFamily {
    let Some(path) = value
        .and_then(|value| url::Url::parse(value).ok())
        .map(|url| url.path().trim_end_matches('/').to_string())
    else {
        return ProtocolFamily::Unknown;
    };
    if path.ends_with("/responses") {
        ProtocolFamily::OpenaiResponses
    } else if path.ends_with("/chat/completions") {
        ProtocolFamily::OpenaiChatCompletions
    } else if path.ends_with("/messages") {
        ProtocolFamily::ClaudeMessages
    } else {
        ProtocolFamily::Unknown
    }
}

pub(crate) fn coding_agent_session_id(
    upstream_url: Option<&str>,
    headers: &[RecordedHeader],
) -> Option<String> {
    let names = match family_from_url(upstream_url) {
        ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
            ["session-id", "x-claude-code-session-id"]
        }
        ProtocolFamily::ClaudeMessages => ["x-claude-code-session-id", "session-id"],
        ProtocolFamily::Unknown => return None,
    };
    names
        .into_iter()
        .find_map(|name| first_nonempty_header_text(headers, name))
}

fn first_nonempty_header_text(headers: &[RecordedHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .filter_map(|header| {
            base64::engine::general_purpose::STANDARD
                .decode(&header.value_base64)
                .ok()
        })
        .filter_map(|bytes| String::from_utf8(bytes).ok())
        .find_map(|value| nonempty(Some(value)))
}

pub(super) fn header_text(headers: &[RecordedHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .and_then(|header| {
            base64::engine::general_purpose::STANDARD
                .decode(&header.value_base64)
                .ok()
        })
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

pub(super) fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(super) fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}
