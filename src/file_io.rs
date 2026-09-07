//! Same-directory staged writes: never truncate the destination before success.
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
use tempfile::NamedTempFile;

pub(crate) fn atomic_write(path: &Path, content: &[u8], overwrite: bool) -> io::Result<()> {
    // Follow an existing symlink, just like opening the file did.
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let staged = stage_write(&path, content, &path)?;
    if overwrite {
        staged.persist(&path).map_err(|error| error.error)?;
    } else {
        staged
            .persist_noclobber(&path)
            .map_err(|error| error.error)?;
    }
    Ok(())
}

pub(crate) fn rename_with_content(old: &Path, new: &Path, content: &[u8]) -> io::Result<()> {
    if old == new {
        return atomic_write(old, content, true);
    }
    let staged = stage_write(new, content, old)?;
    // Refuse a destination created by another process after name selection.
    staged.persist_noclobber(new).map_err(|error| error.error)?;
    if let Err(error) = fs::remove_file(old) {
        // Keep memory pointing to the old file. If cleanup also fails, the extra
        // copy is harmless; neither copy of the original is truncated.
        let _ = fs::remove_file(new);
        return Err(error);
    }
    Ok(())
}

fn stage_write(path: &Path, content: &[u8], permission_source: &Path) -> io::Result<NamedTempFile> {
    let permissions = match fs::metadata(permission_source) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.permissions().readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "destination is not a writable file",
                ));
            }
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".velocimd-")
        .tempfile_in(parent)?;
    staged.write_all(content)?;
    if let Some(permissions) = permissions {
        staged.as_file().set_permissions(permissions)?;
    }
    staged.as_file().sync_all()?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_failure_preserves_existing_destination_and_cleans_temporary_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("Note.md");
        fs::write(&target, "original").unwrap();
        // Failure at commit, after the replacement was staged and synced.
        assert!(atomic_write(&target, b"replacement", false).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn overwrite_preserves_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("Note.md");
        fs::write(&target, "original").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        atomic_write(&target, b"replacement", true).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "replacement");
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
