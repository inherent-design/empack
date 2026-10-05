use super::*;
use crate::application::session_mocks::MockProjectResolver;
use crate::empack::config::{DependencySource, ProjectPlan, ProjectSpec};
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
        loader: Some(ModLoader::Fabric),
        source: DependencySource::Platform {
            project_id: format!("{:08x}", key.bytes().fold(0u32, |sum, b| sum.wrapping_mul(31).wrapping_add(b as u32))),
            project_platform: ProjectPlatform::Modrinth,
            version_pin: None,
        },
    }
}

#[test]
fn test_build_sync_plan_preserves_direct_lookup_contracts() {
    let mut direct_dep = project_spec("jei");
    direct_dep.source = DependencySource::Platform {
        project_id: "238222".to_string(),
        project_platform: ProjectPlatform::CurseForge,
        version_pin: Some("5678901".to_string()),
    };

    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: Some(ModLoader::Fabric),
        loader_version: "0.16.0".to_string(),
        dependencies: vec![direct_dep],
    };

    let sync_plan = plan_for_keys(&plan, &HashSet::new());
    match &sync_plan.actions[0] {
        SyncPlanAction::Add(dep) => {
            match &dep.source {
                DependencySource::Platform { project_id, project_platform, version_pin } => {
                    assert_eq!(project_id, "238222");
                    assert_eq!(*project_platform, ProjectPlatform::CurseForge);
                    assert_eq!(*version_pin, Some("5678901".to_string()));
                }
                DependencySource::Local { .. } | DependencySource::Url(_) => panic!("expected Platform source"),
            }
        }
        other => panic!("expected add action, got {other:?}"),
    }
}

#[test]
fn test_build_sync_plan_adds_missing_and_retains_unlisted_mods() {
    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: Some(ModLoader::Fabric),
        loader_version: "0.16.0".to_string(),
        dependencies: vec![project_spec("fabric_api")],
    };

    let sync_plan = plan_for_keys(&plan, &HashSet::from(["extra_mod".to_string()]));
    assert_eq!(sync_plan.actions.len(), 1);
    assert!(sync_plan.actions.iter().any(|action| matches!(action, SyncPlanAction::Add(_))));
    assert_eq!(sync_plan.retained, vec!["extra_mod"]);
}

#[test]
fn test_build_sync_plan_matches_by_slug_key() {
    let plan = ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: Some(ModLoader::Fabric),
        loader_version: "0.16.0".to_string(),
        dependencies: vec![project_spec("fabric_api")],
    };

    let sync_plan = plan_for_keys(&plan, &HashSet::from(["fabric_api".to_string()]));

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
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "",
        ProjectPlatform::Modrinth,
        None,
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
            loader: Some(ModLoader::Fabric),
            source: DependencySource::Platform {
                project_id: String::new(),
                project_platform: ProjectPlatform::Modrinth,
                version_pin: None,
            },
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
    let resolver = MockProjectResolver::new().with_response("238222".into(), Ok(crate::empack::search::ProjectInfo {
        platform: ProjectPlatform::CurseForge, project_id: "238222".into(), title: "Just Enough Items".into(),
        project_type: "mod".into(), downloads: 0, confidence: 100,
    }));

    let add_resolution = resolve_add_contract(
        "Just Enough Items",
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Forge),
        "238222",
        ProjectPlatform::CurseForge,
        Some("5678901"),
        None,
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
            loader: Some(ModLoader::Forge),
            source: DependencySource::Platform {
                project_id: "238222".to_string(),
                project_platform: ProjectPlatform::CurseForge,
                version_pin: Some("5678901".to_string()),
            },
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
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "AANobbMI",
        ProjectPlatform::Modrinth,
        Some("GoodVers"),
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
            loader: Some(ModLoader::Fabric),
            source: DependencySource::Platform {
                project_id: "AANobbMI".to_string(),
                project_platform: ProjectPlatform::Modrinth,
                version_pin: Some("GoodVers".to_string()),
            },
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
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "",
        ProjectPlatform::Modrinth,
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

// ── project_type_arg exhaustive variant tests ──────────────────────────

#[test]
fn test_project_type_arg_mod() {
    assert_eq!(project_type_arg(ProjectType::Mod), "mod");
}

#[test]
fn test_project_type_arg_datapack() {
    assert_eq!(project_type_arg(ProjectType::Datapack), "datapack");
}

