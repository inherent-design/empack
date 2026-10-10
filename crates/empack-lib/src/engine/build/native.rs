//! Native release assembly shares build acquisition, verification and batch publication.
use super::*;
use crate::engine::{
    artifacts::ArchiveLimits,
    release::producer::{NativeReleaseOptions, NativeReleasePlan},
    staging::PrivateFile,
};
use empack_core::{distribution::Recipe, model::DistributionArchive, path::PortableRelPath};

pub(super) struct NativeArchiveOptions {
    pub recipe: Recipe,
    pub archive: DistributionArchive,
    pub evidence: SourceEvidencePolicy,
    pub limits: ArchiveLimits,
}

pub(super) fn prepare_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &NativeArchiveOptions,
    cancel: &Cancellation,
) -> Result<(ArchiveCandidate, batch::BuiltDistribution)> {
    let recipe = options.recipe;
    ensure!(
        matches!(recipe, Recipe::EMPACK_REFERENCES | Recipe::EMPACK_BUNDLED),
        "Unsupported native recipe"
    );
    ensure!(
        artifact.as_str().ends_with(".empack"),
        "Native release output requires an .empack filename"
    );
    let content = capture_build_content(workspace, external, options.evidence, None, cancel)?;
    let mut release_options = NativeReleaseOptions::from_project(&content.project)?;
    release_options.delivery = recipe.delivery();
    let plan = NativeReleasePlan::prepare(
        &content.project,
        &content.acquired,
        content.sources,
        release_options,
    )?;
    let mut output = PrivateFile::new()?;
    let verified = plan.write(output.file(), options.archive, options.limits, cancel)?;
    let receipt = batch::BuiltDistribution {
        native_release: Some(plan.release().id().into()),
        target: recipe,
        artifact: artifact.clone(),
        bytes: verified.len(),
        content: plan.inventory().clone(),
        members: plan.archive_inventory().clone(),
        resolution: content.project.lock().clone(),
        conversions: Vec::new(),
        user_configuration: None,
        server_runtime: None,
    };
    Ok((
        ArchiveCandidate {
            artifact,
            archive: output,
            verified,
        },
        receipt,
    ))
}
