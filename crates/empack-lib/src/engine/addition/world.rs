//! Interpret a selected world archive as one dependency with independently verified members.
use super::{AcquiredFileInput, AcquiredFileSource, DirectFileInput, FileEvidence};
use crate::engine::{
    archive_source::ZipContentSource,
    artifacts::ArchiveLimits,
    content::{AcquiredContent, ContentPool, InitialObservation, SourceEvidencePolicy},
    mrpack::AcquiredBuildFile,
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    model::*,
    path::{InstallDestination, PathSyntax, PortableRelPath},
};

pub(super) async fn expand(
    scope: &mut WorkScope,
    input: DirectFileInput,
    archive: AcquiredBuildFile,
    limits: ArchiveLimits,
    policy: SourceEvidencePolicy,
) -> Result<RetainedOutput<Vec<AcquiredFileInput>>> {
    let address = archive.content.lease().id();
    let metadata = (limits.entries as u64)
        .checked_mul(4096)
        .context("World metadata estimate overflow")?;
    let inspect = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: metadata,
            open_files: 2,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: metadata,
            open_files: 2,
            ..Default::default()
        },
        move |cancel| {
            let source = ZipContentSource::open(&archive.content, limits, &cancel)?;
            let roots: Vec<_> = source
                .files()
                .filter(|(path, _)| {
                    path.as_str() == "level.dat" || path.as_str().ends_with("/level.dat")
                })
                .map(|(path, member)| (path.clone(), member.bytes))
                .collect();
            ensure!(
                roots.len() == 1 && roots[0].1 > 0,
                "Select an archive containing exactly one nonempty level.dat"
            );
            let prefix = roots[0]
                .0
                .as_str()
                .strip_suffix("level.dat")
                .unwrap()
                .to_owned();
            let members = source
                .files()
                .map(|(path, member)| {
                    ensure!(
                        path.as_str().starts_with(&prefix),
                        "World archive contains content outside its selected world root"
                    );
                    let relative = PortableRelPath::parse(
                        &path.as_str()[prefix.len()..],
                        PathSyntax::ProjectContent,
                    )?;
                    Ok((path.clone(), relative, member.bytes, member.permissions))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok::<_, anyhow::Error>((source, members))
        },
    )?;
    let ((mut source, members), metadata_permit) = scope
        .accept(inspect.wait().await?)?
        .transpose()?
        .into_parts();
    let mut pool = ContentPool::owned(scope, limits.total_bytes).await?;
    let mut files = Vec::new();
    for (member, relative, bytes, permissions) in members {
        let selected = member.clone();
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
                let (content, _) = source.acquire(
                    &selected,
                    &ExpectedContent {
                        digests: None,
                        size: Some(bytes),
                        accepted_observation: None,
                    },
                    policy,
                    InitialObservation::Accepted,
                    &cancel,
                )?;
                Ok::<_, anyhow::Error>((source, content))
            },
        )?;
        let retained = scope.accept(work.wait().await?)?.transpose()?;
        let mut next = None;
        let retained = retained.map(|(source, content)| {
            next = Some(source);
            content
        });
        source = next.context("World archive reader was lost")?;
        let content = pool
            .consolidate_owned(scope, AcquiredContent::retain_resources(retained)?)
            .await?;
        let placements = input
            .placements
            .as_slice()
            .iter()
            .map(|base| {
                Ok(Placement {
                    destination: InstallDestination::parse(&format!(
                        "{}/{}",
                        base.destination.relative().as_str(),
                        relative.as_str()
                    ))?,
                    layer: base.layer,
                    requirements: base.requirements.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        files.push(AcquiredFileInput {
            member: Some(FileSlot::parse(relative.as_str())?),
            key: input.key.clone(),
            title: input.title.clone(),
            kind: ContentKind::World,
            source: AcquiredFileSource::ArchiveMember {
                archive: address.clone(),
                member,
            },
            evidence: FileEvidence::AcceptObserved,
            requirements: input.requirements.clone(),
            placements: NonEmpty::new(placements)?,
            file: AcquiredBuildFile {
                content,
                permissions,
            },
        });
    }
    drop(source);
    let retained = (files.len() as u64)
        .checked_mul(4096)
        .context("World member estimate overflow")?
        .min(metadata);
    Ok(
        RetainedOutput::from_parts(files, metadata_permit).shrink_resources(ResourceRequest {
            memory_bytes: retained,
            ..Default::default()
        })?,
    )
}
