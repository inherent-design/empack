use super::*;
use crate::engine::content::{InitialObservation, SourceEvidencePolicy, verify_stream};
use empack_core::model::GameVersion;
use std::io::{Cursor, Write};
fn content(bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut &*bytes,
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        2 << 20,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn runtime() -> RuntimeResolution {
    RuntimeResolution {
        minecraft: GameVersion::parse("1.20.1").unwrap(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
    }
}
fn jar(manifest: &[u8], include_class: bool) -> AcquiredContent {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "META-INF/MANIFEST.MF",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(manifest).unwrap();
    if include_class {
        zip.start_file(
            "net/minecraft/server/Main.class",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61])
            .unwrap();
    }
    content(&zip.finish().unwrap().into_inner())
}
fn metadata(server: &AcquiredContent, id: &str) -> AcquiredContent {
    let sha1 = server
        .observed_digests()
        .values()
        .iter()
        .find(|digest| digest.algorithm() == empack_core::digest::DigestAlgorithm::Sha1)
        .unwrap()
        .hex();
    content(&serde_json::to_vec(&serde_json::json!({"id":id,"javaVersion":{"majorVersion":17},"downloads":{"server":{"url":"https://piston-data.mojang.com/objects/fixture/server.jar","sha1":sha1,"size":server.lease().len()}}})).unwrap())
}
#[test]
fn vanilla_runtime_binds_version_bytes_and_its_declared_launcher_entry() {
    let server = jar(
        b"Manifest-Version: 1.0\r\nMain-Class: net.minecraft.server.Main\r\n\r\n",
        true,
    );
    let metadata = metadata(&server, "1.20.1");
    assert!(
        plan_from_metadata(
            RuntimeResolution {
                minecraft: GameVersion::parse("1.19").unwrap(),
                ..runtime()
            },
            &metadata
        )
        .is_err()
    );
    let plan = plan_from_metadata(runtime(), &metadata).unwrap();
    let result = plan
        .verify(server, ArchiveLimits::default(), &Cancellation::default())
        .unwrap();
    assert_eq!(result.evidence().java_major, Some(17));
    assert_eq!(result.evidence().main_class, "net.minecraft.server.Main");
    assert_eq!(result.files().len(), 1);
}
#[test]
fn successful_acquisition_does_not_hide_missing_or_unaccounted_launcher_files() {
    for (manifest,class) in [(b"Manifest-Version: 1.0\nMain-Class: net.minecraft.server.Main\n\n".as_slice(),false),
        (b"Manifest-Version: 1.0\nMain-Class: net.minecraft.server.Main\nClass-Path: missing.jar\n\n".as_slice(),true),
        (b"Manifest-Version: 1.0\nMain-Class: bad/path\n\n".as_slice(),true)] {
        let server=jar(manifest,class);
        let plan=plan_from_metadata(runtime(),&metadata(&server,"1.20.1")).unwrap();
        assert!(plan.verify(server,ArchiveLimits::default(),&Cancellation::default()).is_err());
    }
}
#[test]
fn manifest_unfolding_preserves_bytes_and_cannot_override_main_headers_from_sections() {
    let fields=parse_main_attributes(b"Manifest-Version: 1.0\rMain-Class: net.minecraft.\r server.Main\rLabel: \xc3\r \xa9\r\rName: fake\rMain-Class: malicious\r").unwrap();
    assert_eq!(fields["main-class"], "net.minecraft.server.Main");
    assert_eq!(fields["label"], "é");
    for bad in [
        b" dangling\n\n".as_slice(),
        b"Manifest-Version: 1.0\nMain-Class: first\nmain-class: second\n\n".as_slice(),
        b"Manifest-Version: 1.0".as_slice(),
    ] {
        assert!(parse_main_attributes(bad).is_err());
    }
}

fn plan_from_metadata(
    runtime: RuntimeResolution,
    metadata: &AcquiredContent,
) -> Result<VanillaServerPlan> {
    // Synthetic catalog binding for deterministic parsing tests; live resolution uses the official manifest.
    VanillaServerPlan::from_metadata(runtime, metadata, metadata.lease().id())
}

#[test]
fn changed_runtime_bytes_cannot_satisfy_the_selected_metadata() {
    let original = jar(
        b"Manifest-Version: 1.0\nMain-Class: net.minecraft.server.Main\n\n",
        true,
    );
    let plan = plan_from_metadata(runtime(), &metadata(&original, "1.20.1")).unwrap();
    let changed = jar(
        b"Manifest-Version: 1.0\nMain-Class: net.minecraft.server.Main\nLabel: changed\n\n",
        true,
    );
    assert!(
        plan.verify(changed, ArchiveLimits::default(), &Cancellation::default())
            .is_err()
    );
    let mut loader = runtime();
    loader.loader = LoaderKind::Fabric;
    assert!(plan_from_metadata(loader, &metadata(&original, "1.20.1")).is_err());
}
