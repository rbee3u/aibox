//! Control API route composition.
//!
//! Endpoint implementations remain in the parent module while this module
//! owns the public shape of the Console-internal API. Keeping registration by
//! domain makes it possible to move a domain's handlers and wire types without
//! changing the combined Service router.

use super::*;
use axum::Router;
use axum::routing::{get, post};

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
        .route("/_aibox/ui", get(index))
        .route("/_aibox/ui/app.css", get(requests::css))
        .route("/_aibox/ui/app.js", get(requests::js))
        .route("/_aibox/ui/{*path}", get(index))
}

fn overview_routes() -> Router<ServiceState> {
    Router::new()
        .route("/_aibox/api/bootstrap", get(overview::bootstrap))
        .route("/_aibox/api/overview", get(overview::overview))
        .route("/_aibox/api/topology", get(overview::topology))
}

fn tenant_routes() -> Router<ServiceState> {
    Router::new()
        .route(
            "/_aibox/api/tenants",
            get(tenants::list_tenants).post(tenants::create_tenant),
        )
        .route("/_aibox/api/tenants/delete", post(tenants::delete_tenants))
}

fn component_routes() -> Router<ServiceState> {
    Router::new()
        .route("/_aibox/api/components", get(components::list_components))
        .route(
            "/_aibox/api/components/latest",
            get(components::latest_components),
        )
        .route(
            "/_aibox/api/components/latest/check",
            post(components::check_latest_components),
        )
        .route(
            "/_aibox/api/components/install",
            post(components::install_component),
        )
        .route(
            "/_aibox/api/components/remove",
            post(components::remove_component),
        )
}

fn config_routes() -> Router<ServiceState> {
    Router::new()
        .route("/_aibox/api/configs", get(configs::list_configs))
        .route("/_aibox/api/configs/create", post(configs::create_config))
        .route(
            "/_aibox/api/configs/reveal",
            post(configs::reveal_config_file),
        )
        .route("/_aibox/api/configs/save", post(configs::save_config_file))
        .route(
            "/_aibox/api/configs/diagnose",
            post(configs::diagnose_config_file),
        )
        .route("/_aibox/api/configs/apply", post(configs::apply_config))
        .route("/_aibox/api/configs/delete", post(configs::delete_configs))
        .route(
            "/_aibox/api/configs/propagate-auth/preview",
            post(configs::preview_auth_propagation),
        )
        .route(
            "/_aibox/api/configs/propagate-auth/execute",
            post(configs::execute_auth_propagation),
        )
}

fn session_routes() -> Router<ServiceState> {
    Router::new()
        .route("/_aibox/api/sessions", get(sessions::list_sessions))
        .route(
            "/_aibox/api/sessions/summary",
            get(sessions::session_summary),
        )
        .route("/_aibox/api/sessions/detail", get(sessions::session_detail))
        .route(
            "/_aibox/api/sessions/evidence",
            get(sessions::session_evidence),
        )
        .route(
            "/_aibox/api/sessions/delete",
            post(sessions::delete_sessions),
        )
}

fn operation_routes() -> Router<ServiceState> {
    Router::new()
        .route(
            "/_aibox/api/operations/current",
            get(operations::current_operation),
        )
        .route(
            "/_aibox/api/operations/events",
            get(operations::operation_events),
        )
        .route(
            "/_aibox/api/operations/build",
            post(operations::start_build),
        )
        .route(
            "/_aibox/api/operations/{id}/cancel",
            post(operations::cancel_operation),
        )
}
