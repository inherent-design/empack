//! Native handles stay inside the engine. Portable paths carry no native authority.
use anyhow::{Result, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use empack_core::path::PortableRelPath;
use std::fs::File;
mod creation;
pub(super) use creation::rename_new_directory;
#[cfg(windows)]
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ObjectIdentity {
    pub volume: u64,
    pub object: u128,
    pub created: Option<(u64, u32)>,
}

pub(super) fn identity(file: &File) -> Result<ObjectIdentity> {
    let created = file
        .metadata()?
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| (value.as_secs(), value.subsec_nanos()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(ObjectIdentity {
            volume: metadata.dev(),
            object: metadata.ino() as u128,
            created,
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the live file owns the handle and info is a correctly sized output buffer.
        let result = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        ensure!(
            result != 0,
            "Cannot bind native file identity: {}",
            io::Error::last_os_error()
        );
        Ok(ObjectIdentity {
            volume: info.VolumeSerialNumber,
            object: u128::from_le_bytes(info.FileId.Identifier),
            created,
        })
    }
}

pub(super) fn directory_identity(dir: &Dir) -> Result<ObjectIdentity> {
    identity(&dir.try_clone()?.into_std_file())
}

/// Walk one component at a time so no intermediate link is accepted.
pub(super) fn parent(root: &Dir, path: &PortableRelPath) -> Result<(Dir, String)> {
    let mut components = path.components().peekable();
    let mut directory = root.try_clone()?;
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            return Ok((directory, component.to_owned()));
        }
        directory = directory.open_dir_nofollow(component)?;
        reject_reparse(&directory.try_clone()?.into_std_file())?;
    }
    unreachable!("portable paths are nonempty")
}

pub(super) fn open_file(parent: &Dir, name: &str) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        // A racing FIFO must not block before the opened object can be checked.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = parent.open_with(name, &options)?.into_std();
    reject_reparse(&file)?;
    ensure!(
        file.metadata()?.is_file(),
        "Managed input is not a regular file: {name}"
    );
    Ok(file)
}

pub(super) fn reject_reparse(file: &File) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        ensure!(
            file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "Managed paths cannot traverse a reparse point"
        );
    }
    #[cfg(not(windows))]
    let _ = file;
    Ok(())
}

/// Linux capability directories may use O_PATH descriptors, which cannot be fsynced or chmodded.
/// Reopen the retained directory itself with read access, without recovering an ambient pathname.
#[cfg(unix)]
pub(super) fn readable_directory(directory: &Dir) -> Result<File> {
    use cap_std::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .follow(FollowSymlinks::No);
    let file = directory.open_with(".", &options)?.into_std();
    ensure!(
        file.metadata()?.is_dir() && identity(&file)? == directory_identity(directory)?,
        "Retained directory identity changed"
    );
    Ok(file)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn readable_directory_supports_durability_and_permissions_on_capability_handles() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let directory = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let identity_before = directory_identity(&directory).unwrap();
        let file = readable_directory(&directory).unwrap();
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .unwrap();
        file.sync_all().unwrap();
        assert_eq!(identity(&file).unwrap(), identity_before);
    }
}
