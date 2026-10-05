use crate::empack::config::{DependencySource, ProjectPlan, ProjectSpec};
use crate::empack::installed::{DependencyIdentity, InstalledDependency};
use crate::empack::parsing::ModLoader;
use crate::empack::search::{ProjectResolverTrait, SearchError};
use crate::primitives::{ProjectPlatform, ProjectType};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub expected_mods: HashSet<String>,
    pub satisfied: HashSet<String>,
    pub retained: Vec<String>,
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
    pub search_query: String,
    pub project_type: ProjectType,
    pub minecraft_version: String,
    pub loader: Option<ModLoader>,
    pub source: DependencySource,
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
    /// Explicit requested pin; resolved unpinned installs remain unpinned.
    pub requested_pin: Option<String>,
    pub title: String,
    pub commands: Vec<Vec<String>>,
    pub resolved_project_id: String,
    pub resolved_platform: ProjectPlatform,
    pub resolved_project_type: Option<ProjectType>,
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
    #[error("invalid packwiz add command plan")]
    InvalidPlan,
    #[error("invalid provider identifier: {0}")]
    InvalidIdentity(#[from] empack_core::identity::IdentityError),
}

pub fn build_sync_plan(
    project_plan: &ProjectPlan,
    installed: &[InstalledDependency],
) -> anyhow::Result<SyncPlan> {
    let mut expected_mods = HashSet::new();
    let mut satisfied = HashSet::new();
    let mut identities = HashSet::new();
    let mut observed = HashSet::new();
    let mut actions = Vec::new();
    for spec in &project_plan.dependencies {
        expected_mods.insert(spec.key.clone());
        let Some(identity) = DependencyIdentity::from_spec(spec)? else {
            if matches!(spec.source, DependencySource::Platform { .. }) {
                actions.push(SyncPlanAction::Add(SyncDependencyPlan::from_spec(spec)));
            }
            continue;
        };
        anyhow::ensure!(
            !installed.iter().any(|entry| entry
                .identity
                .as_ref()
                .is_some_and(|other| other.project == identity.project
                    && other.project_type != identity.project_type)),
            "Installed dependency '{}' has a different content type. Automatic replacement is not supported",
            spec.key
        );
        anyhow::ensure!(
            identities.insert(identity.clone()),
            "Multiple manifest entries declare the same provider identity: {}",
            spec.key
        );
        let matches: Vec<_> = installed
            .iter()
            .filter(|entry| entry.identity.as_ref() == Some(&identity))
            .collect();
        anyhow::ensure!(
            matches.len() <= 1,
            "Multiple installed files declare the same provider identity: {}",
            spec.key
        );
        if let Some(entry) = matches.first() {
            observed.insert(entry.key.clone());
            let DependencySource::Platform { version_pin, .. } = &spec.source else {
                unreachable!()
            };
            let requested_pin = version_pin
                .as_deref()
                .map(|pin| identity.project.parse_pin(pin))
                .transpose()?;
            if requested_pin.is_some() && entry.version != requested_pin {
                // A pinned reinstall keeps provider identity; packwiz owns replacement
                // of its metadata and required dependency resolution.
                actions.push(SyncPlanAction::Add(SyncDependencyPlan::from_spec(spec)));
            } else {
                satisfied.insert(spec.key.clone());
            }
        } else {
            anyhow::ensure!(
                !installed.iter().any(|entry| entry.key == spec.key),
                "Installed dependency '{}' has a different provider, project, or content type. Automatic replacement is not supported; remove it explicitly and sync again.",
                spec.key
            );
            actions.push(SyncPlanAction::Add(SyncDependencyPlan::from_spec(spec)));
        }
    }
    // A root manifest is not a complete dependency graph. Absence never proves
    // removability: retain transitive and externally managed installations.
    let mut retained: Vec<_> = installed
        .iter()
        .filter(|entry| !observed.contains(&entry.key))
        .map(|entry| entry.key.clone())
        .collect();
    retained.sort();
    Ok(SyncPlan {
        expected_mods,
        satisfied,
        retained,
        actions,
    })
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
        SyncPlanAction::Add(dep) => match &dep.source {
            DependencySource::Local { .. } | DependencySource::Url(_) => {
                unreachable!("build_sync_plan filters out Local entries before dispatch");
            }
            DependencySource::Platform {
                project_id,
                project_platform,
                version_pin,
            } => {
                if !project_id.is_empty() {
                    let mut commands = build_packwiz_add_commands(
                        project_id,
                        *project_platform,
                        version_pin.as_deref(),
                    )
                    .map_err(|source| AddContractError::PlanPackwizAdd {
                        project_id: project_id.clone(),
                        platform: *project_platform,
                        source,
                    })?;
                    for command in &mut commands {
                        crate::empack::packwiz::append_content_type_override(
                            command,
                            *project_platform,
                            dep.project_type,
                        );
                    }
                    return Ok(SyncExecutionAction::Add {
                        key: dep.key.clone(),
                        title: dep.search_query.clone(),
                        commands,
                        resolved_project_id: project_id.clone(),
                        resolved_platform: *project_platform,
                    });
                }
                let resolution = resolve_add_contract(
                    &dep.search_query,
                    Some(dep.project_type),
                    Some(dep.minecraft_version.as_str()),
                    dep.loader,
                    project_id,
                    *project_platform,
                    version_pin.as_deref(),
                    None,
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
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn resolve_add_contract(
    search_query: &str,
    project_type: Option<ProjectType>,
    minecraft_version: Option<&str>,
    loader: Option<ModLoader>,
    direct_project_id: &str,
    direct_platform: ProjectPlatform,
    version_pin: Option<&str>,
    preferred_platform: Option<ProjectPlatform>,
    resolver: &dyn ProjectResolverTrait,
) -> std::result::Result<AddResolution, AddContractError> {
    let direct = !direct_project_id.is_empty();
    let project = if direct {
        resolver
            .resolve_selector(
                crate::empack::search::ProjectSelector {
                    platform: direct_platform,
                    value: direct_project_id.to_owned(),
                },
                version_pin.map(str::to_owned),
            )
            .await
    } else {
        resolver
            .resolve_project(
                search_query,
                project_type.map(project_type_arg),
                minecraft_version,
                loader.map(loader_arg),
                preferred_platform,
            )
            .await
    }
    .map_err(|source| AddContractError::ResolveProject {
        query: search_query.to_owned(),
        source,
    })?;
    let resolved = match project.project_type.as_str() {
        "resourcepack" => ProjectType::ResourcePack,
        "shader" => ProjectType::Shader,
        "datapack" => ProjectType::Datapack,
        "world" => ProjectType::World,
        _ => ProjectType::Mod,
    };
    if project_type.is_some_and(|requested| requested != resolved) {
        return Err(AddContractError::ResolveProject {
            query: search_query.to_owned(),
            source: SearchError::Other(anyhow::anyhow!(
                "Resolved content type differs from the requested type"
            )),
        });
    }
    let project_id = project.project_id;
    let platform = project.platform;
    let title = project.title;
    let confidence = (!direct).then_some(project.confidence);
    let resolved_type = Some(resolved);

    let mut commands =
        build_packwiz_add_commands(&project_id, platform, version_pin).map_err(|source| {
            AddContractError::PlanPackwizAdd {
                project_id: project_id.clone(),
                platform,
                source,
            }
        })?;

    for command in &mut commands {
        crate::empack::packwiz::append_content_type_override(command, platform, resolved);
    }
    Ok(AddResolution {
        requested_pin: version_pin.map(str::to_owned),
        title,
        commands,
        resolved_project_id: project_id,
        resolved_platform: platform,
        resolved_project_type: resolved_type,
        confidence,
    })
}

pub fn build_packwiz_add_commands(
    project_id: &str,
    platform: ProjectPlatform,
    version_pin: Option<&str>,
) -> std::result::Result<Vec<Vec<String>>, AddCommandPlanError> {
    use empack_core::identity::{
        CurseForgeFileId, CurseForgeProjectId, ModrinthProjectId, ModrinthVersionId,
    };
    match platform {
        ProjectPlatform::Modrinth => {
            ModrinthProjectId::parse(project_id)?;
            if let Some(pin) = version_pin {
                ModrinthVersionId::parse(pin)?;
            }
        }
        ProjectPlatform::CurseForge => {
            CurseForgeProjectId::parse(project_id)?;
            if let Some(pin) = version_pin {
                CurseForgeFileId::parse(pin)?;
            }
        }
    }
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

    match version_pin {
        None => Ok(vec![append_yes(base)]),
        Some(version) => Ok(vec![append_yes(with_version(base, version_flag, version))]),
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

pub fn project_type_arg(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Mod => "mod",
        ProjectType::Datapack => "datapack",
        ProjectType::World => "world",
        ProjectType::ResourcePack => "resourcepack",
        ProjectType::Shader => "shader",
    }
}

pub fn loader_arg(loader: ModLoader) -> &'static str {
    match loader {
        ModLoader::Fabric => "fabric",
        ModLoader::Forge => "forge",
        ModLoader::Quilt => "quilt",
        ModLoader::NeoForge => "neoforge",
    }
}

impl SyncDependencyPlan {
    fn from_spec(dep_spec: &ProjectSpec) -> Self {
        Self {
            key: dep_spec.key.clone(),
            search_query: dep_spec.search_query.clone(),
            project_type: dep_spec.project_type,
            minecraft_version: dep_spec.minecraft_version.clone(),
            loader: dep_spec.loader,
            source: dep_spec.source.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    include!("sync.test.rs");
}
