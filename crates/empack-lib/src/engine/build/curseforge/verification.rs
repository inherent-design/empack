//! Consumer semantics are checked against captured intent, independently of the JSON writer.
use super::super::materialized::PreparedGameContent;
use crate::{application::process_runtime::Cancellation, engine::io::copy_bounded};
use anyhow::{Context, Result, ensure};
use empack_core::{
    identity::{PinSelector, ProviderProjectId},
    inventory::{ContentOwner, Representation},
    model::LoaderKind,
    requirements::Requirement,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs::File};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    manifest_type: String,
    manifest_version: u32,
    name: String,
    version: String,
    author: String,
    minecraft: Runtime,
    files: Vec<Reference>,
    overrides: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Runtime {
    version: String,
    mod_loaders: Vec<Loader>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Loader {
    id: String,
    primary: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    #[serde(rename = "projectID")]
    project: u64,
    #[serde(rename = "fileID")]
    file: u64,
    required: bool,
}

pub(super) fn verify_archive_manifest(
    candidate: &mut File,
    game: &PreparedGameContent,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(candidate)?;
    let mut member = archive.by_name("manifest.json")?;
    let maximum = maximum.min(16 << 20);
    ensure!(
        member.size() <= maximum,
        "CurseForge manifest exceeds byte limit"
    );
    let mut bytes = Vec::new();
    copy_bounded(&mut member, &mut bytes, maximum, cancel)?;
    verify_manifest_bytes(&bytes, game, cancel)
}

pub(super) fn verify_manifest_bytes(
    bytes: &[u8],
    game: &PreparedGameContent,
    cancel: &Cancellation,
) -> Result<()> {
    cancel.check()?;
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    ensure!(
        manifest.manifest_type == "minecraftModpack" && manifest.manifest_version == 1,
        "CurseForge manifest format differs from the selected consumer"
    );
    let metadata = &game.project().intent().metadata;
    ensure!(
        manifest.name == metadata.name
            && manifest.version == metadata.version
            && manifest.author == metadata.author.as_deref().unwrap_or(""),
        "CurseForge manifest metadata differs from author intent"
    );
    ensure!(
        manifest.overrides == "overrides",
        "CurseForge override root differs from the selected inventory"
    );
    let runtime = &game.project().lock().runtime;
    ensure!(
        manifest.minecraft.version == runtime.minecraft.as_str(),
        "CurseForge game version differs from the locked runtime"
    );
    if runtime.loader == LoaderKind::Vanilla {
        ensure!(
            manifest.minecraft.mod_loaders.is_empty(),
            "Vanilla export unexpectedly selects a loader"
        );
    } else {
        ensure!(
            manifest.minecraft.mod_loaders.len() == 1,
            "CurseForge export must select exactly one locked loader"
        );
        let loader = &manifest.minecraft.mod_loaders[0];
        ensure!(loader.primary, "Locked loader is not primary");
        let (family, version) = loader
            .id
            .split_once('-')
            .context("Invalid CurseForge loader identifier")?;
        let actual = match family {
            "fabric" => Some(LoaderKind::Fabric),
            "forge" => Some(LoaderKind::Forge),
            "neoforge" => Some(LoaderKind::NeoForge),
            "quilt" => Some(LoaderKind::Quilt),
            _ => None,
        };
        ensure!(
            actual == Some(runtime.loader)
                && runtime
                    .loader_version
                    .as_ref()
                    .is_some_and(|expected| expected.as_str() == version),
            "CurseForge loader differs from the locked runtime"
        );
    }
    let mut expected = BTreeMap::new();
    for entry in game.inventory().entries() {
        cancel.check()?;
        if !matches!(entry.representation, Representation::Download { .. }) {
            continue;
        }
        let ContentOwner::Dependency { key, .. } = &entry.owner else {
            anyhow::bail!("CurseForge reference lacks dependency ownership");
        };
        let pin = game.project().lock().dependencies[key]
            .selected
            .as_ref()
            .context("CurseForge reference lacks a locked selection")?;
        let (ProviderProjectId::CurseForge(project), PinSelector::CurseForgeFile(file)) =
            (&pin.project, &pin.selection)
        else {
            anyhow::bail!("CurseForge reference lacks a CurseForge identity");
        };
        let required = match entry.requirements.client {
            Requirement::Required => true,
            Requirement::Optional(_) => false,
            Requirement::Unsupported => anyhow::bail!("Unsupported client reference"),
        };
        ensure!(
            expected
                .insert(project.get(), (file.get(), required))
                .is_none(),
            "Duplicate selected CurseForge project"
        );
    }
    let mut actual = BTreeMap::new();
    for reference in manifest.files {
        cancel.check()?;
        ensure!(
            actual
                .insert(reference.project, (reference.file, reference.required))
                .is_none(),
            "Duplicate CurseForge manifest project"
        );
    }
    ensure!(
        actual == expected,
        "CurseForge manifest references or participation differ from the selected inventory"
    );
    Ok(())
}
