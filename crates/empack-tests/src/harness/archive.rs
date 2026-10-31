//! Read artifact bytes independently of the product's inventory and decoder.
use std::{collections::BTreeMap, io::Read, path::Path};

/// Inspect one small fixture member without loading unrelated runtime payloads.
pub fn zip_member(path: &Path, name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let entry = archive.by_name(name).unwrap();
    let expected_size = entry.size();
    bounded_bytes(entry, expected_size)
}

fn bounded_bytes(reader: impl Read, expected_size: u64) -> Vec<u8> {
    assert!(
        expected_size <= 16 << 20,
        "fixture member exceeds admitted bytes"
    );
    let mut bytes = Vec::new();
    reader
        .take(expected_size + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes.len() as u64, expected_size);
    bytes
}

pub fn zip_members(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    assert!(
        archive.len() <= 256,
        "fixture archive exceeds admitted member count"
    );
    let mut members = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).unwrap();
        if entry.is_dir() {
            continue;
        }
        total = total.checked_add(entry.size()).unwrap();
        assert!(total <= 16 << 20, "fixture archive exceeds admitted bytes");
        let name = entry.name().unwrap().into_owned();
        let expected_size = entry.size();
        let bytes = bounded_bytes(entry, expected_size);
        assert!(
            members.insert(name, bytes).is_none(),
            "duplicate archive member"
        );
    }
    members
}

pub fn tar_gz_members(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let input = flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap());
    let mut archive = tar::Archive::new(input);
    let mut members = BTreeMap::new();
    let mut total = 0_u64;
    for (index, entry) in archive.entries().unwrap().enumerate() {
        assert!(index < 256, "fixture archive exceeds admitted member count");
        let entry = entry.unwrap();
        if entry.header().entry_type().is_dir() {
            continue;
        }
        assert!(
            entry.header().entry_type().is_file(),
            "special fixture member"
        );
        let size = entry.size();
        total = total.checked_add(size).unwrap();
        assert!(total <= 16 << 20, "fixture archive exceeds admitted bytes");
        let name = entry.path().unwrap().to_str().unwrap().to_owned();
        assert!(
            members.insert(name, bounded_bytes(entry, size)).is_none(),
            "duplicate archive member"
        );
    }
    // TAR ends before the gzip trailer. Consume bounded padding so the decoder
    // also verifies the compressed stream's final checksum and length.
    let trailing = std::io::copy(
        &mut archive.into_inner().take((1 << 20) + 1),
        &mut std::io::sink(),
    )
    .unwrap();
    assert!(
        trailing <= 1 << 20,
        "fixture archive exceeds admitted padding"
    );
    members
}

/// Streaming inspection of runtime-sized fixture archives. Expectations still
/// come from literal fixtures and independently pinned upstream digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub bytes: u64,
    pub sha256: String,
    pub mode: u32,
    /// Only small metadata/scripts are retained; runtime payloads stay streamed.
    pub small: Option<Vec<u8>>,
}

pub fn distribution_inventory(path: &Path, format: &str) -> BTreeMap<String, Member> {
    use sha2::{Digest, Sha256};
    let mut members = BTreeMap::new();
    let mut total = 0_u64;
    let mut insert = |name: String, size: u64, mode: u32, reader: &mut dyn Read| {
        assert!(members.len() < 256, "fixture member count exceeded");
        total = total.checked_add(size).unwrap();
        assert!(total <= 128 << 20, "fixture decoded bytes exceeded");
        let mut digest = Sha256::new();
        let mut count = 0_u64;
        let mut small = (size <= 1 << 20).then(Vec::new);
        let mut reader = reader.take(size + 1);
        let mut buffer = [0; 65536];
        loop {
            let n = reader.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            count += n as u64;
            digest.update(&buffer[..n]);
            if let Some(bytes) = &mut small {
                bytes.extend_from_slice(&buffer[..n]);
            }
        }
        assert_eq!(count, size);
        let sha256 = digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(
            members
                .insert(
                    name,
                    Member {
                        bytes: size,
                        sha256,
                        mode: mode & 0o777,
                        small
                    }
                )
                .is_none(),
            "duplicate fixture member"
        );
    };
    let file = std::fs::File::open(path).unwrap();
    assert!(
        file.metadata().unwrap().len() <= 128 << 20,
        "fixture compressed bytes exceeded"
    );
    match format {
        "zip" => {
            let mut archive = zip::ZipArchive::new(file).unwrap();
            assert!(archive.len() <= 256);
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).unwrap();
                if entry.is_dir() {
                    continue;
                }
                insert(
                    entry.name().unwrap().into_owned(),
                    entry.size(),
                    entry.unix_mode().unwrap(),
                    &mut entry,
                );
            }
        }
        "tar.gz" => {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
            for (i, entry) in archive.entries().unwrap().enumerate() {
                assert!(i < 256);
                let mut entry = entry.unwrap();
                if entry.header().entry_type().is_dir() {
                    continue;
                }
                assert!(entry.header().entry_type().is_file());
                insert(
                    entry.path().unwrap().to_str().unwrap().into(),
                    entry.size(),
                    entry.header().mode().unwrap(),
                    &mut entry,
                );
            }
            let padding = std::io::copy(
                &mut archive.into_inner().take((1 << 20) + 1),
                &mut std::io::sink(),
            )
            .unwrap();
            assert!(padding <= 1 << 20);
        }
        "7z" => {
            let mut archive =
                sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty()).unwrap();
            assert!(archive.archive().files.len() <= 256);
            archive.set_thread_count(1);
            archive
                .for_each_entries(|entry, reader| {
                    if !entry.is_directory {
                        assert!(!entry.is_anti_item);
                        insert(
                            entry.name.clone(),
                            entry.size,
                            entry.windows_attributes >> 16,
                            reader,
                        );
                    }
                    Ok(true)
                })
                .unwrap();
        }
        _ => panic!("unsupported fixture archive format"),
    }
    members
}
