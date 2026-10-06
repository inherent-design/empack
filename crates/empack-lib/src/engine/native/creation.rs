//! Directory publication must fail atomically when the destination already exists.
use super::*;

pub(in crate::engine) fn rename_new_directory(
    parent: &Dir,
    source: &str,
    destination: &str,
    expected: ObjectIdentity,
) -> Result<()> {
    for name in [source, destination] {
        PortableRelPath::parse(name, empack_core::path::PathSyntax::ArtifactName)?;
    }
    let directory = parent.open_dir_nofollow(source)?;
    reject_reparse(&directory.try_clone()?.into_std_file())?;
    ensure!(
        directory_identity(&directory)? == expected,
        "Candidate directory changed"
    );
    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    {
        use std::{ffi::CString, os::fd::AsRawFd};
        let source = CString::new(source)?;
        let destination = CString::new(destination)?;
        // SAFETY: live retained directory descriptors and NUL-terminated component strings.
        // There is no fallback to rename(), which could replace a racing empty directory.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                parent.as_raw_fd(),
                source.as_ptr(),
                parent.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_vendor = "apple")]
        let result = unsafe {
            libc::renameatx_np(
                parent.as_raw_fd(),
                source.as_ptr(),
                parent.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(windows)]
    {
        // cap-std directory capabilities omit FILE_SHARE_DELETE. Only the identity-checked
        // DELETE handle may remain open while renaming this owned candidate.
        drop(directory);
        use cap_std::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
        };
        let mut options = OpenOptions::new();
        options
            .access_mode(DELETE | FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        let handle = parent.open_with(source, &options)?.into_std();
        reject_reparse(&handle)?;
        ensure!(
            identity(&handle)? == expected,
            "Candidate directory changed"
        );
        let name: Vec<u16> = destination.encode_utf16().collect();
        let length = std::mem::offset_of!(FILE_RENAME_INFO, FileName)
            .checked_add(
                name.len()
                    .checked_mul(2)
                    .ok_or_else(|| anyhow::anyhow!("Rename name overflow"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("Rename buffer overflow"))?
            .max(std::mem::size_of::<FILE_RENAME_INFO>());
        let length = u32::try_from(length)?;
        // usize storage gives FILE_RENAME_INFO its required native pointer alignment.
        let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: aligned, zero-initialized storage includes the complete flexible name array;
        // all handles and input bytes outlive the synchronous OS call.
        let result = unsafe {
            (*info).RootDirectory = parent.as_raw_handle();
            (*info).FileNameLength = u32::try_from(name.len() * 2)?;
            let name_output = storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(std::mem::offset_of!(FILE_RENAME_INFO, FileName))
                .cast::<u16>();
            std::ptr::copy_nonoverlapping(name.as_ptr(), name_output, name.len());
            // ReplaceIfExists remains false. RootDirectory binds the relative destination.
            SetFileInformationByHandle(handle.as_raw_handle(), FileRenameInfo, info.cast(), length)
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
    anyhow::bail!("Atomic no-replace directory publication is unsupported on this host");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_creation_never_replaces_an_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let parent = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        parent.create_dir("candidate").unwrap();
        let candidate = parent.open_dir_nofollow("candidate").unwrap();
        let binding = directory_identity(&candidate).unwrap();
        candidate.write("payload", b"complete").unwrap();
        parent.create_dir("target").unwrap();
        let occupied = directory_identity(&parent.open_dir_nofollow("target").unwrap()).unwrap();
        assert!(rename_new_directory(&parent, "candidate", "target", binding).is_err());
        assert_eq!(
            directory_identity(&parent.open_dir_nofollow("target").unwrap()).unwrap(),
            occupied
        );
        assert_eq!(parent.read("candidate/payload").unwrap(), b"complete");
        parent.remove_dir("target").unwrap();
        drop(candidate);
        rename_new_directory(&parent, "candidate", "target", binding).unwrap();
        assert_eq!(
            directory_identity(&parent.open_dir_nofollow("target").unwrap()).unwrap(),
            binding
        );
        assert_eq!(parent.read("target/payload").unwrap(), b"complete");
        assert!(parent.symlink_metadata("candidate").is_err());
    }
}
