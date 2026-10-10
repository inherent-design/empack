//! AllRequested publication across independently verified distribution candidates.
use super::{
    ArchiveCandidate, BuildAcquisitions, PreparedArtifact,
    client::{ClientOptions, prepare_client_archive},
    prepare_archives_publication, prepare_mrpack_recipe,
    server::{ServerOptions, prepare_server_archive},
};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::SourceEvidencePolicy, layout::CollisionIndex, mrpack::OptionalConversion,
        project::WorkspaceSnapshot, staging::PrivateFile, verification::observed_artifacts_for,
    },
};
use anyhow::{Result, ensure};
use empack_core::{
    distribution::{Consumer, Recipe, UpdateAuthority},
    files::ManagedPath,
    inventory::BuildInventory,
    model::NonEmpty,
    path::PortableRelPath,
};

/// An implemented distribution recipe and its explicit output. Further targets add recipes here;
/// runtime recipes remain limited to independently verified runtime preparations.
pub enum DistributionRequest {
    Native {
        artifact: PortableRelPath,
        recipe: Recipe,
        archive: empack_core::model::DistributionArchive,
        evidence: SourceEvidencePolicy,
        limits: crate::engine::artifacts::ArchiveLimits,
    },
    CurseForge {
        recipe: Recipe,
        artifact: PortableRelPath,
        options: super::curseforge::CurseForgeOptions,
    },
    Mrpack {
        recipe: Recipe,
        artifact: PortableRelPath,
        optional: OptionalConversion,
        evidence: SourceEvidencePolicy,
    },
    Prism {
        recipe: Recipe,
        artifact: PortableRelPath,
        options: ClientOptions,
    },
    Server {
        recipe: Recipe,
        artifact: PortableRelPath,
        options: ServerOptions,
        runtime: Box<crate::engine::server_runtime::PreparedServerRuntime>,
    },
}
impl DistributionRequest {
    fn artifact(&self) -> &PortableRelPath {
        match self {
            Self::Native { artifact, .. }
            | Self::CurseForge { artifact, .. }
            | Self::Mrpack { artifact, .. }
            | Self::Prism { artifact, .. }
            | Self::Server { artifact, .. } => artifact,
        }
    }
    fn target(&self) -> Recipe {
        match self {
            Self::Native { recipe, .. }
            | Self::CurseForge { recipe, .. }
            | Self::Mrpack { recipe, .. }
            | Self::Prism { recipe, .. }
            | Self::Server { recipe, .. } => *recipe,
        }
    }
    fn validate(&self) -> Result<()> {
        let recipe = self.target();
        let expected = match self {
            Self::Native { .. } => Consumer::Empack,
            Self::CurseForge { .. } => Consumer::CurseForge,
            Self::Mrpack { .. } => Consumer::Modrinth,
            Self::Prism { .. } => Consumer::Prism,
            Self::Server { .. } => Consumer::Server,
        };
        ensure!(
            recipe.consumer() == expected,
            "Recipe belongs to another consumer adapter"
        );
        ensure!(
            recipe.update_authority() == UpdateAuthority::Snapshot,
            "Consumer update authority requires a bound association or subscription"
        );
        Ok(())
    }
}

