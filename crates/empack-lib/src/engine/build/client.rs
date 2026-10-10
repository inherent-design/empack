//! Full launcher distributions: selected game bytes, captured templates and exact components.
use super::{
    BuildAcquisitions, PreparedArtifact,
    materialized::{PreparedGameContent, prepare_game_content, prepare_native_game_content},
};
use crate::engine::prism::{profile, verify_profile};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, write_archive},
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        layout::CollisionIndex,
        mrpack::AcquiredBuildFile,
        project::WorkspaceSnapshot,
        release::producer::{NativeReleaseOptions, NativeReleasePlan},
        snapshot::SnapshotLimits,
        staging::{MutableStage, PrivateFile},
        templates::{TemplateOptions, prepare_templates},
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    distribution::Recipe,
    distribution::{Consumer, UpdateAuthority},
    files::{FileContent, FilePermissions},
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent},
    path::{PathSyntax, PortableRelPath},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

pub struct ClientOptions {
    pub archive: DistributionArchive,
    pub optional: OptionalPolicy,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub limits: ArchiveLimits,
}
/// Verified selected game representation and launcher declarations, not an offline Minecraft installation.
pub struct PreparedClientBuild {
    publication: PreparedArtifact,
    game: PreparedGameContent,
    inventory: BTreeMap<PortableRelPath, FileContent>,
    user_configuration: bool,
}
impl PreparedClientBuild {
    pub fn game(&self) -> &PreparedGameContent {
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
    #[cfg(test)]
    pub(in crate::engine) fn publish(
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
pub fn prepare_client_full_build(
    workspace: WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientOptions,
    cancel: &Cancellation,
) -> Result<PreparedClientBuild> {
    let candidate = prepare_client_full_archive(&workspace, artifact, external, options, cancel)?;
    let publication = super::prepare_archives_publication(
        workspace,
        vec![candidate.archive],
        &std::collections::BTreeSet::new(),
        cancel,
    )?;
    Ok(PreparedClientBuild {
        publication,
        game: candidate.game,
        inventory: candidate.inventory,
        user_configuration: candidate.user_configuration,
    })
}
pub(super) struct ClientCandidate {
    pub(super) archive: super::ArchiveCandidate,
    pub(super) game: PreparedGameContent,
    pub(super) inventory: BTreeMap<PortableRelPath, FileContent>,
    pub(super) user_configuration: bool,
}
pub(super) fn prepare_client_full_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientOptions,
    cancel: &Cancellation,
) -> Result<ClientCandidate> {
    prepare_client_archive(
        workspace,
        artifact,
        external,
        options,
        Recipe::PRISM_BUNDLED,
        cancel,
    )
}
/// Prepare a Prism reference distribution with an exact native release.
pub fn prepare_client_build(
    workspace: WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientOptions,
    cancel: &Cancellation,
) -> Result<PreparedClientBuild> {
    let built = prepare_client_archive(
        &workspace,
        artifact,
        external,
        options,
        Recipe::PRISM_REFERENCES,
        cancel,
    )?;
    let publication = super::prepare_archives_publication(
        workspace,
        vec![built.archive],
        &std::collections::BTreeSet::new(),
        cancel,
    )?;
    Ok(PreparedClientBuild {
        publication,
        game: built.game,
        inventory: built.inventory,
        user_configuration: built.user_configuration,
    })
}
pub(super) fn prepare_client_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ClientOptions,
    target: Recipe,
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
    ensure!(
        target.consumer() == Consumer::Prism,
        "Prism adapter requires an executable consumer recipe"
    );
    let managed = super::instance_managed(target);
    let subscribed = target.update_authority() == UpdateAuthority::Empack;
    let game = if managed {
        prepare_native_game_content(
            workspace,
            external,
            target,
            &options.optional,
            options.evidence,
            cancel,
        )?
    } else {
        prepare_game_content(
            workspace,
            external,
            target,
            &options.optional,
            options.evidence,
            cancel,
        )?
    };
    let mut template_options = options.templates.clone();
    let mut files = BTreeMap::new();
    let command = if managed {
        let mut release_options = NativeReleaseOptions::from_project(game.project())?;
        release_options.require_subscription =
            target.update_authority() == empack_core::distribution::UpdateAuthority::Empack;
        release_options.delivery = target.delivery();
        release_options
            .policies
            .retain(|destination, _| game.files().contains_key(destination));
        for destination in game.files().keys() {
            ensure!(
                !destination.as_str().split('/').next().is_some_and(|name| [
                    ".empack-layout",
                    ".empack-consumer"
                ]
                .iter()
                .any(|reserved| name.eq_ignore_ascii_case(reserved))),
                "Game content collides with a reserved Prism control path"
            );
        }
        let release = NativeReleasePlan::prepare_selected(&game, release_options)?;
        files.insert(
            path(".minecraft/.empack-consumer/release.json")?,
            generated(release.release().bytes(), cancel)?,
        );
        for (destination, file) in release.assets() {
            files.insert(
                path(&format!(
                    ".minecraft/.empack-consumer/{}",
                    destination.as_str()
                ))?,
                file.clone(),
            );
        }
        Some(format!(
            "empack --workdir \"$INST_DIR\" --yes instance prepare \"$INST_DIR/.minecraft/.empack-consumer/release.json\" --sha256 {} --layout prism --side client{}",
            release.release().id(),
            if subscribed {
                " --require-subscription"
            } else {
                ""
            }
        ))
    } else {
        for (destination, file) in game.files() {
            files.insert(
                path(&format!(".minecraft/{}", destination.as_str()))?,
                file.clone(),
            );
        }
        None
    };
    template_options.values.insert(
        "INSTANCE_PREPARE_COMMAND".into(),
        command.clone().unwrap_or_default(),
    );
    let wrapper = if subscribed {
        "empack --workdir \"$INST_DIR\" --yes instance launch --check-updates --"
    } else if managed {
        "empack --workdir \"$INST_DIR\" --yes instance launch --"
    } else {
        ""
    };
    template_options
        .values
        .insert("INSTANCE_WRAPPER_COMMAND".into(), wrapper.into());
    let templates = prepare_templates(workspace, target, &template_options, cancel)?;
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
    let values = BTreeMap::from([
        (
            "INSTANCE_PREPARE_COMMAND".to_owned(),
            command.unwrap_or_default(),
        ),
        ("INSTANCE_WRAPPER_COMMAND".to_owned(), wrapper.into()),
    ]);
    let default_instance = crate::engine::templates::render_default(
        game.project(),
        include_str!("../../../templates/client/instance.cfg.template"),
        values,
        options.limits.file_bytes,
        cancel,
    )?;
    if !user_configuration {
        files.insert(instance.clone(), generated(&default_instance, cancel)?);
    } else if subscribed {
        ensure!(
            files[&instance].content.lease().len() <= 16 << 20,
            "Launcher configuration exceeds limit"
        );
        let mut bytes = Vec::new();
        files[&instance]
            .content
            .lease()
            .open()
            .read_to_end(&mut bytes)?;
        verify_subscription_commands(&bytes, &default_instance)?;
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
    if managed {
        // Future installed paths participate in the same collision check as packaged inputs.
        for destination in game.files().keys() {
            collisions.insert_file(&path(&format!(".minecraft/{}", destination.as_str()))?)?;
        }
    }
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

/// Require one unambiguous spelling of launcher authority settings while retaining
/// unrelated names, icons and resource settings from user templates.
fn verify_subscription_commands(actual: &[u8], generated: &[u8]) -> Result<()> {
    fn commands(bytes: &[u8]) -> Result<BTreeMap<&str, &str>> {
        let mut general = false;
        let mut groups = BTreeSet::new();
        let mut selected = BTreeMap::new();
        for line in std::str::from_utf8(bytes)?.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            ensure!(
                !line.ends_with('\\'),
                "Subscribed launcher settings cannot use continued lines"
            );
            if line.starts_with('[') {
                let section = line
                    .strip_prefix('[')
                    .and_then(|v| v.strip_suffix(']'))
                    .context("Invalid launcher settings section")?;
                ensure!(
                    section
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        && groups.insert(section),
                    "Ambiguous launcher settings section"
                );
                general = section == "General";
                continue;
            }
            let (key, value) = line.split_once('=').context("Invalid launcher setting")?;
            let key = key.trim();
            ensure!(
                key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "Encoded launcher setting keys are unsupported for subscribed consumers"
            );
            if general
                && matches!(
                    key,
                    "OverrideCommands" | "PreLaunchCommand" | "WrapperCommand"
                )
            {
                ensure!(
                    selected.insert(key, value.trim()).is_none(),
                    "Repeated launcher command setting"
                );
            }
        }
        Ok(selected)
    }
    ensure!(
        commands(actual)? == commands(generated)?,
        "Subscribed Prism templates must retain generated preparation and update commands"
    );
    Ok(())
}