#[test]
fn test_project_type_arg_resourcepack() {
    assert_eq!(project_type_arg(ProjectType::ResourcePack), "resourcepack");
}

#[test]
fn test_project_type_arg_shader() {
    assert_eq!(project_type_arg(ProjectType::Shader), "shader");
}

// ── loader_arg exhaustive variant tests ────────────────────────────────

#[test]
fn test_loader_arg_fabric() {
    assert_eq!(loader_arg(ModLoader::Fabric), "fabric");
}

#[test]
fn test_loader_arg_forge() {
    assert_eq!(loader_arg(ModLoader::Forge), "forge");
}

#[test]
fn test_loader_arg_quilt() {
    assert_eq!(loader_arg(ModLoader::Quilt), "quilt");
}

#[test]
fn test_loader_arg_neoforge() {
    assert_eq!(loader_arg(ModLoader::NeoForge), "neoforge");
}

// ── build_sync_plan invariant tests ────────────────────────────────────

fn make_plan(deps: Vec<ProjectSpec>) -> ProjectPlan {
    ProjectPlan {
        name: "Test Pack".to_string(),
        author: None,
        version: None,
        minecraft_version: "1.21.1".to_string(),
        loader: Some(ModLoader::Fabric),
        loader_version: "0.16.0".to_string(),
        dependencies: deps,
    }
}

#[test]
fn test_build_sync_plan_add_only() {
    let plan = make_plan(vec![project_spec("sodium"), project_spec("iris")]);
    let sync = plan_for_keys(&plan, &HashSet::new());

    let mut to_add: Vec<String> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Add(dep) => Some(dep.key.clone()),
            _ => None,
        })
        .collect();
    to_add.sort();
    assert_eq!(to_add, vec!["iris", "sodium"]);

    let to_remove: Vec<&str> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Remove { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert!(to_remove.is_empty());
}

#[test]
fn test_build_sync_plan_retains_unlisted_roots() {
    let plan = make_plan(vec![]);
    let installed = HashSet::from(["sodium".to_string(), "iris".to_string()]);
    let sync = plan_for_keys(&plan, &installed);

    let to_add: Vec<&str> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Add(dep) => Some(dep.key.as_str()),
            _ => None,
        })
        .collect();
    assert!(to_add.is_empty());

    let mut to_remove: Vec<String> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Remove { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect();
    to_remove.sort();
    assert!(to_remove.is_empty());
    assert_eq!(sync.retained, vec!["iris", "sodium"]);
}

