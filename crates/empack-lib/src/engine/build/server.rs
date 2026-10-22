//! Server distributions compose verified runtime files with selected server content.
mod windows;
use windows::{install_batch, start_batch};

use super::{
    BuildAcquisitions, PreparedArtifact,
    materialized::{PreparedGameContent, prepare_game_content, prepare_native_game_content},
};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, write_archive},
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        layout::CollisionIndex,
        mrpack::AcquiredBuildFile,
        project::WorkspaceSnapshot,
        release::producer::{NativeReleaseOptions, NativeReleasePlan},
        server_runtime::{PreparedServerRuntime, ServerLaunch, ServerRuntimeEvidence},
        snapshot::SnapshotLimits,
        staging::{MutableStage, PrivateFile},
        templates::{TemplateOptions, prepare_templates},
    },
};
use anyhow::{Result, ensure};
use empack_core::{
    distribution::Recipe,
    files::{FileContent, FilePermissions},
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent},
    path::{PathSyntax, PortableRelPath},
};
use std::{collections::BTreeMap, io::Read};

pub struct ServerOptions {
    pub archive: DistributionArchive,
    pub optional: OptionalPolicy,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub limits: ArchiveLimits,
}
pub struct PreparedServerBuild {
    publication: PreparedArtifact,
    candidate: ServerEvidence,
}
pub(super) struct ServerEvidence {
    pub(super) game: PreparedGameContent,
    pub(super) inventory: BTreeMap<PortableRelPath, FileContent>,
    pub(super) user_configuration: bool,
    pub(super) runtime: ServerRuntimeEvidence,
}
impl PreparedServerBuild {
    pub fn game(&self) -> &PreparedGameContent {
        &self.candidate.game
    }
    pub fn inventory(&self) -> &BTreeMap<PortableRelPath, FileContent> {
        &self.candidate.inventory
    }
    pub fn runtime(&self) -> &ServerRuntimeEvidence {
        &self.candidate.runtime
    }
    /// Captured configuration or scripts are retained as user input. Managed entry points
    /// must match generated scripts; standalone bundled scripts are not certified behavior.
    pub fn uses_user_configuration(&self) -> bool {
        self.candidate.user_configuration
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
/// Both recipes require an independently prepared runtime matching the captured resolution.
/// Reference delivery carries a native release; bundled delivery includes selected pack bytes.
pub fn prepare_server_build(
    workspace: WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ServerOptions,
    runtime: &PreparedServerRuntime,
    references: bool,
    cancel: &Cancellation,
) -> Result<PreparedServerBuild> {
    let (archive, candidate) = prepare_server_archive(
        &workspace,
        artifact,
        external,
        options,
        runtime,
        if references {
            Recipe::SERVER_REFERENCES
        } else {
            Recipe::SERVER_BUNDLED
        },
        cancel,
    )?;
    let publication = super::prepare_archives_publication(
        workspace,
        vec![archive],
        &std::collections::BTreeSet::new(),
        cancel,
    )?;
    Ok(PreparedServerBuild {
        publication,
        candidate,
    })
}
fn path(name: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(name, PathSyntax::ArchiveMember)?)
}
fn generated(bytes: &[u8], executable: bool, cancel: &Cancellation) -> Result<AcquiredBuildFile> {
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
            executable,
        },
    })
}
#[derive(Clone, Copy)]
enum ServerInterface {
    Console,
    Gui,
}
impl ServerInterface {
    fn arguments(self) -> &'static str {
        match self {
            Self::Console => "nogui ",
            Self::Gui => "",
        }
    }
}

