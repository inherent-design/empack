//! Read artifact bytes independently of the product's inventory and decoder.
use std::{collections::BTreeMap, io::Read, path::Path};

pub fn zip_members(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    assert!(
        archive.len() <= 256,
        "fixture archive exceeds admitted member count"
    );
    let mut members = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        if entry.is_dir() {
            continue;
        }
        total = total.checked_add(entry.size()).unwrap();
        assert!(total <= 16 << 20, "fixture archive exceeds admitted bytes");
        let name = entry.name().unwrap().into_owned();
        let expected_size = entry.size();
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(expected_size + 1)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes.len() as u64, expected_size, "{name}");
        assert!(
            members.insert(name, bytes).is_none(),
            "duplicate archive member"
        );
    }
    members
}
