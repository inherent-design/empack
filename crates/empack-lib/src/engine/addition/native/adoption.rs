//! Adoption verifies observed payloads and publishes descriptions; it never rewrites game bytes.
use super::*;
use empack_core::{digest::ExpectedDigest, model::ContentLayer};

pub(in crate::engine) fn plan_adoption(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let current = workspace.require_resolved()?;
    let candidate = AdditionCandidate::prepare(
        workspace.intent(),
        workspace.prior_lock().context("Adoption requires a lock")?,
        group,
    )?;
    let mut observed = BTreeMap::new();
    let mut updates = BTreeMap::new();
    let mut metadata_updates = BTreeMap::new();
    for key in candidate.plan().bindings().values() {
        for file in candidate.project().lock().dependencies[key]
            .files
            .as_slice()
        {
            for placement in file.placements.as_slice() {
                let target = ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                };
                let path = ProjectLayout::path(&target)?;
                let digests = workspace.verify_file(&path, &file.expected, cancel)?;
                let sha256 = digests
                    .values()
                    .iter()
                    .find(|value| matches!(value, ExpectedDigest::Sha256(_)))
                    .context("Observed adoption lacks a content address")?
                    .clone();
                if placement.layer == ContentLayer::Common {
                    updates.insert(placement.destination.relative().clone(), sha256);
                }
                ensure!(
                    observed.insert(target, digests).is_none(),
                    "Adoption has duplicate placements"
                );
            }
        }
    }
    for record in workspace.backend_files(cancel)? {
        let target = ManagedPath::Content {
            layer: ContentLayer::Common,
            path: record.destination.relative().clone(),
        };
        let Some(digests) = observed.get(&target) else {
            continue;
        };
        ensure!(
            record.locked_owner(candidate.project())?.is_some(),
            "Observed backend metadata disagrees with adoption"
        );
        ensure!(
            digests.values().contains(&record.digest),
            "Observed backend metadata has a different content digest"
        );
        let metadata = PortableRelPath::parse(
            &format!("pack/{}", record.metadata_path.as_str()),
            PathSyntax::ProjectContent,
        )?;
        let Some(crate::engine::snapshot::Observation::File(file)) =
            workspace.observations().entries().get(&metadata)
        else {
            anyhow::bail!("Adoption metadata is not a captured regular file")
        };
        metadata_updates.insert(record.metadata_path, ExpectedDigest::Sha256(file.content));
    }
    let mut documents = BTreeMap::from([
        (
            ManagedPath::IntentDocument,
            candidate.intent_document().bytes.clone(),
        ),
        (
            ManagedPath::LockDocument,
            candidate.lock_document().to_vec(),
        ),
    ]);
    if candidate.project().lock() == current.lock() {
        documents.insert(
            ManagedPath::LockDocument,
            workspace
                .read_document(
                    &PortableRelPath::parse("empack.lock", PathSyntax::ProjectContent)?,
                    cancel,
                )?
                .context("Captured adoption lock disappeared")?,
        );
    }
    crate::engine::backend::index::refresh_index_with_metadata_updates(
        &workspace,
        &BTreeSet::new(),
        &updates,
        &metadata_updates,
        &mut documents,
        cancel,
    )?;
    let observed =
        verification::observed_mutation_for(workspace.observations(), documents.keys().cloned())?;
    let mut desired = BTreeMap::new();
    for (target, bytes) in &documents {
        let ObservedPath::File(before) = &observed[target] else {
            anyhow::bail!("Adoption document is not an existing regular file")
        };
        desired.insert(
            target.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions: before.permissions,
            },
        );
    }
    let plan = verification::plan_mutation_files(&observed, &desired, &BTreeSet::new())?;
    documents.retain(|target, _| plan.expected().contains_key(target));
    verification::candidate_stage_limits(workspace.observations(), &plan)?;
    Ok(AdditionPreparation {
        workspace,
        candidate,
        plan,
        documents,
        content: BTreeMap::new(),
        references: BTreeSet::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        documents::DocumentCodec, mrpack::tests::project, project::ProjectReader,
        publication::RecoveryReader, snapshot::SnapshotLimits,
    };
    use std::fs;

    #[test]
    fn adoption_refreshes_metadata_index_entries_without_changing_their_role() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let project = project(false, false);
        fs::write(
            root.path().join("empack.yml"),
            DocumentCodec.encode_intent(project.intent()).unwrap(),
        )
        .unwrap();
        fs::write(
            root.path().join("empack.lock"),
            DocumentCodec.encode_lock(&project).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("pack/resourcepacks")).unwrap();
        for name in ["a.zip", "copy.zip", "b.zip"] {
            fs::write(
                root.path().join(format!("pack/resourcepacks/{name}")),
                b"payload",
            )
            .unwrap();
        }
        let metadata = format!(
            "filename='a.zip'\nside='client'\n[download]\nurl='https://example.com/unrelated-name.jar'\nhash-format='sha256'\nhash='{}'\n",
            ExpectedDigest::Sha256(Sha256::digest(b"payload").into()).hex()
        );
        fs::write(
            root.path().join("pack/resourcepacks/a.pw.toml"),
            metadata.as_bytes(),
        )
        .unwrap();
        let index = format!(
            "hash-format='sha1'\n[[files]]\nfile='resourcepacks/a.pw.toml'\nmetafile=true\nalias='retained'\ncustom='kept'\nhash='{}'\n",
            ExpectedDigest::Sha1(sha1::Sha1::digest(metadata.as_bytes()).into()).hex()
        );
        fs::write(root.path().join("pack/index.toml"), index.as_bytes()).unwrap();
        fs::write(
            root.path().join("pack/pack.toml"),
            b"name='Test'\n[index]\nfile='index.toml'\n",
        )
        .unwrap();
        let group = AdditionGroup::from_resolved(&project).unwrap();
        let cancel = Cancellation::default();
        let reader = ProjectReader::new(RecoveryReader::new(state.path().join("state")));
        let snapshot = reader
            .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
            .unwrap();
        plan_adoption(snapshot, &group, &cancel)
            .unwrap()
            .stage(&cancel)
            .unwrap()
            .publish(
                &Publisher::open(&state.path().join("state")).unwrap(),
                &cancel,
            )
            .unwrap();
        let index: toml::Value =
            toml::from_str(&fs::read_to_string(root.path().join("pack/index.toml")).unwrap())
                .unwrap();
        let entry = &index["files"][0];
        assert_eq!(entry["metafile"].as_bool(), Some(true));
        assert_eq!(entry["alias"].as_str(), Some("retained"));
        assert_eq!(entry["custom"].as_str(), Some("kept"));
        assert_eq!(entry["hash-format"].as_str(), Some("sha256"));
        assert_eq!(
            entry["hash"].as_str(),
            Some(
                ExpectedDigest::Sha256(Sha256::digest(metadata.as_bytes()).into())
                    .hex()
                    .as_str()
            )
        );
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/a.pw.toml")).unwrap(),
            metadata.as_bytes()
        );
        let snapshot = reader
            .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
            .unwrap();
        assert!(
            plan_adoption(snapshot, &group, &cancel)
                .unwrap()
                .plan
                .changes()
                .is_empty()
        );
        let mut index = index;
        index["files"][0]["metafile"] = toml::Value::Boolean(false);
        fs::write(
            root.path().join("pack/index.toml"),
            toml::to_string(&index).unwrap(),
        )
        .unwrap();
        let snapshot = reader
            .capture_addition(root.path(), &group, SnapshotLimits::default(), &cancel)
            .unwrap();
        assert!(
            plan_adoption(snapshot, &group, &cancel).is_err(),
            "a metadata update must not reinterpret a direct-file entry"
        );
    }
}