// These scripts contain no project metadata. All arguments remain separate shell arguments.
const START_SH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
JAVA_PATH="${JAVA_HOME:+$JAVA_HOME/bin/}java"
cd -- game
exec "$JAVA_PATH" -jar server.jar {{SERVER_ARGUMENTS}}"$@"
"#;
fn start_script(references: bool, interface: ServerInterface) -> String {
    let script = START_SH.replace("{{SERVER_ARGUMENTS}}", interface.arguments());
    if references {
        script
            .replace("cd -- game\n", "bash ./install_pack.sh\n")
            .replace(
                "exec \"$JAVA_PATH\"",
                "exec empack --workdir \"$PWD\" --yes instance launch -- \"$JAVA_PATH\"",
            )
    } else {
        script
    }
}
fn runtime_start(
    launch: &ServerLaunch,
    references: bool,
    windows: bool,
    interface: ServerInterface,
) -> String {
    let script = if windows {
        start_batch(references, interface)
    } else {
        start_script(references, interface)
    };
    if references {
        return script
            .replace("instance launch --", "instance launch --server --")
            .replace(" -jar server.jar", "");
    }
    let quote = |value: &str| {
        if windows {
            format!("\"{}\"", value.replace('%', "%%"))
        } else {
            format!("'{}'", value.replace('\'', "'\"'\"'"))
        }
    };
    let arguments = match launch {
        ServerLaunch::Jar(path) if path.as_str() == "server.jar" => return script,
        ServerLaunch::Jar(path) => format!("-jar {}", quote(path.as_str())),
        ServerLaunch::Arguments { unix, windows: win } => {
            let path = if windows { win } else { unix };
            format!(
                "@user_jvm_args.txt {}",
                quote(&format!("@{}", path.as_str()))
            )
        }
    };
    script.replace("-jar server.jar", &arguments)
}

fn consumer_script(script: String, target: Recipe) -> String {
    if target.update_authority() == empack_core::distribution::UpdateAuthority::Empack {
        script
            .replace(
                "instance launch --server --",
                "instance launch --server --check-updates --",
            )
            .replace("--side server", "--side server --require-subscription")
    } else {
        script
    }
}

