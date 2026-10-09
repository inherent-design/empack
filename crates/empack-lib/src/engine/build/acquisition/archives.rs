//! One verified provider archive supplies exact members through a retained bounded reader.
use super::*;
use crate::engine::{
    archive_source::ZipContentSource, artifacts::ArchiveLimits, content::AcquiredContent,
    resources::ResourceRequest,
};

pub(super) async fn extract(
    scope: &mut WorkScope,
    archive: AcquiredContent,
    needs: Vec<AcquisitionNeed>,
    evidence: SourceEvidencePolicy,
    limits: ArchiveLimits,
    pool: &mut ContentPool,
) -> Result<Vec<(AcquisitionKey, AcquiredBuildFile)>> {
    let metadata = (limits.entries as u64)
        .checked_mul(4096)
        .context("World archive metadata estimate overflow")?;
    let retained = ResourceRequest {
        memory_bytes: metadata,
        open_files: 2,
        ..Default::default()
    };
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            ..retained
        },
        retained,
        move |cancel| ZipContentSource::open(&archive, limits, &cancel),
    )?;
    let (mut source, _reader_permit) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    let mut files = Vec::new();
    for need in needs {
        let BuildContentSource::ProviderArchiveMember { archive, member } = need.source else {
            anyhow::bail!("Expected provider archive member obligation")
        };
        let bytes = need
            .expected
            .size
            .context("World member has no recorded size")?;
        ensure!(
            bytes <= limits.file_bytes,
            "World member exceeds byte limit"
        );
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 256 << 10,
                open_files: 5,
                scratch_bytes: bytes,
            },
            ResourceRequest {
                open_files: 1,
                scratch_bytes: bytes,
                ..Default::default()
            },
            move |cancel| {
                let (content, permissions) = source.acquire(
                    &member,
                    &need.expected,
                    evidence,
                    InitialObservation::RequireEvidence,
                    &cancel,
                )?;
                content.provider_member_policy(&archive, &member, evidence)?;
                Ok::<_, anyhow::Error>((source, content, permissions))
            },
        )?;
        let retained = scope.accept(work.wait().await?)?.transpose()?;
        let mut reader = None;
        let mut permissions = None;
        let retained = retained.map(|(source, content, mode)| {
            reader = Some(source);
            permissions = Some(mode);
            content
        });
        source = reader.context("World archive reader was lost")?;
        let content = pool
            .consolidate_owned(scope, AcquiredContent::retain_resources(retained)?)
            .await?;
        files.push((
            need.key,
            AcquiredBuildFile {
                content,
                permissions: permissions.context("World member permissions were lost")?,
            },
        ));
    }
    Ok(files)
}
