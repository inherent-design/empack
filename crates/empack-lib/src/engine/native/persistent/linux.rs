//! Linux UUID queries remain handle-relative, including compatibility with older ext4.
use super::{ObjectIdentity, digest, unsupported_or_error};
use anyhow::{Result, ensure};
use std::{fs::File, os::fd::AsRawFd};

pub(in crate::engine::native) fn identity(
    file: &File,
    observed: &ObjectIdentity,
) -> Result<Option<[u8; 32]>> {
    use anyhow::ensure;
    use std::os::fd::{AsRawFd, FromRawFd};
    let Some(created) = observed.created else {
        return Ok(None);
    };
    // Capability directories may be O_PATH, which ioctl cannot query. Reopen the
    // retained directory itself, not an ambient path recovered from /proc.
    // SAFETY: F_GETFL takes no pointer arguments and file owns the descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    ensure!(flags >= 0, "Cannot inspect retained descriptor flags");
    let readable;
    let file = if flags & libc::O_PATH != 0 {
        // SAFETY: the path is a static NUL-terminated '.', relative to the retained fd.
        let fd = unsafe {
            libc::openat(
                file.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return unsupported_or_error(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor.
        readable = unsafe { File::from_raw_fd(fd) };
        &readable
    } else {
        file
    };
    let Some(uuid) = filesystem_uuid(file)? else {
        return Ok(None);
    };
    // The filesystem's export handle includes its object namespace/generation.
    // An inode alone is insufficient for filesystems with multiple subvolumes.
    #[repr(C)]
    struct ExportHandle {
        bytes: u32,
        kind: i32,
        data: [u8; 128],
    }
    let mut handle = ExportHandle {
        bytes: 128,
        kind: 0,
        data: [0; 128],
    };
    let mut mount_id = 0;
    // SAFETY: the buffer starts with the UAPI file_handle header and provides the
    // advertised bounded trailing storage. AT_EMPTY_PATH selects the live descriptor.
    let result = unsafe {
        libc::name_to_handle_at(
            file.as_raw_fd(),
            c"".as_ptr(),
            (&mut handle as *mut ExportHandle).cast(),
            &mut mount_id,
            libc::AT_EMPTY_PATH,
        )
    };
    if result != 0 {
        return unsupported_or_error(std::io::Error::last_os_error());
    }
    ensure!(
        handle.bytes > 0 && handle.bytes <= 128 && handle.kind > 0,
        "Invalid persistent filesystem object handle"
    );
    let mut object = [0; 132];
    let length = handle.bytes as usize;
    object[..4].copy_from_slice(&handle.kind.to_le_bytes());
    object[4..4 + length].copy_from_slice(&handle.data[..length]);
    Ok(Some(digest(
        b"linux-filesystem-export-handle",
        &uuid,
        &object[..4 + length],
        created,
    )))
}

// Linux 6.9 UAPI: fsuuid2 is one length byte followed by sixteen UUID bytes.
#[repr(C)]
struct FilesystemUuid {
    length: u8,
    bytes: [u8; 16],
}
fn filesystem_uuid(file: &File) -> Result<Option<[u8; 16]>> {
    let mut uuid = FilesystemUuid {
        length: 0,
        bytes: [0; 16],
    };
    // SAFETY: the ioctl encodes this fixed output structure's size.
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<FilesystemUuid>(0x15, 0),
            &mut uuid,
        )
    };
    if result == 0 {
        return Ok((uuid.length == 16 && uuid.bytes != [0; 16]).then_some(uuid.bytes));
    }
    let error = std::io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOTTY | libc::ENOTSUP | libc::ENOSYS)
    ) {
        return ext4_uuid(file);
    }
    // Permission and other failures are not permission to use a different source.
    unsupported_or_error(error).map(|_| None)
}

fn filesystem_type(file: &File) -> Result<libc::c_long> {
    // SAFETY: statfs contains only integers and arrays; fstatfs fills the owned output.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::fstatfs(file.as_raw_fd(), &mut stat) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(stat.f_type)
}
fn ext4_uuid(file: &File) -> Result<Option<[u8; 16]>> {
    // The ext-family magic alone does not grant UUID evidence; the ext4 read-only
    // ioctl must also succeed. Never send filesystem-specific requests to other types.
    if filesystem_type(file)? != 0xef53 {
        return Ok(None);
    }
    // EXT4_IOC_GETFSUUID encodes the eight-byte header, not its flexible UUID tail.
    // Linux v6.8 include/uapi/linux/ext4.h and fs/ext4/ioctl.c define this ABI.
    #[repr(C)]
    struct Header {
        length: u32,
        flags: u32,
    }
    #[repr(C)]
    struct Ext4Uuid {
        header: Header,
        bytes: [u8; 16],
    }
    let mut uuid = Ext4Uuid {
        header: Header {
            length: 16,
            flags: 0,
        },
        bytes: [0; 16],
    };
    // SAFETY: the read-only request receives its header followed by 16 writable bytes.
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            libc::_IOR::<Header>(b'f' as u32, 44),
            &mut uuid,
        )
    };
    if result != 0 {
        return unsupported_or_error(std::io::Error::last_os_error()).map(|_| None);
    }
    ensure!(
        uuid.header.length == 16 && uuid.header.flags == 0,
        "Invalid ext4 UUID response"
    );
    Ok((uuid.bytes != [0; 16]).then_some(uuid.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_uuid_matches_handle_bound_ext4_evidence_when_available() {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
        eprintln!("kernel={}", kernel.trim());
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input");
        std::fs::write(&path, b"fixture").unwrap();
        for file in [File::open(temp.path()).unwrap(), File::open(path).unwrap()] {
            let kind = filesystem_type(&file).unwrap();
            let uuid = filesystem_uuid(&file).unwrap();
            let ext4 = ext4_uuid(&file).unwrap();
            eprintln!(
                "filesystem_type={kind:#x}, UUID_available={}, ext4_UUID_available={}",
                uuid.is_some(),
                ext4.is_some()
            );
            assert!(
                uuid.is_some(),
                "native fixture needs persistent UUID evidence (filesystem {kind:#x})"
            );
            if kind == 0xef53 {
                assert_eq!(
                    uuid, ext4,
                    "generic and ext4 interfaces must identify the same filesystem"
                );
            } else {
                assert_eq!(
                    ext4, None,
                    "ext4 query must not be used on another filesystem"
                );
            }
        }
    }
}
