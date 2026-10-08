use super::*;
use crate::{
    application::{
        BuildArgs,
        session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
    },
    engine::{
        build::BuildAcquisitions,
        documents::DocumentCodec,
        import::{ImportFileDecision, ImportPersistence, ImportedRequirement},
        mrpack::OptionalConversion,
    },
};
use empack_core::{
    inventory::OptionalPolicy,
    model::{ContentKind, DependencyKey, DistributionArchive, DistributionIntent, PackMetadata},
    projection::BuildTarget,
    requirements::{ChoiceKey, OptionalChoice, Requirement, Requirements},
};
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    fs,
    io::{Cursor, Read, Write},
};

fn session(root: &Path, yes: bool, dry_run: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some(root.join("project")),
            state_dir: Some(root.join("state")),
            cache_dir: Some(root.join("cache")),
            yes,
            dry_run,
            ..Default::default()
        }))
}
fn archive(members: &[(&str, &[u8])], declared: bool) -> Vec<u8> {
    let files = if declared {
        vec![json!({
            "path":"resourcepacks/theme.zip", "fileSize":7, "downloads":["https://example.com/theme.zip"],
            "hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(sha2::Sha512::digest(b"payload").into()).hex()},
            "env":{"client":"optional","server":"unsupported"}
        })]
    } else {
        vec![]
    };
    let manifest = serde_json::to_vec(&json!({"formatVersion":1,"game":"minecraft","name":"Imported","versionId":"1","files":files,"dependencies":{"minecraft":"1.21.1"}})).unwrap();
    archive_with_manifest("modrinth.index.json", &manifest, members)
}
fn archive_with_manifest(name: &str, manifest: &[u8], members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in std::iter::once((name, manifest)).chain(members.iter().copied()) {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn local(path: &str) -> ImportSource {
    ImportSource::Local {
        path: path.into(),
        expected: ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
    }
}
fn source(root: &Path, bytes: &[u8]) -> ImportSource {
    fs::write(root.join("source.mrpack"), bytes).unwrap();
    // Source path is invocation-relative even when a different workdir is selected.
    local("source.mrpack")
}
fn request(source: ImportSource, replace: bool) -> ImportHostRequest {
    ImportHostRequest {
        source,
        destination: None,
        replacement: if replace {
            ProjectReplacementPolicy::ReplaceManagedContent
        } else {
            ProjectReplacementPolicy::RejectExisting
        },
        evidence: SourceEvidencePolicy::Compatibility,
        supplied: BTreeMap::new(),
        local_files: Vec::new(),
    }
}
fn decisions(content: &VerifiedImportContent) -> Result<ImportCandidateOptions> {
    let source = content.plan().imported();
    let files = content
        .content()
        .keys()
        .enumerate()
        .map(|(index, key)| {
            let (file, kind) = match key {
                ImportContentKey::Declared(index) => {
                    (&source.files[*index], ContentKind::ResourcePack)
                }
                ImportContentKey::Override(index) => {
                    (&source.overrides[*index], ContentKind::Config)
                }
                _ => panic!("fixture has no provider references"),
            };
            let label = format!("file-{index}");
            let requirement = |value| match value {
                ImportedRequirement::Required => Requirement::Required,
                ImportedRequirement::Unsupported => Requirement::Unsupported,
                ImportedRequirement::Optional => Requirement::Optional(OptionalChoice {
                    key: ChoiceKey::parse(&label).unwrap(),
                    default_enabled: true,
                    description: Some("Selected by the import fixture".into()),
                }),
            };
            (
                key.clone(),
                ImportFileDecision {
                    key: DependencyKey::parse(&label).unwrap(),
                    kind,
                    requirements: Requirements {
                        client: requirement(file.requirements.client),
                        server: requirement(file.requirements.server),
                    },
                    persistence: if matches!(key, ImportContentKey::Declared(_)) {
                        ImportPersistence::Url
                    } else {
                        ImportPersistence::Local
                    },
                    provider_destination: None,
                },
            )
        })
        .collect();
    Ok(ImportCandidateOptions {
        metadata: PackMetadata {
            name: "Imported".into(),
            version: "1".into(),
            author: None,
            description: None,
        },
        loader: None,
        acceptable_versions: vec![],
        layout: BTreeMap::new(),
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![BuildTarget::Mrpack])?,
            archive: DistributionArchive::Zip,
        },
        files,
        exclude_auxiliary_members: false,
    })
}
fn observed(mut bytes: &[u8]) -> AcquiredContent {
    let maximum = bytes.len() as u64;
    crate::engine::content::verify_stream(
        &mut bytes,
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        maximum,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap()
}
fn read(root: &Path) -> empack_core::model::ResolvedProject {
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.join("empack.yml")).unwrap(), "test")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap()
}
fn member(archive: &mut zip::ZipArchive<fs::File>, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive
        .by_name(path)
        .unwrap_or_else(|error| panic!("Missing archive member {path}: {error}"))
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
#[tokio::test]
async fn native_import_preserves_layers_and_optional_choices_through_build_and_export() {
    let root = tempfile::tempdir().unwrap();
    let bytes = archive(
        &[
            ("overrides/config/example.toml", b"common"),
            ("client-overrides/config/example.toml", b"client"),
            ("server-overrides/config/example.toml", b"server"),
        ],
        true,
    );
    let mut selected = request(source(root.path(), &bytes), false);
    selected
        .supplied
        .insert(ImportContentKey::Declared(0), observed(b"payload"));
    import(&session(root.path(), true, false), selected, decisions)
        .await
        .unwrap();
    let cached = root.path().join("cache/content-v1").join(format!(
        "{}.blob",
        empack_core::digest::ExpectedDigest::Sha256(sha2::Sha256::digest(b"payload").into()).hex()
    ));
    assert_eq!(
        fs::read(cached).expect("approved imported payload must enter cache"),
        b"payload"
    );
    let mut restored = request(local("source.mrpack"), false);
    restored.destination = Some("from-cache".into());
    import(&session(root.path(), true, false), restored, decisions)
        .await
        .unwrap();
    assert_eq!(
        fs::read(
            root.path()
                .join("project/from-cache/pack/resourcepacks/theme.zip")
        )
        .unwrap(),
        b"payload"
    );
    let project = root.path().join("project");
    for (layer, expected) in [
        ("common", b"common".as_slice()),
        ("client", b"client"),
        ("server", b"server"),
    ] {
        assert_eq!(
            fs::read(project.join(format!("overrides/{layer}/config/example.toml"))).unwrap(),
            expected
        );
    }
    let resolved = read(&project);
    let optional = resolved
        .lock()
        .dependencies
        .values()
        .flat_map(|dep| dep.files.as_slice())
        .flat_map(|file| file.placements.as_slice())
        .find(|placement| placement.destination.relative().as_str() == "resourcepacks/theme.zip")
        .unwrap();
    assert!(matches!(
        optional.requirements.client,
        Requirement::Optional(_)
    ));
    assert_eq!(optional.requirements.server, Requirement::Unsupported);
    let decisions = super::super::BuildDecisions {
        optional: OptionalPolicy::Resolve {
            choices: BTreeMap::new(),
            use_defaults: true,
        },
        mrpack_optional: OptionalConversion::AcknowledgedMetadataLoss,
        ..Default::default()
    };
    super::super::build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["client-full".into()],
            ..Default::default()
        },
        decisions,
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut client = zip::ZipArchive::new(
        fs::File::open(project.join("dist/Imported-1-client-full.zip")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        member(&mut client, ".minecraft/config/example.toml"),
        b"client"
    );
    assert_eq!(
        member(&mut client, ".minecraft/resourcepacks/theme.zip"),
        b"payload"
    );
    super::super::build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["mrpack".into()],
            ..Default::default()
        },
        super::super::BuildDecisions {
            mrpack_optional: OptionalConversion::AcknowledgedMetadataLoss,
            ..Default::default()
        },
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let mut export =
        zip::ZipArchive::new(fs::File::open(project.join("dist/Imported-1.mrpack")).unwrap())
            .unwrap();
    // Both environments replace the common layer, so export contains the two effective
    // views while the original common bytes remain in the editable project.
    assert!(export.by_name("overrides/config/example.toml").is_err());
    for (layer, expected) in [
        ("client-overrides", b"client".as_slice()),
        ("server-overrides", b"server"),
    ] {
        assert_eq!(
            member(&mut export, &format!("{layer}/config/example.toml")),
            expected
        );
    }
    let index: Value = serde_json::from_slice(&member(&mut export, "modrinth.index.json")).unwrap();
    assert_eq!(index["files"][0]["path"], "resourcepacks/theme.zip");
    assert_eq!(index["files"][0]["env"]["client"], "optional");
    assert_eq!(index["files"][0]["env"]["server"], "unsupported");
    assert_eq!(
        index["files"][0]["downloads"][0],
        "https://example.com/theme.zip"
    );
    assert_eq!(
        DocumentCodec.encode_lock(&read(&project)).unwrap(),
        DocumentCodec.encode_lock(&resolved).unwrap(),
        "Build conversion must not rewrite imported intent"
    );
}
#[tokio::test]
async fn import_preview_decline_and_failed_replacement_preserve_the_entire_tree() {
    let root = tempfile::tempdir().unwrap();
    let bytes = archive(&[("overrides/config/example.toml", b"new")], false);
    let input = source(root.path(), &bytes);
    let before = super::super::tests::snapshot(root.path());
    import(
        &session(root.path(), true, true),
        request(input, true),
        decisions,
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    import(
        &session(root.path(), false, false),
        request(local("source.mrpack"), true),
        decisions,
    )
    .await
    .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    fs::create_dir_all(root.path().join("project/pack/config")).unwrap();
    fs::write(root.path().join("project/pack/config/old"), b"old").unwrap();
    fs::write(root.path().join("project/notes.txt"), b"user notes").unwrap();
    let before = super::super::tests::snapshot(root.path());
    assert!(
        import(
            &session(root.path(), true, false),
            request(local("source.mrpack"), false),
            decisions
        )
        .await
        .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    assert!(
        import(
            &session(root.path(), true, false),
            request(local("source.mrpack"), true),
            |_| anyhow::bail!("conversion declined")
        )
        .await
        .is_err()
    );
    assert_eq!(super::super::tests::snapshot(root.path()), before);
    import(
        &session(root.path(), true, false),
        request(local("source.mrpack"), true),
        decisions,
    )
    .await
    .unwrap();
    assert!(!root.path().join("project/pack/config/old").exists());
    assert_eq!(
        fs::read(root.path().join("project/notes.txt")).unwrap(),
        b"user notes"
    );
}

#[tokio::test]
async fn remote_archive_acquisition_enforces_bytes_and_source_assertions_before_replacement() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("project/pack")).unwrap();
    fs::write(root.path().join("project/pack/keep"), b"keep").unwrap();
    let original = super::super::tests::snapshot(root.path());
    let mut server = mockito::Server::new_async().await;
    let bytes = archive(&[("overrides/config/a", b"downloaded")], false);
    let download = server
        .mock("GET", "/source")
        .with_body(bytes.clone())
        .expect(3)
        .create_async()
        .await;
    for mode in ["limit", "digest", "valid"] {
        let limits = ImportLimits {
            archive: ArchiveLimits {
                compressed_bytes: if mode == "limit" { 16 } else { 4096 },
                ..Default::default()
            },
            ..Default::default()
        };
        let expected = ExpectedContent {
            digests: if mode == "digest" {
                Some(
                    empack_core::digest::DigestSet::parse([(
                        "sha256",
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    )])
                    .unwrap(),
                )
            } else {
                None
            },
            size: None,
            accepted_observation: None,
        };
        let result = import_with_services(
            &session(root.path(), true, false),
            request(
                ImportSource::Download {
                    alternatives: NonEmpty::new(vec![format!("{}/source", server.url())]).unwrap(),
                    expected,
                },
                true,
            ),
            decisions,
            ProviderCatalog::for_loopback_tests(&server.url(), None),
            HttpAcquisition::for_loopback_tests(),
            limits,
        )
        .await;
        if mode == "valid" {
            result.unwrap();
            assert_eq!(
                fs::read(root.path().join("project/overrides/common/config/a")).unwrap(),
                b"downloaded"
            );
        } else {
            assert!(result.is_err(), "{mode} must reject the source archive");
            assert_eq!(super::super::tests::snapshot(root.path()), original);
        }
    }
    download.assert_async().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_import_source_and_destination_ancestors_preserve_outside_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let bytes = archive(&[("overrides/config/a", b"new")], false);
    source(root.path(), &bytes);
    fs::write(outside.path().join("sentinel"), b"outside").unwrap();
    std::os::unix::fs::symlink(
        root.path().join("source.mrpack"),
        root.path().join("linked.mrpack"),
    )
    .unwrap();
    assert!(
        import(
            &session(root.path(), true, false),
            request(local("linked.mrpack"), true),
            decisions
        )
        .await
        .is_err()
    );
    assert!(!root.path().join("project").exists());
    fs::create_dir_all(root.path().join("project/overrides/common")).unwrap();
    std::os::unix::fs::symlink(
        outside.path(),
        root.path().join("project/overrides/common/config"),
    )
    .unwrap();
    assert!(
        import(
            &session(root.path(), true, false),
            request(local("source.mrpack"), true),
            decisions
        )
        .await
        .is_err()
    );
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside"
    );
    assert!(!outside.path().join("a").exists());
    assert!(!root.path().join("state").exists());
}

