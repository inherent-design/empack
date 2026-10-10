use super::*;
use crate::engine::release::*;
use std::{fs, path::Path};
fn source(root: &Path, assertions: Vec<SourceDigest>) -> DecodedRelease {
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("assets/config"), b"settings").unwrap();
    let release = DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        pack: "published".into(),
        version: "1".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![ReleaseFile {
            key: "config".into(),
            destination: "config/example.toml".into(),
            layer: ReleaseLayer::Common,
            policy: FilePolicy::Seed,
            client: Participation::Required,
            server: Participation::Required,
            sha256: release::hash(b"settings"),
            bytes: 8,
            readonly: false,
            executable: false,
            assertions,
            asset: None,
            source: ReleaseSource::Asset {
                path: "assets/config".into(),
            },
        }],
    })
    .unwrap();
    fs::write(root.join("release.json"), release.bytes()).unwrap();
    release
}
fn request(source: &Path) -> StageReleaseRequest {
    StageReleaseRequest {
        source: source.to_owned(),
        keys: vec![SigningKey::from_bytes(&[27; 32])],
    }
}
async fn apply(engine: &Engine, prepared: PreparedOperation) -> bool {
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let result = operation.wait().await;
    let complete = matches!(
        &*result,
        OperationOutcome::Completed(ExecutionOutcome::Completed(
            ExecutionReceipt::ReleasePublication(_)
        ))
    );
    drop(result);
    engine.release_completed(operation.id());
    complete
}
#[tokio::test]
async fn immutable_release_staging_verifies_assets_signatures_and_republication() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let release = source(input.path(), vec![]);
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    for _ in 0..2 {
        let Preparation::Ready(prepared) = engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .unwrap()
        else {
            panic!()
        };
        let OperationPreview::ReleasePublication(view) = prepared.view() else {
            panic!()
        };
        assert_eq!(view.release, release.id());
        assert_eq!(view.keys.len(), 1);
        if !root.path().join("dist").exists() {
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }
        assert!(apply(&engine, prepared).await);
    }
    let output = root.path().join(format!("dist/releases/{}", release.id()));
    assert_eq!(fs::read(output.join("assets/config")).unwrap(), b"settings");
    let encoded = fs::read(output.join("release.json")).unwrap();
    let trust = trust::PublisherTrust::enroll(
        "published".into(),
        "https://publisher.test",
        vec![SigningKey::from_bytes(&[27; 32]).verifying_key()],
    )
    .unwrap();
    assert_eq!(
        trust
            .release(
                &encoded,
                release.id(),
                &semver::Version::parse("0.6.0-beta").unwrap()
            )
            .unwrap()
            .release()
            .id(),
        release.id()
    );
    assert!(!root.path().join("dist/channels").exists());
    fs::write(output.join("assets/config"), b"tampered").unwrap();
    assert!(
        engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .is_err()
    );
    assert_eq!(fs::read(output.join("assets/config")).unwrap(), b"tampered");
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn publication_refuses_false_source_assertions_and_late_source_changes() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source(
        input.path(),
        vec![SourceDigest {
            algorithm: "sha512".into(),
            value: "a".repeat(128),
        }],
    );
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    assert!(
        engine
            .prepare(root.path().to_owned(), request(input.path()))
            .await
            .is_err()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    source(input.path(), vec![]);
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), request(input.path()))
        .await
        .unwrap()
    else {
        panic!()
    };
    fs::write(input.path().join("assets/config"), b"modified").unwrap();
    assert!(!apply(&engine, prepared).await);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    engine.shutdown().await;
}
#[test]
fn signing_key_selection_rejects_project_sources_and_wrong_kinds() {
    let root = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let key = secrets.path().join("key");
    fs::write(&key, "1b".repeat(32)).unwrap();
    assert_eq!(
        release::signing::read_keys(root.path(), input.path(), &[key], &Default::default())
            .unwrap()
            .len(),
        1
    );
    let key = input.path().join("key");
    fs::write(&key, "1b".repeat(32)).unwrap();
    assert!(
        release::signing::read_keys(root.path(), input.path(), &[key], &Default::default())
            .is_err()
    );
    assert!(
        release::signing::read_keys(
            root.path(),
            input.path(),
            &[secrets.path().to_owned()],
            &Default::default()
        )
        .is_err()
    );
}
