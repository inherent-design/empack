//! Server distributions compose verified runtime files with selected server content.
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
    distribution::Delivery,
    files::{FileContent, FilePermissions},
    inventory::OptionalPolicy,
    model::{DistributionArchive, ExpectedContent},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::collections::BTreeMap;

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
    /// Captured configuration or scripts are retained as user input, not certified executable behavior.
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
        &workspace, artifact, external, options, runtime, references, cancel,
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
// These scripts contain no project metadata. All arguments remain separate shell arguments.
const START_SH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
JAVA_PATH="${JAVA_HOME:+$JAVA_HOME/bin/}java"
cd -- game
exec "$JAVA_PATH" -jar server.jar "$@"
"#;
const START_BAT: &str = "@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%~dp0\" || exit /b 1\r\ncd game || exit /b 1\r\nif defined JAVA_HOME (\r\n  \"%JAVA_HOME%\\bin\\java.exe\" -jar server.jar %*\r\n) else (\r\n  java -jar server.jar %*\r\n)\r\n";
fn start_script(references: bool) -> String {
    if references {
        START_SH.replace("cd -- game", "bash ./install_pack.sh\ncd -- game")
    } else {
        START_SH.into()
    }
}
fn start_batch(references: bool) -> String {
    let command = if references {
        "call install_pack.bat\r\nif errorlevel 1 exit /b %errorlevel%\r\n"
    } else {
        ""
    };
    START_BAT.replace("cd game", &format!("{command}cd game"))
}
fn runtime_start(launch: &ServerLaunch, references: bool, windows: bool) -> String {
    let script = if windows {
        start_batch(references)
    } else {
        start_script(references)
    };
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

fn install_batch(release: Option<&str>) -> String {
    let command = release.map(|id| format!("empack --workdir \"%cd%\" --yes instance prepare \"%cd%\\.empack-consumer\\release.json\" --sha256 {id} --layout game --side server\r\nif errorlevel 1 exit /b %errorlevel%\r\n")).unwrap_or_default();
    format!(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%~dp0\" || exit /b 1\r\n{command}echo Server pack ready. Run start.bat to start it.\r\necho Review and accept the Minecraft EULA yourself before playing.\r\nexit /b 0\r\n"
    )
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
    references: bool,
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
    let target = if references {
        BuildTarget::Server
    } else {
        BuildTarget::ServerFull
    };
    let game = if references {
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
    let release = if references {
        let mut release_options = NativeReleaseOptions::from_project(game.project())?;
        release_options.delivery = Delivery::References;
        release_options
            .policies
            .retain(|destination, _| game.files().contains_key(destination));
        Some(NativeReleasePlan::prepare_selected(&game, release_options)?)
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
    for (destination, file) in runtime.files() {
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
        "install_pack.sh",
        "install_pack.bat",
        "game/server.properties",
    ]
    .into_iter()
    .any(|name| files.contains_key(&path(name).expect("static path")));
    for (name, bytes, executable) in [
        (
            "start.sh",
            runtime_start(runtime.launch(), references, false).into_bytes(),
            true,
        ),
        (
            "start.bat",
            runtime_start(runtime.launch(), references, true).into_bytes(),
            false,
        ),
        (
            "install_pack.bat",
            install_batch(release.as_ref().map(|release| release.release().id())).into_bytes(),
            false,
        ),
        (
            "install_pack.sh",
            install_script(release.as_ref().map(|release| release.release().id())).into_bytes(),
            true,
        ),
    ] {
        let destination = path(name)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = files.entry(destination) {
            entry.insert(generated(&bytes, executable, cancel)?);
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
    if references {
        for destination in game.files().keys() {
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
