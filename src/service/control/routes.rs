//! Control API route composition.
//!
//! Endpoint implementations remain in the parent module while this module
//! owns the public shape of the Console-internal API. Keeping registration by
//! domain makes it possible to move a domain's handlers and wire types without
//! changing the combined Service router.

use super::{
    assets, components, configs, index, operations, overview, requests, sessions, tenants,
};
use crate::service::state::ServiceState;
use axum::Router;
use axum::routing::{get, post};

/// Stable route descriptions shared by Axum registration and the generated
/// Console contract. The Console still owns handwritten clients; this list is
/// a test-facing compatibility manifest.
#[cfg(test)]
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct EndpointDescription {
    pub(crate) key: &'static str,
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
}

/// Declare each Control API route once.
///
/// One invocation produces both the path constants used by Axum registration
/// and the ordered test-facing [`ENDPOINTS`] manifest that generates
/// `console/src/api/generated/routes.ts`. Declaring a route in one place keeps
/// the generated Console manifest from drifting out of the router. A path that
/// serves several methods lists one `method => key` pair per method, and
/// `ENDPOINTS` preserves the declaration order because the generated file is
/// compared byte for byte.
macro_rules! control_routes {
    ($($name:ident = $path:literal { $($method:literal => $key:literal),+ $(,)? })+) => {
        $(pub(crate) const $name: &str = $path;)+

        #[cfg(test)]
        pub(crate) const ENDPOINTS: &[EndpointDescription] = &[
            $($(EndpointDescription {
                key: $key,
                method: $method,
                path: $name,
            },)+)+
        ];
    };
}

control_routes! {
    UI = "/_aibox/ui" { "GET" => "ui" }
    UI_CSS = "/_aibox/ui/app.css" { "GET" => "ui_css" }
    UI_JS = "/_aibox/ui/app.js" { "GET" => "ui_js" }
    UI_ASSET = "/_aibox/ui/{*path}" { "GET" => "ui_asset" }
    BOOTSTRAP = "/_aibox/api/bootstrap" { "GET" => "bootstrap" }
    OVERVIEW = "/_aibox/api/overview" { "GET" => "overview" }
    TOPOLOGY = "/_aibox/api/topology" { "GET" => "topology" }
    TENANTS = "/_aibox/api/tenants" { "GET" => "tenants_list", "POST" => "tenants_create" }
    TENANTS_DELETE = "/_aibox/api/tenants/delete" { "POST" => "tenants_delete" }
    COMPONENTS = "/_aibox/api/components" { "GET" => "components_list" }
    COMPONENTS_LATEST = "/_aibox/api/components/latest" { "GET" => "components_latest" }
    COMPONENTS_LATEST_CHECK = "/_aibox/api/components/latest/check" { "POST" => "components_latest_check" }
    COMPONENTS_INSTALL = "/_aibox/api/components/install" { "POST" => "components_install" }
    COMPONENTS_REMOVE = "/_aibox/api/components/remove" { "POST" => "components_remove" }
    CONFIGS = "/_aibox/api/configs" { "GET" => "configs_list" }
    CONFIGS_CREATE = "/_aibox/api/configs/create" { "POST" => "configs_create" }
    CONFIGS_REVEAL = "/_aibox/api/configs/reveal" { "POST" => "configs_reveal" }
    CONFIGS_SAVE = "/_aibox/api/configs/save" { "POST" => "configs_save" }
    CONFIGS_DIAGNOSE = "/_aibox/api/configs/diagnose" { "POST" => "configs_diagnose" }
    CONFIGS_APPLY = "/_aibox/api/configs/apply" { "POST" => "configs_apply" }
    CONFIGS_DELETE = "/_aibox/api/configs/delete" { "POST" => "configs_delete" }
    CONFIGS_PROPAGATE_PREVIEW = "/_aibox/api/configs/propagate-auth/preview" { "POST" => "configs_propagate_preview" }
    CONFIGS_PROPAGATE_EXECUTE = "/_aibox/api/configs/propagate-auth/execute" { "POST" => "configs_propagate_execute" }
    SESSIONS = "/_aibox/api/sessions" { "GET" => "sessions_list" }
    SESSIONS_SUMMARY = "/_aibox/api/sessions/summary" { "GET" => "sessions_summary" }
    SESSIONS_DETAIL = "/_aibox/api/sessions/detail" { "GET" => "sessions_detail" }
    SESSIONS_EVIDENCE = "/_aibox/api/sessions/evidence" { "GET" => "sessions_evidence" }
    SESSIONS_DELETE = "/_aibox/api/sessions/delete" { "POST" => "sessions_delete" }
    OPERATIONS_CURRENT = "/_aibox/api/operations/current" { "GET" => "operations_current" }
    OPERATIONS_EVENTS = "/_aibox/api/operations/events" { "GET" => "operations_events" }
    OPERATIONS_BUILD = "/_aibox/api/operations/build" { "POST" => "operations_build" }
    OPERATIONS_CANCEL = "/_aibox/api/operations/{id}/cancel" { "POST" => "operations_cancel" }
    REQUESTS = "/_aibox/api/requests" { "GET" => "requests_list" }
    REQUESTS_DELETE = "/_aibox/api/requests/delete" { "POST" => "requests_delete" }
    REQUEST_DETAIL = "/_aibox/api/requests/{id}" { "GET" => "request_detail" }
    REQUEST_BODY = "/_aibox/api/requests/{id}/request-body" { "GET" => "request_body" }
    RESPONSE_BODY = "/_aibox/api/requests/{id}/response-body" { "GET" => "response_body" }
    REQUEST_BODY_DECODED = "/_aibox/api/requests/{id}/request-body-decoded" { "GET" => "request_body_decoded" }
    RESPONSE_BODY_DECODED = "/_aibox/api/requests/{id}/response-body-decoded" { "GET" => "response_body_decoded" }
    RESPONSE_EVENT_TIMINGS = "/_aibox/api/requests/{id}/response-event-timings" { "GET" => "response_event_timings" }
}

