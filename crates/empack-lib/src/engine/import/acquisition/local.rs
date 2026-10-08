//! Exact local associations retain the imported source assertions and logical placement.
use super::*;
use crate::engine::acquisition::{LocalFileRequest, acquire_local_file};
use empack_core::identity::PinSelector;
use std::path::PathBuf;

pub struct ImportLocalFile {
    /// A displayed exact selector, declared destination, or unambiguous provider filename.
    pub selector: String,
    /// Absolute host path. The CLI resolves relative paths before calling the engine.
    pub source: PathBuf,
}

impl ImportContentKey {
    pub fn selector(&self) -> String {
        match self {
            Self::Declared(index) => format!("declared:{index}"),
            Self::Override(index) => format!("override:{index}"),
            Self::Provider { pin, filename } => {
                let (provider, selection) = match &pin.selection {
                    PinSelector::ModrinthVersion(id) => ("modrinth", id.to_string()),
                    PinSelector::CurseForgeFile(id) => ("curseforge", id.to_string()),
                };
                format!("provider:{provider}:{}:{selection}:{filename}", pin.project)
            }
        }
    }
}

impl ImportContentPlan {
    /// Resolve the complete association set before reading any selected payload. Only download
    /// obligations accept replacements; embedded members remain bound to their source archive.
    pub async fn acquire_local_files(
        &self,
        scope: &mut WorkScope,
        files: &[ImportLocalFile],
        mut provided: BTreeMap<ImportContentKey, AcquiredContent>,
        evidence: SourceEvidencePolicy,
    ) -> Result<BTreeMap<ImportContentKey, AcquiredContent>> {
        ensure!(
            files.len() <= self.limits.records,
            "Too many import associations"
        );
        let mut selected = BTreeMap::new();
        for file in files {
            scope.cancellation().check()?;
            ensure!(
                file.source.is_absolute(),
                "Import source must be an absolute selected file"
            );
            let matches = self
                .needs
                .iter()
                .filter(|need| {
                    if !matches!(need.source, ImportedAcquisition::Downloads(_)) {
                        return false;
                    }
                    let alias = match &need.key {
                        ImportContentKey::Declared(index) => {
                            self.imported.files[*index].destination.relative().as_str()
                        }
                        ImportContentKey::Provider { filename, .. } => filename,
                        ImportContentKey::Override(_) => return false,
                    };
                    file.selector == need.key.selector() || file.selector == alias
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Import selector '{}' must match exactly one download obligation; use its displayed exact selector",
                file.selector
            );
            let need = matches[0];
            ensure!(
                !provided.contains_key(&need.key)
                    && selected.insert(need.key.clone(), (need, file)).is_none(),
                "Repeated import association for {}",
                need.key.selector()
            );
        }
        if selected.is_empty() {
            return Ok(provided);
        }
        let mut pool = ContentPool::owned(scope, self.limits.total_bytes).await?;
        for (key, (need, file)) in selected {
            let content = acquire_local_file(
                scope,
                LocalFileRequest {
                    source: file.source.clone(),
                    expected: need.expected.clone(),
                    maximum: self.limits.transfer.file_bytes,
                    evidence,
                    initial: InitialObservation::RequireEvidence,
                },
            )
            .await?
            .content;
            provided.insert(key, pool.consolidate_owned(scope, content).await?);
        }
        Ok(provided)
    }
}
