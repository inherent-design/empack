use crate::Result;
use crate::empack::config::{ProjectPlan, ProjectSpec, VersionOverride};
use crate::empack::parsing::ModLoader;
use crate::empack::search::ProjectResolverTrait;
use crate::primitives::{ProjectPlatform, ProjectType};
use anyhow::anyhow;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub expected_mods: HashSet<String>,
    pub actions: Vec<SyncPlanAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPlanAction {
    Add(SyncDependencyPlan),
    Remove { key: String, title: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncDependencyPlan {
    pub key: String,
    pub normalized_key: String,
    pub search_query: String,
    pub project_type: ProjectType,
    pub minecraft_version: String,
    pub loader: ModLoader,
    pub project_id: Option<String>,
    pub project_platform: Option<ProjectPlatform>,
    pub version_override: Option<VersionOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncExecutionAction {
    Add {
        key: String,
        title: String,
        commands: Vec<Vec<String>>,
        resolved_project_id: String,
        resolved_platform: ProjectPlatform,
    },
    Remove { key: String, title: String },
}

pub fn normalize_mod_key(key: &str) -> String {
    key.to_lowercase().replace(' ', "_").replace('-', "_")
}

pub fn build_sync_plan(project_plan: &ProjectPlan, installed_mods: &HashSet<String>) -> SyncPlan {
    let mut expected_mods = HashSet::new();
    let mut actions = Vec::new();

    for dep_spec in &project_plan.dependencies {
        let normalized_key = normalize_mod_key(&dep_spec.key);
        expected_mods.insert(normalized_key.clone());

        if installed_mods.contains(&normalized_key) {
            continue;
        }

        actions.push(SyncPlanAction::Add(SyncDependencyPlan::from_spec(
            dep_spec,
            normalized_key,
        )));
    }

    for installed_mod in installed_mods {
        if !expected_mods.contains(installed_mod) {
            actions.push(SyncPlanAction::Remove {
                key: installed_mod.clone(),
                title: installed_mod.clone(),
            });
        }
    }

    SyncPlan {
        expected_mods,
        actions,
    }
}

pub async fn resolve_sync_action(
    action: &SyncPlanAction,
    resolver: &dyn ProjectResolverTrait,
) -> Result<SyncExecutionAction> {
    match action {
        SyncPlanAction::Remove { key, title } => Ok(SyncExecutionAction::Remove {
            key: key.clone(),
            title: title.clone(),
        }),
        SyncPlanAction::Add(dep) => {
            let (project_id, platform) = if let Some(project_id) = &dep.project_id {
                (
                    project_id.clone(),
                    dep.project_platform.unwrap_or(ProjectPlatform::Modrinth),
                )
            } else {
                let project = resolver
                    .resolve_project(
                        &dep.search_query,
                        Some(project_type_arg(dep.project_type)),
                        Some(dep.minecraft_version.as_str()),
                        Some(loader_arg(dep.loader)),
                    )
                    .await?;
                (project.project_id, project.platform)
            };

            Ok(SyncExecutionAction::Add {
                key: dep.key.clone(),
                title: dep.search_query.clone(),
                commands: build_packwiz_add_commands(
                    &project_id,
                    platform,
                    dep.version_override.as_ref(),
                )?,
                resolved_project_id: project_id,
                resolved_platform: platform,
            })
        }
    }
}

pub fn build_packwiz_add_commands(
    project_id: &str,
    platform: ProjectPlatform,
    version_override: Option<&VersionOverride>,
) -> Result<Vec<Vec<String>>> {
    let (platform_cmd, id_flag, version_flag) = match platform {
        ProjectPlatform::Modrinth => ("modrinth", "--project-id", "--version-id"),
        ProjectPlatform::CurseForge => ("curseforge", "--addon-id", "--file-id"),
    };

    let base = vec![
        platform_cmd.to_string(),
        "add".to_string(),
        id_flag.to_string(),
        project_id.to_string(),
    ];

    match version_override {
        None => Ok(vec![append_yes(base)]),
        Some(VersionOverride::Single(version)) => {
            Ok(vec![append_yes(with_version(base, version_flag, version))])
        }
        Some(VersionOverride::Multiple(versions)) if versions.is_empty() => {
            Err(anyhow!("Version override list cannot be empty"))
        }
        Some(VersionOverride::Multiple(versions)) => Ok(versions
            .iter()
            .map(|version| append_yes(with_version(base.clone(), version_flag, version)))
            .collect()),
    }
}

fn append_yes(mut command: Vec<String>) -> Vec<String> {
    command.push("-y".to_string());
    command
}

fn with_version(command: Vec<String>, version_flag: &str, version: &str) -> Vec<String> {
    let mut command = command;
    command.push(version_flag.to_string());
    command.push(version.to_string());
    command
}

fn project_type_arg(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Mod => "mod",
        ProjectType::Datapack => "datapack",
        ProjectType::ResourcePack => "resourcepack",
        ProjectType::Shader => "shader",
    }
}

fn loader_arg(loader: ModLoader) -> &'static str {
    match loader {
        ModLoader::Fabric => "fabric",
        ModLoader::Forge => "forge",
        ModLoader::Quilt => "quilt",
        ModLoader::NeoForge => "neoforge",
    }
}

impl SyncDependencyPlan {
    fn from_spec(dep_spec: &ProjectSpec, normalized_key: String) -> Self {
        Self {
            key: dep_spec.key.clone(),
            normalized_key,
            search_query: dep_spec.search_query.clone(),
            project_type: dep_spec.project_type,
            minecraft_version: dep_spec.minecraft_version.clone(),
            loader: dep_spec.loader,
            project_id: dep_spec.project_id.clone(),
            project_platform: dep_spec.project_platform,
            version_override: dep_spec.version_override.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    include!("sync.test.rs");
}