//! Utilities.
//!
//! This module defines the atomic file write every file the crate materializes goes through,
//! which is the only utility not needing a wallet: the wallet ones are in
//! [`crate::wallet::utils`].

use super::*;

/// Make a newly created file's directory entry durable.
///
/// [`File::sync_all`](fs::File::sync_all) covers a file's own data and metadata but not the entry
/// that names it, so without this a freshly created file can still vanish on power loss.
#[cfg(not(target_os = "windows"))]
pub(crate) fn sync_dir<P: AsRef<Path>>(dir: P) -> Result<(), io::Error> {
    fs::File::open(dir)?.sync_all()
}

/// Windows cannot open a directory as a file and offers no portable equivalent, so the directory
/// entry is left to the filesystem's own ordering there.
#[cfg(target_os = "windows")]
pub(crate) fn sync_dir<P: AsRef<Path>>(_dir: P) -> Result<(), io::Error> {
    Ok(())
}

/// Atomically materialize a file at `path`.
///
/// The `write` closure writes to a temporary file in the same directory, which
/// is then fsynced and renamed into place. Since `rename` is atomic on a
/// single filesystem, a reader (or a crash) never observes a partially-written
/// file. The fsync (plus a best-effort directory fsync) makes the result
/// durable across a power loss.
/// A `path` which is a symlink is followed, so the file it points to is the one replaced, and the
/// new file is given the permissions of the one it replaces, as a caller may be writing a file of
/// its own, with its own mode, through a path of its choosing.
pub(crate) fn atomic_write_with<F>(path: &Path, write: F) -> Result<(), Error>
where
    F: FnOnce(&Path) -> Result<(), Error>,
{
    // canonicalizing resolves a symlink (and only works on an existing file, so a new one is
    // written at the path as given)
    let resolved = fs::canonicalize(path).ok();
    let path = resolved.as_deref().unwrap_or(path);
    let parent = file_parent(path)?;
    fs::create_dir_all(parent)?;
    let permissions = fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = atomic_tmp_path(path)?;
    // drop any temp file leaked by an earlier crash
    let _ = fs::remove_file(&tmp);
    if let Err(e) = write(&tmp) {
        // don't leave a partial temp file behind (best-effort)
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if let Some(permissions) = permissions {
        fs::set_permissions(&tmp, permissions)?;
    }
    // fsync the temp file contents before making them visible
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, path)?;
    // fsync the parent directory so the rename entry itself is durable (a no-op on Windows, where
    // a directory can't be opened as a file: the data is already fsynced and only the durability
    // of the rename across a power loss is left to the filesystem)
    sync_dir(parent)?;
    Ok(())
}

/// The directory the file at `path` is in, which is the current one if `path` names a file with no
/// directory part (`path.parent()` is empty then, which no file operation accepts).
fn file_parent(path: &Path) -> Result<&Path, Error> {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Ok(Path::new(".")),
        Some(parent) => Ok(parent),
        None => Err(Error::Internal {
            details: format!("cannot atomically write {path:?}: no parent directory"),
        }),
    }
}

/// The temporary file `atomic_write_with` stages `path` in before renaming it into place.
///
/// Exposed so a loader can recover a temp file left behind by a crash between the write and the
/// rename.
pub(crate) fn atomic_tmp_path(path: &Path) -> Result<PathBuf, Error> {
    let parent = file_parent(path)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Internal {
            details: format!("cannot atomically write {path:?}: invalid file name"),
        })?;
    Ok(parent.join(format!(".{file_name}.tmp")))
}

/// Atomically write `bytes` to `path`.
#[cfg(feature = "wallet")]
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    atomic_write_with(path, |tmp| Ok(fs::write(tmp, bytes)?))
}

#[cfg(all(test, feature = "wallet"))]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        // parent dir does not exist yet: it must be created
        let path = dir.path().join("sub").join("f.bin");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        // overwrites cleanly
        atomic_write(&path, b"world!!").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"world!!");
        // no temp file is left behind
        let tmp = path.parent().unwrap().join(".f.bin.tmp");
        assert!(!tmp.exists());
        // a failing write leaves the previous file intact and no partial temp file behind
        let res = atomic_write_with(&path, |tmp| {
            fs::write(tmp, b"partial")?;
            Err(Error::Internal {
                details: s!("write failed"),
            })
        });
        assert!(matches!(res, Err(Error::Internal { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"world!!");
        assert!(!tmp.exists());
    }
}
