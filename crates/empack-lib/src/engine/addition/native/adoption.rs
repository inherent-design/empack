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
        updates.insert(record.metadata_path, ExpectedDigest::Sha256(file.content));
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
    crate::engine::backend::index::refresh_index_with_updates(
        &workspace,
        &BTreeSet::new(),
        &updates,
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
    })
}
