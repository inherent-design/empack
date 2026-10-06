//! Server distributions compose verified runtime files with selected server content.
use super::{
    BuildAcquisitions, PreparedArtifact,
    materialized::{PreparedGameContent, prepare_bootstrap_game_content, prepare_game_content},
};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        artifacts::{ArchiveLimits, write_archive},
        bootstrap_tools::{InstallerArtifact, InstallerAssets, InstallerRelease},
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        layout::CollisionIndex,
        mrpack::AcquiredBuildFile,
        packwiz::InstallerInteraction,
        project::WorkspaceSnapshot,
        server_runtime::{PreparedServerRuntime, ServerLaunch, ServerRuntimeEvidence},
        snapshot::SnapshotLimits,
        staging::{MutableStage, PrivateFile},
        templates::{TemplateOptions, prepare_templates},
    },
};
use anyhow::{Result, ensure};
use empack_core::{
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
pub struct ServerBootstrap {
    pub assets: InstallerAssets,
    pub interaction: InstallerInteraction,
}
pub struct PreparedServerBuild {
    publication: PreparedArtifact,
    candidate: ServerEvidence,
}
pub(super) struct ServerEvidence {
    pub(super) game: PreparedGameContent,
    pub(super) inventory: BTreeMap<PortableRelPath, FileContent>,
    pub(super) user_configuration: bool,
    pub(super) toolchain: Vec<InstallerRelease>,
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
    pub fn toolchain(&self) -> &[InstallerRelease] {
        &self.candidate.toolchain
    }
    /// Captured configuration or scripts are retained as user input, not certified executable behavior.
    pub fn uses_user_configuration(&self) -> bool {
        self.candidate.user_configuration
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
/// Both recipes require an independently prepared runtime matching the captured resolution.
/// `None` selects a full pack; `Some` selects a bootstrap pack with exact bundled tools.
pub fn prepare_server_build(
    workspace: WorkspaceSnapshot,
    artifact: PortableRelPath,
    external: &BuildAcquisitions,
    options: &ServerOptions,
    runtime: &PreparedServerRuntime,
    bootstrap: Option<&ServerBootstrap>,
    cancel: &Cancellation,
) -> Result<PreparedServerBuild> {
    let (archive, candidate) = prepare_server_archive(
        &workspace, artifact, external, options, runtime, bootstrap, cancel,
    )?;
    let publication = super::prepare_archives_publication(workspace, vec![archive], cancel)?;
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
exec "$JAVA_PATH" -jar server.jar "$@"
"#;
const START_BAT: &str = "@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%~dp0\"\r\nif defined JAVA_HOME (\r\n  \"%JAVA_HOME%\\bin\\java.exe\" -jar server.jar %*\r\n) else (\r\n  java -jar server.jar %*\r\n)\r\n";
fn start_script(bootstrap: bool) -> String {
    if bootstrap {
        START_SH.replace(
            "exec \"$JAVA_PATH\"",
            "bash ./install_pack.sh \"$JAVA_PATH\"\nexec \"$JAVA_PATH\"",
        )
    } else {
        START_SH.into()
    }
}
fn start_batch(bootstrap: bool) -> String {
    let command = if bootstrap {
        "call install_pack.bat\r\nif errorlevel 1 exit /b %errorlevel%\r\n"
    } else {
        ""
    };
    START_BAT.replace(
        "if defined JAVA_HOME",
        &format!("{command}if defined JAVA_HOME"),
    )
}
fn runtime_start(launch: &ServerLaunch, bootstrap: bool, windows: bool) -> String {
    let script = if windows {
        start_batch(bootstrap)
    } else {
        start_script(bootstrap)
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

fn install_batch(bootstrap: Option<&ServerBootstrap>) -> String {
    let command=bootstrap.map(|bootstrap| {
        let headless=if bootstrap.interaction==InstallerInteraction::Headless { " --no-gui" } else { "" };
        format!("set \"JAVA_PATH=java\"\r\nif defined JAVA_HOME set \"JAVA_PATH=%JAVA_HOME%\\bin\\java.exe\"\r\nif not \"%~1\"==\"\" set \"JAVA_PATH=%~1\"\r\n\"%JAVA_PATH%\" -jar packwiz-installer-bootstrap.jar --bootstrap-no-update --bootstrap-main-jar packwiz-installer.jar{headless} -s server pack/pack.toml\r\nif errorlevel 1 exit /b %errorlevel%\r\n")
    }).unwrap_or_default();
    format!(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%~dp0\" || exit /b 1\r\n{command}echo Server pack ready. Run start.bat to start it.\r\necho Review and accept the Minecraft EULA yourself before playing.\r\nexit /b 0\r\n"
    )
}
fn install_script(bootstrap: Option<&ServerBootstrap>) -> String {
    let command = bootstrap.map(|bootstrap| {
        let headless = if bootstrap.interaction == InstallerInteraction::Headless {
            " --no-gui"
        } else {
            ""
        };
        format!(r#"JAVA_PATH="${{1:-${{JAVA_HOME:+$JAVA_HOME/bin/}}java}}"
"$JAVA_PATH" -jar packwiz-installer-bootstrap.jar --bootstrap-no-update --bootstrap-main-jar packwiz-installer.jar{headless} -s server pack/pack.toml
"#)
    }).unwrap_or_default();
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
    bootstrap: Option<&ServerBootstrap>,
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
    let target = if bootstrap.is_some() {
        BuildTarget::Server
    } else {
        BuildTarget::ServerFull
    };
    let game = if bootstrap.is_some() {
        prepare_bootstrap_game_content(
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
    let mut files = game.files().clone();
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
        insert(destination.clone(), file.clone())?;
    }
    if let Some(bootstrap) = bootstrap {
        let tree = game.packwiz(bootstrap.interaction, cancel)?;
        for (destination, file) in tree.files() {
            insert(
                path(&format!("pack/{}", destination.as_str()))?,
                file.clone(),
            )?;
        }
        for artifact in [InstallerArtifact::Bootstrap, InstallerArtifact::Installer] {
            insert(
                path(artifact.release().filename)?,
                AcquiredBuildFile {
                    content: bootstrap.assets.content(artifact).clone(),
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
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
        "server.properties",
    ]
    .into_iter()
    .any(|name| files.contains_key(&path(name).expect("static path")));
    for (name, bytes, executable) in [
        (
            "start.sh",
            runtime_start(runtime.launch(), bootstrap.is_some(), false).into_bytes(),
            true,
        ),
        (
            "start.bat",
            runtime_start(runtime.launch(), bootstrap.is_some(), true).into_bytes(),
            false,
        ),
        (
            "install_pack.bat",
            install_batch(bootstrap).into_bytes(),
            false,
        ),
        (
            "install_pack.sh",
            install_script(bootstrap).into_bytes(),
            true,
        ),
    ] {
        let destination = path(name)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = files.entry(destination) {
            entry.insert(generated(&bytes, executable, cancel)?);
        }
    }
    let properties = path("server.properties")?;
    if let std::collections::btree_map::Entry::Vacant(entry) = files.entry(properties) {
        let metadata = &game.project().intent().metadata;
        let mut renderer = crate::empack::templates::TemplateEngine::new();
        renderer.set_pack_variables(
            &metadata.name,
            metadata.author.as_deref().unwrap_or_default(),
            runtime.runtime().minecraft.as_str(),
            &metadata.version,
        );
        entry.insert(generated(
            renderer.render_template("server.properties")?.as_bytes(),
            false,
            cancel,
        )?);
    }
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
            toolchain: if bootstrap.is_some() {
                vec![
                    InstallerArtifact::Bootstrap.release(),
                    InstallerArtifact::Installer.release(),
                ]
            } else {
                vec![]
            },
            runtime: runtime.evidence().clone(),
        },
    ))
}
#[cfg(test)]
pub(super) mod tests;
