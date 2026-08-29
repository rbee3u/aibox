//! Tenant Control API handlers and wire types.

use super::*;
use crate::service::coordination::tenant::{
    DeleteTenantsCommand, TenantCatalogEntry, TenantCoordinator,
};

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

pub(super) async fn list_tenants(State(state): State<ServiceState>) -> Response<Body> {
    match TenantCoordinator::new(state).list().await {
        Ok(entries) => json_response(
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
        ),
        Err(error) => result_error(error),
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CreateTenantRequest {
    name: String,
}

pub(super) async fn create_tenant(
    State(state): State<ServiceState>,
    Json(request): Json<CreateTenantRequest>,
) -> Response<Body> {
    match TenantCoordinator::new(state).create(request.name).await {
        Ok(created) => json_response(
            StatusCode::OK,
            &CreatedTenantResponse {
                created: created.name,
                home: created.home,
            },
        ),
        Err(error) => result_error(error),
    }
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
) -> Response<Body> {
    let command = DeleteTenantsCommand {
        names: request.names,
        all: request.all,
        confirmation: request.confirmation,
    };
    match TenantCoordinator::new(state).delete(command).await {
        Ok(deleted) => json_response(
            StatusCode::OK,
            &DeletedTenantsResponse {
                deleted: deleted.names,
                all: deleted.all,
            },
        ),
        Err(error) => result_error(error),
    }
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
