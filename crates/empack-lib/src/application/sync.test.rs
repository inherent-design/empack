use super::*;
use crate::application::session_mocks::MockProjectResolver;
use crate::empack::search::{ProjectInfo, SearchError};
use crate::empack::config::{ProjectPlan, ProjectSpec, VersionOverride};
use crate::empack::parsing::ModLoader;
use crate::primitives::{ProjectPlatform, ProjectType};
use std::collections::HashSet;

fn project_spec(key: &str) -> ProjectSpec {
    ProjectSpec {
        key: key.to_string(),
        search_query: format!("{key} title"),
        project_type: ProjectType::Mod,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        project_id: None,
        project_platform: None,
        version_override: None,
    }
}

#[test]
fn test_build_sync_plan_preserves_direct_lookup_contracts() {
    let mut direct_dep = project_spec("jei");
    direct_dep.project_id = Some("238222".to_string());
    direct_dep.project_platform = Some(ProjectPlatform::CurseForge);
    direct_dep.version_override = Some(VersionOverride::Single("5678901".to_string()));

    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        loader_version: "0.16.0".to_string(),
        dependencies: vec![direct_dep],
    };

    let sync_plan = build_sync_plan(&plan, &HashSet::new());
    match &sync_plan.actions[0] {
        SyncPlanAction::Add(dep) => {
            assert_eq!(dep.project_id.as_deref(), Some("238222"));
            assert_eq!(dep.project_platform, Some(ProjectPlatform::CurseForge));
            assert_eq!(dep.version_override, Some(VersionOverride::Single("5678901".to_string())));
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[test]
fn test_build_sync_plan_adds_missing_and_removes_extra_mods() {
    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        loader_version: "0.16.0".to_string(),
        dependencies: vec![project_spec("fabric_api")],
    };

    let sync_plan = build_sync_plan(&plan, &HashSet::from(["extra_mod".to_string()]));
    assert_eq!(sync_plan.actions.len(), 2);
    assert!(sync_plan.actions.iter().any(|action| matches!(action, SyncPlanAction::Add(_))));
    assert!(sync_plan.actions.iter().any(|action| matches!(action, SyncPlanAction::Remove { key, .. } if key == "extra_mod")));
}

#[test]
fn test_build_sync_plan_normalizes_dependency_keys_before_matching_installed_mods() {
    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        loader_version: "0.16.0".to_string(),
        dependencies: vec![project_spec("Fabric-API")],
    };

    let sync_plan = build_sync_plan(&plan, &HashSet::from(["fabric_api".to_string()]));

    assert!(sync_plan.actions.is_empty());
    assert!(sync_plan.expected_mods.contains("fabric_api"));
}

#[test]
fn test_build_packwiz_add_commands_for_curseforge_version_override() {
    let commands = build_packwiz_add_commands(
        "238222",
        ProjectPlatform::CurseForge,
        Some(&VersionOverride::Single("5678901".to_string())),
    )
    .unwrap();

    assert_eq!(commands[0], vec!["curseforge", "add", "--addon-id", "238222", "--file-id", "5678901", "-y"]);
}

#[test]
fn test_build_packwiz_add_commands_for_multiple_versions() {
    let commands = build_packwiz_add_commands(
        "AANobbMI",
        ProjectPlatform::Modrinth,
        Some(&VersionOverride::Multiple(vec!["first".to_string(), "second".to_string()])),
    )
    .unwrap();

    assert_eq!(commands.len(), 2);
    assert!(commands[0].contains(&"first".to_string()));
    assert!(commands[1].contains(&"second".to_string()));
}

#[test]
fn test_build_packwiz_add_commands_rejects_empty_version_override_list() {
    let error = build_packwiz_add_commands(
        "AANobbMI",
        ProjectPlatform::Modrinth,
        Some(&VersionOverride::Multiple(vec![])),
    )
    .unwrap_err();

    assert_eq!(error, AddCommandPlanError::EmptyVersionOverrideList);
}

