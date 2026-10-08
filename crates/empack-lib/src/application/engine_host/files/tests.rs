use super::*;
use crate::{
    application::{
        BuildArgs, InitArgs,
        session_mocks::{MockCommandSession, MockConfigProvider, MockFileSystemProvider},
    },
    engine::{
        addition::{FileEvidence, FileKindPolicy},
        build::BuildAcquisitions,
        documents::DocumentCodec,
    },
};
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    model::*,
    path::InstallDestination,
    requirements::{Requirement, Requirements},
};
use sha2::{Digest, Sha512};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
};

fn session(root: &Path, yes: bool, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_filesystem(MockFileSystemProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            yes,
            dry_run: dry,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
}
async fn fixture(root: &Path) {
    fs::create_dir(root.join("project")).unwrap();
    initialize(
        &session(root, true, false),
        &InitArgs {
            mc_version: Some("1.21.1".into()),
            modloader: Some("vanilla".into()),
            pack_name: Some("File Pack".into()),
            pack_version: Some("1.0".into()),
            author: Some("Tester".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    super::super::tests::snapshot(root)
}
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in entries {
        zip.start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn expected(bytes: &[u8]) -> ExpectedContent {
    ExpectedContent {
        digests: Some(
            DigestSet::new(vec![ExpectedDigest::Sha512(Sha512::digest(bytes).into())]).unwrap(),
        ),
        size: Some(bytes.len() as u64),
        accepted_observation: None,
    }
}
fn input(
    key: &str,
    source: DirectFileSource,
    bytes: &[u8],
    kind: ContentKind,
    destination: &str,
) -> DirectFileInput {
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Unsupported,
    };
    DirectFileInput {
        key: DependencyKey::parse(key).unwrap(),
        title: key.into(),
        source,
        evidence: FileEvidence::Declared(expected(bytes)),
        kind,
        kind_policy: FileKindPolicy::RequireRecognized,
        requirements: requirements.clone(),
        placements: NonEmpty::new(vec![Placement {
            layer: ContentLayer::Client,
            destination: InstallDestination::parse(destination).unwrap(),
            requirements,
        }])
        .unwrap(),
    }
}
async fn add(
    root: &Path,
    inputs: Vec<DirectFileInput>,
    yes: bool,
    dry: bool,
    limits: DirectFileLimits,
) -> Result<()> {
    add_with_transport(
        &session(root, yes, dry),
        NonEmpty::new(inputs)?,
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::UpdateSameIdentity,
        HttpAcquisition::for_loopback_tests(),
        limits,
    )
    .await
}
#[tokio::test]
async fn direct_files_publish_one_batch_preserve_provenance_and_build_current_bytes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let assets = archive(&[("pack.mcmeta", b"{}"), ("assets/demo/a.txt", b"fresh")]);
    fs::write(root.path().join("download.bin"), &assets).unwrap();
    let mut server = mockito::Server::new_async().await;
    let remote = server
        .mock("GET", "/transient")
        .with_body(b"enabled=true")
        .expect(4)
        .create_async()
        .await;
    let inputs = || {
        vec![
            input(
                "assets",
                DirectFileSource::Local("download.bin".into()),
                &assets,
                ContentKind::ResourcePack,
                "resourcepacks/selected.zip",
            ),
            input(
                "settings",
                DirectFileSource::Download {
                    origins: NonEmpty::new(vec!["https://example.invalid/config.txt".into()])
                        .unwrap(),
                    alternatives: NonEmpty::new(vec![format!("{}/transient", server.url())])
                        .unwrap(),
                },
                b"enabled=true",
                ContentKind::Config,
                "config/chosen.txt",
            ),
        ]
    };
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        add(root.path(), inputs(), yes, dry, DirectFileLimits::default())
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    add(
        root.path(),
        inputs(),
        true,
        false,
        DirectFileLimits::default(),
    )
    .await
    .unwrap();
    let project = root.path().join("project");
    assert_eq!(
        fs::read(project.join("overrides/client/resourcepacks/selected.zip")).unwrap(),
        assets
    );
    assert_eq!(
        fs::read(project.join("overrides/client/config/chosen.txt")).unwrap(),
        b"enabled=true"
    );
    let intent = DocumentCodec
        .decode_intent(&fs::read(project.join("empack.yml")).unwrap(), "test")
        .unwrap();
    let resolved = DocumentCodec
        .decode_lock(
            &fs::read(project.join("empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap();
    assert_eq!(resolved.intent().roots.len(), 2);
    assert!(
        matches!(&resolved.intent().roots[&DependencyKey::parse("assets").unwrap()].source,SourceIntent::Local(path) if path.as_str()=="overrides/client/resourcepacks/selected.zip")
    );
    let lock = fs::read_to_string(project.join("empack.lock")).unwrap();
    assert!(!lock.contains(&server.url()));
    assert!(!lock.contains("download.bin"));
    assert_eq!(fs::read(root.path().join("download.bin")).unwrap(), assets);
    let before = snapshot(&project);
    add(
        root.path(),
        inputs(),
        true,
        false,
        DirectFileLimits::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        snapshot(&project),
        before,
        "re-adding exact files preserves documents and bytes"
    );
    // Exercise the public local-file entry point as well as the injected HTTP fixture.
    add_files(
        &session(root.path(), true, false),
        NonEmpty::new(vec![inputs().remove(0)]).unwrap(),
        SourceEvidencePolicy::Compatibility,
        ExistingDependencyPolicy::UpdateSameIdentity,
    )
    .await
    .unwrap();
    assert_eq!(snapshot(&project), before);

    build(
        &session(root.path(), true, false),
        &BuildArgs {
            targets: vec!["client-full".into()],
            ..Default::default()
        },
        BuildDecisions::default(),
        BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let path = fs::read_dir(project.join("dist"))
        .unwrap()
        .map(|v| v.unwrap().path())
        .find(|p| p.extension().is_some_and(|v| v == "zip"))
        .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(".minecraft/resourcepacks/selected.zip")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, assets);
    bytes.clear();
    zip.by_name(".minecraft/config/chosen.txt")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"enabled=true");
    remote.assert_async().await;
}
#[tokio::test]
async fn direct_file_failures_preserve_the_entire_project_and_do_not_accept_a_subset() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    fs::write(root.path().join("first"), b"good").unwrap();
    fs::write(root.path().join("second"), b"bad").unwrap();
    let before = snapshot(root.path());
    let values = || {
        vec![
            input(
                "first",
                DirectFileSource::Local("first".into()),
                b"good",
                ContentKind::Config,
                "config/first",
            ),
            input(
                "second",
                DirectFileSource::Local("second".into()),
                b"expected",
                ContentKind::Config,
                "config/second",
            ),
        ]
    };
    assert!(
        add(
            root.path(),
            values(),
            true,
            false,
            DirectFileLimits::default()
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot(root.path()), before);
    let mut selected = values();
    selected[1].evidence = FileEvidence::AcceptObserved;
    let limits = DirectFileLimits {
        transfer: TransferLimits {
            file_bytes: 5,
            transfer_bytes: 6,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        add(root.path(), selected, true, false, limits)
            .await
            .is_err()
    );
    assert_eq!(snapshot(root.path()), before);
    let mut selected = values();
    selected[1].evidence = FileEvidence::AcceptObserved;
    selected[1].placements = selected[0].placements.clone();
    assert!(
        add(
            root.path(),
            selected,
            true,
            false,
            DirectFileLimits::default()
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot(root.path()), before);
}
#[tokio::test]
async fn typed_archives_require_structural_safety_and_explicit_unidentified_acceptance() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    for (bytes, kind, accept, success) in [
        (b"not a zip".to_vec(), ContentKind::Mod, true, false),
        (
            archive(&[("../escape", b"x")]),
            ContentKind::Mod,
            true,
            false,
        ),
        (
            archive(&[("modrinth.index.json", b"{}")]),
            ContentKind::Mod,
            true,
            false,
        ),
        (
            archive(&[("example/A.class", b"class")]),
            ContentKind::Mod,
            false,
            false,
        ),
        (
            archive(&[("example/A.class", b"class")]),
            ContentKind::Mod,
            true,
            true,
        ),
        (
            archive(&[("fabric.mod.json", b"{}")]),
            ContentKind::Mod,
            false,
            true,
        ),
        (
            archive(&[("pack.mcmeta", b"{}"), ("data/demo/a.json", b"{}")]),
            ContentKind::DataPack,
            false,
            true,
        ),
        (
            archive(&[("shaders/program.fsh", b"shader")]),
            ContentKind::ShaderPack,
            false,
            true,
        ),
    ] {
        fs::write(root.path().join("source"), &bytes).unwrap();
        let before = snapshot(root.path());
        let key = format!("item-{}", Sha512::digest(&bytes)[0]);
        let mut selected = input(
            &key,
            DirectFileSource::Local("source".into()),
            &bytes,
            kind,
            &format!("test/{key}.zip"),
        );
        if accept {
            selected.kind_policy = FileKindPolicy::AcceptUnrecognized;
        }
        let result = add(
            root.path(),
            vec![selected],
            true,
            false,
            DirectFileLimits::default(),
        )
        .await;
        if success {
            result.unwrap();
        } else {
            assert!(result.is_err());
            assert_eq!(snapshot(root.path()), before);
        }
    }
}
#[tokio::test]
async fn direct_downloads_share_the_batch_budget_and_validate_every_origin_first() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    let remote = server
        .mock("GET", "/file")
        .with_body(b"four")
        .expect(2)
        .create_async()
        .await;
    let make = |key: &str| {
        input(
            key,
            DirectFileSource::Download {
                origins: NonEmpty::new(vec!["https://example.invalid/file".into()]).unwrap(),
                alternatives: NonEmpty::new(vec![format!("{}/file", server.url())]).unwrap(),
            },
            b"four",
            ContentKind::Config,
            &format!("config/{key}"),
        )
    };
    let before = snapshot(root.path());
    let mut invalid = make("second");
    invalid.source = DirectFileSource::Download {
        origins: NonEmpty::new(vec!["https://example.invalid/file?token=secret".into()]).unwrap(),
        alternatives: NonEmpty::new(vec![format!("{}/file", server.url())]).unwrap(),
    };
    assert!(
        add(
            root.path(),
            vec![make("first"), invalid],
            true,
            false,
            DirectFileLimits::default()
        )
        .await
        .is_err()
    );
    let limits = DirectFileLimits {
        transfer: TransferLimits {
            file_bytes: 4,
            transfer_bytes: 7,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        add(
            root.path(),
            vec![make("first"), make("second")],
            true,
            false,
            limits
        )
        .await
        .is_err()
    );
    remote.assert_async().await;
    assert_eq!(snapshot(root.path()), before);
}
#[cfg(unix)]
#[tokio::test]
async fn direct_file_symlinks_and_strict_unasserted_sources_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("sentinel"), b"safe").unwrap();
    std::os::unix::fs::symlink(outside.path().join("sentinel"), root.path().join("link")).unwrap();
    let before = snapshot(&root.path().join("project"));
    let selected = input(
        "unsafe",
        DirectFileSource::Local("link".into()),
        b"safe",
        ContentKind::Config,
        "config/unsafe",
    );
    assert!(
        add(
            root.path(),
            vec![selected],
            true,
            false,
            DirectFileLimits::default()
        )
        .await
        .is_err()
    );
    fs::write(root.path().join("source"), b"safe").unwrap();
    let mut selected = input(
        "strict",
        DirectFileSource::Local("source".into()),
        b"safe",
        ContentKind::Config,
        "config/strict",
    );
    selected.evidence = FileEvidence::AcceptObserved;
    assert!(
        add_files(
            &session(root.path(), true, false),
            NonEmpty::new(vec![selected]).unwrap(),
            SourceEvidencePolicy::StrongSourceRequired,
            ExistingDependencyPolicy::RejectExisting
        )
        .await
        .is_err()
    );
    assert_eq!(snapshot(&root.path().join("project")), before);
    assert_eq!(fs::read(outside.path().join("sentinel")).unwrap(), b"safe");
}

#[tokio::test]
async fn direct_file_resolution_preserves_a_concurrent_document_edit() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut server = mockito::Server::new_async().await;
    let path = root.path().join("project/empack.lock");
    let mut changed = fs::read(&path).unwrap();
    changed.extend_from_slice(b"\n# independent edit\n");
    let written = changed.clone();
    server
        .mock("GET", "/file")
        .with_body_from_request(move |_| {
            fs::write(&path, &written).unwrap();
            b"four".to_vec()
        })
        .create_async()
        .await;
    let mut before = snapshot(root.path());
    before.insert("project/empack.lock".into(), changed);
    let selected = input(
        "settings",
        DirectFileSource::Download {
            origins: NonEmpty::new(vec!["https://example.invalid/file".into()]).unwrap(),
            alternatives: NonEmpty::new(vec![format!("{}/file", server.url())]).unwrap(),
        },
        b"four",
        ContentKind::Config,
        "config/settings",
    );
    let error = add(
        root.path(),
        vec![selected],
        true,
        false,
        DirectFileLimits::default(),
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("changed project documents"),
        "{error:#}"
    );
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn typed_archives_reject_bad_member_crc_even_when_the_archive_hash_matches() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("pack.mcmeta", &b"{}"[..]),
        ("assets/demo/file.txt", &b"original content"[..]),
    ] {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    let offset = {
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        zip.by_name("assets/demo/file.txt")
            .unwrap()
            .data_start()
            .unwrap() as usize
    };
    bytes[offset] ^= 1;
    fs::write(root.path().join("source.zip"), &bytes).unwrap();
    let before = snapshot(root.path());
    for policy in [
        FileKindPolicy::RequireRecognized,
        FileKindPolicy::AcceptUnrecognized,
    ] {
        // The independently asserted archive digest matches these damaged container bytes.
        let mut selected = input(
            "assets",
            DirectFileSource::Local("source.zip".into()),
            &bytes,
            ContentKind::ResourcePack,
            "resourcepacks/assets.zip",
        );
        selected.kind_policy = policy;
        assert!(
            add(
                root.path(),
                vec![selected],
                true,
                false,
                DirectFileLimits::default()
            )
            .await
            .is_err(),
            "member integrity must be checked independently of the outer archive digest"
        );
        assert_eq!(snapshot(root.path()), before);
    }
}
