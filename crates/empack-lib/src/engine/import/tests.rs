use super::*;
use crate::engine::{
    content::verify_stream,
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};
fn mr() -> Value {
    json!({"formatVersion":1,"game":"minecraft","name":"Example","versionId":"1","files":[],"dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.0"}})
}
fn cf() -> Value {
    json!({"manifestVersion":1,"manifestType":"minecraftModpack","name":"Example","version":"1","files":[{"projectID":238222,"fileID":7364663,"required":false}],"minecraft":{"version":"1.21.1","modLoaders":[{"id":"neoforge-21.1.219","primary":true}]},"overrides":"custom"})
}
fn file(path: &str) -> Value {
    json!({"path":path,"fileSize":7,"downloads":["https://example.com/different-name.zip"],"hashes":{"sha1":"11".repeat(20),"sha512":"22".repeat(64)},"env":{"client":"optional","server":"unsupported"}})
}
fn archive(manifest: &str, bytes: &[u8], members: &[(&str, &[u8])]) -> AcquiredContent {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in std::iter::once((manifest, bytes)).chain(members.iter().copied()) {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn inspect_json(name: &str, value: &Value, members: &[(&str, &[u8])]) -> Result<ImportedProject> {
    inspect(
        archive(name, &serde_json::to_vec(value).unwrap(), members),
        ImportLimits::default(),
        &Cancellation::default(),
    )
}
#[test]
fn imports_keep_exact_declarations_layers_and_unknown_optional_defaults() {
    let mut value = mr();
    value["files"] = json!([file("resourcepacks/declared.zip")]);
    let result = inspect_json(
        "modrinth.index.json",
        &value,
        &[
            ("overrides/config/example.toml", b"common"),
            ("client-overrides/config/example.toml", b"client"),
            ("server-overrides/config/example.toml", b"server"),
            ("readme.txt", b"auxiliary"),
        ],
    )
    .unwrap();
    assert_eq!(
        result.files[0].destination.relative().as_str(),
        "resourcepacks/declared.zip"
    );
    assert_eq!(
        result.files[0].requirements,
        ImportedRequirements {
            client: ImportedRequirement::Optional,
            server: ImportedRequirement::Unsupported
        }
    );
    assert_eq!(result.files[0].expected.size, Some(7));
    assert_eq!(
        result.files[0]
            .expected
            .digests
            .as_ref()
            .unwrap()
            .values()
            .len(),
        2
    );
    assert!(result.providers.is_empty());
    assert!(
        matches!(&result.files[0].acquisition, ImportedAcquisition::Downloads(urls) if urls == &["https://example.com/different-name.zip"])
    );
    assert_eq!(result.files[0].source.pointer.as_deref(), Some("/files/0"));
    assert_eq!(result.overrides.len(), 3);
    let mut source = ZipContentSource::open(
        result.archive(),
        ArchiveLimits::default(),
        &Cancellation::default(),
    )
    .unwrap();
    for record in &result.overrides {
        let ImportedAcquisition::Embedded(member) = &record.acquisition else {
            panic!()
        };
        let (content, permissions) = source
            .acquire(
                member,
                &record.expected,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &Cancellation::default(),
            )
            .unwrap();
        let mut bytes = Vec::new();
        content.lease().open().read_to_end(&mut bytes).unwrap();
        assert_eq!(
            bytes,
            match record.layer {
                ContentLayer::Common => b"common",
                ContentLayer::Client => b"client",
                ContentLayer::Server => b"server",
            }
        );
        assert!(permissions.executable);
    }
    assert_eq!(result.auxiliary_members, vec![path("readme.txt").unwrap()]);
    assert_eq!(result.dependency_coverage, Coverage::Unknown);
}
#[test]
fn curseforge_preserves_pins_and_does_not_invent_mod_paths_or_types() {
    let value = cf();
    let result = inspect_json(
        "manifest.json",
        &value,
        &[("custom/resources/config.bin", b"\0binary")],
    )
    .unwrap();
    assert_eq!(result.providers.len(), 1);
    assert_eq!(result.providers[0].selection.project.to_string(), "238222");
    assert_eq!(
        result.providers[0].requirements.client,
        ImportedRequirement::Optional
    );
    assert_eq!(result.runtime.loaders[0].kind, LoaderKind::NeoForge);
    assert_eq!(result.runtime.loaders[0].version.as_str(), "21.1.219");
    assert!(result.runtime.loaders[0].primary);
    assert_eq!(
        result.overrides[0].destination.relative().as_str(),
        "resources/config.bin"
    );
    assert!(result.files.is_empty());
}
#[test]
fn all_runtime_families_and_vanilla_survive_without_silent_loader_selection() {
    for (name, kind) in [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("forge", LoaderKind::Forge),
        ("neoforge", LoaderKind::NeoForge),
    ] {
        let mut value = mr();
        value["dependencies"] = json!({"minecraft":"1.20.1",name:"historical-version"});
        assert_eq!(
            inspect_json("modrinth.index.json", &value, &[])
                .unwrap()
                .runtime
                .loaders[0]
                .kind,
            kind
        );
    }
    let mut value = mr();
    value["dependencies"] = json!({"minecraft":"1.20.1"});
    assert!(
        inspect_json("modrinth.index.json", &value, &[])
            .unwrap()
            .runtime
            .loaders
            .is_empty()
    );
    value["dependencies"] =
        json!({"minecraft":"1.20.1","fabric-loader":"0.16.0","quilt-loader":"0.28.0"});
    assert_eq!(
        inspect_json("modrinth.index.json", &value, &[])
            .unwrap()
            .runtime
            .loaders
            .len(),
        2
    );
    value["dependencies"]["unknown-runtime"] = json!("1");
    assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
}
#[test]
fn declared_content_checks_original_hashes_when_reading_embedded_bytes() {
    let mut value = mr();
    let mut input = file("mods/content.jar");
    input["downloads"] = json!([]);
    input["hashes"] = json!({"md5":"321c3cf486ed509164edec1e1981fec8"});
    value["files"] = json!([input]);
    let result = inspect_json(
        "modrinth.index.json",
        &value,
        &[("mods/content.jar", b"payload")],
    )
    .unwrap();
    assert_eq!(result.diagnostics.len(), 1);
    let file = &result.files[0];
    let ImportedAcquisition::Embedded(member) = &file.acquisition else {
        panic!()
    };
    let mut archive = ZipContentSource::open(
        result.archive(),
        ArchiveLimits::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(
        archive
            .acquire(
                member,
                &file.expected,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                &Cancellation::default()
            )
            .is_ok()
    );
    let other = inspect_json(
        "modrinth.index.json",
        &value,
        &[("mods/content.jar", b"changed")],
    )
    .unwrap();
    let mut archive = ZipContentSource::open(
        other.archive(),
        ArchiveLimits::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(
        archive
            .acquire(
                member,
                &file.expected,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                &Cancellation::default()
            )
            .is_err()
    );
    assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
    assert!(
        inspect_json(
            "modrinth.index.json",
            &value,
            &[("mods/content.jar", b"bad-size")]
        )
        .is_err()
    );
}
#[test]
fn rejects_bad_environments_paths_urls_and_duplicate_destinations_with_record_location() {
    for (key, replacement) in [
        ("path", json!("../escape")),
        ("path", json!("C:/escape")),
        ("env", json!({"client":"typo"})),
        (
            "env",
            json!({"client":"unsupported","server":"unsupported"}),
        ),
        ("hashes", json!({})),
        (
            "downloads",
            json!(["https://secret-marker@example.com/file"]),
        ),
        ("downloads", json!(["https://example.com/illegal space"])),
    ] {
        let mut value = mr();
        let mut input = file("mods/a.jar");
        input[key] = replacement;
        value["files"] = json!([input]);
        let error = inspect_json("modrinth.index.json", &value, &[])
            .err()
            .unwrap();
        let diagnostic = format!("{error:#}");
        assert!(diagnostic.contains("/files/0"));
        assert!(!diagnostic.contains("secret-marker"));
    }
    for second in ["mods/a.jar", "mods/A.jar", "mods/a.jar/child"] {
        let mut value = mr();
        value["files"] = json!([file("mods/a.jar"), file(second)]);
        assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
    }
    assert!(
        inspect_json(
            "modrinth.index.json",
            &mr(),
            &[
                ("overrides/config/A", b"a"),
                ("client-overrides/config/a", b"b")
            ]
        )
        .is_err()
    );
}
#[test]
fn format_roots_versions_and_unselected_archive_members_are_validated() {
    let mut value = mr();
    value["formatVersion"] = json!(2);
    assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
    value = mr();
    value["game"] = json!("another-game");
    assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
    value = mr();
    value["client-overrides"] = json!("overrides/nested");
    assert!(inspect_json("modrinth.index.json", &value, &[]).is_err());
    assert!(inspect_json("modrinth.index.json", &mr(), &[("../unused", b"bad")]).is_err());
    assert!(inspect_json("modrinth.index.json", &mr(), &[("manifest.json", b"{}")]).is_err());
    let unknown = inspect_json("pack.toml", &json!({}), &[]).err().unwrap();
    assert!(
        unknown
            .to_string()
            .contains("recognized but not implemented")
    );
    let mut value = cf();
    let duplicate = value["files"][0].clone();
    value["files"].as_array_mut().unwrap().push(duplicate);
    assert!(inspect_json("manifest.json", &value, &[]).is_err());
}
#[test]
fn duplicate_json_evidence_cannot_be_silently_overwritten() {
    let text = serde_json::to_string(&mr()).unwrap().replace(
        "\"minecraft\":\"1.20.1\"",
        "\"minecraft\":\"1.20.1\",\"minecraft\":\"1.19.4\"",
    );
    assert!(
        inspect(
            archive("modrinth.index.json", text.as_bytes(), &[]),
            ImportLimits::default(),
            &Cancellation::default()
        )
        .is_err()
    );
}
#[tokio::test]
async fn inspection_is_admitted_owned_cancelable_and_retains_its_archive() {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 30,
        scratch_bytes: 1 << 26,
        open_files: 8,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let source = archive(
        "modrinth.index.json",
        &serde_json::to_vec(&mr()).unwrap(),
        &[],
    );
    let id = source.lease().id();
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(inspect_import(&mut scope, source, ImportLimits::default()).await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(Ok(project)) = &*outcome else {
        panic!("import inspection failed")
    };
    assert_eq!(project.source_id(), id);
    assert!(governor.status().reserved.memory_bytes > 0);
    assert_eq!(governor.status().reserved.scratch_bytes, 0);
    drop(outcome);
    assert!(runtime.release_completed(handle.id()));
    drop(handle);
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved.memory_bytes, 0);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        inspect(
            archive(
                "modrinth.index.json",
                &serde_json::to_vec(&mr()).unwrap(),
                &[]
            ),
            ImportLimits::default(),
            &cancel
        )
        .is_err()
    );
}
#[test]
fn limits_apply_before_manifest_reads_and_to_all_normalized_records() {
    for limits in [
        ImportLimits {
            manifest_bytes: 1,
            ..ImportLimits::default()
        },
        ImportLimits {
            archive: ArchiveLimits {
                compressed_bytes: 1,
                ..ArchiveLimits::default()
            },
            ..ImportLimits::default()
        },
        ImportLimits {
            records: 1,
            ..ImportLimits::default()
        },
    ] {
        let source = archive(
            "modrinth.index.json",
            &serde_json::to_vec(&mr()).unwrap(),
            &[("overrides/a", b"a"), ("overrides/b", b"b")],
        );
        assert!(inspect(source, limits, &Cancellation::default()).is_err());
    }
}

#[test]
fn datapack_layout_inference_keeps_competing_evidence_and_ignores_substrings() {
    let result = inspect_json(
        "modrinth.index.json",
        &mr(),
        &[
            ("overrides/config/paxi/datapacks/a.zip", b"paxi"),
            ("overrides/config/openloader/data/b.zip", b"open"),
            ("overrides/datapacks/c.zip", b"root"),
            ("overrides/misleading/config/paxi/datapacks/d.zip", b"other"),
        ],
    )
    .unwrap();
    let proposals = result.datapack_layout_proposals();
    assert_eq!(proposals.len(), 3);
    assert!(proposals.values().all(|evidence| evidence.len() == 1));
    assert_eq!(
        proposals[&path("config/paxi/datapacks").unwrap()][0]
            .member
            .as_str(),
        "overrides/config/paxi/datapacks/a.zip"
    );
}

#[test]
fn inspection_preserves_transient_downloads_without_authorizing_transport_or_persistence() {
    for url in [
        "https://example.com/file?token=secret-marker",
        "http://example.com/file",
    ] {
        let mut value = mr();
        let mut input = file("resourcepacks/declared.zip");
        input["downloads"] = json!([url]);
        value["files"] = json!([input]);
        let imported = inspect_json("modrinth.index.json", &value, &[]).unwrap();
        assert!(
            matches!(&imported.files[0].acquisition,ImportedAcquisition::Downloads(urls) if urls == &[url])
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|d| d.location.pointer.as_deref() == Some("/files/0/downloads/0"))
        );
        assert!(super::super::documents::validate_download_url(url).is_err());
        assert!(!format!("{:?}", imported.diagnostics).contains("secret-marker"));
    }
}
