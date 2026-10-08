use super::*;

#[test]
fn delete_validation_preserves_protection_and_confirmation_order() {
    let protected = DeleteTenantsCommand {
        names: vec![tenant::DEFAULT_TENANT_NAME.to_string()],
        all: false,
        confirmation: "wrong".to_string(),
    };
    let error = validate_delete_command(&protected).unwrap_err();
    assert_eq!(
        crate::application_error::ApplicationError::kind(&error),
        Some(ApplicationErrorKind::Conflict)
    );

    let mismatch = DeleteTenantsCommand {
        names: vec!["work".to_string()],
        all: false,
        confirmation: "wrong".to_string(),
    };
    let error = validate_delete_command(&mismatch).unwrap_err();
    assert_eq!(
        crate::application_error::ApplicationError::kind(&error),
        Some(ApplicationErrorKind::InvalidInput)
    );
    assert_eq!(error.to_string(), "confirmation does not match Tenant name");
}
