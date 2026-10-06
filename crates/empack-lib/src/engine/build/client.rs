//! Full launcher distributions: selected game bytes, captured templates and exact components.
use super::{
    BuildAcquisitions, PreparedArtifact,
    materialized::{MaterializedGame, prepare_game_content},
};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, write_archive},
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        layout::CollisionIndex,
        mrpack::AcquiredBuildFile,
        project::WorkspaceSnapshot,
        snapshot::SnapshotLimits,
        staging::{MutableStage, PrivateFile},
        templates::{TemplateOptions, prepare_templates},
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::{FileContent, FilePermissions},
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent, LoaderKind, RuntimeResolution},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

pub struct ClientFullOptions {
    pub archive: DistributionArchive,
    pub optional: OptionalPolicy,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub limits: ArchiveLimits,
}
/// Complete selected pack content and launcher declarations, not an offline Minecraft installation.
pub struct PreparedClientBuild {
    publication: PreparedArtifact,
    game: MaterializedGame,
    inventory: BTreeMap<PortableRelPath, FileContent>,
    user_configuration: bool,
}
impl PreparedClientBuild {
    pub fn game(&self) -> &MaterializedGame {
        &self.game
    }
    pub fn inventory(&self) -> &BTreeMap<PortableRelPath, FileContent> {
        &self.inventory
    }
    /// A captured user configuration replaced the default. Its arbitrary commands remain user input.
    pub fn uses_user_configuration(&self) -> bool {
        self.user_configuration
    }
    pub fn bytes(&self) -> u64 {
        self.publication.bytes
    }
    pub fn publish(
        self,
        publisher: &crate::engine::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<crate::engine::publication::PublicationReceipt> {
        self.publication.publish(publisher, cancel)
    }
}
fn path(value: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(value, PathSyntax::ArchiveMember)?)
}
fn generated(bytes: &[u8], cancel: &Cancellation) -> Result<AcquiredBuildFile> {
    Ok(AcquiredBuildFile {
        content: verify_stream(
            &mut &*bytes,
            &ExpectedContent {
                digests: None,
                size: Some(bytes.len() as u64),
                accepted_observation: None,
            },
            16 << 20,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            cancel,
        )?,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    })
}
fn loader(runtime: &RuntimeResolution) -> Result<Option<(&'static str, String)>> {
    let uid = match runtime.loader {
        LoaderKind::Vanilla => return Ok(None),
        LoaderKind::Fabric => "net.fabricmc.fabric-loader",
        LoaderKind::Quilt => "org.quiltmc.quilt-loader",
        LoaderKind::Forge => "net.minecraftforge",
        LoaderKind::NeoForge => "net.neoforged",
    };
    let version = runtime
        .loader_version
        .as_ref()
        .context("Client loader lacks exact version")?
        .as_str();
    let version = if runtime.loader == LoaderKind::Forge {
        crate::empack::versions::canonicalize_forge_loader_version(
            runtime.minecraft.as_str(),
            version,
        )
    } else {
        version.into()
    };
    Ok(Some((uid, version)))
}
fn profile(runtime: &RuntimeResolution) -> Result<Vec<u8>> {
    let mut components =
        vec![json!({"uid":"net.minecraft","version":runtime.minecraft.as_str(),"important":true})];
    if let Some((uid, version)) = loader(runtime)? {
        components.push(json!({"uid":uid,"version":version}));
    }
    Ok(serde_json::to_vec_pretty(
        &json!({"formatVersion":1,"components":components}),
    )?)
}
/// Inspect actual component bytes, including a user-owned replacement. User extra components
/// may extend the profile; they cannot disable, duplicate or change the locked game/loader.
fn verify_profile(bytes: &[u8], runtime: &RuntimeResolution) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(
        value["formatVersion"] == 1,
        "Unsupported client component format"
    );
    let components = value["components"]
        .as_array()
        .context("Client components are not an array")?;
    let expected_loader = loader(runtime)?;
    let mut seen = BTreeSet::new();
    let mut game = false;
    let mut selected_loader = false;
    for component in components {
        let uid = component["uid"]
            .as_str()
            .context("Client component has no UID")?;
        let version = component["version"]
            .as_str()
            .filter(|value| !value.is_empty())
            .context("Client component lacks an exact version")?;
        ensure!(seen.insert(uid), "Duplicate client component");
        if uid == "net.minecraft" {
            ensure!(
                version == runtime.minecraft.as_str()
                    && component["important"] == true
                    && component["disabled"] != true,
                "Client game differs from locked runtime"
            );
            game = true;
        } else if matches!(
            uid,
            "net.fabricmc.fabric-loader"
                | "org.quiltmc.quilt-loader"
                | "net.minecraftforge"
                | "net.neoforged"
        ) {
            ensure!(
                expected_loader
                    .as_ref()
                    .is_some_and(|(expected_uid, expected_version)| uid == *expected_uid
                        && version == expected_version)
                    && component["disabled"] != true,
                "Client loader differs from locked runtime"
            );
            selected_loader = true;
        }
    }
    ensure!(
        game && (selected_loader == expected_loader.is_some()),
        "Client profile omits the locked runtime"
    );
    Ok(())
}

