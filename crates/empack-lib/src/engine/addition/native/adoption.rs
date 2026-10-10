//! Adoption verifies observed payloads and publishes descriptions; it never rewrites game bytes.
use super::*;

pub(in crate::engine) fn plan_adoption(
    workspace: MutationSnapshot,
    group: &AdditionGroup,
    cancel: &Cancellation,
) -> Result<AdditionPreparation> {
    cancel.check()?;
    let workspace = workspace.into_workspace();
    let current = workspace
        .prior_lock()
        .map(|lock| lock.bind(workspace.intent()))
        .transpose()?;
    let candidate =
        AdditionCandidate::prepare_adoption(workspace.intent(), workspace.prior_lock(), group)?;
    let mut placements = BTreeSet::new();
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
                workspace.verify_file(&path, &file.expected, cancel)?;
                ensure!(
                    placements.insert(target),
                    "Adoption has duplicate placements"
                );
            }
        }
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
    if current
        .as_ref()
        .is_some_and(|current| candidate.project().lock() == current.lock())
    {
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
    let observed =
        verification::observed_mutation_for(workspace.observations(), documents.keys().cloned())?;
    let mut desired = BTreeMap::new();
    for (target, bytes) in &documents {
        let permissions = match &observed[target] {
            ObservedPath::File(before) => before.permissions,
            ObservedPath::Absent if *target == ManagedPath::LockDocument => {
                empack_core::files::FilePermissions {
                    readonly: false,
                    executable: false,
                }
            }
            _ => anyhow::bail!("Adoption document is not a regular file"),
        };
        desired.insert(
            target.clone(),
            FileContent {
                content: ContentId::from_sha256(Sha256::digest(bytes).into()),
                bytes: bytes.len() as u64,
                permissions,
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
    use empack_core::digest::ExpectedDigest;
    use std::fs;

    #[test]
    fn adoption_preserves_foreign_metadata_without_observing_it() {
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
        assert_eq!(
            fs::read(root.path().join("pack/index.toml")).unwrap(),
            index.as_bytes()
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
        fs::write(root.path().join("pack/index.toml"), b"invalid = [").unwrap();
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
    }
}
