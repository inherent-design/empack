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
fn references(value: &ResolvedProject) -> DependencyContents {
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
                    DependencyContent::Reference,
                )
            })
        })
        .collect()
}

#[test]
fn reference_updates_retire_only_verified_old_materializations_and_index_entries() {
    for (edited, missing) in [(false, false), (false, true), (true, false)] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let current = project(false, false);
        save(root.path(), &empty());
        let cancel = Cancellation::default();
        let publisher = Publisher::open(&state.path().join("state")).unwrap();
        prepare(root.path(), state.path(), &current, b"payload")
            .unwrap()
            .publish(&publisher, &cancel)
            .unwrap();
        put(root.path(), "pack/index.toml", b"hash-format='sha256'\n[[files]]\nfile='resourcepacks/a.zip'\nhash='239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5'\n");
        put(
            root.path(),
            "pack/pack.toml",
            b"name='Test'\n[index]\nfile='index.toml'\n",
        );
        let mut lock = current.lock().clone();
        for dependency in lock.dependencies.values_mut() {
            dependency.files = NonEmpty::new(
                dependency
                    .files
                    .as_slice()
                    .iter()
                    .cloned()
                    .map(|mut file| {
                        let digest = empack_core::digest::DigestSet::new(vec![
                            empack_core::digest::ExpectedDigest::Sha256(
                                Sha256::digest(b"replacement").into(),
                            ),
                        ])
                        .unwrap();
                        file.expected.digests = Some(digest.clone());
                        file.expected.size = Some(11);
                        file.provenance.declared_digests = Some(digest);
                        file
                    })
                    .collect(),
            )
            .unwrap();
        }
        let requested = ResolvedProject::validate(
            current.intent().clone(),
            lock,
            current.lock().intent_revision,
        )
        .unwrap();
        let group = AdditionGroup::from_resolved(&requested).unwrap();
        if edited {
            put(root.path(), "pack/resourcepacks/a.zip", b"user edit");
        }
        if missing {
            for name in ["a.zip", "b.zip", "copy.zip"] {
                fs::remove_file(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap();
            }
        }
        let snapshot = ProjectReader::new(RecoveryReader::new(state.path().join("state")))
            .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
            .unwrap();
        let plan = plan_update(snapshot, &group, references(&requested), &cancel);
        if edited {
            assert!(plan.is_err());
            assert_eq!(
                fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
                b"user edit"
            );
            continue;
        }
        let prepared = plan.unwrap().stage(&cancel).unwrap();
        assert_eq!(prepared.references().len(), 2);
        prepared.publish(&publisher, &cancel).unwrap();
        for name in ["a.zip", "b.zip", "copy.zip"] {
            assert!(
                !root
                    .path()
                    .join(format!("pack/resourcepacks/{name}"))
                    .exists()
            );
        }
        let index: toml::Value =
            toml::from_str(&fs::read_to_string(root.path().join("pack/index.toml")).unwrap())
                .unwrap();
        assert!(index["files"].as_array().unwrap().is_empty());
        assert_eq!(
            fs::read(root.path().join("pack/unrelated.bin")).unwrap(),
            b"unrelated"
        );
        let restored = DocumentCodec.decode_lock(
            &fs::read(root.path().join("empack.lock")).unwrap(),
            &DocumentCodec
                .decode_intent(&fs::read(root.path().join("empack.yml")).unwrap(), "intent")
                .unwrap(),
            "lock",
        );
        assert!(restored.is_ok());
    }
}

#[test]
fn reference_additions_preserve_restricted_evidence_and_reject_local_deferral() {
    use empack_core::identity::{CurseForgeProjectId, ProviderProjectId};
    for local in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        save(root.path(), &empty());
        let base = project(true, false);
        let key = base.intent().roots.keys().next().unwrap().clone();
        let mut intent = base.intent().clone();
        let provider = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
        intent.roots.get_mut(&key).unwrap().source = SourceIntent::Provider(provider.clone());
        let source = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "intent")
            .unwrap();
        let pin = ResolvedPin {
            project: provider.clone(),
            selection: provider.parse_pin("456").unwrap(),
        };
        let mut lock = base.lock().clone();
        lock.intent_revision = source.semantic_revision();
        let dependency = lock.dependencies.get_mut(&key).unwrap();
        dependency.identity = ResolvedIdentity::Provider(provider);
        dependency.selected = Some(pin.clone());
        dependency.files = NonEmpty::new(
            dependency
                .files
                .as_slice()
                .iter()
                .cloned()
                .map(|mut file| {
                    file.acquisition = if local {
                        AcquisitionSpec::Local(
                            PortableRelPath::parse("pack/source.jar", PathSyntax::ProjectContent)
                                .unwrap(),
                        )
                    } else {
                        AcquisitionSpec::Provider {
                            pin: pin.clone(),
                            slot: file.slot.clone(),
                            alternatives: vec![],
                        }
                    };
                    file
                })
                .collect(),
        )
        .unwrap();
        let requested =
            ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap();
        let group = AdditionGroup::from_resolved(&requested).unwrap();
        let cancel = Cancellation::default();
        let snapshot = ProjectReader::new(RecoveryReader::new(state.path().join("state")))
            .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
            .unwrap();
        let plan = plan_addition(snapshot, &group, references(&requested), &cancel);
        if local {
            assert!(plan.is_err());
            continue;
        }
        let receipt = plan
            .unwrap()
            .stage(&cancel)
            .unwrap()
            .publish(
                &Publisher::open(&state.path().join("state")).unwrap(),
                &cancel,
            )
            .unwrap();
        assert_eq!(
            receipt.project.lock().dependencies[&key].files,
            requested.lock().dependencies[&key].files
        );
        assert!(!root.path().join("pack/resourcepacks").exists());
    }
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

