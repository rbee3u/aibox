//! Concrete Service coordinators for management use cases.

pub(crate) mod component;
pub(crate) mod config;
pub(crate) mod operation;
pub(crate) mod session;
pub(crate) mod tenant;

use crate::application_error::{ApplicationErrorKind, application_error};
use anyhow::Result;

pub(super) async fn run_blocking<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(result) => result,
        Err(error) => Err(application_error(
            ApplicationErrorKind::Internal,
            format!("management worker failed: {error}"),
        )),
    }
}
