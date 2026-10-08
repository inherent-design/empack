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
        .expect_at_least(4)
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
    update::update_with_services(
        &session(root.path(), false),
        vec!["adventure".into()],
        services(),
    )
    .await
    .unwrap();
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
        .by_name(".minecraft/saves/MyWorld/region/r.0.0.mca")
        .unwrap()
        .read_to_end(&mut region)
        .unwrap();
    assert_eq!(region, b"region");
    fs::write(world.join("untracked.txt"), b"keep").unwrap();
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
}
