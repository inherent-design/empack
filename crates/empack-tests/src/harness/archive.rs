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
