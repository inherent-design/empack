use super::*;
use crate::engine::{
    archive_source::ZipContentSource,
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
};
use empack_core::model::ExpectedContent;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/archives/zip-headers.json"
    )))
    .unwrap()
}

fn hex(value: &Value) -> Vec<u8> {
    let text = value.as_str().unwrap();
    assert!(text.is_ascii() && text.len().is_multiple_of(2));
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
        .collect()
}

fn check_readers(bytes: &[u8], accepted: bool, label: &str) {
    let limits = ArchiveLimits::default();
    let cancel = Cancellation::default();
    let preflight = preflight_zip(&mut Cursor::new(bytes), bytes.len() as u64, limits, &cancel);
    let expected = BTreeMap::from([(path("empty"), content(b""))]);
    let artifact = verify_archive(
        &mut Cursor::new(bytes),
        DistributionArchive::Zip,
        &expected,
        limits,
        &cancel,
    );
    let acquired = verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &cancel,
    )
    .unwrap();
    let source = ZipContentSource::open(&acquired, limits, &cancel);
    let opaque = ZipContentSource::inspect_opaque(&acquired, limits, &cancel);
    assert_eq!(
        preflight.is_ok(),
        accepted,
        "{label}: preflight={preflight:?}, artifact accepted={}, source accepted={}, opaque accepted={}",
        artifact.is_ok(),
        source.is_ok(),
        opaque.is_ok()
    );
    assert_eq!(artifact.is_ok(), accepted, "{label}: {artifact:?}");
    assert_eq!(source.is_ok(), accepted, "{label}: source reader");
    assert_eq!(opaque.is_ok(), accepted, "{label}: opaque reader");
    if accepted {
        let mut source = source.unwrap();
        assert_eq!(
            source
                .files()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            ["empty"]
        );
        let (member, permissions) = source
            .acquire(
                &path("empty"),
                &ExpectedContent {
                    size: Some(0),
                    digests: None,
                    accepted_observation: None,
                },
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &cancel,
            )
            .unwrap();
        assert_eq!(member.lease().len(), 0);
        assert!(!permissions.executable && !permissions.readonly);
        let opaque = opaque.unwrap();
        assert_eq!(
            opaque.iter().map(|name| name.as_str()).collect::<Vec<_>>(),
            ["empty"]
        );
        let artifact = artifact.unwrap();
        assert_eq!(artifact.members(), 1);
        assert_eq!(artifact.unpacked_bytes(), 0);
        assert!(artifact.directories().is_empty());
    }
}

#[test]
fn literal_zip_headers_agree_across_source_opaque_and_artifact_readers() {
    let corpus = corpus();
    let valid: BTreeMap<_, _> = corpus["valid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["id"].as_str().unwrap(), hex(&row["hex"])))
        .collect();
    for (id, bytes) in &valid {
        check_readers(bytes, true, id);
    }
    for row in corpus["invalid"].as_array().unwrap() {
        let mut bytes = valid[row["base"].as_str().unwrap()].clone();
        if let Some(length) = row["truncate"].as_u64() {
            bytes.truncate(length as usize);
        } else {
            let offset = row["offset"].as_u64().unwrap() as usize;
            let patch = hex(&row["hex"]);
            bytes[offset..offset + patch.len()].copy_from_slice(&patch);
        }
        check_readers(&bytes, false, row["id"].as_str().unwrap());
    }
}

#[test]
fn zip64_extension_data_shares_the_metadata_budget() {
    let corpus = corpus();
    let base = hex(&corpus["valid"][1]["hex"]);
    // Literal fixture: 35-byte local record, 51-byte directory, 56-byte
    // ZIP64 end record, locator, then ordinary end record. The extension is
    // one unknown application data block; it does not change file contents.
    for extension in [6, 16 * 1024 * 1024 - 51, 16 * 1024 * 1024 - 50] {
        let mut bytes = base.clone();
        bytes[90..98].copy_from_slice(&(44 + extension as u64).to_le_bytes());
        let mut data = vec![0; extension];
        data[..2].copy_from_slice(&u16::MAX.to_le_bytes());
        data[2..6].copy_from_slice(&((extension - 6) as u32).to_le_bytes());
        bytes.splice(142..142, data);
        check_readers(
            &bytes,
            extension + 51 <= 16 * 1024 * 1024,
            "ZIP64 extension budget",
        );
    }
}
