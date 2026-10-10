//! Native release assembly shares build acquisition, verification and batch publication.
use super::*;
use crate::engine::{
    artifacts::ArchiveLimits,
    release::producer::{NativeReleaseOptions, NativeReleasePlan},
    staging::PrivateFile,
};
use empack_core::{
    distribution::{Consumer, Recipe},
    model::DistributionArchive,
    path::PortableRelPath,
};

pub(crate) fn supports(recipe: Recipe) -> bool {
    recipe.consumer() == Consumer::Empack
}

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
    ensure!(supports(recipe), "Unsupported native recipe");
    ensure!(
        artifact.as_str().ends_with(".empack"),
        "Native release output requires an .empack filename"
    );
    let (selection, _) = acquisition::select_game_inputs(
        workspace,
        recipe,
        &empack_core::inventory::OptionalPolicy::Preserve,
        cancel,
    )?;
    let owners = selection
        .entries()
        .iter()
        .map(|entry| entry.owner.clone())
        .collect();
    let mut content =
        capture_build_content(workspace, external, options.evidence, Some(&owners), cancel)?;
    content.acquired.retain(|key, _| {
        owners.contains(&ContentOwner::Dependency {
            key: key.dependency.clone(),
            slot: key.slot.clone(),
        })
    });
    let mut release_options = NativeReleaseOptions::from_project(&content.project)?;
    release_options.require_subscription =
        recipe.update_authority() == empack_core::distribution::UpdateAuthority::Empack;
    release_options.delivery = recipe.delivery();
    release_options.environments = recipe.environments();
    let plan = NativeReleasePlan::prepare(
        &content.project,
        &content.acquired,
        content.sources,
        release_options,
    )?;
    let mut output = PrivateFile::new()?;
    let verified = plan.write(output.file(), options.archive, options.limits, cancel)?;
    let receipt = batch::BuiltDistribution {
        modrinth_hosting: None,
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