#[tokio::test]
async fn chunked_archive_limit_rejects_without_waiting_for_the_rest_of_the_body() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("project/pack")).unwrap();
    fs::write(root.path().join("project/pack/keep"), b"keep").unwrap();
    let original = super::super::tests::snapshot(root.path());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut input = [0; 4096];
        assert!(stream.read(&mut input).unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n12345678\r\n")
            .unwrap();
        // No final chunk: rejection must happen before EOF, and retire the connection.
        stream.read(&mut input)
    });
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        import_with_services(
            &session(root.path(), true, false),
            request(
                ImportSource::Download {
                    alternatives: NonEmpty::new(vec![format!("http://{address}/source")]).unwrap(),
                    expected: ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                },
                true,
            ),
            |_| panic!("oversized archive must not reach decisions"),
            ProviderCatalog::for_loopback_tests(&format!("http://{address}"), None),
            HttpAcquisition::for_loopback_tests(),
            ImportLimits {
                archive: ArchiveLimits {
                    compressed_bytes: 7,
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        result.unwrap_err().downcast_ref(),
        Some(crate::engine::acquisition::TransferError::ByteLimit)
    ));
    assert_eq!(worker.join().unwrap().unwrap(), 0);
    assert_eq!(super::super::tests::snapshot(root.path()), original);
}

