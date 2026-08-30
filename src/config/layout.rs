//! Typed Named Config path boundary.

use super::{ConfigFile, NamedConfigName};
use crate::tenant::TenantAgent;
use std::path::PathBuf;

pub(super) fn named_config_dir(selected: &TenantAgent, config: &NamedConfigName) -> PathBuf {
    selected.named_config_catalog_dir().join(config.as_str())
}

pub(super) fn named_config_file(
    selected: &TenantAgent,
    config: &NamedConfigName,
    file: ConfigFile,
) -> PathBuf {
    named_config_dir(selected, config).join(file.as_str(selected.agent()))
}
