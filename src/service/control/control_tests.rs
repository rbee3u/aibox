use super::*;

#[test]
fn application_error_categories_have_stable_http_statuses() {
    let cases = [
        (ApplicationErrorKind::InvalidInput, StatusCode::BAD_REQUEST),
        (ApplicationErrorKind::NotFound, StatusCode::NOT_FOUND),
        (ApplicationErrorKind::Conflict, StatusCode::CONFLICT),
        (
            ApplicationErrorKind::InputTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (ApplicationErrorKind::Busy, StatusCode::CONFLICT),
        (
            ApplicationErrorKind::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (kind, expected) in cases {
        assert_eq!(status_for_application_error(kind), expected, "{kind:?}");
    }
}

#[test]
fn unclassified_domain_errors_remain_bad_requests() {
    let response = result_error(anyhow::anyhow!("invalid selector"));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Every route shares one error envelope.
///
/// Two shapes existed before: `{"error":{"code":N,"message":M}}` on most routes
/// and `{"error":M}` on the Requests routes. The Console decodes only a string
/// `error`, so every message from the other shape was silently replaced by the
/// bare HTTP status line. Nothing asserted the shape on either side, which is
/// exactly why the split survived.
#[tokio::test]
async fn every_control_error_carries_the_message_as_a_plain_string() {
    use http_body_util::BodyExt as _;

    for (label, status, message) in [
        ("domain error", StatusCode::CONFLICT, "tenant is protected"),
        (
            "request read error",
            StatusCode::NOT_FOUND,
            "no such request",
        ),
    ] {
        let response = api_error(status, message);
        assert_eq!(response.status(), status, "{label}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, serde_json::json!({"error": message}), "{label}");
        assert_eq!(
            body["error"].as_str(),
            Some(message),
            "{label}: the Console reads `error` as a string"
        );
    }
}
