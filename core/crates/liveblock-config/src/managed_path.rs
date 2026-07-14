//! Fail-closed validation for file paths received over desktop UI/IPC.
//!
//! Callers provide a fixed application-owned directory and extension. The
//! returned path is canonicalized to `root/file_name`; nested paths, traversal,
//! symlinks, special files, and alternate roots are rejected.

use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedPathMode {
    ExistingRegularFile,
    ExistingOrNewRegularFile,
}

pub fn validate_managed_file_path(
    candidate: &Path,
    root: &Path,
    extension: &str,
    mode: ManagedPathMode,
) -> Result<PathBuf> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() {
        return Err(invalid("managed root must not be a symlink"));
    }
    let root = fs::canonicalize(root)?;
    if !fs::metadata(&root)?.is_dir() {
        return Err(invalid("managed root is not a directory"));
    }
    let file_name = candidate
        .file_name()
        .ok_or_else(|| invalid("managed path has no filename"))?;
    let actual_extension = candidate
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("managed path has no extension"))?;
    if !actual_extension.eq_ignore_ascii_case(extension) {
        return Err(invalid("managed path has an unexpected extension"));
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| invalid("managed path has no parent"))?;
    if fs::canonicalize(parent)? != root {
        return Err(invalid("managed path is outside its application directory"));
    }

    let normalized = root.join(file_name);
    match fs::symlink_metadata(&normalized) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid("managed path is not a regular non-symlink file"));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if mode == ManagedPathMode::ExistingRegularFile {
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    }
    Ok(normalized)
}

/// Move a validated regular file without replacing an existing destination.
/// On Unix a hard-link reservation supplies atomic create-new semantics; both
/// application directories are required to live on the same data volume.
pub fn move_regular_file_no_replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::hard_link(source, destination)?;
        if let Err(error) = fs::remove_file(source) {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination_wide: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Zero flags deliberately omits MOVEFILE_REPLACE_EXISTING.
        if unsafe { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), 0) } == 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        if destination.exists() {
            return Err(Error::new(ErrorKind::AlreadyExists, "destination exists"));
        }
        fs::rename(source, destination)
    }
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_immediate_managed_regular_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("screenshots");
        fs::create_dir(&root).unwrap();
        let image = root.join("frame.png");
        fs::write(&image, b"png").unwrap();

        assert_eq!(
            validate_managed_file_path(&image, &root, "png", ManagedPathMode::ExistingRegularFile,)
                .unwrap(),
            fs::canonicalize(&image).unwrap()
        );
        assert!(validate_managed_file_path(
            &root.join("new.json"),
            &root,
            "json",
            ManagedPathMode::ExistingOrNewRegularFile,
        )
        .is_ok());
        assert!(validate_managed_file_path(
            &image,
            &root,
            "json",
            ManagedPathMode::ExistingRegularFile,
        )
        .is_err());
        assert!(validate_managed_file_path(
            &temp.path().join("outside.png"),
            &root,
            "png",
            ManagedPathMode::ExistingOrNewRegularFile,
        )
        .is_err());

        let nested = root.join("nested");
        fs::create_dir(&nested).unwrap();
        assert!(validate_managed_file_path(
            &nested.join("frame.png"),
            &root,
            "png",
            ManagedPathMode::ExistingOrNewRegularFile,
        )
        .is_err());
    }

    #[test]
    fn no_replace_move_preserves_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        let destination = temp.path().join("destination.png");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"existing").unwrap();
        assert!(move_regular_file_no_replace(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_roots_and_files() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let real_root = temp.path().join("real-labels");
        fs::create_dir(&real_root).unwrap();
        let root_link = temp.path().join("labels");
        symlink(&real_root, &root_link).unwrap();
        assert!(validate_managed_file_path(
            &root_link.join("new.json"),
            &root_link,
            "json",
            ManagedPathMode::ExistingOrNewRegularFile,
        )
        .is_err());

        let root = real_root;
        let target = root.join("real.json");
        fs::write(&target, b"{}").unwrap();
        let link = root.join("link.json");
        symlink(&target, &link).unwrap();
        assert!(validate_managed_file_path(
            &link,
            &root,
            "json",
            ManagedPathMode::ExistingRegularFile,
        )
        .is_err());
    }
}
