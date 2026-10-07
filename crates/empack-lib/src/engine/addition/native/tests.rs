use super::*;
use crate::engine::{
    content::verify_stream, documents::DocumentCodec, mrpack::tests::project,
    project::ProjectReader, publication::RecoveryReader, snapshot::SnapshotLimits,
};
use empack_core::{files::FilePermissions, model::*};
use std::{fs, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn save(root: &Path, value: &ResolvedProject) {
    let mut intent = b"# user intent\n".to_vec();
    intent.extend(DocumentCodec.encode_intent(value.intent()).unwrap());
    put(root, "empack.yml", &intent);
    let mut lock = b" \n".to_vec();
    lock.extend(DocumentCodec.encode_lock(value).unwrap());
    put(root, "empack.lock", &lock);
    put(root, "pack/unrelated.bin", b"unrelated");
}
fn empty() -> ResolvedProject {
    let base = project(false, false);
    let mut intent = base.intent().clone();
    intent.roots.clear();
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "empty")
        .unwrap();
    let mut lock = base.lock().clone();
    lock.dependencies.clear();
    lock.required_edges.clear();
    lock.coverage.clear();
    lock.intent_revision = decoded.semantic_revision();
    ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap()
}
fn files(value: &ResolvedProject, mut bytes: &[u8]) -> BTreeMap<LockedFileKey, AcquiredBuildFile> {
    let content = verify_stream(
        &mut bytes,
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap();
    value
        .lock()
        .dependencies
        .iter()
        .flat_map(|(key, dependency)| {
            dependency.files.as_slice().iter().map(|file| {
                (
                    LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    },
                    AcquiredBuildFile {
                        content: content.clone(),
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                )
            })
        })
        .collect()
}
fn prepare(
    root: &Path,
    state: &Path,
    value: &ResolvedProject,
    bytes: &[u8],
) -> Result<PreparedAddition> {
    let cancel = Cancellation::default();
    let group = AdditionGroup::from_resolved(value)?;
    let snapshot = ProjectReader::new(RecoveryReader::new(state.join("state"))).capture_addition(
        root,
        &group,
        SnapshotLimits::default(),
        &cancel,
    )?;
    prepare_addition(snapshot, &group, files(value, bytes), &cancel)
}
#[test]
fn addition_publishes_exact_files_then_readdition_preserves_raw_documents() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    save(root.path(), &empty());
    let value = project(false, false);
    let prepared = prepare(root.path(), state.path(), &value, b"payload").unwrap();
    assert_eq!(prepared.files().changes().len(), 5);
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    let publisher = Publisher::open(&state.path().join("state")).unwrap();
    prepared
        .publish(&publisher, &Cancellation::default())
        .unwrap();
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert_eq!(
            fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
            b"payload"
        );
    }
    // Preserve cosmetic edits too, even when the serializer would normalize them.
    let lock = root.path().join("empack.lock");
    let mut raw = b" \n".to_vec();
    raw.extend(fs::read(&lock).unwrap());
    fs::write(&lock, &raw).unwrap();
    let intent = fs::read(root.path().join("empack.yml")).unwrap();
    let again = prepare(root.path(), state.path(), &value, b"payload").unwrap();
    assert!(again.files().changes().is_empty());
    again.publish(&publisher, &Cancellation::default()).unwrap();
    assert_eq!(fs::read(&lock).unwrap(), raw);
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
    assert_eq!(
        fs::read(root.path().join("pack/unrelated.bin")).unwrap(),
        b"unrelated"
    );
    fs::remove_file(root.path().join("pack/resourcepacks/a.zip")).unwrap();
    let repair = prepare(root.path(), state.path(), &value, b"payload").unwrap();
    assert_eq!(repair.files().changes().len(), 1);
    repair
        .publish(&publisher, &Cancellation::default())
        .unwrap();
}
#[test]
fn acquisition_and_destination_failures_leave_the_whole_project_unchanged() {
    for case in ["wrong-bytes", "untracked", "directory", "edited"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let value = project(false, false);
        let absent = empty();
        save(root.path(), if case == "edited" { &value } else { &absent });
        if case == "untracked" || case == "edited" {
            put(root.path(), "pack/resourcepacks/a.zip", b"user bytes");
        }
        if case == "directory" {
            put(root.path(), "pack/resourcepacks/a.zip/sentinel", b"keep");
        }
        let before = fs::read(root.path().join("empack.yml")).unwrap();
        assert!(
            prepare(
                root.path(),
                state.path(),
                &value,
                if case == "wrong-bytes" {
                    b"wrong"
                } else {
                    b"payload"
                }
            )
            .is_err(),
            "{case}"
        );
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
        assert!(!root.path().join("pack/resourcepacks/b.zip").exists());
    }
}
#[test]
fn publication_rejects_changes_after_preparation() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    save(root.path(), &empty());
    let prepared = prepare(
        root.path(),
        state.path(),
        &project(false, false),
        b"payload",
    )
    .unwrap();
    put(root.path(), "pack/resourcepacks/a.zip", b"new user content");
    assert!(
        prepared
            .publish(
                &Publisher::open(&state.path().join("state")).unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"new user content"
    );
    assert!(!root.path().join("pack/resourcepacks/b.zip").exists());
}
#[cfg(unix)]
#[test]
fn selected_links_fail_but_unrelated_links_remain_outside_the_read_set() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    save(root.path(), &empty());
    put(outside.path(), "sentinel", b"outside");
    symlink(outside.path(), root.path().join("pack/unrelated-link")).unwrap();
    let value = project(false, false);
    assert!(prepare(root.path(), state.path(), &value, b"payload").is_ok());
    symlink(outside.path(), root.path().join("pack/resourcepacks")).unwrap();
    assert!(prepare(root.path(), state.path(), &value, b"payload").is_err());
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside"
    );
}
#[test]
fn updated_bytes_retire_only_owned_derivatives_and_rebind_the_index() {
    for no_hashes in [false, true] {
        updated_index_case(no_hashes);
    }
}
fn updated_index_case(no_hashes: bool) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let current = project(false, false);
    save(root.path(), &current);
    for name in ["a.zip", "b.zip", "copy.zip"] {
        put(
            root.path(),
            &format!("pack/resourcepacks/{name}"),
            b"payload",
        );
    }
    let metadata = format!(
        "name = 'assets'\nfilename = 'a.zip'\nside = 'client'\n[download]\nurl = 'https://example.com/a'\nhash-format = 'sha256'\nhash = '{}'\n",
        empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(b"payload").into()).hex()
    );
    put(
        root.path(),
        "pack/resourcepacks/assets.pw.toml",
        metadata.as_bytes(),
    );
    put(root.path(), "pack/index.toml", b"hash-format = 'sha1'\nuser-field = 'keep'\n[[files]]\nfile = 'resourcepacks/assets.pw.toml'\nmetafile = true\n[[files]]\nfile = 'resourcepacks/b.zip'\npreserve = true\nuser-entry-field = 'keep entry'\n[[files]]\nfile = 'resourcepacks/b.zip'\nalias = 'resourcepacks/aliased.zip'\npreserve = true\nuser-entry-field = 'keep entry'\n[[files]]\nfile = 'unrelated.bin'\n");
    let index = fs::read(root.path().join("pack/index.toml")).unwrap();
    let mut pack = format!(
        "name = 'test'\n[index]\nfile = 'index.toml'\nhash-format = 'sha256'\nhash = '{}'\n",
        empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(&index).into()).hex()
    );
    if no_hashes {
        pack.push_str("\n[options]\nno-internal-hashes = true\n");
    }
    put(root.path(), "pack/pack.toml", pack.as_bytes());
    let mut lock = current.lock().clone();
    for dependency in lock.dependencies.values_mut() {
        let mut updated = dependency.files.as_slice().to_vec();
        for file in &mut updated {
            file.expected.digests = Some(
                empack_core::digest::DigestSet::new(vec![
                    empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(b"updated").into()),
                ])
                .unwrap(),
            );
        }
        for file in &mut updated {
            file.provenance.declared_digests = file.expected.digests.clone();
        }
        dependency.files = NonEmpty::new(updated).unwrap();
    }
    let incoming = ResolvedProject::validate(
        current.intent().clone(),
        lock,
        current.lock().intent_revision,
    )
    .unwrap();
    prepare(root.path(), state.path(), &incoming, b"updated")
        .unwrap()
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        !root
            .path()
            .join("pack/resourcepacks/assets.pw.toml")
            .exists()
    );
    let bytes = fs::read(root.path().join("pack/index.toml")).unwrap();
    let index: toml::Value = toml::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
    let entries = index["files"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let direct: Vec<_> = entries
        .iter()
        .filter(|entry| entry["file"].as_str() == Some("resourcepacks/b.zip"))
        .collect();
    assert_eq!(direct.len(), 2, "both index aliases must remain");
    for direct in direct {
        assert_eq!(index["hash-format"].as_str(), Some("sha1"));
        assert_eq!(direct["hash-format"].as_str(), Some("sha256"));
        assert_eq!(direct["preserve"].as_bool(), Some(true));
        assert_eq!(direct["user-entry-field"].as_str(), Some("keep entry"));
        if no_hashes {
            assert!(direct.get("hash").is_none());
        } else {
            assert_eq!(
                direct["hash"].as_str().unwrap(),
                empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(b"updated").into())
                    .hex()
            );
        }
    }
    assert!(
        entries
            .iter()
            .any(|entry| entry["file"].as_str() == Some("unrelated.bin"))
    );
    assert_eq!(index["user-field"].as_str(), Some("keep"));
    let pack: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pack/pack.toml")).unwrap()).unwrap();
    if no_hashes {
        assert!(pack["index"].get("hash").is_none());
    } else {
        assert_eq!(
            pack["index"]["hash"].as_str().unwrap(),
            empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(&bytes).into()).hex()
        );
    }
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"updated"
    );
    assert!(
        prepare(root.path(), state.path(), &incoming, b"updated")
            .unwrap()
            .files()
            .changes()
            .is_empty()
    );
}
#[test]
fn input_aliases_bind_to_existing_keys_and_extra_slots_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let current = crate::engine::addition::tests::fixture(
        &[("retained-alias", "Project1", "Version1")],
        &["retained-alias"],
        &[],
        true,
    );
    let incoming = crate::engine::addition::tests::fixture(
        &[("input-alias", "Project1", "Version1")],
        &["input-alias"],
        &[],
        true,
    );
    save(root.path(), &current);
    let group = AdditionGroup::from_resolved(&incoming).unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(state.path().join("state")));
    let cancel = Cancellation::default();
    let mut acquired = files(&incoming, b"payload");
    let first = acquired.values().next().unwrap().clone();
    acquired.insert(
        LockedFileKey {
            dependency: DependencyKey::parse("input-alias").unwrap(),
            slot: FileSlot::parse("extra").unwrap(),
        },
        first,
    );
    let snapshot = reader
        .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
        .unwrap();
    assert!(prepare_addition(snapshot, &group, acquired, &cancel).is_err());
    let prepared = prepare(root.path(), state.path(), &incoming, b"payload").unwrap();
    assert!(
        prepared
            .candidate()
            .project()
            .intent()
            .roots
            .contains_key(&DependencyKey::parse("retained-alias").unwrap())
    );
    assert!(
        !prepared
            .candidate()
            .project()
            .intent()
            .roots
            .contains_key(&DependencyKey::parse("input-alias").unwrap())
    );
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &cancel,
        )
        .unwrap();
    assert!(
        prepare(root.path(), state.path(), &incoming, b"payload")
            .unwrap()
            .files()
            .changes()
            .is_empty()
    );
}

#[test]
fn unrelated_malformed_metadata_does_not_authorize_or_block_addition() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    save(root.path(), &empty());
    put(root.path(), "pack/unrelated.pw.toml", b"broken = [");
    put(root.path(), "pack/directory.pw.toml/sentinel", b"keep");
    let prepared = prepare(
        root.path(),
        state.path(),
        &project(false, false),
        b"payload",
    )
    .unwrap();
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("pack/unrelated.pw.toml")).unwrap(),
        b"broken = ["
    );
    assert_eq!(
        fs::read(root.path().join("pack/directory.pw.toml/sentinel")).unwrap(),
        b"keep"
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
}
