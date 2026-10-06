//! AllRequested publication across independently verified distribution candidates.
use super::{
    ArchiveCandidate, BuildAcquisitions, PreparedArtifact,
    client::{ClientBootstrap, ClientOptions, prepare_client_archive, prepare_client_full_archive},
    prepare_archives_publication, prepare_mrpack,
    server::{ServerBootstrap, ServerOptions, prepare_server_archive},
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
    Mrpack {
        artifact: PortableRelPath,
        optional: OptionalConversion,
        evidence: SourceEvidencePolicy,
    },
    Client {
        artifact: PortableRelPath,
        options: ClientOptions,
        bootstrap: ClientBootstrap,
    },
    Server {
        artifact: PortableRelPath,
        options: ServerOptions,
        runtime: crate::engine::server_runtime::PreparedServerRuntime,
        bootstrap: ServerBootstrap,
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
            Self::Mrpack { artifact, .. }
            | Self::Client { artifact, .. }
            | Self::ClientFull { artifact, .. }
            | Self::Server { artifact, .. }
            | Self::ServerFull { artifact, .. } => artifact,
        }
    }
    fn target(&self) -> BuildTarget {
        match self {
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
    pub observed: Vec<crate::engine::mrpack::ObservedFileEvidence>,
    pub backend_comparisons: Vec<crate::engine::backend::BackendDigestComparison>,
    pub conversions: Vec<String>,
    pub user_configuration: Option<bool>,
    pub toolchain: Vec<crate::engine::bootstrap_tools::InstallerRelease>,
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
    pub fn publish(
        self,
        publisher: &crate::engine::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<crate::engine::publication::PublicationReceipt> {
        self.publication.publish(publisher, cancel)
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
                    observed: plan.observed().to_vec(),
                    backend_comparisons: plan.backend_comparisons().to_vec(),
                    conversions: plan.conversions().to_vec(),
                    user_configuration: None,
                    toolchain: Vec::new(),
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
                let bootstrap = if let DistributionRequest::Server { bootstrap, .. } = request {
                    Some(bootstrap)
                } else {
                    None
                };
                let (archive, built) = prepare_server_archive(
                    &workspace,
                    artifact.clone(),
                    external,
                    options,
                    runtime,
                    bootstrap,
                    cancel,
                )?;
                let evidence = BuiltDistribution {
                    target: request.target(),
                    artifact: artifact.clone(),
                    bytes: archive.verified.len(),
                    content: built.game.inventory().clone(),
                    members: built.inventory,
                    resolution: built.game.project().lock().clone(),
                    observed: built.game.observed().to_vec(),
                    backend_comparisons: built.game.backend_comparisons().to_vec(),
                    conversions: Vec::new(),
                    user_configuration: Some(built.user_configuration),
                    toolchain: built.toolchain,
                    server_runtime: Some(built.runtime),
                };
                (archive, evidence)
            }
            DistributionRequest::ClientFull { artifact, options }
            | DistributionRequest::Client {
                artifact, options, ..
            } => {
                let built = if let DistributionRequest::Client { bootstrap, .. } = request {
                    prepare_client_archive(
                        &workspace,
                        artifact.clone(),
                        external,
                        options,
                        Some(bootstrap),
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
                    observed: built.game.observed().to_vec(),
                    backend_comparisons: built.game.backend_comparisons().to_vec(),
                    conversions: Vec::new(),
                    user_configuration: Some(built.user_configuration),
                    toolchain: built.toolchain,
                    server_runtime: None,
                };
                (built.archive, evidence)
            }
        };
        artifacts.push(evidence);
        candidates.push(candidate);
    }
    let publication = prepare_archives_publication(workspace, candidates, cancel)?;
    Ok(PreparedBuildBatch {
        publication,
        artifacts,
    })
}

#[cfg(test)]
mod tests;
