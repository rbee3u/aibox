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