#[tokio::test]
async fn restricted_curseforge_import_requires_explicit_bytes_before_any_publication() {
    use empack_core::{
        identity::{CurseForgeProjectId, ProviderProjectId},
        model::ResolvedPin,
        path::InstallDestination,
    };
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let manifest = serde_json::to_vec(&json!({"manifestVersion":1,"manifestType":"minecraftModpack","name":"Imported","version":"1","files":[{"projectID":123,"fileID":456,"required":false}],"minecraft":{"version":"1.21.1","modLoaders":[]},"overrides":"overrides"})).unwrap();
    let bytes = archive_with_manifest("manifest.json", &manifest, &[]);
    source(root.path(), &bytes);
    let project = server
        .mock("GET", "/mods/123")
        .with_body(
            json!({"data":{"id":123,"gameId":432,"classId":6,"slug":"fixture","name":"Fixture"}})
                .to_string(),
        )
        .expect(5)
        .create_async()
        .await;
    let metadata_file = server.mock("GET", "/mods/123/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":123,"fileName":"fixture.jar","fileLength":7,"hashes":[{"algo":2,"value":"321c3cf486ed509164edec1e1981fec8"}],"downloadUrl":null,"gameVersions":["1.21.1"],"dependencies":[]}}).to_string()).expect(5).create_async().await;
    let original = super::super::tests::snapshot(root.path());
    let preview = import_with_services(
        &session(root.path(), true, true),
        request(local("source.mrpack"), false),
        |_| panic!("restricted content must stop before conversion"),
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        HttpAcquisition::for_loopback_tests(),
        ImportLimits::default(),
    )
    .await;
    assert!(preview.is_err());
    assert_eq!(super::super::tests::snapshot(root.path()), original);
    let result = import_with_services(
        &session(root.path(), true, false),
        request(local("source.mrpack"), false),
        |_| panic!("restricted content must stop before conversion"),
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        HttpAcquisition::for_loopback_tests(),
        ImportLimits::default(),
    )
    .await;
    assert!(
        format!("{:#}", result.unwrap_err()).contains("content obligations need explicit input")
    );
    assert!(!root.path().join("project").exists());
    let records = || {
        fs::read_dir(root.path().join("state/pending-imports"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"))
            .collect::<Vec<_>>()
    };
    assert_eq!(records().len(), 1);
    let saved_path = records().pop().unwrap();
    let pending_tree = super::super::tests::snapshot(root.path());
    super::super::clean(&session(root.path(), true, true), &["import".into()])
        .await
        .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), pending_tree);
    fs::remove_file(root.path().join("source.mrpack")).unwrap();
    let identity = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
    let pin = ResolvedPin {
        selection: identity.parse_pin("456").unwrap(),
        project: identity,
    };
    let key = ImportContentKey::Provider {
        pin,
        filename: "fixture.jar".into(),
    };
    let before_resume = super::super::tests::snapshot(root.path());
    let preview = import_with_services(
        &session(root.path(), true, true),
        request(ImportSource::Saved, false),
        |_| panic!("pending preview must not convert"),
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        HttpAcquisition::for_loopback_tests(),
        ImportLimits::default(),
    )
    .await;
    assert!(preview.is_err());
    assert_eq!(super::super::tests::snapshot(root.path()), before_resume);
    let mut wrong = request(ImportSource::Saved, false);
    wrong.supplied.insert(
        key.clone(),
        crate::engine::content::verify_stream(
            &mut b"changed".as_slice(),
            &ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            7,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap(),
    );
    let wrong = import_with_services(
        &session(root.path(), true, false),
        wrong,
        |_| panic!("incorrect bytes must not reach conversion"),
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        HttpAcquisition::for_loopback_tests(),
        ImportLimits::default(),
    )
    .await;
    assert!(wrong.is_err());
    assert_eq!(super::super::tests::snapshot(root.path()), before_resume);
    let mut selected = request(ImportSource::Saved, false);
    selected.supplied.insert(
        key.clone(),
        crate::engine::content::verify_stream(
            &mut b"payload".as_slice(),
            &ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            7,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap(),
    );
    import_with_services(
        &session(root.path(), true, false),
        selected,
        move |content| {
            assert_eq!(content.content().len(), 1);
            let mut options = ImportCandidateOptions {
                metadata: PackMetadata {
                    name: "Imported".into(),
                    version: "1".into(),
                    author: None,
                    description: None,
                },
                loader: None,
                acceptable_versions: vec![],
                layout: BTreeMap::new(),
                distribution: DistributionIntent {
                    targets: NonEmpty::new(vec![BuildTarget::Mrpack])?,
                    archive: DistributionArchive::Zip,
                },
                files: BTreeMap::new(),
                exclude_auxiliary_members: false,
            };
            let optional = Requirement::Optional(OptionalChoice {
                key: ChoiceKey::parse("fixture")?,
                default_enabled: false,
                description: Some("Imported optional file".into()),
            });
            options.files.insert(
                key,
                ImportFileDecision {
                    key: DependencyKey::parse("fixture")?,
                    kind: ContentKind::Mod,
                    requirements: Requirements {
                        client: optional.clone(),
                        server: optional,
                    },
                    persistence: ImportPersistence::Provider,
                    provider_destination: Some(InstallDestination::parse("mods/fixture.jar")?),
                },
            );
            Ok(options)
        },
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        HttpAcquisition::for_loopback_tests(),
        ImportLimits::default(),
    )
    .await
    .unwrap();
    let resolved = read(&root.path().join("project"));
    let file = &resolved.lock().dependencies[&DependencyKey::parse("fixture").unwrap()]
        .files
        .as_slice()[0];
    assert!(matches!(
        file.placements.as_slice()[0].requirements.client,
        Requirement::Optional(_)
    ));
    assert_eq!(
        file.expected.digests.as_ref().unwrap().values()[0].algorithm(),
        empack_core::digest::DigestAlgorithm::Md5
    );
    assert_eq!(
        fs::read(root.path().join("project/pack/mods/fixture.jar")).unwrap(),
        b"payload"
    );
    assert!(
        records().is_empty(),
        "successful import conditionally discards its saved record"
    );
    // Invalid saved data is never parsed as cleanup authority.
    fs::write(&saved_path, b"invalid record").unwrap();
    let before_cleanup = super::super::tests::snapshot(root.path());
    super::super::clean(&session(root.path(), true, true), &["import".into()])
        .await
        .unwrap();
    assert_eq!(super::super::tests::snapshot(root.path()), before_cleanup);
    super::super::clean(&session(root.path(), true, false), &["import".into()])
        .await
        .unwrap();
    let mut expected = before_cleanup;
    expected.remove(&saved_path.strip_prefix(root.path()).unwrap().to_path_buf());
    assert_eq!(super::super::tests::snapshot(root.path()), expected);
    project.assert_async().await;
    metadata_file.assert_async().await;
}

#[tokio::test]
async fn local_archive_can_use_declared_strong_source_evidence() {
    use empack_core::digest::{DigestSet, ExpectedDigest};
    let root = tempfile::tempdir().unwrap();
    let bytes = archive(&[], false);
    source(root.path(), &bytes);
    let known_digest = ExpectedDigest::Sha512(sha2::Sha512::digest(&bytes).into());
    let before = super::super::tests::snapshot(root.path());
    for mode in ["missing", "weak", "wrong-digest", "wrong-size", "valid"] {
        let digests = match mode {
            "missing" => None,
            "weak" => Some(
                DigestSet::new(vec![ExpectedDigest::Sha1(
                    sha1::Sha1::digest(&bytes).into(),
                )])
                .unwrap(),
            ),
            "wrong-digest" => Some(DigestSet::new(vec![ExpectedDigest::Sha512([0; 64])]).unwrap()),
            _ => Some(DigestSet::new(vec![known_digest.clone()]).unwrap()),
        };
        let input = ImportSource::Local {
            path: "source.mrpack".into(),
            expected: ExpectedContent {
                digests,
                size: Some(bytes.len() as u64 + u64::from(mode == "wrong-size")),
                accepted_observation: None,
            },
        };
        let mut selected = request(input, false);
        selected.evidence = SourceEvidencePolicy::StrongSourceRequired;
        let result = import(&session(root.path(), true, false), selected, decisions).await;
        if mode == "valid" {
            result.unwrap();
            assert_eq!(
                read(&root.path().join("project")).intent().metadata.name,
                "Imported"
            );
        } else {
            assert!(result.is_err(), "{mode} evidence must not authorize import");
            assert_eq!(super::super::tests::snapshot(root.path()), before);
        }
    }
}

#[tokio::test]
async fn provider_modpack_pages_reach_verified_import_and_reject_changed_archive_bytes() {
    use empack_core::{digest::ExpectedDigest, model::ProviderKind};
    for provider in [ProviderKind::Modrinth, ProviderKind::CurseForge] {
        for changed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("project/pack")).unwrap();
            fs::write(root.path().join("project/pack/old"), b"keep on failure").unwrap();
            let original = super::super::tests::snapshot(root.path());
            let mut server = mockito::Server::new_async().await;
            let bytes = archive(&[("overrides/config/new", b"imported")], false);
            let url = format!("{}/archive", server.url());
            let selector = match provider {
                ProviderKind::Modrinth => {
                    server.mock("GET", "/project/a-pack").with_body(json!({"id":"Pack0001","slug":"a-pack","title":"A pack","project_type":"modpack"}).to_string()).create_async().await;
                    server.mock("GET", "/project/Pack0001/version/1.0").with_body(json!({"id":"File0001","project_id":"Pack0001","version_number":"1.0","version_type":"release","status":"listed","date_published":"2025-01-01T00:00:00Z","files":[{"filename":"pack.mrpack","primary":true,"size":bytes.len(),"hashes":{"sha512":ExpectedDigest::Sha512(sha2::Sha512::digest(&bytes).into()).hex()},"url":url}],"game_versions":["1.21.1"],"loaders":[],"dependencies":[]}).to_string()).create_async().await;
                    ModpackSelector::parse(
                        provider,
                        "https://modrinth.com/modpack/a-pack/version/1.0",
                    )
                    .unwrap()
                }
                ProviderKind::CurseForge => {
                    server.mock("GET", "/mods/1001").with_body(json!({"data":{"id":1001,"slug":"a-pack","name":"A pack","classId":4471,"gameId":432}}).to_string()).create_async().await;
                    server.mock("GET", "/mods/1001/files/2001").with_body(json!({"data":{"id":2001,"modId":1001,"gameId":432,"fileName":"pack.zip","fileLength":bytes.len(),"hashes":[{"algo":2,"value":ExpectedDigest::Md5(md5::Md5::digest(&bytes).into()).hex()}],"downloadUrl":url,"gameVersions":["1.21.1"],"dependencies":[],"releaseType":1,"fileDate":"2025-01-01T00:00:00Z","isAvailable":true}}).to_string()).create_async().await;
                    ModpackSelector::parse(provider, "1001")
                        .unwrap()
                        .with_version("2001")
                        .unwrap()
                }
            };
            let mut body = bytes;
            if changed {
                body[0] ^= 1;
            }
            let payload = server
                .mock("GET", "/archive")
                .with_body(body)
                .expect(1)
                .create_async()
                .await;
            let result = import_with_services(
                &session(root.path(), true, false),
                request(
                    ImportSource::Provider {
                        selector,
                        releases: ReleasePolicy::PreferStable,
                        supplied_archive: None,
                    },
                    true,
                ),
                decisions,
                ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
                HttpAcquisition::for_loopback_tests(),
                ImportLimits::default(),
            )
            .await;
            if changed {
                assert!(
                    result.is_err(),
                    "Changed provider archive must not be accepted"
                );
                assert_eq!(super::super::tests::snapshot(root.path()), original);
            } else {
                result.unwrap();
                assert_eq!(
                    fs::read(root.path().join("project/overrides/common/config/new")).unwrap(),
                    b"imported"
                );
                assert!(!root.path().join("project/pack/old").exists());
            }
            payload.assert_async().await;
        }
    }
}

