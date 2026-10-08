//! Named Config catalog, Current Config access, one-shot Config Application,
//! and the entry points for global Codex Credential Propagation.

mod application;
mod auth;
mod catalog;
mod comparison;
mod definition;
mod editing;
mod metadata;
mod model;
mod native;
mod storage;
mod visual;

pub(crate) use application::{application_status, apply_named_config};
#[cfg(test)]
pub(crate) use auth::PropagationOutcome;
pub(crate) use auth::{
    AuthPropagationPlan, AuthPropagationPreview, AuthPropagationReport,
    credential_propagation_source_available, execute_auth_propagation, plan_auth_propagation_from,
    preview_auth_propagation,
};
#[cfg(test)]
pub(crate) use catalog::ConfigCatalogState;
pub(crate) use catalog::{
    ConfigCatalogEntry, CurrentConfigInspection, create_named_config, delete_named_configs,
    inspect_current_config, inspect_named_configs,
};
pub(crate) use comparison::{ConfigComparison, ConfigComparisonDraft, compare_configs};
pub(crate) use editing::{
    config_file_warnings, diagnose_config_file, inspect_named_codex_auth, read_config_file_target,
    save_config_file_target, visual_config_state,
};
pub(crate) use visual::{
    CodexAuthInspection, CustomProviderInput, CustomProviderState, VisualAuthInput,
    VisualConfigOptionInput, VisualConfigOptionState, VisualConfigState,
};

pub(crate) use model::{
    ApplicationStatus, ConfigDiagnostic, ConfigDrift, ConfigEdit, ConfigFile, ConfigFileSnapshot,
    ConfigSaveResult, ConfigTarget, LastApplication, NamedConfigName,
};

// Bound every untrusted native Config file before allocating it all.
use crate::foundation::MAX_NATIVE_CONFIG_BYTES as MAX_CONFIG_BYTES;
const LAST_APPLICATION_SECTION: &str = "last_application";

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