pub fn prepare_client_full_build(
    workspace: WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientFullOptions,
    cancel: &Cancellation,
) -> Result<PreparedClientBuild> {
    let candidate = prepare_client_full_archive(&workspace, artifact, external, options, cancel)?;
    let publication =
        super::prepare_archives_publication(workspace, vec![candidate.archive], cancel)?;
    Ok(PreparedClientBuild {
        publication,
        game: candidate.game,
        inventory: candidate.inventory,
        user_configuration: candidate.user_configuration,
    })
}
pub(super) struct ClientCandidate {
    pub(super) archive: super::ArchiveCandidate,
    pub(super) game: MaterializedGame,
    pub(super) inventory: BTreeMap<PortableRelPath, FileContent>,
    pub(super) user_configuration: bool,
}
pub(super) fn prepare_client_full_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientFullOptions,
    cancel: &Cancellation,
) -> Result<ClientCandidate> {
    let suffix = match options.archive {
        DistributionArchive::Zip => ".zip",
        DistributionArchive::TarGz => ".tar.gz",
        DistributionArchive::SevenZip => ".7z",
    };
    ensure!(
        artifact.as_str().ends_with(suffix),
        "Client artifact extension differs from selected format"
    );
    let game = prepare_game_content(
        workspace,
        external,
        BuildTarget::ClientFull,
        &options.optional,
        options.evidence,
        cancel,
    )?;
    let templates = prepare_templates(
        workspace,
        BuildTarget::ClientFull,
        &options.templates,
        cancel,
    )?;
    let mut files = BTreeMap::new();
    for (destination, file) in game.files() {
        files.insert(
            path(&format!(".minecraft/{}", destination.as_str()))?,
            file.clone(),
        );
    }
    for (destination, file) in templates.files() {
        ensure!(
            !files.contains_key(destination),
            "Template collides with selected game content: {}",
            destination.as_str()
        );
        files.insert(
            destination.clone(),
            AcquiredBuildFile {
                content: file.content.clone(),
                permissions: file.permissions,
            },
        );
    }
    let instance = path("instance.cfg")?;
    let user_configuration = files.contains_key(&instance);
    if !user_configuration {
        let metadata = &game.project().intent().metadata;
        let runtime = &game.project().lock().runtime;
        let mut renderer = crate::empack::templates::TemplateEngine::new();
        renderer.set_pack_variables(
            &metadata.name,
            metadata.author.as_deref().unwrap_or_default(),
            runtime.minecraft.as_str(),
            &metadata.version,
        );
        renderer.set_variable("BOOTSTRAP", "");
        files.insert(
            instance,
            generated(renderer.render_template("instance.cfg")?.as_bytes(), cancel)?,
        );
    }
    let components = path("mmc-pack.json")?;
    if !files.contains_key(&components) {
        files.insert(
            components.clone(),
            generated(&profile(&game.project().lock().runtime)?, cancel)?,
        );
    }
    let mut profile_bytes = vec![];
    let component_file = &files[&components];
    ensure!(
        component_file.content.lease().len() <= 16 << 20,
        "Client profile exceeds limit"
    );
    component_file
        .content
        .lease()
        .open()
        .read_to_end(&mut profile_bytes)?;
    verify_profile(&profile_bytes, &game.project().lock().runtime)?;
    let mut collisions = CollisionIndex::default();
    let mut inventory = BTreeMap::new();
    for (destination, file) in &files {
        collisions.insert_file(destination)?;
        inventory.insert(
            destination.clone(),
            FileContent {
                content: file.content.lease().id(),
                bytes: file.content.lease().len(),
                permissions: file.permissions,
            },
        );
    }
    let mut stage = MutableStage::empty()?;
    for (destination, file) in &files {
        stage.write(
            destination,
            &mut file.content.lease().open(),
            options.limits.file_bytes,
            cancel,
        )?;
    }
    let mut frozen = stage.freeze(
        SnapshotLimits {
            file_bytes: options.limits.file_bytes,
            total_bytes: options.limits.total_bytes,
            entries: options.limits.entries,
            depth: options.limits.depth,
        },
        cancel,
    )?;
    let mut archive = PrivateFile::new()?;
    let verified = write_archive(
        &mut frozen,
        archive.file(),
        options.archive,
        &inventory,
        options.limits,
        cancel,
    )?;
    Ok(ClientCandidate {
        archive: super::ArchiveCandidate {
            artifact,
            archive,
            verified,
        },
        game,
        inventory,
        user_configuration,
    })
}

#[cfg(test)]
mod tests;
