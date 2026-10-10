//! AllRequested publication across independently verified distribution candidates.
use super::{
    ArchiveCandidate, BuildAcquisitions, PreparedArtifact,
    client::{ClientOptions, prepare_client_archive, prepare_client_full_archive},
    prepare_archives_publication, prepare_mrpack,
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
    files::ManagedPath, inventory::BuildInventory, model::NonEmpty, path::PortableRelPath,
    projection::BuildTarget,
};

/// An implemented distribution recipe and its explicit output. Further targets add recipes here;
/// runtime recipes remain limited to independently verified runtime preparations.
pub enum DistributionRequest {
    CurseForge {
        artifact: PortableRelPath,
        options: super::curseforge::CurseForgeOptions,
    },
    Mrpack {
        artifact: PortableRelPath,
        optional: OptionalConversion,
        evidence: SourceEvidencePolicy,
    },
    Client {
        artifact: PortableRelPath,
        options: ClientOptions,
    },
    Server {
        artifact: PortableRelPath,
        options: ServerOptions,
        runtime: crate::engine::server_runtime::PreparedServerRuntime,
    },
    ServerFull {
        artifact: PortableRelPath,
        options: ServerOptions,
        runtime: crate::engine::server_runtime::PreparedServerRuntime,
    },
    ClientFull {
        artifact: PortableRelPath,
        options: ClientOptions,
    },
}
impl DistributionRequest {
    fn artifact(&self) -> &PortableRelPath {
        match self {
            Self::CurseForge { artifact, .. }
            | Self::Mrpack { artifact, .. }
            | Self::Client { artifact, .. }
            | Self::ClientFull { artifact, .. }
            | Self::Server { artifact, .. }
            | Self::ServerFull { artifact, .. } => artifact,
        }
    }
    fn target(&self) -> BuildTarget {
        match self {
            Self::CurseForge { .. } => BuildTarget::CurseForge,
            Self::Mrpack { .. } => BuildTarget::Mrpack,
            Self::Client { .. } => BuildTarget::Client,
            Self::ClientFull { .. } => BuildTarget::ClientFull,
            Self::Server { .. } => BuildTarget::Server,
            Self::ServerFull { .. } => BuildTarget::ServerFull,
        }
    }
}
pub struct BuiltDistribution {
    pub target: BuildTarget,
    pub artifact: PortableRelPath,
    pub bytes: u64,
    /// Pack-content inventory: full-client launcher/template files are verified by its recipe too.
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
            DistributionRequest::CurseForge { artifact, options } => {
                super::curseforge::prepare_archive(
                    &workspace,
                    artifact.clone(),
                    external,
                    options,
                    cancel,
                )?
            }
            DistributionRequest::Mrpack {
                artifact,
                optional,
                evidence,
            } => {
                ensure!(
                    artifact.as_str().ends_with(".mrpack"),
                    "Mrpack output requires a .mrpack filename"
                );
                let plan = prepare_mrpack(&workspace, external, *evidence, *optional, cancel)?;
                let mut archive = PrivateFile::new()?;
                let verified = plan.write(archive.file(), cancel)?;
                let evidence = BuiltDistribution {
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
            DistributionRequest::ServerFull {
                artifact,
                options,
                runtime,
            }
            | DistributionRequest::Server {
                artifact,
                options,
                runtime,
                ..
            } => {
                let references = matches!(request, DistributionRequest::Server { .. });
                let (archive, built) = prepare_server_archive(
                    &workspace,
                    artifact.clone(),
                    external,
                    options,
                    runtime,
                    references,
                    cancel,
                )?;
                let evidence = BuiltDistribution {
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
            DistributionRequest::ClientFull { artifact, options }
            | DistributionRequest::Client {
                artifact, options, ..
            } => {
                let built = if let DistributionRequest::Client { .. } = request {
                    prepare_client_archive(
                        &workspace,
                        artifact.clone(),
                        external,
                        options,
                        true,
                        cancel,
                    )?
                } else {
                    prepare_client_full_archive(
                        &workspace,
                        artifact.clone(),
                        external,
                        options,
                        cancel,
                    )?
                };
                let evidence = BuiltDistribution {
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
