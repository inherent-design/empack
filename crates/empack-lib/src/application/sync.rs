use crate::empack::config::{ProjectPlan, ProjectSpec, VersionOverride};
use crate::empack::parsing::ModLoader;
use crate::empack::search::{ProjectResolverTrait, SearchError};
use crate::primitives::{ProjectPlatform, ProjectType};
use std::collections::HashSet;
use thiserror::Error;

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
    Remove {
        key: String,
        title: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddResolution {
    pub title: String,
    pub commands: Vec<Vec<String>>,
    pub resolved_project_id: String,
    pub resolved_platform: ProjectPlatform,
    pub confidence: Option<u8>,
}

#[derive(Debug, Error)]
pub enum AddContractError {
    #[error("failed to resolve project '{query}': {source}")]
    ResolveProject {
        query: String,
        #[source]
        source: SearchError,
    },

    #[error("failed to plan packwiz add for {platform} project '{project_id}': {source}")]
    PlanPackwizAdd {
        project_id: String,
        platform: ProjectPlatform,
        #[source]
        source: AddCommandPlanError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AddCommandPlanError {
    #[error("version override list cannot be empty")]
    EmptyVersionOverrideList,
}

pub fn normalize_mod_key(key: &str) -> String {
    key.to_lowercase().replace([' ', '-'], "_")
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
) -> std::result::Result<SyncExecutionAction, AddContractError> {
    match action {
        SyncPlanAction::Remove { key, title } => Ok(SyncExecutionAction::Remove {
            key: key.clone(),
            title: title.clone(),
        }),
        SyncPlanAction::Add(dep) => {
            let resolution = resolve_add_contract(
                &dep.search_query,
                dep.project_type,
                Some(dep.minecraft_version.as_str()),
                Some(dep.loader),
                dep.project_id.as_deref(),
                dep.project_platform,
                dep.version_override.as_ref(),
                resolver,
            )
            .await?;

            Ok(SyncExecutionAction::Add {
                key: dep.key.clone(),
                title: resolution.title,
                commands: resolution.commands,
                resolved_project_id: resolution.resolved_project_id,
                resolved_platform: resolution.resolved_platform,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn resolve_add_contract(
    search_query: &str,
    project_type: ProjectType,
    minecraft_version: Option<&str>,
    loader: Option<ModLoader>,
    direct_project_id: Option<&str>,
    direct_platform: Option<ProjectPlatform>,
    version_override: Option<&VersionOverride>,
    resolver: &dyn ProjectResolverTrait,
) -> std::result::Result<AddResolution, AddContractError> {
    let (project_id, platform, title, confidence) = if let Some(project_id) = direct_project_id {
        (
            project_id.to_string(),
            direct_platform.unwrap_or(ProjectPlatform::Modrinth),
            search_query.to_string(),
            None,
        )
    } else {
        let project = resolver
            .resolve_project(
                search_query,
                Some(project_type_arg(project_type)),
                minecraft_version,
                loader.map(loader_arg),
            )
            .await
            .map_err(|source| AddContractError::ResolveProject {
                query: search_query.to_string(),
                source,
            })?;
        (
            project.project_id,
            project.platform,
            project.title,
            Some(project.confidence),
        )
    };

    let commands =
        build_packwiz_add_commands(&project_id, platform, version_override).map_err(|source| {
            AddContractError::PlanPackwizAdd {
                project_id: project_id.clone(),
                platform,
                source,
            }
        })?;

    Ok(AddResolution {
        title,
        commands,
        resolved_project_id: project_id,
        resolved_platform: platform,
        confidence,
    })
}

pub fn build_packwiz_add_commands(
    project_id: &str,
    platform: ProjectPlatform,
    version_override: Option<&VersionOverride>,
) -> std::result::Result<Vec<Vec<String>>, AddCommandPlanError> {
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
            Err(AddCommandPlanError::EmptyVersionOverrideList)
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