#[tokio::test]
async fn test_resolve_add_contract_matches_sync_search_resolution() {
    let resolver = MockProjectResolver::new().with_project_response(
        "Sodium".to_string(),
        ProjectInfo {
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            title: "Sodium".to_string(),
            downloads: 1_000_000,
            confidence: 95,
            project_type: "mod".to_string(),
        },
    );

    let add_resolution = resolve_add_contract(
        "Sodium",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        None,
        None,
        None,
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "sodium".to_string(),
            normalized_key: "sodium".to_string(),
            search_query: "Sodium".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Fabric,
            project_id: None,
            project_platform: None,
            version_override: None,
        }),
        &resolver,
    )
    .await
    .unwrap();

    match sync_resolution {
        SyncExecutionAction::Add {
            title,
            commands,
            resolved_project_id,
            resolved_platform,
            ..
        } => {
            assert_eq!(title, add_resolution.title);
            assert_eq!(commands, add_resolution.commands);
            assert_eq!(resolved_project_id, add_resolution.resolved_project_id);
            assert_eq!(resolved_platform, add_resolution.resolved_platform);
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_add_contract_matches_sync_direct_id_defaults() {
    let resolver = MockProjectResolver::new();

    let add_resolution = resolve_add_contract(
        "Sodium",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        Some("AANobbMI"),
        None,
        None,
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "sodium".to_string(),
            normalized_key: "sodium".to_string(),
            search_query: "Sodium".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Fabric,
            project_id: Some("AANobbMI".to_string()),
            project_platform: None,
            version_override: None,
        }),
        &resolver,
    )
    .await
    .unwrap();

    match sync_resolution {
        SyncExecutionAction::Add {
            commands,
            resolved_project_id,
            resolved_platform,
            ..
        } => {
            assert_eq!(commands, add_resolution.commands);
            assert_eq!(resolved_project_id, add_resolution.resolved_project_id);
            assert_eq!(resolved_platform, ProjectPlatform::Modrinth);
            assert_eq!(resolved_platform, add_resolution.resolved_platform);
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_add_contract_matches_sync_curseforge_version_override() {
    let resolver = MockProjectResolver::new();
    let version_override = VersionOverride::Single("5678901".to_string());

    let add_resolution = resolve_add_contract(
        "Just Enough Items",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Forge),
        Some("238222"),
        Some(ProjectPlatform::CurseForge),
        Some(&version_override),
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "jei".to_string(),
            normalized_key: "jei".to_string(),
            search_query: "Just Enough Items".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Forge,
            project_id: Some("238222".to_string()),
            project_platform: Some(ProjectPlatform::CurseForge),
            version_override: Some(version_override.clone()),
        }),
        &resolver,
    )
    .await
    .unwrap();

    match sync_resolution {
        SyncExecutionAction::Add {
            title,
            commands,
            resolved_project_id,
            resolved_platform,
            ..
        } => {
            assert_eq!(title, add_resolution.title);
            assert_eq!(commands, add_resolution.commands);
            assert_eq!(resolved_project_id, add_resolution.resolved_project_id);
            assert_eq!(resolved_platform, add_resolution.resolved_platform);
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_add_contract_matches_sync_multiple_version_overrides() {
    let resolver = MockProjectResolver::new();
    let version_override = VersionOverride::Multiple(vec![
        "bad-version".to_string(),
        "good-version".to_string(),
    ]);

    let add_resolution = resolve_add_contract(
        "Sodium",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        Some("AANobbMI"),
        None,
        Some(&version_override),
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "sodium".to_string(),
            normalized_key: "sodium".to_string(),
            search_query: "Sodium".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Fabric,
            project_id: Some("AANobbMI".to_string()),
            project_platform: None,
            version_override: Some(version_override.clone()),
        }),
        &resolver,
    )
    .await
    .unwrap();

    match sync_resolution {
        SyncExecutionAction::Add {
            commands,
            resolved_project_id,
            resolved_platform,
            ..
        } => {
            assert_eq!(commands, add_resolution.commands);
            assert_eq!(resolved_project_id, add_resolution.resolved_project_id);
            assert_eq!(resolved_platform, add_resolution.resolved_platform);
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_add_contract_wraps_resolver_failures() {
    let resolver = MockProjectResolver::new();

    let error = resolve_add_contract(
        "Missing Mod",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        None,
        None,
        None,
        &resolver,
    )
    .await
    .unwrap_err();

    match error {
        AddContractError::ResolveProject { query, source } => {
            assert_eq!(query, "Missing Mod");
            assert!(matches!(source, SearchError::NoResults { query } if query == "Missing Mod"));
        }
        other => panic!("expected resolver error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_add_contract_wraps_plan_failures() {
    let resolver = MockProjectResolver::new();

    let error = resolve_add_contract(
        "Sodium",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        Some("AANobbMI"),
        None,
        Some(&VersionOverride::Multiple(vec![])),
        &resolver,
    )
    .await
    .unwrap_err();

    match error {
        AddContractError::PlanPackwizAdd {
            project_id,
            platform,
            source,
        } => {
            assert_eq!(project_id, "AANobbMI");
            assert_eq!(platform, ProjectPlatform::Modrinth);
            assert_eq!(source, AddCommandPlanError::EmptyVersionOverrideList);
        }
        other => panic!("expected command planning error, got {other:?}"),
    }
}