//! Synchronization restores exact recorded selections; it never resolves an implicit upgrade.
mod native;
pub mod suspension;
pub(in crate::engine) mod recorded;
use super::documents::{DecodedIntent, DecodedLock, DocumentCodec, DocumentEdit, PreparedDocument};
use anyhow::{Result, ensure};
use empack_core::model::ResolvedProject;
pub(in crate::engine) use native::plan_synchronization_with_resolution;
pub use native::{PreparedSynchronization, SynchronizationReceipt, prepare_synchronization};

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
            intent: PreparedDocument {
                expected: source.raw_revision(),
                bytes: source.original().to_vec(),
                edit: DocumentEdit::Unchanged,
            },
            lock,
            preserve_lock,
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
            intent: PreparedDocument {
                expected: source.raw_revision(),
                bytes: source.original().to_vec(),
                edit: DocumentEdit::Unchanged,
            },
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
            intent: PreparedDocument {
                expected: source.raw_revision(),
                bytes: source.original().to_vec(),
                edit: DocumentEdit::Unchanged,
            },
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
                desired.roots.values_mut().next().unwrap().version =
                    VersionIntent::Exact(PinSelector::ModrinthVersion(
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
