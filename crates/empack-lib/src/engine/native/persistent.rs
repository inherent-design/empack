//! Durable identity is observed through an opened object, never inferred from a path.
use super::ObjectIdentity;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs::File;

fn digest(namespace: &[u8], volume: &[u8], object: &[u8], created: (u64, u32)) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"empack-persistent-object-v1");
    for field in [namespace, volume, object] {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field);
    }
    hash.update(created.0.to_le_bytes());
    hash.update(created.1.to_le_bytes());
    hash.finalize().into()
}

#[cfg(target_os = "macos")]
pub(super) fn identity(file: &File, observed: &ObjectIdentity) -> Result<Option<[u8; 32]>> {
    use anyhow::ensure;
    use std::os::fd::AsRawFd;
    let Some(created) = observed.created else {
        return Ok(None);
    };
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    #[repr(C)]
    struct Attributes {
        length: u32,
        returned: libc::attribute_set_t,
        uuid: [u8; 16],
    }
    // SAFETY: every field is an integer or byte array; all-zero is a valid value.
    let mut attributes: Attributes = unsafe { std::mem::zeroed() };
    // SAFETY: the file owns a live descriptor, and both buffers have their declared sizes.
    let result = unsafe {
        libc::fgetattrlist(
            file.as_raw_fd(),
            (&mut request as *mut libc::attrlist).cast(),
            (&mut attributes as *mut Attributes).cast(),
            std::mem::size_of::<Attributes>(),
            0,
        )
    };
    if result != 0 {
        return unsupported_or_error(std::io::Error::last_os_error());
    }
    if attributes.returned.volattr & libc::ATTR_VOL_UUID == 0 {
        return Ok(None);
    }
    ensure!(
        attributes.length as usize == std::mem::size_of::<Attributes>(),
        "Unexpected filesystem identity attribute length"
    );
    if attributes.uuid == [0; 16] {
        return Ok(None);
    }
    Ok(Some(digest(
        b"darwin-volume-uuid-inode",
        &attributes.uuid,
        &observed.object.to_le_bytes(),
        created,
    )))
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(super) use linux::identity;

#[cfg(windows)]
pub(super) fn identity(_file: &File, observed: &ObjectIdentity) -> Result<Option<[u8; 32]>> {
    Ok(observed.created.map(|created| {
        digest(
            b"windows-volume-serial-file-id",
            &observed.volume.to_le_bytes(),
            &observed.object.to_le_bytes(),
            created,
        )
    }))
}

#[cfg(unix)]
fn unsupported_or_error(error: std::io::Error) -> Result<Option<[u8; 32]>> {
    match error.raw_os_error() {
        // Missing capability does not prevent live observations. A caller that
        // needs durable evidence must refuse it explicitly instead of downgrading.
        Some(libc::ENOTSUP | libc::ENOTTY | libc::ENOSYS | libc::EACCES | libc::EPERM) => Ok(None),
        _ => Err(error.into()),
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(super) fn identity(_file: &File, _observed: &ObjectIdentity) -> Result<Option<[u8; 32]>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_and_object_namespaces_cannot_alias_equal_object_numbers() {
        let original = digest(b"volume-uuid", &[1; 16], &[2; 16], (3, 4));
        for changed in [
            digest(b"volume-uuid", &[9; 16], &[2; 16], (3, 4)),
            digest(b"volume-uuid", &[1; 16], &[9; 16], (3, 4)),
            digest(b"volume-uuid", &[1; 16], &[2; 16], (3, 5)),
            digest(b"other-kind", &[1; 16], &[2; 16], (3, 4)),
        ] {
            assert_ne!(changed, original);
        }
    }

    #[test]
    fn persistent_observation_follows_retained_object_not_its_path_or_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("original");
        std::fs::write(&path, b"same bytes").unwrap();
        let file = File::open(&path).unwrap();
        let before = crate::engine::native::identity(&file).unwrap();
        assert!(
            before.persistent.is_some(),
            "test filesystem must support durable identity; creation evidence: {:?}",
            before.created
        );
        std::fs::rename(&path, temp.path().join("moved")).unwrap();
        let reopened = File::open(temp.path().join("moved")).unwrap();
        assert_eq!(crate::engine::native::identity(&reopened).unwrap(), before);
        assert_eq!(crate::engine::native::identity(&file).unwrap(), before);
        std::fs::write(&path, b"same bytes").unwrap();
        let replacement = crate::engine::native::identity(&File::open(&path).unwrap()).unwrap();
        assert_ne!(replacement.persistent, before.persistent);
    }
}
