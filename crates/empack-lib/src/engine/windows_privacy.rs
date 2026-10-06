//! Windows storage privacy is checked on native handles, before project bytes enter it.
use super::native;
use anyhow::{Result, ensure};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use std::{
    ffi::c_void,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, *},
    Storage::FileSystem::{
        CreateDirectoryW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, READ_CONTROL, WRITE_DAC,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: each instance owns one allocation returned by a LocalAlloc-based Win32 API.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn succeeded(value: i32) -> Result<()> {
    ensure!(
        value != 0,
        "Windows storage privacy check failed: {}",
        io::Error::last_os_error()
    );
    Ok(())
}
fn status(value: u32) -> Result<()> {
    ensure!(
        value == 0,
        "Windows storage privacy check failed: {}",
        io::Error::from_raw_os_error(value as i32)
    );
    Ok(())
}

// The caller retains the descriptor or token containing this SID for the entire call.
unsafe fn sid_string(sid: PSID) -> Result<String> {
    ensure!(!sid.is_null(), "Missing Windows storage owner");
    let mut text = ptr::null_mut();
    // SAFETY: the caller supplies a live SID from the security APIs; output is an owned allocation.
    succeeded(unsafe { ConvertSidToStringSidW(sid, &mut text) })?;
    let _allocation = LocalAllocation(text.cast());
    let mut length = 0;
    // SAFETY: successful conversion returns a null-terminated UTF-16 SID string.
    unsafe {
        while *text.add(length) != 0 {
            length += 1;
        }
        Ok(String::from_utf16(std::slice::from_raw_parts(
            text, length,
        ))?)
    }
}
fn current_user() -> Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: process pseudo-handle is valid; token is a writable output slot.
    succeeded(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    // SAFETY: successful OpenProcessToken returned a uniquely owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: zero buffer requests the required allocation size.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        );
    }
    ensure!(
        length as usize >= std::mem::size_of::<TOKEN_USER>() && length <= 65536,
        "Invalid token user buffer size"
    );
    let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: usize storage has TOKEN_USER's required alignment and sufficient writable capacity.
    succeeded(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    })?;
    // SAFETY: successful call initialized TOKEN_USER and its SID within the retained buffer.
    unsafe { sid_string((*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid) }
}
fn descriptor(sddl: &str) -> Result<LocalAllocation> {
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    // SAFETY: text is null-terminated and descriptor is an output slot.
    succeeded(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    })?;
    Ok(LocalAllocation(descriptor))
}
fn private_descriptor() -> Result<LocalAllocation> {
    descriptor(&format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        current_user()?
    ))
}
fn security_handle(directory: &Dir, writable: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES | if writable { WRITE_DAC } else { 0 })
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = directory.open_with(".", &options)?.into_std();
    native::reject_reparse(&file)?;
    ensure!(
        native::identity(&file)? == native::directory_identity(directory)?,
        "Storage directory changed during privacy check"
    );
    Ok(file)
}
fn trusted(sid: &str, user: &str) -> bool {
    sid == user || sid == "S-1-5-18" || sid == "S-1-5-32-544"
}
/// Existing shared directories are rejected, never silently reassigned or hardened.
pub(super) fn verify(directory: &Dir) -> Result<()> {
    let file = security_handle(directory, false)?;
    let user = current_user()?;
    let mut owner = ptr::null_mut();
    let mut acl: *mut ACL = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: file owns the handle; outputs point into the returned owned descriptor.
    status(unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        )
    })?;
    let _allocation = LocalAllocation(descriptor);
    // SAFETY: owner remains valid while the descriptor allocation is retained.
    ensure!(
        trusted(&unsafe { sid_string(owner)? }, &user),
        "Storage owner is not this user or a trusted system administrator"
    );
    ensure!(!acl.is_null(), "Storage has an unrestricted Windows DACL");
    // SAFETY: GetSecurityInfo returned a valid ACL contained in the retained descriptor.
    for index in 0..unsafe { (*acl).AceCount } {
        let mut ace = ptr::null_mut();
        // SAFETY: index is below AceCount and ace is an output slot.
        succeeded(unsafe { GetAce(acl, u32::from(index), &mut ace) })?;
        // SAFETY: successful GetAce returns an ACE with a header.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        match header.AceType {
            0 => {
                // ACCESS_ALLOWED_ACE_TYPE
                ensure!(
                    usize::from(header.AceSize) >= std::mem::size_of::<ACCESS_ALLOWED_ACE>(),
                    "Invalid storage access entry"
                );
                // SAFETY: type and size establish the layout; variable SID bytes belong to this ACE.
                let sid = unsafe {
                    sid_string(
                        ptr::addr_of!((*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart)
                            .cast_mut()
                            .cast(),
                    )?
                };
                ensure!(
                    trusted(&sid, &user)
                        || (sid == "S-1-3-0" && u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0),
                    "Storage permits access by another Windows principal"
                );
            }
            1 => {} // ACCESS_DENIED_ACE_TYPE cannot grant access.
            _ => anyhow::bail!("Storage contains an unsupported Windows access entry"),
        }
    }
    Ok(())
}

