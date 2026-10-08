use super::*;
use empack_core::{
    identity::CurseForgeFileId,
    model::*,
    path::InstallDestination,
    requirements::{Requirement, Requirements},
};
use std::io::{Cursor, Read, Write};

#[tokio::test]
async fn provider_world_keeps_archive_ownership_across_preview_sync_update_build_adopt_remove() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut source = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("ProviderName/level.dat", b"world".as_slice()),
        ("ProviderName/region/r.0.0.mca", b"region".as_slice()),
    ] {
        source
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        source.write_all(bytes).unwrap();
    }
    let bytes = source.finish().unwrap().into_inner();
    let mut server = mockito::Server::new_async().await;
    server.mock("GET","/mods/42").with_body(json!({"data":{"id":42,"gameId":432,"classId":17,"slug":"provider-world","name":"Provider World"}}).to_string()).create_async().await;
    server.mock("GET","/mods/42/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":42,"fileName":"world-v1.zip","fileLength":bytes.len(),"downloadUrl":format!("{}/world.zip",server.url()),"hashes":[{"algo":2,"value":md5::Md5::digest(&bytes).iter().map(|byte|format!("{byte:02x}")).collect::<String>()}],"gameVersions":["1.21.1"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z"}}).to_string()).create_async().await;
    let download = server
        .mock("GET", "/world.zip")
        .with_body(bytes)
        .expect_at_least(2)
        .create_async()
        .await;
    let mut next = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("RenamedRoot/level.dat", b"new world".as_slice()),
        ("RenamedRoot/region/r.1.0.mca", b"new region".as_slice()),
    ] {
        next.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        next.write_all(bytes).unwrap();
    }
    let next = next.finish().unwrap().into_inner();
    let next_file = json!({"id":457,"gameId":432,"modId":42,"fileName":"world-v2.zip","fileLength":next.len(),"downloadUrl":format!("{}/next.zip",server.url()),"hashes":[{"algo":2,"value":md5::Md5::digest(&next).iter().map(|byte|format!("{byte:02x}")).collect::<String>()}],"gameVersions":["1.21.1"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-02-01T00:00:00Z"});
    server
        .mock("GET", "/mods/42/files/457")
        .with_body(json!({"data":next_file.clone()}).to_string())
        .create_async()
        .await;
    server.mock("GET","/mods/42/files").match_query(mockito::Matcher::Any).with_body(json!({"data":[next_file],"pagination":{"index":0,"pageSize":50,"resultCount":1,"totalCount":1}}).to_string()).create_async().await;
    let next_download = server
        .mock("GET", "/next.zip")
        .with_body(next)
        .expect_at_least(2)
        .create_async()
        .await;
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits {
            archive: crate::engine::artifacts::ArchiveLimits {
                entries: 32,
                total_bytes: 4096,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let input = || {
        NonEmpty::new(vec![AddHostInput::Provider(ProviderAddInput {
            selector: ProjectSelector::parse(ProviderKind::CurseForge, "42").unwrap(),
            key: Some(DependencyKey::parse("adventure").unwrap()),
            kind: Some(ContentKind::World),
            pin: Some(PinSelector::CurseForgeFile(
                CurseForgeFileId::parse("456").unwrap(),
            )),
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Unsupported,
            },
            folder: None,
            files: ProviderFiles::PrimaryPlaced(
                NonEmpty::new(vec![Placement {
                    destination: InstallDestination::parse("saves/MyWorld").unwrap(),
                    layer: ContentLayer::Common,
                    requirements: Requirements {
                        client: Requirement::Required,
                        server: Requirement::Unsupported,
                    },
                }])
                .unwrap(),
            ),
        })])
        .unwrap()
    };
    let before = super::super::super::tests::snapshot(root.path());
    dependencies::add_with_services(
        &session(root.path(), true),
        input(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::RejectExisting,
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::super::tests::snapshot(root.path()), before);
    dependencies::add_with_services(
        &session(root.path(), false),
        input(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::RejectExisting,
        services(),
    )
    .await
    .unwrap();
    let world = root.path().join("project/pack/saves/MyWorld");
    assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"world");
    let key = DependencyKey::parse("adventure").unwrap();
    let selected = project(root.path());
    assert!(matches!(
        selected.lock().dependencies[&key].identity,
        ResolvedIdentity::Provider(_)
    ));
    for file in selected.lock().dependencies[&key].files.as_slice() {
        let AcquisitionSpec::ProviderArchiveMember { archive, .. } = &file.acquisition else {
            panic!("Lost world source")
        };
        assert!(
            archive
                .expected
                .digests
                .as_ref()
                .unwrap()
                .values()
                .iter()
                .all(|digest| digest.algorithm() == empack_core::digest::DigestAlgorithm::Md5)
        );
        assert!(file.expected.digests.is_none());
    }
    for _ in 0..2 {
        let before = super::super::super::tests::snapshot(&root.path().join("project"));
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
        assert_eq!(
            super::super::super::tests::snapshot(&root.path().join("project")),
            before
        );
    }
    // Unpin through authored intent, reconcile without upgrading, then explicitly update.
    let mut intent = project(root.path()).intent().clone();
    intent.roots.get_mut(&key).unwrap().version = VersionIntent::FollowCompatible;
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        false,
        services(),
        crate::engine::runtime_catalog::RuntimeCatalog::new(HttpAcquisition::for_loopback_tests()),
    )
    .await
    .unwrap();
    assert_eq!(
        project(root.path()).lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap())
    );
    fs::write(world.join("untracked.txt"), b"keep").unwrap();
    update::update_with_services(
        &session(root.path(), false),
        vec!["adventure".into()],
        services(),
    )
    .await
    .unwrap();
    assert!(!world.join("region/r.0.0.mca").exists());
    assert_eq!(
        fs::read(world.join("region/r.1.0.mca")).unwrap(),
        b"new region"
    );
    assert_eq!(fs::read(world.join("untracked.txt")).unwrap(), b"keep");
    assert_eq!(
        project(root.path()).lock().dependencies[&key]
            .selected
            .as_ref()
            .unwrap()
            .selection,
        PinSelector::CurseForgeFile(CurseForgeFileId::parse("457").unwrap())
    );
    update::adopt_with_services(
        &session(root.path(), false),
        vec!["adventure".into()],
        services(),
    )
    .await
    .unwrap();
    build(
        &session(root.path(), false),
        &crate::application::BuildArgs {
            targets: vec!["client-full".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let artifact = fs::read_dir(root.path().join("project/dist"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(artifact).unwrap()).unwrap();
    let mut region = Vec::new();
    archive
        .by_name(".minecraft/saves/MyWorld/region/r.1.0.mca")
        .unwrap()
        .read_to_end(&mut region)
        .unwrap();
    assert_eq!(region, b"new region");
    crate::application::execute_command_with_session(
        crate::application::Commands::Remove {
            mods: vec!["adventure".into()],
            deps: false,
            forget: false,
            acknowledge_unknown: false,
        },
        &session(root.path(), false),
    )
    .await
    .unwrap();
    assert!(!world.join("level.dat").exists());
    assert!(!world.join("region/r.0.0.mca").exists());
    assert_eq!(fs::read(world.join("untracked.txt")).unwrap(), b"keep");
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    assert!(project(root.path()).intent().roots.is_empty());
    download.assert_async().await;
    next_download.assert_async().await;
}

#[tokio::test]
async fn supplied_restricted_world_is_identified_then_interpreted_without_remote_payloads() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("World/level.dat", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"supplied world").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    fs::write(root.path().join("renamed.zip"), &bytes).unwrap();
    let filtered: Vec<_> = bytes
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, 9 | 10 | 13 | 32))
        .collect();
    let file = json!({"id":456,"gameId":432,"modId":42,"fileFingerprint":murmur2::murmur2(&filtered,1),"fileName":"world.zip","fileLength":bytes.len(),"downloadUrl":null,"hashes":[{"algo":2,"value":md5::Md5::digest(&bytes).iter().map(|byte|format!("{byte:02x}")).collect::<String>()}],"gameVersions":["1.21.1"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z"});
    let mut server = mockito::Server::new_async().await;
    server.mock("GET","/mods/42").with_body(json!({"data":{"id":42,"gameId":432,"classId":17,"slug":"provider-world","name":"Provider World"}}).to_string()).create_async().await;
    server
        .mock("GET", "/mods/42/files/456")
        .with_body(json!({"data":file.clone()}).to_string())
        .create_async()
        .await;
    let identify = server
        .mock("POST", "/fingerprints/432")
        .with_body(
            json!({"data":{"isCacheBuilt":true,"exactMatches":[{"id":42,"file":file}]}})
                .to_string(),
        )
        .expect(2)
        .create_async()
        .await;
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits {
            archive: crate::engine::artifacts::ArchiveLimits {
                entries: 16,
                total_bytes: 4096,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    let input = || {
        NonEmpty::new(vec![AddHostInput::IdentifiedFile {
            source: crate::engine::addition::DirectFileSource::Local(
                root.path().join("renamed.zip"),
            ),
            kind: Some(ContentKind::World),
            file_plan: Some(crate::engine::documents::ProviderFileSelection {
                requirements: requirements.clone(),
                files: BTreeMap::from([(
                    "world.zip".into(),
                    NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse("saves/Supplied").unwrap(),
                        layer: ContentLayer::Common,
                        requirements: requirements.clone(),
                    }])
                    .unwrap(),
                )]),
            }),
            providers: NonEmpty::new(vec![ProviderKind::CurseForge]).unwrap(),
        }])
        .unwrap()
    };
    let before = super::super::super::tests::snapshot(root.path());
    dependencies::add_with_services(
        &session(root.path(), true),
        input(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::RejectExisting,
        services(),
    )
    .await
    .unwrap();
    assert_eq!(super::super::super::tests::snapshot(root.path()), before);
    dependencies::add_with_services(
        &session(root.path(), false),
        input(),
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::RejectExisting,
        services(),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/pack/saves/Supplied/level.dat")).unwrap(),
        b"supplied world"
    );
    let resolved = project(root.path());
    let dependency = resolved.lock().dependencies.values().next().unwrap();
    assert!(matches!(dependency.identity, ResolvedIdentity::Provider(_)));
    assert_eq!(
        dependency.selected.as_ref().unwrap().selection,
        PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap())
    );
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    synchronize(&session(root.path(), false), false)
        .await
        .unwrap();
    // The ordinary CLI selector path must attach the same read-only cache capability.
    let AcquisitionSpec::ProviderArchiveMember { archive, .. } =
        &dependency.files.as_slice()[0].acquisition
    else {
        panic!("Missing archive")
    };
    let original = crate::engine::content::verify_stream(
        &mut bytes.as_slice(),
        &archive.expected,
        1 << 20,
        SourceEvidencePolicy::Compatibility,
        crate::engine::content::InitialObservation::RequireEvidence,
        &crate::application::process_runtime::Cancellation::default(),
    )
    .unwrap();
    let cache = crate::engine::content::cache::ContentCache::new(
        root.path().join("cache/content-v1"),
        crate::engine::content::store::ContentStoreLimits::default(),
    )
    .unwrap();
    let active = session(root.path(), false);
    scoped(
        &active,
        governor(active.config().app_config()),
        move |mut scope| async move { cache.publish(&mut scope, vec![original]).await },
    )
    .await
    .unwrap();
    let plan = json!({"schema":1,"environment":{"client":"required","server":"unsupported"},"files":{"world.zip":[{"destination":"saves/Supplied","layer":"common","environment":{"client":"required","server":"unsupported"}}]}});
    fs::write(
        root.path().join("world-plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    let mut selected = options("42");
    selected.platform = Some(SearchPlatform::Curseforge);
    selected.version_id = None;
    selected.file_id = Some("456".into());
    selected.file_plan = Some("world-plan.json".into());
    selected.force = true;
    let before = super::super::super::tests::snapshot(root.path());
    add_with_catalog(
        &session(root.path(), true),
        selected,
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
    )
    .await
    .unwrap();
    assert_eq!(super::super::super::tests::snapshot(root.path()), before);
    identify.assert_async().await;
}

#[tokio::test]
async fn fresh_provider_world_sync_retains_interpreted_bytes_and_previews_without_publication() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("World/level.dat", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"fresh world").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let mut server = mockito::Server::new_async().await;
    server.mock("GET","/mods/42").with_body(json!({"data":{"id":42,"gameId":432,"classId":17,"slug":"fresh-world","name":"Fresh World"}}).to_string()).create_async().await;
    server.mock("GET","/mods/42/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":42,"fileName":"world.zip","fileLength":bytes.len(),"downloadUrl":format!("{}/world.zip",server.url()),"hashes":[{"algo":2,"value":md5::Md5::digest(&bytes).iter().map(|byte|format!("{byte:02x}")).collect::<String>()}],"gameVersions":["1.21.1"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z"}}).to_string()).create_async().await;
    let download = server
        .mock("GET", "/world.zip")
        .with_body(bytes)
        .expect(3)
        .create_async()
        .await;
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        transport: HttpAcquisition::for_loopback_tests(),
        files: DirectFileLimits {
            archive: crate::engine::artifacts::ArchiveLimits {
                entries: 16,
                total_bytes: 4096,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let mut intent = project(root.path()).intent().clone();
    intent.layout.insert(
        ContentKind::World,
        empack_core::path::PortableRelPath::parse(
            "saves",
            empack_core::path::PathSyntax::ProjectContent,
        )
        .unwrap(),
    );
    intent.roots.insert(
        DependencyKey::parse("world-alias").unwrap(),
        DependencyIntent {
            source: SourceIntent::Provider(empack_core::identity::ProviderProjectId::CurseForge(
                empack_core::identity::CurseForgeProjectId::parse("42").unwrap(),
            )),
            kind: ContentKind::World,
            version: VersionIntent::Exact(PinSelector::CurseForgeFile(
                CurseForgeFileId::parse("456").unwrap(),
            )),
            placement: PlacementIntent::Automatic,
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Unsupported,
            },
        },
    );
    fs::write(
        root.path().join("project/empack.yml"),
        DocumentCodec.encode_intent(&intent).unwrap(),
    )
    .unwrap();
    let before = super::super::super::tests::snapshot(root.path());
    for materialize in [false, true] {
        synchronization::synchronize_with_services(
            &session(root.path(), true),
            materialize,
            services(),
            crate::engine::runtime_catalog::RuntimeCatalog::new(
                HttpAcquisition::for_loopback_tests(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(super::super::super::tests::snapshot(root.path()), before);
    }
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        false,
        services(),
        crate::engine::runtime_catalog::RuntimeCatalog::new(HttpAcquisition::for_loopback_tests()),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/pack/saves/fresh-world/level.dat")).unwrap(),
        b"fresh world"
    );
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert!(matches!(
        project(root.path())
            .intent()
            .roots
            .values()
            .next()
            .unwrap()
            .placement,
        PlacementIntent::Automatic
    ));
    download.assert_async().await;
}
