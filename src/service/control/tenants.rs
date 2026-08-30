//! Tenant Control API handlers and wire types.

use super::{ControlResult, json_response};
use crate::service::coordination::{DeleteTenantsCommand, TenantCatalogEntry, TenantCoordinator};
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
    let entries = TenantCoordinator::new(state).list().await?;
    Ok(json_response(
        StatusCode::OK,
        &entries
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
            .collect::<Vec<_>>(),
    ))
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
    let created = TenantCoordinator::new(state).create(request.name).await?;
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
    let deleted = TenantCoordinator::new(state).delete(command).await?;
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
