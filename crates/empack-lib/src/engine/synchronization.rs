//! Synchronization restores exact recorded selections; it never resolves an implicit upgrade.
mod native;
pub(in crate::engine) mod recorded;
pub mod suspension;
use super::documents::{DecodedIntent, DecodedLock, DocumentCodec, PreparedDocument};
use anyhow::{Result, ensure};
use empack_core::model::ResolvedProject;
pub use native::{PreparedSynchronization, SynchronizationReceipt, prepare_synchronization};
pub(in crate::engine) use native::{plan_selection_intent, plan_synchronization_with_resolution};

/// Intent edits requiring new provider/content/runtime evidence cannot reuse the old lock.
#[derive(Debug, thiserror::Error)]
#[error("Synchronization requires resolution before publication: {0}")]
pub struct ResolutionRequired(#[source] pub empack_core::model::ModelError);

/// A coherent restoration candidate. Native observation and publication remain separate.
pub struct SynchronizationCandidate {
    project: ResolvedProject,
    intent: PreparedDocument,
    lock: Vec<u8>,
    preserve_lock: bool,
}
impl SynchronizationCandidate {
    /// Rebind authoring-only edits while retaining every exact selection, content assertion and
    /// graph edge. Changed semantic requirements fail until an explicit resolver satisfies them.
    pub fn prepare(source: &DecodedIntent, prior: &DecodedLock) -> Result<Self> {
        let mut lock =
            empack_core::synchronization::rebind_prior_aliases(source.intent(), prior.lock())?;
        let preserve_lock = lock.intent_revision == source.semantic_revision();
        if !preserve_lock
            && source
                .intent()
                .roots
                .values()
                .any(|root| matches!(root.source, empack_core::model::SourceIntent::Search { .. }))
        {
            return Err(ResolutionRequired(empack_core::model::ModelError(
                "Changed unresolved search intent requires fresh resolution".into(),
            ))
            .into());
        }
        lock.intent_revision = source.semantic_revision();
        let project =
            ResolvedProject::validate(source.intent().clone(), lock, source.semantic_revision())
                .map_err(ResolutionRequired)?;
        let lock = DocumentCodec.encode_lock(&project)?;
        Ok(Self {
            project,
            intent: DocumentCodec.replace_intent(source, source.intent())?,
            lock,
            preserve_lock,
        })
    }
    /// Change provider pin intent without selecting versions or inspecting installed payloads.
    pub fn prepare_selection_intent(
        source: &DecodedIntent,
        prior: &DecodedLock,
        keys: &empack_core::model::NonEmpty<empack_core::model::DependencyKey>,
        pinned: bool,
    ) -> Result<Self> {
        use empack_core::model::{DependencySelection, SourceIntent};
        let current = prior.bind(source)?;
        let mut intent = current.intent().clone();
        let mut unique = std::collections::BTreeSet::new();
        for key in keys.as_slice() {
            ensure!(unique.insert(key), "Repeated pin key: {}", key.as_str());
            let root = intent.roots.get_mut(key).ok_or_else(|| {
                anyhow::anyhow!("Pin requires an explicit dependency: {}", key.as_str())
            })?;
            let selected = &current.lock().dependencies[key];
            ensure!(
                matches!(
                    root.source,
                    SourceIntent::Provider(_) | SourceIntent::ProviderSelector(_)
                ),
                "Pin requires a resolved provider source: {}",
                key.as_str()
            );
            if pinned {
                root.selection = DependencySelection::ProviderPin(
                    selected
                        .selected
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Provider selection missing"))?
                        .selection
                        .clone(),
                );
                // An automatic override would be inapplicable beside an exact pin.
                root.releases = None;
            } else {
                root.selection = DependencySelection::Automatic;
            }
        }
        let document = DocumentCodec.replace_intent(source, &intent)?;
        let decoded = document.decode("selection intent")?;
        let mut lock = current.lock().clone();
        lock.intent_revision = decoded.semantic_revision();
        let project = ResolvedProject::validate(intent, lock, decoded.semantic_revision())?;
        Ok(Self {
            lock: DocumentCodec.encode_lock(&project)?,
            preserve_lock: project.lock() == prior.lock(),
            project,
            intent: document,
        })
    }
    /// Bind an explicit runtime migration to the original author documents.
    /// Only runtime requests and provider replacements may change authoring intent.
    pub fn prepare_migration(
        source: &DecodedIntent,
        prior: &DecodedLock,
        proposed: &ResolvedProject,
    ) -> Result<Self> {
        let current = prior.bind(source)?;
        let mut allowed = current.intent().clone();
        allowed.runtime = proposed.intent().runtime.clone();
        for (key, root) in &mut allowed.roots {
            let next =
                proposed.intent().roots.get(key).ok_or_else(|| {
                    anyhow::anyhow!("Migration cannot remove an explicit dependency")
                })?;
            if root.source != next.source {
                ensure!(
                    matches!(
                        root.source,
                        empack_core::model::SourceIntent::Provider(_)
                            | empack_core::model::SourceIntent::ProviderSelector(_)
                    ) && matches!(next.source, empack_core::model::SourceIntent::Provider(_)),
                    "Migration replacements require explicit provider identities"
                );
                root.source = next.source.clone();
                root.selection = empack_core::model::DependencySelection::Automatic;
            }
        }
        ensure!(
            &allowed == proposed.intent(),
            "Migration changes unrelated authoring intent"
        );
        empack_core::synchronization::SynchronizationResolution::prepare(
            proposed.intent(),
            prior.lock(),
            proposed,
        )?;
        Ok(Self {
            project: proposed.clone(),
            intent: DocumentCodec.replace_intent(source, proposed.intent())?,
            lock: DocumentCodec.encode_lock(proposed)?,
            preserve_lock: proposed.lock() == prior.lock(),
        })
    }
    /// Bind freshly resolved changed intent without upgrading still-valid prior selections.
    pub fn prepare_resolved(
        source: &DecodedIntent,
        prior: &DecodedLock,
        proposed: &ResolvedProject,
    ) -> Result<Self> {
        ensure!(
            proposed.lock().intent_revision == source.semantic_revision(),
            "Fresh synchronization resolution has another intent revision"
        );
        empack_core::synchronization::SynchronizationResolution::prepare(
            source.intent(),
            prior.lock(),
            proposed,
        )?;
        Ok(Self {
            project: proposed.clone(),
            intent: DocumentCodec.replace_intent(source, source.intent())?,
            lock: DocumentCodec.encode_lock(proposed)?,
            preserve_lock: proposed.lock() == prior.lock(),
        })
    }
    /// Create a first lock from fresh resolution; pre-existing native content remains unowned.
    pub fn prepare_initial(source: &DecodedIntent, proposed: &ResolvedProject) -> Result<Self> {
        ensure!(
            proposed.lock().intent_revision == source.semantic_revision(),
            "Initial synchronization resolution has another intent revision"
        );
        let mut empty = proposed.lock().clone();
        empty.dependencies.clear();
        empty.required_edges.clear();
        empty.coverage.clear();
        empack_core::synchronization::SynchronizationResolution::prepare(
            source.intent(),
            &empty,
            proposed,
        )?;
        Ok(Self {
            project: proposed.clone(),
            intent: DocumentCodec.replace_intent(source, source.intent())?,
            lock: DocumentCodec.encode_lock(proposed)?,
            preserve_lock: false,
        })
    }
    pub(in crate::engine) fn prepare_input(
        source: &DecodedIntent,
        prior: Option<&DecodedLock>,
        proposed: Option<&ResolvedProject>,
    ) -> Result<Self> {
        match (prior, proposed) {
            (Some(prior), Some(proposed)) => Self::prepare_resolved(source, prior, proposed),
            (Some(prior), None) => Self::prepare(source, prior),
            (None, Some(proposed)) => Self::prepare_initial(source, proposed),
            (None, None) => {
                anyhow::bail!("Synchronization needs fresh resolution when empack.lock is absent")
            }
        }
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn intent_document(&self) -> &PreparedDocument {
        &self.intent
    }
    pub fn lock_document(&self) -> &[u8] {
        &self.lock
    }
    /// The native adapter preserves the original raw lock bytes when no rebinding is needed.
    pub fn preserves_lock_document(&self) -> bool {
        self.preserve_lock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::documents::DocumentEdit;
    use crate::engine::{addition::tests::fixture, mrpack::tests::project};
    use empack_core::{identity::PinSelector, model::*};
    fn candidate(
        current: &ResolvedProject,
        desired: &ProjectIntent,
    ) -> Result<SynchronizationCandidate> {
        let mut raw = b"# author comment\n".to_vec();
        raw.extend(DocumentCodec.encode_intent(desired)?);
        SynchronizationCandidate::prepare(
            &DocumentCodec.decode_intent(&raw, "intent")?,
            &DocumentCodec.decode_prior_lock(&DocumentCodec.encode_lock(current)?, "lock")?,
        )
    }
    #[test]
    fn sync_cannot_discard_negative_evidence_for_a_retained_pin() {
        use empack_core::{
            identity::{ModrinthProjectId, ProviderProjectId},
            path::InstallDestination,
        };
        let project = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
        let mut prior = project.lock().clone();
        prior
            .dependencies
            .values_mut()
            .next()
            .unwrap()
            .incompatibilities
            .push(ProviderIncompatibility {
                project: Some(ProviderProjectId::Modrinth(
                    ModrinthProjectId::parse("Absent01").unwrap(),
                )),
                selection: None,
            });
        let mut intent = project.intent().clone();
        let mut locked = project.lock().clone();
        let dep = locked.dependencies.values_mut().next().unwrap();
        let mut files = dep.files.as_slice().to_vec();
        let mut placement = files[0].placements.as_slice()[0].clone();
        placement.destination = InstallDestination::parse("mods/moved.jar").unwrap();
        files[0].placements = NonEmpty::new(vec![placement]).unwrap();
        dep.files = NonEmpty::new(files).unwrap();
        intent.roots.values_mut().next().unwrap().placement = PlacementIntent::ByFile(
            dep.files
                .as_slice()
                .iter()
                .map(|file| (file.slot.clone(), file.placements.clone()))
                .collect(),
        );
        let proposed =
            ResolvedProject::validate(intent.clone(), locked, project.lock().intent_revision)
                .unwrap();
        let error = empack_core::synchronization::SynchronizationResolution::prepare(
            &intent, &prior, &proposed,
        )
        .unwrap_err();
        assert!(error.to_string().contains("incompatibilities"), "{error}");
    }
    #[test]
    fn ordinary_sync_preserves_exact_selections_unlisted_content_and_source_evidence() {
        let current = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("unlisted", "Project2", "Version2"),
            ],
            &["root"],
            &[("root", "unlisted")],
            true,
        );
        let synced = candidate(&current, current.intent()).unwrap();
        assert_eq!(synced.project().lock(), current.lock());
        assert!(synced.preserves_lock_document());
        assert_eq!(synced.intent_document().edit, DocumentEdit::Unchanged);
        assert!(
            synced
                .intent_document()
                .bytes
                .starts_with(b"# author comment\n")
        );
        let weak = project(true, false);
        let synced = candidate(&weak, weak.intent()).unwrap();
        assert_eq!(synced.project().lock(), weak.lock());
    }
    #[test]
    fn authoring_metadata_edits_rebind_the_lock_without_resolving_new_files() {
        let current = project(false, false);
        let mut desired = current.intent().clone();
        desired.metadata.name = "Renamed display title".into();
        let synced = candidate(&current, &desired).unwrap();
        assert!(!synced.preserves_lock_document());
        assert_eq!(synced.project().intent(), &desired);
        assert_eq!(
            synced.project().lock().dependencies,
            current.lock().dependencies
        );
        assert_eq!(synced.project().lock().coverage, current.lock().coverage);
        assert_eq!(
            synced.project().lock().required_edges,
            current.lock().required_edges
        );
        let source = DocumentCodec
            .decode_intent(&synced.intent_document().bytes, "result")
            .unwrap();
        DocumentCodec
            .decode_lock(synced.lock_document(), &source, "result")
            .unwrap();
    }
    #[test]
    fn changed_pins_and_runtime_require_resolution_instead_of_relabeling_old_evidence() {
        let current = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
        for runtime in [false, true] {
            let mut desired = current.intent().clone();
            if runtime {
                desired.runtime.minecraft = GameVersion::parse("1.21.1").unwrap();
            } else {
                desired.roots.values_mut().next().unwrap().selection =
                    DependencySelection::ProviderPin(PinSelector::ModrinthVersion(
                        empack_core::identity::ModrinthVersionId::parse("Version2").unwrap(),
                    ));
            }
            assert!(
                candidate(&current, &desired)
                    .err()
                    .unwrap()
                    .downcast_ref::<ResolutionRequired>()
                    .is_some()
            );
        }
    }
    #[test]
    fn root_demotion_rebinds_intent_without_inventing_deletion_authority() {
        let current = project(false, false);
        let mut desired = current.intent().clone();
        desired.roots.clear();
        let synced = candidate(&current, &desired).unwrap();
        assert!(synced.project().intent().roots.is_empty());
        assert_eq!(
            synced.project().lock().dependencies,
            current.lock().dependencies
        );
    }
}

#[cfg(test)]
mod resolution_tests;
