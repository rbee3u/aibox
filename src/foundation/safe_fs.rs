//! Mechanical filesystem primitives for paths that may be container-writable.
//!
//! These helpers validate only the final path entry. Domain callers remain
//! responsible for validating ancestors and deciding which paths may be
//! created, replaced, or removed and which permissions they require.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Return `false` when the path is absent and `true` when its final entry is a
/// real directory; reject any other final entry type.
pub(crate) fn real_dir_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => bail!("{kind} is not a real directory: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Return `false` when the path is absent and `true` when its final entry is a
/// regular file; reject any other final entry type.
pub(crate) fn real_file_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Open an existing regular file without following a final symlink.
pub(crate) fn open_real_file(path: &Path, kind: &str) -> Result<fs::File> {
    if !real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    let file = open_no_follow(path).with_context(|| format!("open {kind} {}", path.display()))?;
    if !file.metadata()?.file_type().is_file() {
        bail!("{kind} is not a regular file: {}", path.display());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

/// Create a directory when absent, rejecting a symlink or non-directory final
/// entry. Newly created directories receive mode `0700` on Unix.
pub(crate) fn ensure_real_dir(path: &Path, kind: &str) -> Result<()> {
    if real_dir_exists(path, kind)? {
        return Ok(());
    }
    fs::create_dir_all(path).with_context(|| format!("create {kind} {}", path.display()))?;
    if real_dir_exists(path, kind)? {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("chmod 0700 {kind} {}", path.display()))?;
        }
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }
        Ok(())
    } else {
        bail!("{kind} disappeared while being created: {}", path.display())
    }
}

/// Remove a regular final path entry when present, rejecting symlinks and
/// other entry types.
pub(crate) fn remove_real_file_if_exists(path: &Path, kind: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
        Ok(meta) if !meta.file_type().is_file() => {
            bail!("{kind} is not a regular file: {}", path.display())
        }
        Ok(_) => {
            fs::remove_file(path).with_context(|| format!("remove {kind} {}", path.display()))?;
            if let Some(parent) = path.parent() {
                sync_dir(parent)?;
            }
            Ok(())
        }
    }
}

/// Remove a directory tree when its final path entry is a real directory,
/// rejecting symlinks and files.
pub(crate) fn remove_real_dir_if_exists(path: &Path, kind: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
        Ok(meta) if !meta.file_type().is_dir() => {
            bail!("{kind} is not a real directory: {}", path.display())
        }
        Ok(_) => {
            fs::remove_dir_all(path)
                .with_context(|| format!("delete {kind} {}", path.display()))?;
            if let Some(parent) = path.parent() {
                sync_dir(parent)?;
            }
            Ok(())
        }
    }
}

/// Flush directory entry updates to stable storage.
pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    fs::File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

/// Create one new regular file without following a final symlink.
pub(crate) fn create_new_file(path: &Path, kind: &str, mode: u32) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("create {kind} {}", path.display()))?;
    set_file_mode(&file, mode)?;
    if !file.metadata()?.file_type().is_file() {
        bail!("{kind} is not a regular file: {}", path.display());
    }
    Ok(file)
}

/// One same-directory temporary file prepared for an atomic replacement.
pub(crate) struct PreparedAtomicWrite {
    temporary: tempfile::NamedTempFile,
    parent: PathBuf,
}

impl PreparedAtomicWrite {
    /// Create a temporary file in `parent` and apply the requested Unix mode.
    pub(crate) fn new(parent: &Path, prefix: &str, mode: Option<u32>, kind: &str) -> Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix(prefix)
            .tempfile_in(parent)
            .with_context(|| format!("create temporary {kind} in {}", parent.display()))?;
        if let Some(mode) = mode {
            set_file_mode(temporary.as_file(), mode)?;
        }
        Ok(Self {
            temporary,
            parent: parent.to_path_buf(),
        })
    }

    /// Write bytes to the prepared temporary file.
    pub(crate) fn write_all(&mut self, content: &[u8]) -> Result<()> {
        self.temporary.write_all(content)?;
        Ok(())
    }

    /// Sync and atomically publish the prepared file without syncing its
    /// containing directory.
    pub(crate) fn persist(self, target: &Path, action: &str) -> Result<()> {
        self.temporary.as_file().sync_all()?;
        self.temporary
            .persist(target)
            .map_err(|error| error.error)
            .with_context(|| format!("{action} {}", target.display()))?;
        Ok(())
    }

    /// Sync, atomically publish, and sync the containing directory.
    pub(crate) fn commit(self, target: &Path, action: &str) -> Result<()> {
        let parent = self.parent.clone();
        self.persist(target, action)?;
        sync_dir(&parent)
    }
}