/// Create the selected boundary with a protected, inheritable DACL from its first instant.
pub(super) fn create(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let descriptor = private_descriptor()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both pointers remain alive for this call; directory creation copies the descriptor.
    succeeded(unsafe { CreateDirectoryW(path.as_ptr(), &attributes) })
}

/// Only for a newly created empty temporary parent owned by this operation.
pub(super) fn protect_empty_temporary(directory: &Dir) -> Result<()> {
    ensure!(
        directory.entries()?.next().is_none(),
        "Private staging parent must be empty"
    );
    let file = security_handle(directory, true)?;
    set_dacl(&file, &private_descriptor()?)?;
    verify(directory)
}
fn set_dacl(file: &File, descriptor: &LocalAllocation) -> Result<()> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = ptr::null_mut();
    // SAFETY: descriptor is valid and all output slots are writable.
    succeeded(unsafe {
        GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted)
    })?;
    ensure!(
        present != 0 && !acl.is_null(),
        "Private storage descriptor has no DACL"
    );
    // SAFETY: file has WRITE_DAC; the ACL lives through the call and is copied by Windows.
    status(unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        )
    })
}

#[cfg(test)]
pub(super) fn share_for_test(directory: &Dir) -> Result<()> {
    let shared = descriptor("D:P(A;OICI;FA;;;WD)")?;
    set_dacl(&security_handle(directory, true)?, &shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_creation_does_not_inherit_shared_parent_access() {
        let parent = tempfile::tempdir().unwrap();
        let parent_dir =
            Dir::open_ambient_dir(parent.path(), cap_std::ambient_authority()).unwrap();
        let shared = descriptor("D:P(A;OICI;FA;;;WD)").unwrap();
        set_dacl(&security_handle(&parent_dir, true).unwrap(), &shared).unwrap();
        assert!(verify(&parent_dir).is_err());
        assert!(crate::engine::publication::Publisher::open_existing(parent.path()).is_err());
        // Refusal must not modify the caller's shared directory.
        assert!(verify(&parent_dir).is_err());
        let path = parent.path().join("private");
        create(&path).unwrap();
        let dir = Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap();
        verify(&dir).unwrap();
        dir.create_dir("retained").unwrap();
        verify(&dir.open_dir("retained").unwrap()).unwrap();
        crate::engine::publication::Publisher::open_existing(&path)
            .unwrap()
            .unwrap();
    }
    #[test]
    fn unrestricted_dacl_is_rejected() {
        let parent = tempfile::tempdir().unwrap();
        let directory = Dir::open_ambient_dir(parent.path(), cap_std::ambient_authority()).unwrap();
        let file = security_handle(&directory, true).unwrap();
        // SAFETY: the fixture owns this temporary directory and a WRITE_DAC handle.
        status(unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
            )
        })
        .unwrap();
        assert!(verify(&directory).is_err());
        protect_empty_temporary(&directory).unwrap();
    }
    #[test]
    fn staging_directories_are_private() {
        let stage = crate::engine::staging::MutableStage::empty().unwrap();
        drop(stage);
    }
}
