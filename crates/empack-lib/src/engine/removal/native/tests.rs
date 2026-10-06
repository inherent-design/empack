use super::*;
use crate::engine::{
    documents::DocumentCodec, mrpack::tests::project, project::ProjectReader,
    publication::RecoveryReader, snapshot::SnapshotLimits,
};
use std::{fs, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let destination = root.join(name);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, bytes).unwrap();
}
fn fixture(root: &Path) {
    let project = project(false, false);
    put(
        root,
        "empack.yml",
        &DocumentCodec.encode_intent(project.intent()).unwrap(),
    );
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    for name in ["a.zip", "b.zip", "copy.zip"] {
        put(root, &format!("pack/resourcepacks/{name}"), b"payload");
    }
    put(root, "pack/resourcepacks/unrequested.zip", b"keep");
    put(root, "templates/client/custom", b"user template");
    put(root, "dist/previous.zip", b"published distribution");
    put(root, "original-download.zip", b"source");
}
fn selected() -> NonEmpty<DependencyKey> {
    NonEmpty::new(vec![DependencyKey::parse("assets").unwrap()]).unwrap()
}
fn prepare(root: &Path, state: &Path, mode: RemovalMode) -> Result<PreparedRemoval> {
    let cancel = Cancellation::default();
    let snapshot = ProjectReader::new(RecoveryReader::new(state.join("state"))).capture_mutation(
        root,
        SnapshotLimits::default(),
        &cancel,
    )?;
    prepare_removal(snapshot, &selected(), mode, &cancel)
}
fn kept(root: &Path) {
    for (name, bytes) in [
        ("pack/resourcepacks/unrequested.zip", b"keep".as_slice()),
        ("templates/client/custom", b"user template"),
        ("dist/previous.zip", b"published distribution"),
        ("original-download.zip", b"source"),
    ] {
        assert_eq!(fs::read(root.join(name)).unwrap(), bytes);
    }
}
#[test]
fn exact_removal_publishes_documents_and_files_without_collecting_unrequested_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
    assert_eq!(prepared.files().changes().len(), 5);
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert!(!state.path().join("state").exists());
    let receipt = prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(receipt.project.lock().dependencies.is_empty());
    assert!(receipt.project.intent().roots.is_empty());
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert!(
            !root
                .path()
                .join(format!("pack/resourcepacks/{name}"))
                .exists()
        );
    }
    kept(root.path());
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.path().join("empack.yml")).unwrap(), "result")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.path().join("empack.lock")).unwrap(),
            &intent,
            "result",
        )
        .unwrap();
}
#[test]
fn changed_bytes_and_directory_targets_reject_the_entire_removal() {
    for directory in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let before = fs::read(root.path().join("empack.yml")).unwrap();
        let target = root.path().join("pack/resourcepacks/a.zip");
        fs::remove_file(&target).unwrap();
        if directory {
            put(&target, "sentinel", b"keep directory");
        } else {
            fs::write(&target, b"edited").unwrap();
        }
        let error = prepare(root.path(), state.path(), RemovalMode::RemoveContent)
            .err()
            .unwrap();
        let diagnostic = format!("{error:#}");
        assert!(
            if directory {
                diagnostic.contains("directory")
            } else {
                diagnostic.contains("differs")
            },
            "{diagnostic}"
        );
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
            b"payload"
        );
        if directory {
            assert_eq!(
                fs::read(target.join("sentinel")).unwrap(),
                b"keep directory"
            );
        }
        kept(root.path());
    }
}
#[test]
fn document_and_payload_edits_after_preparation_prevent_all_publication() {
    for name in ["empack.yml", "pack/resourcepacks/a.zip"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
        let mut edited = fs::read(root.path().join(name)).unwrap();
        edited.extend(b"\n# user edit");
        put(root.path(), name, &edited);
        assert!(
            prepared
                .publish(
                    &Publisher::open(&state.path().join("state")).unwrap(),
                    &Cancellation::default()
                )
                .is_err()
        );
        assert_eq!(fs::read(root.path().join(name)).unwrap(), edited);
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
            b"payload"
        );
        kept(root.path());
    }
}
#[test]
fn explicit_root_demotion_publishes_only_documents_and_retains_exact_files() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let prepared = prepare(root.path(), state.path(), RemovalMode::ForgetRoots).unwrap();
    assert!(prepared.files().changes().iter().all(|change| matches!(
        change.target(),
        ManagedPath::IntentDocument | ManagedPath::LockDocument
    )));
    let receipt = prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(receipt.mode, RemovalMode::ForgetRoots);
    assert!(receipt.project.intent().roots.is_empty());
    assert_eq!(receipt.project.lock().dependencies.len(), 1);
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert_eq!(
            fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
            b"payload"
        );
    }
    kept(root.path());
}
#[test]
fn ignored_locked_placements_remain_observed_removal_targets() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(root.path(), "pack/.packwizignore", b"**\n");
    let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
    assert_eq!(prepared.files().changes().len(), 5);
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    kept(root.path());
    assert_eq!(
        fs::read(root.path().join("pack/.packwizignore")).unwrap(),
        b"**\n"
    );
}
#[cfg(unix)]
#[test]
fn removal_refuses_linked_ancestors_without_deleting_outside_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::rename(
        root.path().join("pack/resourcepacks"),
        outside.path().join("content"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("content"),
        root.path().join("pack/resourcepacks"),
    )
    .unwrap();
    let error = prepare(root.path(), state.path(), RemovalMode::RemoveContent)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("link"), "{error:#}");
    assert_eq!(
        fs::read(outside.path().join("content/a.zip")).unwrap(),
        b"payload"
    );
}
fn metadata(filename: &str, payload: &[u8]) -> Vec<u8> {
    format!("name = 'untrusted title'\nfilename = '{filename}'\nside = 'client'\n[download]\nurl = 'https://example.com/different-name'\nhash-format = 'sha256'\nhash = '{}'\n", ExpectedDigest::Sha256(Sha256::digest(payload).into()).hex()).into_bytes()
}
fn backend(root: &Path, mismatch: bool) {
    let selected = metadata("a.zip", if mismatch { b"wrong" } else { b"payload" });
    let unrequested = metadata("unrequested.zip", b"keep");
    put(root, "pack/resourcepacks/actual-name.pw.toml", &selected);
    // A metadata stem equal to the manifest key must not select this unrelated installation.
    put(root, "pack/resourcepacks/assets.pw.toml", &unrequested);
    let entries = [("resourcepacks/actual-name.pw.toml", selected), ("resourcepacks/assets.pw.toml", unrequested)].into_iter()
        .map(|(file, bytes)| serde_json::json!({"file":file,"hash":ExpectedDigest::Sha256(Sha256::digest(bytes).into()).hex(),"metafile":true})).collect::<Vec<_>>();
    let index = toml::to_string(
        &serde_json::json!({"hash-format":"sha256","files":entries,"user-field":"retained"}),
    )
    .unwrap()
    .into_bytes();
    let pack = format!(
        "name = 'backend title'\n[index]\nfile = 'index.toml'\nhash-format = 'sha256'\nhash = '{}'\n",
        ExpectedDigest::Sha256(Sha256::digest(&index).into()).hex()
    );
    put(root, "pack/index.toml", &index);
    put(root, "pack/pack.toml", pack.as_bytes());
}
#[test]
fn metadata_removal_uses_exact_file_ownership_and_rebinds_the_index() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    backend(root.path(), false);
    let before = fs::read(root.path().join("pack/resourcepacks/assets.pw.toml")).unwrap();
    let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        !root
            .path()
            .join("pack/resourcepacks/actual-name.pw.toml")
            .exists()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/assets.pw.toml")).unwrap(),
        before
    );
    let index = fs::read(root.path().join("pack/index.toml")).unwrap();
    let decoded: toml::Value = toml::from_str(std::str::from_utf8(&index).unwrap()).unwrap();
    assert_eq!(decoded["user-field"].as_str(), Some("retained"));
    let files = decoded["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0]["file"].as_str(),
        Some("resourcepacks/assets.pw.toml")
    );
    let pack: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pack/pack.toml")).unwrap()).unwrap();
    assert_eq!(
        pack["index"]["hash"].as_str().unwrap(),
        ExpectedDigest::Sha256(Sha256::digest(index).into()).hex()
    );
    kept(root.path());
}
#[test]
fn derivative_digest_drift_cannot_authorize_metadata_or_content_deletion() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    backend(root.path(), true);
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    assert!(prepare(root.path(), state.path(), RemovalMode::RemoveContent).is_err());
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
}

#[test]
fn index_aliases_cannot_leave_a_stale_reference_to_removed_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    backend(root.path(), false);
    let index = fs::read_to_string(root.path().join("pack/index.toml"))
        .unwrap()
        .replace(
            "resourcepacks/actual-name.pw.toml",
            "resourcepacks/ACTUAL-NAME.pw.toml",
        );
    put(root.path(), "pack/index.toml", index.as_bytes());
    let mut pack: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pack/pack.toml")).unwrap()).unwrap();
    pack["index"]["hash"] =
        toml::Value::String(ExpectedDigest::Sha256(Sha256::digest(index.as_bytes()).into()).hex());
    put(
        root.path(),
        "pack/pack.toml",
        toml::to_string(&pack).unwrap().as_bytes(),
    );
    let error = prepare(root.path(), state.path(), RemovalMode::RemoveContent)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("Index aliases"), "{error:#}");
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
}
