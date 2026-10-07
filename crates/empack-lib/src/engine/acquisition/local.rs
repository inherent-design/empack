//! Explicit host-file reads into private verified content, without project mutation authority.
use super::{InitialObservation, SourceEvidencePolicy, validate_expectation, verify_stream};
use crate::engine::{
    mrpack::AcquiredBuildFile,
    native,
    resources::ResourceRequest,
    runtime::WorkScope,
    snapshot::{FileObservation, ProjectReadRoot, observe_file},
};
use anyhow::{Context, Result, ensure};
use empack_core::{files::FilePermissions, model::ExpectedContent};
use std::{fs::File, io::Seek, path::PathBuf};

pub struct LocalFileRequest {
    /// An explicitly selected absolute host path, never persisted as project intent.
    pub source: PathBuf,
    pub expected: ExpectedContent,
    pub maximum: u64,
    pub evidence: SourceEvidencePolicy,
    pub initial: InitialObservation,
}
struct SelectedFile {
    root: ProjectReadRoot,
    name: String,
    file: File,
    observed: FileObservation,
}
impl SelectedFile {
    fn capture(
        source: PathBuf,
        maximum: u64,
        cancel: &crate::application::process_runtime::Cancellation,
    ) -> Result<Self> {
        cancel.check()?;
        ensure!(
            source.is_absolute(),
            "Local source must be an absolute selected path"
        );
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .context("Local source must name a UTF-8 file")?
            .to_owned();
        let root = ProjectReadRoot::open(source.parent().context("Local source has no parent")?)?;
        let mut file = native::open_file(&root.directory, &name)?;
        let observed = observe_file(&mut file, maximum, cancel)?;
        root.check_binding()?;
        file.rewind()?;
        Ok(Self {
            root,
            name,
            file,
            observed,
        })
    }
    fn verify(
        mut self,
        request: LocalFileRequest,
        cancel: &crate::application::process_runtime::Cancellation,
    ) -> Result<AcquiredBuildFile> {
        self.root.check_binding()?;
        ensure!(
            native::identity(&native::open_file(&self.root.directory, &self.name)?)?
                == self.observed.object,
            "Selected local file was replaced before acquisition"
        );
        let content = verify_stream(
            &mut self.file,
            &request.expected,
            self.observed.bytes,
            request.evidence,
            request.initial,
            cancel,
        )?;
        ensure!(
            content.lease().id().bytes() == &self.observed.content
                && content.lease().len() == self.observed.bytes,
            "Selected local file changed during acquisition"
        );
        let mut current = native::open_file(&self.root.directory, &self.name)?;
        ensure!(
            observe_file(&mut current, self.observed.bytes, cancel)? == self.observed,
            "Selected local file changed during acquisition"
        );
        self.root.check_binding()?;
        Ok(AcquiredBuildFile {
            content,
            permissions: FilePermissions {
                readonly: self.observed.readonly,
                #[cfg(unix)]
                executable: self.observed.mode & 0o111 != 0,
                #[cfg(not(unix))]
                executable: false,
            },
        })
    }
}
/// Capture only the selected regular file, then charge its actual size before private copying.
/// Blocking filesystem reads are cooperatively cancelled between bounded read operations.
pub async fn acquire_local_file(
    scope: &mut WorkScope,
    request: LocalFileRequest,
) -> Result<AcquiredBuildFile> {
    validate_expectation(
        &request.expected,
        request.maximum,
        request.evidence,
        request.initial,
    )?;
    let source = request.source.clone();
    let maximum = request.maximum;
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 128 << 10,
            open_files: 4,
            ..Default::default()
        },
        ResourceRequest {
            open_files: 2,
            ..Default::default()
        },
        move |cancel| SelectedFile::capture(source, maximum, &cancel),
    )?;
    let selected = scope.accept(work.wait().await?)?.transpose()?;
    let bytes = selected.observed.bytes;
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 256 << 10,
            scratch_bytes: bytes,
            open_files: 6,
        },
        ResourceRequest {
            scratch_bytes: bytes,
            open_files: 1,
            ..Default::default()
        },
        move |cancel| {
            let (selected, _capture) = selected.into_parts();
            selected.verify(request, &cancel)
        },
    )?;
    let (mut acquired, mut permit) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    acquired.content.retain_reservation(&mut permit)?;
    Ok(acquired)
}

#[cfg(test)]
mod tests;
