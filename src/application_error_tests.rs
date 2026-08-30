use super::*;
use anyhow::Context as _;

#[test]
fn classification_survives_anyhow_context() {
    let error = Err::<(), _>(application_error(
        ApplicationErrorKind::Conflict,
        "stale edit",
    ))
    .context("save Config")
    .unwrap_err();
    assert_eq!(
        ApplicationError::kind(&error),
        Some(ApplicationErrorKind::Conflict)
    );
    assert_eq!(format!("{error:#}"), "save Config: stale edit");
}