pub struct BuiltDistribution {
    /// Exact portable payload identity; archive hashes are a separate namespace.
    pub native_release: Option<String>,
    pub modrinth_hosting: Option<crate::engine::mrpack::HostingEligibility>,
    pub target: Recipe,
    pub artifact: PortableRelPath,
    pub bytes: u64,
    /// Pack-content inventory: bundled Prism launcher/template files are verified by its recipe too.
    pub content: BuildInventory,
    pub members: std::collections::BTreeMap<PortableRelPath, empack_core::files::FileContent>,
    pub resolution: empack_core::model::ResolutionLock,
    pub conversions: Vec<String>,
    pub user_configuration: Option<bool>,
    pub server_runtime: Option<crate::engine::server_runtime::ServerRuntimeEvidence>,
}
pub struct PreparedBuildBatch {
    publication: PreparedArtifact,
    artifacts: Vec<BuiltDistribution>,
}
impl PreparedBuildBatch {
    pub fn artifacts(&self) -> &[BuiltDistribution] {
        &self.artifacts
    }
    #[cfg(test)]
    pub(in crate::engine) fn publish(
        self,
        publisher: &crate::engine::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<crate::engine::publication::PublicationReceipt> {
        self.publication.publish(publisher, cancel)
    }
    pub(in crate::engine) fn publish_with_evidence(
        self,
        publisher: &crate::engine::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<(
        crate::engine::publication::PublicationReceipt,
        Vec<BuiltDistribution>,
    )> {
        let receipt = self.publication.publish(publisher, cancel)?;
        Ok((receipt, self.artifacts))
    }
}
/// No artifact becomes visible until every requested recipe succeeds. Cancellation or any
/// later recipe failure drops all private candidates; the one journal publishes their union.
pub fn prepare_build_batch(
    workspace: WorkspaceSnapshot,
    requests: NonEmpty<DistributionRequest>,
    external: &BuildAcquisitions,
    cancel: &Cancellation,
) -> Result<PreparedBuildBatch> {
    prepare_build_batch_with_cleanup(
        workspace,
        requests,
        external,
        &std::collections::BTreeSet::new(),
        cancel,
    )
}
/// Exact obsolete paths are verified and published with all successful candidates, never first.
pub(in crate::engine) fn prepare_build_batch_with_cleanup(
    workspace: WorkspaceSnapshot,
    requests: NonEmpty<DistributionRequest>,
    external: &BuildAcquisitions,
    removals: &std::collections::BTreeSet<ManagedPath>,
    cancel: &Cancellation,
) -> Result<PreparedBuildBatch> {
    let mut collisions = CollisionIndex::default();
    for request in requests.as_slice() {
        request.validate()?;
        collisions.insert_file(request.artifact())?;
    }
    observed_artifacts_for(
        workspace.observations(),
        requests
            .as_slice()
            .iter()
            .map(|request| ManagedPath::Artifact(request.artifact().clone())),
    )?;
    let mut candidates = Vec::new();
    let mut artifacts = Vec::new();
    for request in requests.as_slice() {
        cancel.check()?;
        let (candidate, evidence) = match request {
            DistributionRequest::Native {
                artifact,
                recipe,
                archive,
                evidence,
                limits,
            } => super::native::prepare_archive(
                &workspace,
                artifact.clone(),
                external,
                &super::native::NativeArchiveOptions {
                    recipe: *recipe,
                    archive: *archive,
                    evidence: *evidence,
                    limits: *limits,
                },
                cancel,
            )?,
            DistributionRequest::CurseForge {
                artifact, options, ..
            } => super::curseforge::prepare_archive(
                &workspace,
                artifact.clone(),
                external,
                options,
                cancel,
            )?,
            DistributionRequest::Mrpack {
                recipe,
                artifact,
                optional,
                evidence,
            } => {
                ensure!(
                    artifact.as_str().ends_with(".mrpack"),
                    "Mrpack output requires a .mrpack filename"
                );
                let plan = prepare_mrpack_recipe(
                    &workspace, external, *evidence, *optional, *recipe, cancel,
                )?;
                let mut archive = PrivateFile::new()?;
                let verified = plan.write(archive.file(), cancel)?;
                let evidence = BuiltDistribution {
                    native_release: None,
                    modrinth_hosting: Some(plan.hosting_eligibility().clone()),
                    target: request.target(),
                    artifact: artifact.clone(),
                    bytes: verified.len(),
                    content: plan.inventory().clone(),
                    members: plan.archive_inventory().clone(),
                    resolution: plan.resolution().clone(),
                    conversions: plan.conversions().to_vec(),
                    user_configuration: None,
                    server_runtime: None,
                };
                (
                    ArchiveCandidate {
                        artifact: artifact.clone(),
                        archive,
                        verified,
                    },
                    evidence,
                )
            }
            DistributionRequest::Server {
                recipe,
                artifact,
                options,
                runtime,
            } => {
                let (archive, built) = prepare_server_archive(
                    &workspace,
                    artifact.clone(),
                    external,
                    options,
                    runtime,
                    *recipe,
                    cancel,
                )?;
                let evidence = BuiltDistribution {
                    native_release: None,
                    modrinth_hosting: None,
                    target: request.target(),
                    artifact: artifact.clone(),
                    bytes: archive.verified.len(),
                    content: built.game.inventory().clone(),
                    members: built.inventory,
                    resolution: built.game.project().lock().clone(),
                    conversions: Vec::new(),
                    user_configuration: Some(built.user_configuration),
                    server_runtime: Some(built.runtime),
                };
                (archive, evidence)
            }
            DistributionRequest::Prism {
                recipe,
                artifact,
                options,
            } => {
                let built = prepare_client_archive(
                    &workspace,
                    artifact.clone(),
                    external,
                    options,
                    *recipe,
                    cancel,
                )?;
                let evidence = BuiltDistribution {
                    native_release: None,
                    modrinth_hosting: None,
                    target: request.target(),
                    artifact: artifact.clone(),
                    bytes: built.archive.verified.len(),
                    content: built.game.inventory().clone(),
                    members: built.inventory,
                    resolution: built.game.project().lock().clone(),
                    conversions: Vec::new(),
                    user_configuration: Some(built.user_configuration),
                    server_runtime: None,
                };
                (built.archive, evidence)
            }
        };
        artifacts.push(evidence);
        candidates.push(candidate);
    }
    let publication = prepare_archives_publication(workspace, candidates, removals, cancel)?;
    Ok(PreparedBuildBatch {
        publication,
        artifacts,
    })
}

#[cfg(test)]
mod tests;