pub(super) fn router() -> Router<ServiceState> {
    Router::new()
        .merge(ui_routes())
        .merge(overview_routes())
        .merge(tenant_routes())
        .merge(component_routes())
        .merge(config_routes())
        .merge(session_routes())
        .merge(operation_routes())
        .merge(requests::api_router())
}

fn ui_routes() -> Router<ServiceState> {
    Router::new()
        .route(UI, get(index))
        .route(UI_CSS, get(assets::css))
        .route(UI_JS, get(assets::js))
        .route(UI_ASSET, get(index))
}

fn overview_routes() -> Router<ServiceState> {
    Router::new()
        .route(BOOTSTRAP, get(overview::bootstrap))
        .route(OVERVIEW, get(overview::overview))
        .route(TOPOLOGY, get(overview::topology))
}

fn tenant_routes() -> Router<ServiceState> {
    Router::new()
        .route(
            TENANTS,
            get(tenants::list_tenants).post(tenants::create_tenant),
        )
        .route(TENANTS_DELETE, post(tenants::delete_tenants))
}

fn component_routes() -> Router<ServiceState> {
    Router::new()
        .route(COMPONENTS, get(components::list_components))
        .route(COMPONENTS_LATEST, get(components::latest_components))
        .route(
            COMPONENTS_LATEST_CHECK,
            post(components::check_latest_components),
        )
        .route(COMPONENTS_INSTALL, post(components::install_component))
        .route(COMPONENTS_REMOVE, post(components::remove_component))
}

fn config_routes() -> Router<ServiceState> {
    Router::new()
        .route(CONFIGS, get(configs::list_configs))
        .route(CONFIGS_CREATE, post(configs::create_config))
        .route(CONFIGS_REVEAL, post(configs::reveal_config_file))
        .route(CONFIGS_SAVE, post(configs::save_config_file))
        .route(CONFIGS_DIAGNOSE, post(configs::diagnose_config_file))
        .route(CONFIGS_APPLY, post(configs::apply_config))
        .route(CONFIGS_DELETE, post(configs::delete_configs))
        .route(
            CONFIGS_PROPAGATE_PREVIEW,
            post(configs::preview_auth_propagation),
        )
        .route(
            CONFIGS_PROPAGATE_EXECUTE,
            post(configs::execute_auth_propagation),
        )
}

fn session_routes() -> Router<ServiceState> {
    Router::new()
        .route(SESSIONS, get(sessions::list_sessions))
        .route(SESSIONS_SUMMARY, get(sessions::session_summary))
        .route(SESSIONS_DETAIL, get(sessions::session_detail))
        .route(SESSIONS_EVIDENCE, get(sessions::session_evidence))
        .route(SESSIONS_DELETE, post(sessions::delete_sessions))
}

fn operation_routes() -> Router<ServiceState> {
    Router::new()
        .route(OPERATIONS_CURRENT, get(operations::current_operation))
        .route(OPERATIONS_EVENTS, get(operations::operation_events))
        .route(OPERATIONS_BUILD, post(operations::start_build))
        .route(OPERATIONS_CANCEL, post(operations::cancel_operation))
}
