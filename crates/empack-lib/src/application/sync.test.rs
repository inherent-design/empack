use super::*;
use crate::application::session_mocks::MockProjectResolver;
use crate::empack::config::{ProjectPlan, ProjectSpec};
use crate::empack::parsing::ModLoader;
use crate::empack::search::{ProjectInfo, SearchError};
use crate::primitives::{ProjectPlatform, ProjectType};
use std::collections::HashSet;

fn project_spec(key: &str) -> ProjectSpec {
    ProjectSpec {
        key: key.to_string(),
        search_query: format!("{key} title"),
        project_type: ProjectType::Mod,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        project_id: format!("test-id-{key}"),
        project_platform: ProjectPlatform::Modrinth,
        version_pin: None,
    }
}

#[test]
fn test_build_sync_plan_preserves_direct_lookup_contracts() {
    let mut direct_dep = project_spec("jei");
    direct_dep.project_id = "238222".to_string();
    direct_dep.project_platform = ProjectPlatform::CurseForge;
    direct_dep.version_pin = Some("5678901".to_string());

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
            assert_eq!(dep.project_id, "238222");
            assert_eq!(dep.project_platform, ProjectPlatform::CurseForge);
            assert_eq!(dep.version_pin, Some("5678901".to_string()));
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
fn test_build_sync_plan_matches_by_slug_key() {
    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: ModLoader::Fabric,
        loader_version: "0.16.0".to_string(),
        dependencies: vec![project_spec("fabric_api")],
    };

    let sync_plan = build_sync_plan(&plan, &HashSet::from(["fabric_api".to_string()]));

    assert!(sync_plan.actions.is_empty());
    assert!(sync_plan.expected_mods.contains("fabric_api"));
}

#[test]
fn test_build_packwiz_add_commands_for_curseforge_version_pin() {
    let commands = build_packwiz_add_commands(
        "238222",
        ProjectPlatform::CurseForge,
        Some("5678901"),
    )
    .unwrap();

    assert_eq!(commands[0], vec!["curseforge", "add", "--addon-id", "238222", "--file-id", "5678901", "-y"]);
}

#[test]
fn test_build_packwiz_add_commands_for_modrinth_no_pin() {
    let commands = build_packwiz_add_commands(
        "AANobbMI",
        ProjectPlatform::Modrinth,
        None,
    )
    .unwrap();

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0], vec!["modrinth", "add", "--project-id", "AANobbMI", "-y"]);
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
        "",
        ProjectPlatform::Modrinth,
        None,
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "sodium".to_string(),
            search_query: "Sodium".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Fabric,
            project_id: String::new(),
            project_platform: ProjectPlatform::Modrinth,
            version_pin: None,
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
async fn test_resolve_add_contract_matches_sync_curseforge_version_pin() {
    let resolver = MockProjectResolver::new();

    let add_resolution = resolve_add_contract(
        "Just Enough Items",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Forge),
        "238222",
        ProjectPlatform::CurseForge,
        Some("5678901"),
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "jei".to_string(),
            search_query: "Just Enough Items".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Forge,
            project_id: "238222".to_string(),
            project_platform: ProjectPlatform::CurseForge,
            version_pin: Some("5678901".to_string()),
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
async fn test_resolve_add_contract_matches_sync_modrinth_version_pin() {
    let resolver = MockProjectResolver::new();

    let add_resolution = resolve_add_contract(
        "Sodium",
        ProjectType::Mod,
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "AANobbMI",
        ProjectPlatform::Modrinth,
        Some("good-version"),
        &resolver,
    )
    .await
    .unwrap();

    let sync_resolution = resolve_sync_action(
        &SyncPlanAction::Add(SyncDependencyPlan {
            key: "sodium".to_string(),
            search_query: "Sodium".to_string(),
            project_type: ProjectType::Mod,
            minecraft_version: "1.21.1".to_string(),
            loader: ModLoader::Fabric,
            project_id: "AANobbMI".to_string(),
            project_platform: ProjectPlatform::Modrinth,
            version_pin: Some("good-version".to_string()),
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
        "",
        ProjectPlatform::Modrinth,
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
