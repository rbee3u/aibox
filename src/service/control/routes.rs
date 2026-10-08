//! Single route inventory for Axum registration and the Console contract.

use super::{
    assets, components, configs, index, operations, overview, requests, sessions, tenants,
};
use crate::service::state::ServiceState;
use axum::Router;
use axum::routing::{MethodFilter, MethodRouter};

/// Generated Console route manifest entry.
#[cfg(test)]
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct EndpointDescription {
    pub(crate) key: &'static str,
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
}

/// Declare paths, methods and handlers once for both Axum and the Console manifest.
///
/// Multiple methods share one MethodRouter. Manifest order remains declaration
/// order so contract generation is deterministic. Clients and DTOs stay separate.
macro_rules! control_routes {
    ($($name:ident = $path:literal { $($method:ident => $key:literal => $handler:path),+ $(,)? })+) => {
        $(pub(crate) const $name: &str = $path;)+

        pub(super) fn router() -> Router<ServiceState> {
            Router::new()
                $(.route($name, MethodRouter::new()$(.on(MethodFilter::$method, $handler))+))+
        }

        #[cfg(test)]
        pub(crate) const ENDPOINTS: &[EndpointDescription] = &[
            $($(EndpointDescription {
                key: $key,
                method: stringify!($method),
                path: $name,
            },)+)+
        ];
    };
}

control_routes! {

    // Console assets
    UI = "/_aibox/ui" { GET => "ui" => index }
    UI_CSS = "/_aibox/ui/assets/style.css" { GET => "ui_css" => assets::css }
    UI_JS = "/_aibox/ui/assets/index.js" { GET => "ui_js" => assets::js }
    UI_ASSET = "/_aibox/ui/{*path}" { GET => "ui_asset" => index }

    // Overview
    BOOTSTRAP = "/_aibox/api/bootstrap" { GET => "bootstrap" => overview::bootstrap }
    OVERVIEW = "/_aibox/api/overview" { GET => "overview" => overview::overview }
    TOPOLOGY = "/_aibox/api/topology" { GET => "topology" => overview::topology }

    // Tenant lifecycle
    TENANTS = "/_aibox/api/tenants" { GET => "tenants_list" => tenants::list_tenants, POST => "tenants_create" => tenants::create_tenant }
    TENANTS_DELETE = "/_aibox/api/tenants/delete" { POST => "tenants_delete" => tenants::delete_tenants }

    // Tenant-local Components
    COMPONENTS = "/_aibox/api/components" { GET => "components_list" => components::list_components }
    COMPONENTS_LATEST = "/_aibox/api/components/latest" { GET => "components_latest" => components::latest_components }
    COMPONENTS_LATEST_CHECK = "/_aibox/api/components/latest/check" { POST => "components_latest_check" => components::check_latest_components }
    COMPONENTS_INSTALL = "/_aibox/api/components/install" { POST => "components_install" => components::install_component }
    COMPONENTS_REMOVE = "/_aibox/api/components/remove" { POST => "components_remove" => components::remove_component }

    // Configs
    CONFIGS = "/_aibox/api/configs" { GET => "configs_list" => configs::list_configs }
    CONFIGS_CREATE = "/_aibox/api/configs/create" { POST => "configs_create" => configs::create_config }
    CONFIGS_REVEAL = "/_aibox/api/configs/reveal" { POST => "configs_reveal" => configs::reveal_config_file }
    CONFIGS_SAVE = "/_aibox/api/configs/save" { POST => "configs_save" => configs::save_config_file }
    CONFIGS_COMPARE = "/_aibox/api/configs/compare" { POST => "configs_compare" => configs::compare_configs }
    CONFIGS_DIAGNOSE = "/_aibox/api/configs/diagnose" { POST => "configs_diagnose" => configs::diagnose_config_file }
    CONFIGS_APPLY = "/_aibox/api/configs/apply" { POST => "configs_apply" => configs::apply_config }
    CONFIGS_DELETE = "/_aibox/api/configs/delete" { POST => "configs_delete" => configs::delete_configs }
    CONFIGS_PROPAGATE_PREVIEW = "/_aibox/api/configs/propagate-auth/preview" { POST => "configs_propagate_preview" => configs::preview_auth_propagation }
    CONFIGS_PROPAGATE_EXECUTE = "/_aibox/api/configs/propagate-auth/execute" { POST => "configs_propagate_execute" => configs::execute_auth_propagation }

    // Sessions
    SESSIONS = "/_aibox/api/sessions" { GET => "sessions_list" => sessions::list_sessions }
    SESSIONS_SUMMARY = "/_aibox/api/sessions/summary" { GET => "sessions_summary" => sessions::session_summary }
    SESSIONS_DETAIL = "/_aibox/api/sessions/detail" { GET => "sessions_detail" => sessions::session_detail }
    SESSIONS_EVIDENCE = "/_aibox/api/sessions/evidence" { GET => "sessions_evidence" => sessions::session_evidence }
    SESSIONS_DELETE = "/_aibox/api/sessions/delete" { POST => "sessions_delete" => sessions::delete_sessions }

    // Management Operations
    OPERATIONS_CURRENT = "/_aibox/api/operations/current" { GET => "operations_current" => operations::current_operation }
    OPERATIONS_EVENTS = "/_aibox/api/operations/events" { GET => "operations_events" => operations::operation_events }
    OPERATIONS_BUILD = "/_aibox/api/operations/build" { POST => "operations_build" => operations::start_build }
    OPERATIONS_CANCEL = "/_aibox/api/operations/{id}/cancel" { POST => "operations_cancel" => operations::cancel_operation }

    // Request inspection and deletion
    REQUESTS = "/_aibox/api/requests" { GET => "requests_list" => requests::list_requests }
    REQUESTS_DELETE = "/_aibox/api/requests/delete" { POST => "requests_delete" => requests::delete_requests }
    REQUEST_DETAIL = "/_aibox/api/requests/{id}" { GET => "request_detail" => requests::request_detail }
    REQUEST_BODY = "/_aibox/api/requests/{id}/request-body" { GET => "request_body" => requests::request_body }
    RESPONSE_BODY = "/_aibox/api/requests/{id}/response-body" { GET => "response_body" => requests::response_body }
    REQUEST_BODY_DECODED = "/_aibox/api/requests/{id}/request-body-decoded" { GET => "request_body_decoded" => requests::decoded_request_body }
    RESPONSE_BODY_DECODED = "/_aibox/api/requests/{id}/response-body-decoded" { GET => "response_body_decoded" => requests::decoded_response_body }
    RESPONSE_EVENT_TIMINGS = "/_aibox/api/requests/{id}/response-event-timings" { GET => "response_event_timings" => requests::response_event_timings }
}
