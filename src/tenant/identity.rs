use anyhow::{Result, bail};
use std::fmt;
use std::path::PathBuf;

/// An AIBox-managed, runnable Tenant.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ManagedTenant {
    pub(super) name: ManagedTenantName,
    pub(super) home_dir: PathBuf,
    pub(super) root_dir: PathBuf,
}

/// A validated Managed Tenant name.
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct ManagedTenantName(String);

impl ManagedTenantName {
    /// Parse a lowercase DNS label without touching the filesystem.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        validate_name("tenant", value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the validated name as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ManagedTenantName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One Console-selected Tenant, independent of any filesystem view.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TenantSelection {
    /// The management-only Tenant backed by the real host Home.
    Host,
    /// One validated Managed Tenant name.
    Managed(ManagedTenantName),
}

impl TenantSelection {
    /// Decode the stable Control wire key into a closed Tenant selection.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value == "host" {
            return Ok(Self::Host);
        }
        let Some(name) = value.strip_prefix("managed:") else {
            bail!("unknown Tenant selection: {value}");
        };
        Ok(Self::Managed(ManagedTenantName::parse(name)?))
    }
}

/// A persistent Coding Agent identity.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum Tenant {
    /// An AIBox-managed, runnable Tenant.
    Managed(ManagedTenant),
    /// The management-only Tenant backed by the real host Home.
    Host {
        /// Real host Home containing native Coding Agent state.
        home_dir: PathBuf,
        /// Root containing host-only AIBox state.
        root_dir: PathBuf,
    },
}

/// Validate a Tenant or Named Config name as a lowercase DNS label.
pub(crate) fn validate_name(kind: &str, value: &str) -> Result<()> {
    if is_safe_name(value) {
        Ok(())
    } else {
        bail!("invalid {kind} name '{value}': expected a 1-63 character lowercase DNS label")
    }
}

/// Whether a user-controlled name is a 1-63 character lowercase DNS label.
pub(crate) fn is_safe_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}