#[tokio::test]
async fn restricted_provider_archive_accepts_only_matching_explicit_manual_bytes() {
    use empack_core::{digest::ExpectedDigest, model::ProviderKind};
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("project/pack")).unwrap();
    fs::write(
        root.path().join("project/pack/old"),
        b"retain until verified",
    )
    .unwrap();
    let bytes = archive(&[("overrides/config/new", b"manual archive")], false);
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/mods/1001")
        .with_body(
            json!({"data":{"id":1001,"slug":"a-pack","name":"A pack","classId":4471,"gameId":432}})
                .to_string(),
        )
        .create_async()
        .await;
    server.mock("GET","/mods/1001/files/2001").with_body(json!({"data":{"id":2001,"modId":1001,"gameId":432,"fileName":"pack.zip","fileLength":bytes.len(),"hashes":[{"algo":2,"value":ExpectedDigest::Md5(md5::Md5::digest(&bytes).into()).hex()}],"downloadUrl":null,"gameVersions":["1.21.1"],"dependencies":[],"releaseType":1,"fileDate":"2025-01-01T00:00:00Z","isAvailable":true}}).to_string()).create_async().await;
    for mode in ["missing", "changed", "preview", "valid"] {
        let mut manual = bytes.clone();
        if mode == "changed" {
            manual[0] ^= 1;
        }
        fs::write(root.path().join("manual.zip"), manual).unwrap();
        let before = super::super::tests::snapshot(root.path());
        let source = ImportSource::Provider {
            selector: ModpackSelector::parse(ProviderKind::CurseForge, "1001")
                .unwrap()
                .with_version("2001")
                .unwrap(),
            releases: ReleasePolicy::PreferStable,
            supplied_archive: (mode != "missing").then(|| "manual.zip".into()),
        };
        let result = import_with_services(
            &session(root.path(), true, mode == "preview"),
            request(source, true),
            decisions,
            ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
            HttpAcquisition::for_loopback_tests(),
            ImportLimits::default(),
        )
        .await;
        match mode {
            "valid" => {
                result.unwrap();
                assert_eq!(
                    fs::read(root.path().join("project/overrides/common/config/new")).unwrap(),
                    b"manual archive"
                );
            }
            "preview" => {
                result.unwrap();
                assert_eq!(super::super::tests::snapshot(root.path()), before);
            }
            _ => {
                assert!(result.is_err());
                assert_eq!(super::super::tests::snapshot(root.path()), before);
            }
        }
    }
}