/// Publish an already synced same-directory temporary file and sync its parent.
pub(crate) fn publish_atomic_file(temporary: &Path, target: &Path, action: &str) -> Result<()> {
    fs::rename(temporary, target)
        .with_context(|| format!("{action} {} as {}", temporary.display(), target.display()))?;
    let parent = target.parent().context("atomic target has no parent")?;
    sync_dir(parent)
}

fn set_file_mode(file: &fs::File, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (file, mode);
    Ok(())
}

/// Size-bounded snapshot of one optional native file.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct FileSnapshot {
    pub present: bool,
    pub content: Vec<u8>,
    pub mode: Option<u32>,
}

impl FileSnapshot {
    /// Capture a size-bounded regular file without following a final symlink.
    pub(crate) fn capture_with_limit(path: &Path, limit: u64) -> Result<Self> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                present: false,
                content: Vec::new(),
                mode: None,
            }),
            Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            Ok(meta) if !meta.file_type().is_file() => {
                bail!(
                    "configuration path is not a regular file: {}",
                    path.display()
                )
            }
            Ok(_) => {
                let file = open_real_file(path, "configuration file")?;
                let metadata = file.metadata()?;
                if metadata.len() > limit {
                    bail!(
                        "configuration file exceeds {limit} bytes: {}",
                        path.display()
                    );
                }
                let mut content = Vec::new();
                file.take(limit.saturating_add(1))
                    .read_to_end(&mut content)?;
                if content.len() as u64 > limit {
                    bail!(
                        "configuration file exceeds {limit} bytes: {}",
                        path.display()
                    );
                }
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    Some(metadata.permissions().mode() & 0o7777)
                };
                #[cfg(not(unix))]
                let mode = None;
                Ok(Self {
                    present: true,
                    content,
                    mode,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_snapshots_enforce_the_read_limit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        fs::write(&path, b"12345").unwrap();

        let error = FileSnapshot::capture_with_limit(&path, 4)
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds 4 bytes"), "{error}");
        assert_eq!(
            FileSnapshot::capture_with_limit(&path, 5).unwrap().content,
            b"12345"
        );
    }

    #[test]
    fn file_snapshots_distinguish_absence_and_reject_non_files() {
        let root = tempfile::tempdir().unwrap();
        let missing = FileSnapshot::capture_with_limit(&root.path().join("missing"), 16).unwrap();
        assert!(!missing.present);
        assert!(missing.content.is_empty());
        assert_eq!(missing.mode, None);

        let error = FileSnapshot::capture_with_limit(root.path(), 16)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a regular file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_file_primitives_reject_symlinks_and_fifos() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::write(&target, b"target").unwrap();
        let link = root.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(open_real_file(&link, "test file").is_err());
        assert!(create_new_file(&link, "test file", 0o600).is_err());

        let fifo = root.path().join("fifo");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo_path` is a valid NUL-terminated path and mode has no pointers.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert!(open_real_file(&fifo, "test file").is_err());
        assert!(create_new_file(&fifo, "test file", 0o600).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn prepared_atomic_write_publishes_content_and_requested_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("metadata.json");
        let mut write =
            PreparedAtomicWrite::new(root.path(), ".metadata-", Some(0o600), "metadata").unwrap();
        write.write_all(b"{}\n").unwrap();
        write.commit(&target, "replace metadata file").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"{}\n");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