fn install_script(release: Option<&str>) -> String {
    let command = release.map(|id| format!("empack --workdir \"$PWD\" --yes instance prepare \"$PWD/.empack-consumer/release.json\" --sha256 {id} --layout game --side server\n")).unwrap_or_default();
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${{BASH_SOURCE[0]}}")"
{command}printf '%s\n' 'Server pack ready. Run start.sh or start.bat to start it.' 'Review and accept the Minecraft EULA yourself before playing.'
"#
    )
}
pub(super) fn prepare_server_archive(
    workspace: &WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ServerOptions,
    runtime: &PreparedServerRuntime,
    target: Recipe,
    cancel: &Cancellation,
) -> Result<(super::ArchiveCandidate, ServerEvidence)> {
    ensure!(
        &workspace.require_resolved()?.lock().runtime == runtime.runtime(),
        "Prepared server runtime differs from captured lock"
    );
    if options.evidence == SourceEvidencePolicy::StrongSourceRequired {
        ensure!(
            runtime
                .evidence()
                .expected
                .digests
                .as_ref()
                .is_some_and(|digests| digests.values().iter().any(|digest| matches!(
                    digest.algorithm(),
                    empack_core::digest::DigestAlgorithm::Sha256
                        | empack_core::digest::DigestAlgorithm::Sha512
                ))),
            "Strong-source build policy requires an independent strong runtime declaration"
        );
        ensure!(runtime.files().values().all(|file| matches!(file.content.evidence(),
            empack_core::digest::IntegrityEvidence::MatchedExpected {expected, ..}
            if matches!(expected.strongest(), empack_core::digest::DigestAlgorithm::Sha256 | empack_core::digest::DigestAlgorithm::Sha512))),
            "Strong-source build policy rejects weak or undeclared loader output evidence");
    }
    let suffix = match options.archive {
        DistributionArchive::Zip => ".zip",
        DistributionArchive::TarGz => ".tar.gz",
        DistributionArchive::SevenZip => ".7z",
    };
    ensure!(
        artifact.as_str().ends_with(suffix),
        "Server artifact extension differs from selected format"
    );
    ensure!(
        target.consumer() == empack_core::distribution::Consumer::Server,
        "Server adapter requires an executable consumer recipe"
    );
    let managed = super::instance_managed(target);
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
    let templates = prepare_templates(workspace, target, &options.templates, cancel)?;
    let mut files = BTreeMap::new();
    let release = if managed {
        let mut release_options = NativeReleaseOptions::from_project(game.project())?;
        release_options.require_subscription =
            target.update_authority() == empack_core::distribution::UpdateAuthority::Empack;
        release_options.delivery = target.delivery();
        release_options
            .policies
            .retain(|destination, _| game.files().contains_key(destination));
        Some(
            NativeReleasePlan::prepare_selected(&game, release_options)?
                .with_server_runtime(runtime)?,
        )
    } else {
        for (destination, file) in game.files() {
            files.insert(
                path(&format!("game/{}", destination.as_str()))?,
                file.clone(),
            );
        }
        None
    };
    let mut insert = |destination: PortableRelPath, file: AcquiredBuildFile| -> Result<()> {
        ensure!(
            !files.contains_key(&destination),
            "Server inputs collide at {}",
            destination.as_str()
        );
        files.insert(destination, file);
        Ok(())
    };
    for (destination, file) in runtime.files().iter().filter(|_| !managed) {
        insert(
            path(&format!("game/{}", destination.as_str()))?,
            file.clone(),
        )?;
    }
    if let Some(release) = &release {
        insert(
            path(".empack-consumer/release.json")?,
            generated(release.release().bytes(), false, cancel)?,
        )?;
        for (destination, file) in release.assets() {
            insert(
                path(&format!(".empack-consumer/{}", destination.as_str()))?,
                file.clone(),
            )?;
        }
    }
    for (destination, file) in templates.files() {
        insert(
            destination.clone(),
            AcquiredBuildFile {
                content: file.content.clone(),
                permissions: file.permissions,
            },
        )?;
    }
    let user_configuration = [
        "start.sh",
        "start.bat",
        "start_gui.sh",
        "start_gui.bat",
        "install_pack.sh",
        "install_pack.bat",
        "game/server.properties",
    ]
    .into_iter()
    .any(|name| files.contains_key(&path(name).expect("static path")));
    for (name, bytes, executable) in [
        (
            "start.sh",
            consumer_script(
                runtime_start(runtime.launch(), managed, false, ServerInterface::Console),
                target,
            )
            .into_bytes(),
            true,
        ),
        (
            "start.bat",
            consumer_script(
                runtime_start(runtime.launch(), managed, true, ServerInterface::Console),
                target,
            )
            .into_bytes(),
            false,
        ),
        (
            "start_gui.sh",
            consumer_script(
                runtime_start(runtime.launch(), managed, false, ServerInterface::Gui),
                target,
            )
            .into_bytes(),
            true,
        ),
        (
            "start_gui.bat",
            consumer_script(
                runtime_start(runtime.launch(), managed, true, ServerInterface::Gui),
                target,
            )
            .into_bytes(),
            false,
        ),
        (
            "install_pack.bat",
            consumer_script(
                install_batch(release.as_ref().map(|release| release.release().id())),
                target,
            )
            .into_bytes(),
            false,
        ),
        (
            "install_pack.sh",
            consumer_script(
                install_script(release.as_ref().map(|release| release.release().id())),
                target,
            )
            .into_bytes(),
            true,
        ),
    ] {
        let destination = path(name)?;
        match files.entry(destination) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(generated(&bytes, executable, cancel)?);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) if managed => {
                ensure!(
                    entry.get().content.lease().len() == bytes.len() as u64,
                    "Managed server template {name} must match the generated script; remove the replacement to restore required preparation and launch integration"
                );
                let mut actual = Vec::new();
                entry
                    .get()
                    .content
                    .lease()
                    .open()
                    .read_to_end(&mut actual)?;
                ensure!(
                    actual == bytes,
                    "Managed server template {name} must match the generated script; remove the replacement to restore required preparation and launch integration"
                );
                entry.get_mut().permissions = FilePermissions {
                    readonly: false,
                    executable,
                };
            }
            _ => {}
        }
    }
    let properties = path("game/server.properties")?;
    if !game.files().contains_key(&path("server.properties")?)
        && let std::collections::btree_map::Entry::Vacant(entry) = files.entry(properties)
    {
        let bytes = crate::engine::templates::render_default(
            game.project(),
            include_str!("../../../templates/server/server.properties.template"),
            [],
            options.limits.file_bytes,
            cancel,
        )?;
        entry.insert(generated(&bytes, false, cancel)?);
    }
    let mut collisions = CollisionIndex::default();
    if managed {
        for destination in game.files().keys().chain(runtime.files().keys()) {
            collisions.insert_file(&path(&format!("game/{}", destination.as_str()))?)?;
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
    Ok((
        super::ArchiveCandidate {
            artifact,
            archive,
            verified,
        },
        ServerEvidence {
            game,
            inventory,
            user_configuration,
            runtime: runtime.evidence().clone(),
        },
    ))
}
#[cfg(test)]
pub(super) mod tests;
