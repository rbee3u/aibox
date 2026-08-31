//! Component Control API handlers and wire types.

use super::{ControlResult, default_tenant_selection, json_response};
use crate::component::{ComponentInspection, ComponentKind, ComponentStatus, LatestSnapshot};
use crate::service::coordination::{ComponentCoordinator, ComponentInstallation};
use crate::service::state::ServiceState;
use crate::tenant::TenantSelection;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

/// Stable wire representation of native Component inspection state.
///
/// The domain status carries an optional installed version, while the
/// Control API intentionally keeps the historical string values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ComponentStatusWire {
    NotInstalled,
    Installed,
    Incomplete,
    Modified,
    Unmanaged,
}

impl From<&ComponentStatus> for ComponentStatusWire {
    fn from(status: &ComponentStatus) -> Self {
        match status {
            ComponentStatus::Installed { .. } => Self::Installed,
            ComponentStatus::Modified => Self::Modified,
            ComponentStatus::Incomplete => Self::Incomplete,
            ComponentStatus::Unmanaged => Self::Unmanaged,
            ComponentStatus::NotInstalled => Self::NotInstalled,
        }
    }
}

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
    pub(crate) kind: ComponentKind,
    pub(crate) supports_version: bool,
    pub(crate) status: Option<ComponentStatusWire>,
    pub(crate) version: Option<String>,
    pub(crate) error: Option<String>,
}

pub(super) async fn list_components(
    State(state): State<ServiceState>,
    Query(query): Query<ComponentQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let inspections = ComponentCoordinator::new(state).list(selection).await?;
    Ok(json_response(
        StatusCode::OK,
        &component_rows_from(inspections),
    ))
}

/// Project native Component inspections onto the stable wire rows.
///
/// Shared with the Topology view, which embeds the same rows per Tenant.
pub(super) fn component_rows_from(inspections: Vec<ComponentInspection>) -> Vec<ComponentRow> {
    inspections
        .into_iter()
        .map(|inspection| {
            let (status, version) = inspection.status.map_or((None, None), |status| {
                let version = match &status {
                    ComponentStatus::Installed { version } => version.clone(),
                    _ => None,
                };
                (Some(ComponentStatusWire::from(&status)), version)
            });
            ComponentRow {
                kind: inspection.kind,
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
) -> Json<Option<LatestSnapshot>> {
    Json(ComponentCoordinator::new(state).latest().await)
}

pub(super) async fn check_latest_components(State(state): State<ServiceState>) -> ControlResult {
    let snapshot = ComponentCoordinator::new(state).check_latest().await?;
    Ok(json_response(StatusCode::OK, &snapshot))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ComponentMutation {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    component: ComponentKind,
    version: Option<String>,
}

pub(super) async fn install_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    Ok(
        match ComponentCoordinator::new(state)
            .install(selection, request.component, request.version)
            .await?
        {
            ComponentInstallation::Completed(installed) => {
                json_response(StatusCode::OK, &InstalledComponentResponse { installed })
            }
            ComponentInstallation::Started(operation) => {
                json_response(StatusCode::ACCEPTED, &operation)
            }
        },
    )
}

pub(super) async fn remove_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let removed = ComponentCoordinator::new(state)
        .remove(selection, request.component)
        .await?;
    Ok(json_response(
        StatusCode::OK,
        &RemovedComponentResponse { removed },
    ))
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
