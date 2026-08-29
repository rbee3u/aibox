//! Component Control API handlers and wire types.

use super::*;
use crate::component::ComponentInspection;
use crate::service::coordination::component::{ComponentCoordinator, ComponentInstallation};
use crate::tenant::TenantSelection;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentQuery {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ComponentRow {
    pub(crate) kind: String,
    pub(crate) supports_version: bool,
    pub(crate) status: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) error: Option<String>,
}

pub(super) async fn list_components(
    State(state): State<ServiceState>,
    Query(query): Query<ComponentQuery>,
) -> Response<Body> {
    let selection = match TenantSelection::parse(&query.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match ComponentCoordinator::new(state).list(selection).await {
        Ok(inspections) => json_response(StatusCode::OK, &component_rows_from(inspections)),
        Err(error) => result_error(error),
    }
}

pub(super) fn component_rows(selected: &Tenant) -> Result<Vec<ComponentRow>> {
    Ok(component_rows_from(component::inspect_catalog(selected)?))
}

fn component_rows_from(inspections: Vec<ComponentInspection>) -> Vec<ComponentRow> {
    inspections
        .into_iter()
        .map(|inspection| {
            let (status, version) = inspection.status.map_or((None, None), |status| {
                let version = match &status {
                    ComponentStatus::Installed { version } => version.clone(),
                    _ => None,
                };
                (Some(component_status_name(&status).to_string()), version)
            });
            ComponentRow {
                kind: inspection.kind.name().to_string(),
                supports_version: inspection.kind.supports_version(),
                status,
                version,
                error: inspection.error,
            }
        })
        .collect()
}

pub(super) async fn latest_components(
    State(state): State<ServiceState>,
) -> Json<Option<component_updates::LatestSnapshot>> {
    Json(ComponentCoordinator::new(state).latest().await)
}

pub(super) async fn check_latest_components(State(state): State<ServiceState>) -> Response<Body> {
    match ComponentCoordinator::new(state).check_latest().await {
        Ok(snapshot) => json_response(StatusCode::OK, &snapshot),
        Err(error) => result_error(error),
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentMutation {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    component: String,
    version: Option<String>,
}

pub(super) async fn install_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> Response<Body> {
    let selection = match TenantSelection::parse(&request.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match ComponentCoordinator::new(state)
        .install(selection, request.component, request.version)
        .await
    {
        Ok(ComponentInstallation::Completed(installed)) => {
            json_response(StatusCode::OK, &InstalledComponentResponse { installed })
        }
        Ok(ComponentInstallation::Started(operation)) => {
            json_response(StatusCode::ACCEPTED, &operation)
        }
        Err(error) => result_error(error),
    }
}

pub(super) async fn remove_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> Response<Body> {
    let selection = match TenantSelection::parse(&request.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match ComponentCoordinator::new(state)
        .remove(selection, request.component)
        .await
    {
        Ok(removed) => json_response(StatusCode::OK, &RemovedComponentResponse { removed }),
        Err(error) => result_error(error),
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct InstalledComponentResponse {
    installed: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RemovedComponentResponse {
    removed: &'static str,
}

fn component_status_name(status: &ComponentStatus) -> &'static str {
    match status {
        ComponentStatus::Installed { .. } => "installed",
        ComponentStatus::Modified => "modified",
        ComponentStatus::Incomplete => "incomplete",
        ComponentStatus::Unmanaged => "unmanaged",
        ComponentStatus::NotInstalled => "not-installed",
    }
}
