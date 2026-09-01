//! Request Group repair and periodic compaction of the oldest ungrouped Requests.

use super::RequestStore;
use super::layout::{
    is_grouping_tmp_name, new_grouping_tmp_name, parse_request_group_name,
    read_collection_inventory, rename_noreplace, request_group_directory_name,
    request_locations_in, restrict_dir,
};
use crate::foundation::sync::{lock_unpoisoned, write_unpoisoned};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Ungrouped Request directories that trigger one Request Group compaction.
pub(crate) const UNGROUPED_COMPACT_THRESHOLD: usize = 500;

/// Requests moved into a new Request Group when compaction runs.
pub(super) const GROUP_SIZE: usize = 200;

/// How long the Service waits after start, and between later compaction ticks.
pub(crate) const REQUEST_GROUP_COMPACT_INTERVAL: Duration = Duration::from_secs(10 * 60);

impl RequestStore {
    /// Roll back unfinished grouping directories, reconcile Request Group counts,
    /// and compact at most one group of the oldest eligible ungrouped Requests.
    pub(crate) fn compact_once(&self) -> Result<()> {
        let _namespace = write_unpoisoned(&self.namespace);
        if let Err(error) = self
            .repair_unlocked()
            .and_then(|()| self.compact_unlocked())
        {
            self.warning("request group compact failed", None);
            return Err(error);
        }
        Ok(())
    }

    /// Restore unfinished grouping directories and Request Group count suffixes.
    pub(super) fn repair_unlocked(&self) -> Result<()> {
        if !crate::foundation::safe_fs::real_dir_exists(&self.root, "Request collection")? {
            return Ok(());
        }
        let mut grouping_tmps = Vec::new();
        let mut groups = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Request collection {}", self.root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.warning("request collection entry could not be inspected", None);
                    continue;
                }
            };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if is_grouping_tmp_name(name) {
                grouping_tmps.push(path);
            } else if let Ok(group) = parse_request_group_name(name) {
                groups.push((path, group));
            }
        }
        for path in grouping_tmps {
            self.rollback_grouping_tmp(&path)?;
        }
        for (path, group) in groups {
            if path.exists() {
                self.reconcile_group(&path, &group.timestamp, group.count)?;
            }
        }
        Ok(())
    }

    fn rollback_grouping_tmp(&self, tmp: &Path) -> Result<()> {
        if !crate::foundation::safe_fs::real_dir_exists(tmp, "Request grouping directory")? {
            return Ok(());
        }
        for entry in fs::read_dir(tmp)
            .with_context(|| format!("read Request grouping directory {}", tmp.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.warning("request grouping entry could not be inspected", None);
                    continue;
                }
            };
            let source = entry.path();
            let metadata = match fs::symlink_metadata(&source) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.warning("request grouping entry could not be inspected", None);
                    continue;
                }
            };
            if !metadata.file_type().is_dir() {
                self.warning("unexpected request grouping entry ignored", None);
                continue;
            }
            let Some(name) = source.file_name() else {
                continue;
            };
            let destination = self.root.join(name);
            if let Err(error) = rename_noreplace(&source, &destination) {
                self.warning("request grouping rollback failed", None);
                let _ = error;
            }
        }
        match fs::remove_dir(tmp) {
            Ok(()) => crate::foundation::safe_fs::sync_dir(&self.root)?,
            Err(_) => self.warning("request grouping directory retained", None),
        }
        Ok(())
    }

    fn reconcile_group(&self, path: &Path, timestamp: &str, counted: usize) -> Result<()> {
        let actual = request_locations_in(path, |_| {})?.len();
        if actual == 0 {
            match fs::remove_dir(path) {
                Ok(()) => crate::foundation::safe_fs::sync_dir(&self.root)?,
                Err(_) => self.warning("empty Request Group could not be removed", None),
            }
            return Ok(());
        }
        if actual == counted {
            return Ok(());
        }
        let target = self
            .root
            .join(request_group_directory_name(timestamp, actual));
        if path == target {
            return Ok(());
        }
        rename_noreplace(path, &target).with_context(|| {
            format!(
                "reconcile Request Group {} to {}",
                path.display(),
                target.display()
            )
        })?;
        crate::foundation::safe_fs::sync_dir(&self.root)?;
        Ok(())
    }

    fn compact_unlocked(&self) -> Result<()> {
        let inventory = self.inventory_unlocked()?;
        if inventory.hot.len() <= UNGROUPED_COMPACT_THRESHOLD {
            return Ok(());
        }
        let active = lock_unpoisoned(&self.active);
        let mut eligible: Vec<_> = inventory
            .hot
            .into_iter()
            .filter(|location| !location.active_prefixed && !active.contains_key(&location.id))
            .collect();
        drop(active);
        eligible.sort_by(|left, right| left.name.cmp(&right.name));
        if eligible.len() < GROUP_SIZE {
            return Ok(());
        }
        let batch: Vec<_> = eligible.into_iter().take(GROUP_SIZE).collect();
        let timestamp = batch[0]
            .name
            .get(..20)
            .context("eligible Request directory name has no timestamp")?;
        let tmp = self.root.join(new_grouping_tmp_name());
        fs::create_dir(&tmp)
            .with_context(|| format!("create Request grouping directory {}", tmp.display()))?;
        restrict_dir(&tmp)?;
        for location in &batch {
            rename_noreplace(&location.path, &tmp.join(&location.name))?;
        }
        crate::foundation::safe_fs::sync_dir(&tmp)?;
        let published = self
            .root
            .join(request_group_directory_name(timestamp, GROUP_SIZE));
        rename_noreplace(&tmp, &published).with_context(|| {
            format!(
                "publish Request Group {} from {}",
                published.display(),
                tmp.display()
            )
        })?;
        crate::foundation::safe_fs::sync_dir(&self.root)?;
        Ok(())
    }

    /// Snapshot ungrouped Requests, Request Groups, and unfinished grouping directories.
    pub(super) fn inventory_unlocked(&self) -> Result<super::layout::CollectionInventory> {
        read_collection_inventory(&self.root, |category| self.warning(category, None))
    }
}

#[cfg(test)]
#[path = "compact_tests.rs"]
mod tests;
