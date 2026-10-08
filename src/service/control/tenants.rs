//! Tenant Control API handlers and wire types.

use super::{ControlResult, json_response};
use crate::management::{DeleteTenantsCommand, TenantCatalogEntry};
use crate::service::state::ServiceState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum TenantRow {
    Host {
        name: Option<String>,
        display_name: String,
        home: String,
        exists: bool,
    },
    Managed {
        name: String,
        display_name: String,
        home: String,
        exists: bool,
    },
}

pub(super) async fn list_tenants(State(state): State<ServiceState>) -> ControlResult {
    let entries = state.management.tenants.list().await?;
    let rows = entries
        .into_iter()
        .map(|entry| match entry {
            TenantCatalogEntry::Host { home, exists } => TenantRow::Host {
                name: None,
                display_name: "Host Tenant".to_string(),
                home,
                exists,
            },
            TenantCatalogEntry::Managed { name, home } => TenantRow::Managed {
                display_name: name.clone(),
                name,
                home,
                exists: true,
            },
        })
        .collect::<Vec<_>>();
    Ok(json_response(StatusCode::OK, &rows))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CreateTenantRequest {
    name: String,
}

pub(super) async fn create_tenant(
    State(state): State<ServiceState>,
    Json(request): Json<CreateTenantRequest>,
) -> ControlResult {
    let created = state.management.tenants.create(request.name).await?;
    Ok(json_response(
        StatusCode::OK,
        &CreatedTenantResponse {
            created: created.name,
            home: created.home,
        },
    ))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeleteSelection {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

pub(super) async fn delete_tenants(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteSelection>,
) -> ControlResult {
    let command = DeleteTenantsCommand {
        names: request.names,
        all: request.all,
        confirmation: request.confirmation,
    };
    let deleted = state.management.tenants.delete(command).await?;
    Ok(json_response(
        StatusCode::OK,
        &DeletedTenantsResponse {
            deleted: deleted.names,
            all: deleted.all,
        },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CreatedTenantResponse {
    created: String,
    home: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedTenantsResponse {
    deleted: Vec<String>,
    all: bool,
}