fn observe_all(root: &Path, value: &ResolvedProject, bytes: &[u8]) {
    for dependency in value.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })
                .unwrap();
                put(root, path.as_str(), bytes);
            }
        }
    }
}
fn observed_proposal(value: &ResolvedProject, bytes: &[u8]) -> ResolvedProject {
    let digests =
        empack_core::digest::DigestSet::new(vec![empack_core::digest::ExpectedDigest::Sha256(
            Sha256::digest(bytes).into(),
        )])
        .unwrap();
    let mut lock = value.lock().clone();
    for dependency in lock.dependencies.values_mut() {
        dependency.files = NonEmpty::new(
            dependency
                .files
                .as_slice()
                .iter()
                .cloned()
                .map(|mut file| {
                    file.expected.digests = Some(digests.clone());
                    file.expected.size = Some(bytes.len() as u64);
                    file.expected.accepted_observation = None;
                    file.provenance.declared_digests = Some(digests.clone());
                    file
                })
                .collect(),
        )
        .unwrap();
    }
    ResolvedProject::validate(value.intent().clone(), lock, value.lock().intent_revision).unwrap()
}
fn adopt(root: &Path, state: &Path, value: &ResolvedProject) -> Result<PreparedAddition> {
    let cancel = Cancellation::default();
    let group = AdditionGroup::from_resolved(value)?;
    let snapshot = ProjectReader::new(RecoveryReader::new(state.join("state"))).capture_addition(
        root,
        &group,
        SnapshotLimits::default(),
        &cancel,
    )?;
    plan_adoption(snapshot, &group, &cancel)?.stage(&cancel)
}
#[test]
fn adoption_records_existing_files_without_rewriting_or_implicitly_adding_them() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let value = project(false, false);
    save(root.path(), &empty());
    observe_all(root.path(), &value, b"payload");
    let file = root.path().join("pack/resourcepacks/a.zip");
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    assert!(prepare(root.path(), state.path(), &value, b"payload").is_err());
    let adopted = adopt(root.path(), state.path(), &value).unwrap();
    assert!(
        adopted
            .files()
            .changes()
            .iter()
            .all(|change| !matches!(change.target(), ManagedPath::Content { .. }))
    );
    let receipt = adopted
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(
        receipt.project.lock().dependencies,
        value.lock().dependencies
    );
    assert_eq!(fs::read(&file).unwrap(), b"payload");
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    assert!(
        adopt(root.path(), state.path(), &value)
            .unwrap()
            .files()
            .changes()
            .is_empty()
    );
}
#[test]
fn adoption_of_changed_bytes_requires_complete_evidence_and_current_observations() {
    for failure in ["none", "missing", "wrong", "late"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let previous = project(false, false);
        save(root.path(), &previous);
        let proposed = observed_proposal(&previous, b"observed");
        observe_all(root.path(), &proposed, b"observed");
        let before_lock = fs::read(root.path().join("empack.lock")).unwrap();
        if failure == "missing" {
            fs::remove_file(root.path().join("pack/resourcepacks/b.zip")).unwrap();
        }
        if failure == "wrong" {
            put(root.path(), "pack/resourcepacks/b.zip", b"wrong");
        }
        let planned = adopt(root.path(), state.path(), &proposed);
        if matches!(failure, "missing" | "wrong") {
            assert!(planned.is_err());
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                before_lock
            );
            continue;
        }
        if failure == "late" {
            put(
                root.path(),
                "pack/resourcepacks/a.zip",
                b"edited after preview",
            );
        }
        let result = planned.unwrap().publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        );
        if failure == "late" {
            assert!(result.is_err());
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                before_lock
            );
        } else {
            assert_eq!(
                result.unwrap().project.lock().dependencies,
                proposed.lock().dependencies
            );
            assert_eq!(
                fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
                b"observed"
            );
        }
    }
}
#[test]
fn adoption_checks_observed_provider_metadata_against_the_proposed_pin() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let previous = crate::engine::addition::tests::fixture(
        &[("root", "Project1", "Version1")],
        &["root"],
        &[],
        true,
    );
    let proposed = crate::engine::addition::tests::fixture(
        &[("root", "Project1", "Version2")],
        &["root"],
        &[],
        true,
    );
    save(root.path(), &previous);
    observe_all(root.path(), &proposed, b"payload");
    let metadata = format!(
        "filename = 'a.zip'\nside = 'client'\n[download]\nurl = 'https://example.com/a.zip'\nhash-format = 'sha256'\nhash = '{}'\n[update.modrinth]\nmod-id = 'Project1'\nversion = 'Version1'\n",
        empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(b"payload").into()).hex()
    );
    put(
        root.path(),
        "pack/Project1/resourcepacks/a.pw.toml",
        metadata.as_bytes(),
    );
    assert!(adopt(root.path(), state.path(), &proposed).is_err());
    put(
        root.path(),
        "pack/Project1/resourcepacks/a.pw.toml",
        metadata.replace("Version1", "Version2").as_bytes(),
    );
    let receipt = adopt(root.path(), state.path(), &proposed)
        .unwrap()
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(
        receipt.project.lock().dependencies,
        proposed.lock().dependencies
    );
}
