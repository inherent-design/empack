//! Explicit per-role decisions reuse document semantics and bounded native file acquisition.
use super::*;
use crate::engine::{
    acquisition::{LocalFileRequest, acquire_local_file},
    content::InitialObservation,
    documents::{DocumentCodec, ProviderFileSelection},
};
use empack_core::model::ExpectedContent;
use std::io::Read;

pub(super) async fn read(
    session: &dyn Session,
    source: PathBuf,
) -> Result<crate::engine::runtime::RetainedOutput<ProviderFileSelection>> {
    initialize::discover(session, move |mut scope| async move {
        let acquired = acquire_local_file(
            &mut scope,
            LocalFileRequest {
                source,
                expected: ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                maximum: 1 << 20,
                evidence: SourceEvidencePolicy::Compatibility,
                initial: InitialObservation::Accepted,
            },
        )
        .await?;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 1,
                memory_bytes: 16 << 20,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 16 << 20,
                ..Default::default()
            },
            move |cancel| {
                cancel.check()?;
                let mut bytes = Vec::new();
                acquired
                    .content
                    .lease()
                    .open()
                    .take((1 << 20) + 1)
                    .read_to_end(&mut bytes)?;
                let selected = DocumentCodec.decode_provider_files(&bytes, "--file-plan")?;
                cancel.check()?;
                Ok::<_, anyhow::Error>(selected)
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await
}
