//! Policy-free operating-system primitives shared by all domains.

pub(crate) mod platform;
pub(crate) mod safe_fs;
pub(crate) mod sync;

/// Upper bound on any untrusted native configuration file AIBox reads whole.
pub(crate) const MAX_NATIVE_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