#[test]
fn test_build_sync_plan_mixed_add_and_retain() {
    let plan = make_plan(vec![project_spec("sodium"), project_spec("iris")]);
    let installed = HashSet::from(["sodium".to_string(), "lithium".to_string()]);
    let sync = plan_for_keys(&plan, &installed);

    let to_add: Vec<String> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Add(dep) => Some(dep.key.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(to_add, vec!["iris"]);

    let to_remove: Vec<String> = sync
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncPlanAction::Remove { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect();
    assert!(to_remove.is_empty());
    assert_eq!(sync.retained, vec!["lithium"]);
}

#[test]
fn test_build_sync_plan_noop() {
    let plan = make_plan(vec![project_spec("sodium")]);
    let installed = HashSet::from(["sodium".to_string()]);
    let sync = plan_for_keys(&plan, &installed);

    assert!(sync.actions.is_empty());
    assert!(sync.expected_mods.contains("sodium"));
}

#[test]
fn test_build_sync_plan_empty_both() {
    let plan = make_plan(vec![]);
    let sync = plan_for_keys(&plan, &HashSet::new());

    assert!(sync.actions.is_empty());
    assert!(sync.expected_mods.is_empty());
}

#[test]
fn test_build_sync_plan_from_spec_always_produces_platform_source() {
    let dep = project_spec("sodium");
    let plan = SyncDependencyPlan::from_spec(&dep);
    assert!(matches!(plan.source, DependencySource::Platform { .. }));
}

#[test]
fn test_build_sync_plan_tracks_local_keys_in_expected_mods() {
    let mut dep = project_spec("local-mod");
    dep.source = DependencySource::Local {
        path: "pack/mods/local-mod.jar".to_string(),
        source_url: Some("https://example.com/local-mod.jar".to_string()),
        sha256: "deadbeef".to_string(),
    };

    let plan = make_plan(vec![dep]);
    let sync = plan_for_keys(&plan, &HashSet::new());

    assert!(sync.expected_mods.contains("local-mod"));
    assert!(sync.actions.is_empty());
}

// ── build_packwiz_add_commands platform x version matrix ───────────────

#[test]
fn test_build_packwiz_add_commands_modrinth_no_version() {
    let cmds = build_packwiz_add_commands("AANobbMI", ProjectPlatform::Modrinth, None).unwrap();
    assert_eq!(cmds, vec![vec![
        "modrinth", "add", "--project-id", "AANobbMI", "-y",
    ]]);
}

#[test]
fn test_build_packwiz_add_commands_modrinth_with_version() {
    let cmds =
        build_packwiz_add_commands("AANobbMI", ProjectPlatform::Modrinth, Some("Vers0123"))
            .unwrap();
    assert_eq!(cmds, vec![vec![
        "modrinth", "add", "--project-id", "AANobbMI", "--version-id", "Vers0123", "-y",
    ]]);
}

#[test]
fn test_build_packwiz_add_commands_curseforge_no_version() {
    let cmds =
        build_packwiz_add_commands("238222", ProjectPlatform::CurseForge, None).unwrap();
    assert_eq!(cmds, vec![vec![
        "curseforge", "add", "--addon-id", "238222", "-y",
    ]]);
}

#[test]
fn test_build_packwiz_add_commands_curseforge_with_version() {
    let cmds =
        build_packwiz_add_commands("238222", ProjectPlatform::CurseForge, Some("5678901"))
            .unwrap();
    assert_eq!(cmds, vec![vec![
        "curseforge", "add", "--addon-id", "238222", "--file-id", "5678901", "-y",
    ]]);
}

// ── resolve_add_contract tests ─────────────────────────────────────────

#[tokio::test]
async fn test_resolve_add_contract_modrinth_search_resolution() {
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

    let resolution = resolve_add_contract(
        "Sodium",
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "",
        ProjectPlatform::Modrinth,
        None,
        None,
        &resolver,
    )
    .await
    .unwrap();

    assert_eq!(resolution.resolved_project_id, "AANobbMI");
    assert_eq!(resolution.resolved_platform, ProjectPlatform::Modrinth);
    assert_eq!(resolution.title, "Sodium");
    assert_eq!(resolution.confidence, Some(95));
    assert_eq!(resolution.commands, vec![vec![
        "modrinth", "add", "--project-id", "AANobbMI", "-y",
    ]]);
}

#[tokio::test]
async fn test_resolve_add_contract_curseforge_direct_id() {
    let resolver = MockProjectResolver::new().with_response("238222".into(), Ok(crate::empack::search::ProjectInfo {
        platform: ProjectPlatform::CurseForge, project_id: "238222".into(), title: "Just Enough Items".into(),
        project_type: "mod".into(), downloads: 0, confidence: 100,
    }));

    let resolution = resolve_add_contract(
        "Just Enough Items",
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Forge),
        "238222",
        ProjectPlatform::CurseForge,
        None,
        None,
        &resolver,
    )
    .await
    .unwrap();

    assert_eq!(resolution.resolved_project_id, "238222");
    assert_eq!(resolution.resolved_platform, ProjectPlatform::CurseForge);
    assert_eq!(resolution.title, "Just Enough Items");
    assert_eq!(resolution.confidence, None);
    assert_eq!(resolution.commands, vec![vec![
        "curseforge", "add", "--addon-id", "238222", "-y",
    ]]);
}

#[tokio::test]
async fn test_resolve_add_contract_modrinth_direct_id_with_version_pin() {
    let resolver = MockProjectResolver::new().with_response("AANobbMI".into(), Ok(crate::empack::search::ProjectInfo {
        platform: ProjectPlatform::Modrinth, project_id: "AANobbMI".into(), title: "Sodium".into(),
        project_type: "mod".into(), downloads: 0, confidence: 100,
    }));

    let resolution = resolve_add_contract(
        "Sodium",
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "AANobbMI",
        ProjectPlatform::Modrinth,
        Some("Vers0abc"),
        None,
        &resolver,
    )
    .await
    .unwrap();

    assert_eq!(resolution.resolved_project_id, "AANobbMI");
    assert_eq!(resolution.resolved_platform, ProjectPlatform::Modrinth);
    assert_eq!(resolution.title, "Sodium");
    assert_eq!(resolution.confidence, None);
    assert_eq!(resolution.commands, vec![vec![
        "modrinth", "add", "--project-id", "AANobbMI", "--version-id", "Vers0abc", "-y",
    ]]);
}

#[tokio::test]
async fn test_resolve_add_contract_search_failure() {
    let resolver = MockProjectResolver::new();

    let err = resolve_add_contract(
        "NonexistentMod",
        Some(ProjectType::Mod),
        Some("1.21.1"),
        Some(ModLoader::Fabric),
        "",
        ProjectPlatform::Modrinth,
        None,
        None,
        &resolver,
    )
    .await
    .unwrap_err();

    match err {
        AddContractError::ResolveProject { query, .. } => {
            assert_eq!(query, "NonexistentMod");
        }
        other => panic!("expected ResolveProject error, got {other:?}"),
    }
}

fn plan_for_keys(plan: &ProjectPlan, keys: &HashSet<String>) -> SyncPlan {
    let installed: Vec<_> = keys.iter().map(|key| InstalledDependency {
        key: key.clone(), identity: DependencyIdentity::from_spec(&project_spec(key)).unwrap(), version: None,
    }).collect();
    build_sync_plan(plan, &installed).unwrap()
}

#[test]
fn identity_reconciliation_retains_required_content_across_repeated_sync() {
    let mut root = project_spec("renderer-alias");
    root.source = project_spec("canonical-renderer").source;
    let installed = vec![
        InstalledDependency { key: "canonical-renderer".into(), identity: DependencyIdentity::from_spec(&root).unwrap(), version: Some(empack_core::identity::PinSelector::ModrinthVersion(empack_core::identity::ModrinthVersionId::parse("Version1").unwrap())) },
        InstalledDependency { key: "required-library".into(), identity: DependencyIdentity::from_spec(&project_spec("required-library")).unwrap(), version: None },
    ];
    let plan = make_plan(vec![root]);
    for _ in 0..2 {
        let sync = build_sync_plan(&plan, &installed).unwrap();
        assert!(sync.actions.is_empty());
        assert!(sync.satisfied.contains("renderer-alias"));
        assert_eq!(sync.retained, vec!["required-library"]);
    }
}

#[test]
fn pin_changes_schedule_reinstallation_and_then_converge() {
    let mut root = project_spec("alias");
    let DependencySource::Platform { version_pin, .. } = &mut root.source else { unreachable!() };
    *version_pin = Some("Version2".into());
    let mut installed = vec![InstalledDependency { key: "canonical".into(), identity: DependencyIdentity::from_spec(&root).unwrap(), version: Some(empack_core::identity::PinSelector::ModrinthVersion(empack_core::identity::ModrinthVersionId::parse("Version1").unwrap())) }];
    let plan = make_plan(vec![root]);
    let sync = build_sync_plan(&plan, &installed).unwrap();
    assert_eq!(sync.actions.len(), 1);
    assert!(matches!(sync.actions[0], SyncPlanAction::Add(_)));
    installed[0].version = Some(empack_core::identity::PinSelector::ModrinthVersion(empack_core::identity::ModrinthVersionId::parse("Version2").unwrap()));
    assert!(build_sync_plan(&plan, &installed).unwrap().actions.is_empty());
}

#[test]
fn duplicate_identity_and_key_collisions_fail_before_execution() {
    let root = project_spec("root");
    let installed = InstalledDependency { key: "root".into(), identity: DependencyIdentity::from_spec(&root).unwrap(), version: None };
    assert!(build_sync_plan(&make_plan(vec![root.clone()]), &[installed.clone(), installed.clone()]).is_err());
    let mut alias = root.clone(); alias.key = "alias".into();
    assert!(build_sync_plan(&make_plan(vec![root, alias]), &[]).is_err());
    let wrong_identity = InstalledDependency { key: "root".into(), identity: DependencyIdentity::from_spec(&project_spec("other")).unwrap(), version: None };
    assert!(build_sync_plan(&make_plan(vec![project_spec("root")]), &[wrong_identity]).is_err());
}

#[test]
fn malformed_provider_identifiers_never_produce_commands() {
    for (platform, project, pin) in [
        (ProjectPlatform::Modrinth, "sodium", None),
        (ProjectPlatform::Modrinth, "AANobbMI", Some("latest")),
        (ProjectPlatform::CurseForge, "0238222", None),
        (ProjectPlatform::CurseForge, "238222", Some("-1")),
    ] {
        assert!(build_packwiz_add_commands(project, platform, pin).is_err());
    }
}
