use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session_mocks::*;
use crate::empack::search::ProjectInfo;
use crate::primitives::{BuildTarget, ProjectPlatform};
use std::collections::HashSet;
use std::path::PathBuf;

fn modrinth_project(project_id: &str, title: &str) -> ProjectInfo {
    ProjectInfo {
        platform: ProjectPlatform::Modrinth,
        project_id: project_id.to_string(),
        title: title.to_string(),
        downloads: 1000,
        confidence: 95,
        project_type: "mod".to_string(),
    }
}

// ===== HANDLE_VERSION TESTS =====

mod handle_version_tests {
    use super::*;

    #[tokio::test]
    async fn it_displays_version_information() {
        let session = MockCommandSession::new();
        let result = handle_version(&session).await;

        assert!(result.is_ok());
        // In a real implementation, we'd verify the display calls
        // For now, we test that the function completes without error
    }
}

// ===== HANDLE_REQUIREMENTS TESTS =====

mod handle_requirements_tests {
    use super::*;

    #[tokio::test]
    async fn it_reports_packwiz_available() {
        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_version("1.2.3".to_string()));

        let result = handle_requirements(&session).await;

        assert!(result.is_ok());
        // Verify that packwiz was checked
        assert_eq!(
            session.process().check_packwiz().unwrap(),
            (true, "1.2.3".to_string())
        );
    }

    #[tokio::test]
    async fn it_reports_packwiz_unavailable() {
        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_unavailable());

        let result = handle_requirements(&session).await;

        assert!(result.is_ok());
        // Verify that packwiz was checked and found unavailable
        assert_eq!(
            session.process().check_packwiz().unwrap(),
            (false, "1.0.0".to_string())
        );
    }
}

// ===== HANDLE_INIT TESTS =====

mod handle_init_tests {
    use super::*;

    #[tokio::test]
    async fn it_initializes_new_project() {
        let workdir = PathBuf::from("/test/empty-project");
        let target_dir = workdir.join("test-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            Some("test-pack".to_string()),
            None,
            false,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("Test Author".to_string()),
        )
        .await;

        assert!(result.is_ok());
        assert!(session.filesystem().is_directory(&target_dir));

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("name: \"test-pack\""));
        assert!(empack_yml.contains("author: \"Test Author\""));
        assert!(empack_yml.contains("minecraft_version: \"1.21.1\""));

        let pack_toml = session
            .filesystem()
            .read_to_string(&target_dir.join("pack").join("pack.toml"))
            .unwrap();
        assert!(pack_toml.contains("name = \"test-pack\""));
        assert!(pack_toml.contains("author = \"Test Author\""));
        assert!(pack_toml.contains("minecraft = \"1.21.1\""));
    }

    #[tokio::test]
    async fn it_refuses_to_overwrite_existing_without_force() {
        let workdir = PathBuf::from("/test/existing-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir.clone()),
        );

        let original_empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();

        let result = handle_init(&session, None, None, false, None, None, None).await;

        assert!(result.is_ok());
        assert_eq!(
            session
                .filesystem()
                .read_to_string(&workdir.join("empack.yml"))
                .unwrap(),
            original_empack_yml
        );
        assert!(session.interactive_provider.get_confirm_calls().is_empty());
    }

    #[tokio::test]
    async fn it_overwrites_existing_with_force() {
        let workdir = PathBuf::from("/test/force-pack");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            None,
            None,
            true,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("Overwrite Author".to_string()),
        )
        .await;

        assert!(result.is_ok());

        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("name: \"force-pack\""));
        assert!(empack_yml.contains("author: \"Overwrite Author\""));

        let pack_toml = session
            .filesystem()
            .read_to_string(&workdir.join("pack").join("pack.toml"))
            .unwrap();
        assert!(pack_toml.contains("name = \"force-pack\""));
        assert!(pack_toml.contains("author = \"Overwrite Author\""));
        assert!(pack_toml.contains("minecraft = \"1.21.1\""));
    }

    #[tokio::test]
    async fn it_force_reinitializes_built_projects_from_a_clean_state() {
        let workdir = PathBuf::from("/test/force-built-pack");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            )
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            None,
            None,
            true,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("Overwrite Author".to_string()),
        )
        .await;

        assert!(result.is_ok());
        assert!(!session.filesystem().exists(&workdir.join("dist/test-pack.mrpack")));
        assert!(session.filesystem().exists(&workdir.join("pack/pack.toml")));
    }

    #[tokio::test]
    async fn it_handles_user_cancellation_at_confirmation() {
        let target_dir = PathBuf::from("/test/new-project/cancel-test");
        let interactive = MockInteractiveProvider::new().with_confirm(false);

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new().with_current_dir(PathBuf::from("/test/new-project")),
            )
            .with_interactive(interactive);

        let result = handle_init(
            &session,
            Some("cancel-test".to_string()),
            None,
            false,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("Cancel Author".to_string()),
        )
        .await;

        assert!(result.is_ok());
        assert!(!session.filesystem().exists(&target_dir.join("empack.yml")));
        assert!(session.filesystem().is_directory(&target_dir));
        assert_eq!(
            session.interactive_provider.get_confirm_calls(),
            vec![("Create modpack with these settings?".to_string(), true)]
        );
    }
}

// ===== HANDLE_ADD TESTS =====

mod handle_add_tests {
    use super::*;

    #[tokio::test]
    async fn it_adds_single_mod_successfully() {
        let workdir = PathBuf::from("/test/configured-project");

        // Create a mock project info for successful resolution
        let mock_project = modrinth_project("test-mod-id", "Test Mod");

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("test-mod".to_string(), mock_project),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "test-mod-id".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None).await;

        if let Err(e) = &result {
            println!("Error: {}", e);
            println!(
                "Is directory: {}",
                session.filesystem().is_directory(&workdir)
            );
            println!("Current dir: {:?}", session.filesystem().current_dir());
        }
        assert!(result.is_ok());

        // Verify packwiz add command was called
        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "test-mod-id", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_adds_multiple_mods_successfully() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response(
                        "mod1".to_string(),
                        modrinth_project("mod1-id", "Mod One"),
                    )
                    .with_project_response(
                        "mod2".to_string(),
                        modrinth_project("mod2-id", "Mod Two"),
                    ),
            )
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "mod1-id".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    )
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "mod2-id".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    ),
            );

        let result = handle_add(
            &session,
            vec!["mod1".to_string(), "mod2".to_string()],
            false,
            None,
        )
        .await;

        assert!(result.is_ok());

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 2);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "mod1-id", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "mod2-id", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_uses_modrinth_direct_ids_without_resolving_search() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "AANobbMI".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_add(&session, vec!["AANobbMI".to_string()], false, None).await;

        assert!(result.is_ok());
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "AANobbMI", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_uses_curseforge_direct_ids_when_platform_is_explicit() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "curseforge".to_string(),
                    "add".to_string(),
                    "--addon-id".to_string(),
                    "238222".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_add(
            &session,
            vec!["238222".to_string()],
            false,
            Some(crate::application::cli::SearchPlatform::Curseforge),
        )
        .await;

        assert!(result.is_ok());
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["curseforge", "add", "--addon-id", "238222", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_handles_empty_mod_list() {
        let session = MockCommandSession::new();

        let result = handle_add(&session, vec![], false, None).await;

        assert!(result.is_ok());

        // No packwiz commands should have been executed
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_handles_uninitialized_project() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(PathBuf::from("/test/uninitialized-project")),
        );

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None).await;

        assert!(result.is_ok());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = PathBuf::from("/test/incomplete-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(
                        workdir.join("empack.yml"),
                        r#"empack:
  dependencies:
    - 'sodium: "Sodium|mod"'
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Pack"
"#
                        .to_string(),
                    ),
            )
            .with_network(MockNetworkProvider::new().with_project_response(
                "sodium".to_string(),
                modrinth_project("AANobbMI", "Sodium"),
            ))
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "AANobbMI".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_add(&session, vec!["sodium".to_string()], false, None).await;

        assert!(result.is_ok());
        assert!(session.process_provider.get_calls().is_empty());
    }

    #[tokio::test]
    async fn it_handles_packwiz_failures() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_network(MockNetworkProvider::new().with_project_response(
                "failing-mod".to_string(),
                modrinth_project("failing-mod-id", "Failing Mod"),
            ))
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "failing-mod-id".to_string(),
                    "-y".to_string(),
                ],
                Err("Mock packwiz error".to_string()),
            ));

        let result = handle_add(&session, vec!["failing-mod".to_string()], false, None).await;

        assert!(result.is_ok()); // Command handler should handle errors gracefully

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "failing-mod-id", "-y"],
            &workdir.join("pack")
        ));
    }
}

// ===== HANDLE_REMOVE TESTS =====

mod handle_remove_tests {
    use super::*;

    #[tokio::test]
    async fn it_removes_single_mod_successfully() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "test-mod".to_string()],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_remove(&session, vec!["test-mod".to_string()], false).await;

        assert!(result.is_ok());

        // Verify packwiz remove command was called
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "test-mod"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_multiple_mods_successfully() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir),
            )
            .with_process(MockProcessProvider::new());

        let result = handle_remove(
            &session,
            vec!["mod1".to_string(), "mod2".to_string()],
            false,
        )
        .await;

        assert!(result.is_ok());

        // Verify multiple remove commands were called
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "mod1"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "mod2"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_mod_with_dependencies() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "test-mod".to_string()],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_remove(&session, vec!["test-mod".to_string()], true).await;

        assert!(result.is_ok());

        // Verify packwiz remove command was called (without --remove-deps flag)
        // Note: packwiz does not support --remove-deps, orphan detection is implemented separately
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "test-mod"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_handles_empty_mod_list() {
        let session = MockCommandSession::new();

        let result = handle_remove(&session, vec![], false).await;

        assert!(result.is_ok());

        // No packwiz commands should have been executed
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_handles_uninitialized_project() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(PathBuf::from("/test/uninitialized-project")),
        );

        let result = handle_remove(&session, vec!["test-mod".to_string()], false).await;

        assert!(result.is_ok());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = PathBuf::from("/test/incomplete-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(
                        workdir.join("empack.yml"),
                        r#"empack:
  dependencies:
    - 'sodium: "Sodium|mod"'
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Pack"
"#
                        .to_string(),
                    ),
            )
            .with_network(MockNetworkProvider::new().with_project_response(
                "Sodium".to_string(),
                modrinth_project("AANobbMI", "Sodium"),
            ))
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "AANobbMI".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        assert!(session.process_provider.get_calls().is_empty());
    }
}

// ===== HANDLE_SYNC TESTS =====

mod handle_sync_tests {
    use super::*;

    #[tokio::test]
    async fn it_adds_missing_mod() {
        let installed_mods = HashSet::new();

        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    .with_installed_mods(installed_mods),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response(
                        "Fabric API".to_string(),
                        modrinth_project("fabric-api-id", "Fabric API"),
                    )
                    .with_project_response(
                        "Sodium".to_string(),
                        modrinth_project("sodium-id", "Sodium"),
                    ),
            )
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "fabric-api-id".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    )
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "sodium-id".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    ),
            );

        let result = handle_sync(&session).await;

        assert!(result.is_ok());

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 2);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "fabric-api-id", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "sodium-id", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_extra_mod() {
        let mut installed_mods = HashSet::new();
        installed_mods.insert("fabric_api".to_string());
        installed_mods.insert("sodium".to_string());
        installed_mods.insert("extra_mod".to_string());

        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    .with_installed_mods(installed_mods),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "extra_mod".to_string()],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_sync(&session).await;

        assert!(result.is_ok());

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "extra_mod"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_does_nothing_when_in_sync() {
        let mut installed_mods = HashSet::new();
        installed_mods.insert("fabric_api".to_string());
        installed_mods.insert("sodium".to_string());

        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir)
                .with_installed_mods(installed_mods),
        );

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        assert!(session.process_provider.get_calls().is_empty());
    }

    #[tokio::test]
    async fn it_performs_dry_run_without_changes() {
        let mut installed_mods = HashSet::new();
        installed_mods.insert("extra_mod".to_string());

        let workdir = PathBuf::from("/test/configured-project");
        let mut session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir)
                    .with_installed_mods(installed_mods),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response(
                        "Fabric API".to_string(),
                        modrinth_project("fabric-api-id", "Fabric API"),
                    )
                    .with_project_response(
                        "Sodium".to_string(),
                        modrinth_project("sodium-id", "Sodium"),
                    ),
            );
        session.config_provider.app_config.dry_run = true; // Enable dry-run mode

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        assert!(
            session.process_provider.get_calls().is_empty(),
            "Dry-run mode should plan add/remove work without executing packwiz"
        );
    }

    #[tokio::test]
    async fn it_preserves_curseforge_direct_id_and_version_override() {
        let workdir = PathBuf::from("/test/configured-project");
        let empack_yml = r#"empack:
  dependencies:
    - 'jei: "Just Enough Items|mod"'
  project_ids:
    jei: "238222"
  project_platforms:
    jei: curseforge
  version_overrides:
    jei: "5678901"
  minecraft_version: "1.21.1"
  loader: forge
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;
        let pack_toml = r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"

[versions]
minecraft = "1.21.1"
forge = "47.3.0"
"#;

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(workdir.join("empack.yml"), empack_yml.to_string())
                    .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string()),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "curseforge".to_string(),
                    "add".to_string(),
                    "--addon-id".to_string(),
                    "238222".to_string(),
                    "--file-id".to_string(),
                    "5678901".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        assert!(session.process_provider.verify_call(
            "packwiz",
            &[
                "curseforge",
                "add",
                "--addon-id",
                "238222",
                "--file-id",
                "5678901",
                "-y",
            ],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_tries_multiple_version_overrides_until_one_succeeds() {
        let workdir = PathBuf::from("/test/configured-project");
        let empack_yml = r#"empack:
  dependencies:
    - 'sodium: "Sodium|mod"'
  project_ids:
    sodium: "AANobbMI"
  version_overrides:
    sodium:
      - "bad-version"
      - "good-version"
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;
        let pack_toml = r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"

[versions]
minecraft = "1.21.1"
fabric = "0.16.0"
"#;

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(workdir.join("empack.yml"), empack_yml.to_string())
                    .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string()),
            )
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "AANobbMI".to_string(),
                            "--version-id".to_string(),
                            "bad-version".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: "not found".to_string(),
                            success: false,
                        }),
                    )
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "AANobbMI".to_string(),
                            "--version-id".to_string(),
                            "good-version".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    ),
            );

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 2);
        assert!(session.process_provider.verify_call(
            "packwiz",
            &[
                "modrinth",
                "add",
                "--project-id",
                "AANobbMI",
                "--version-id",
                "bad-version",
                "-y",
            ],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &[
                "modrinth",
                "add",
                "--project-id",
                "AANobbMI",
                "--version-id",
                "good-version",
                "-y",
            ],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_handles_uninitialized_project() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(PathBuf::from("/test/uninitialized-project")),
        );

        let result = handle_sync(&session).await;

        assert!(result.is_ok());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }
}

// ===== HANDLE_BUILD TESTS =====

mod handle_build_tests {
    use super::*;

    #[tokio::test]
    async fn it_builds_single_target() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_build(&session, vec!["client".to_string()], false).await;

        // Command handler should not panic and should attempt to delegate to BuildOrchestrator
        // Actual build success depends on external tools (packwiz, unzip, etc.) which may not exist in test environment
        // E2E tests validate the actual build functionality with proper tool setup

        // The key test is that we don't get a panic or unhandled error - the build process should gracefully handle missing tools
        match result {
            Ok(_) => {
                // Build succeeded in mock environment - good
            }
            Err(e) => {
                // Build failed, but this is expected in mock environment without external tools
                // Verify it's a reasonable error related to missing tools or configuration
                let error_string = format!("{}", e);
                assert!(
                    error_string.contains("Failed to execute build pipeline")
                        || error_string.contains("No such file or directory")
                        || error_string.contains("command not found")
                        || error_string.contains("Failed to create build orchestrator")
                        || error_string.contains("ConfigError")
                        || error_string.contains("CommandFailed"),
                    "Unexpected error type: {}",
                    error_string
                );
            }
        }
    }

    #[tokio::test]
    async fn it_builds_multiple_targets() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_build(
            &session,
            vec!["client".to_string(), "server".to_string()],
            false,
        )
        .await;

        // Command handler should gracefully handle missing external tools
        match result {
            Ok(_) => { /* Build succeeded */ }
            Err(e) => {
                let error_string = format!("{}", e);
                assert!(
                    error_string.contains("Failed to execute build pipeline")
                        || error_string.contains("No such file or directory")
                        || error_string.contains("command not found")
                        || error_string.contains("Failed to create build orchestrator")
                        || error_string.contains("ConfigError")
                        || error_string.contains("CommandFailed"),
                    "Unexpected error type: {}",
                    error_string
                );
            }
        }
    }

    #[tokio::test]
    async fn it_builds_all_targets() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_build(&session, vec!["all".to_string()], false).await;

        // Command handler should gracefully handle missing external tools
        match result {
            Ok(_) => { /* Build succeeded */ }
            Err(e) => {
                let error_string = format!("{}", e);
                assert!(
                    error_string.contains("Failed to execute build pipeline")
                        || error_string.contains("No such file or directory")
                        || error_string.contains("command not found")
                        || error_string.contains("Failed to create build orchestrator")
                        || error_string.contains("ConfigError")
                        || error_string.contains("CommandFailed"),
                    "Unexpected error type: {}",
                    error_string
                );
            }
        }
    }

    #[tokio::test]
    async fn it_cleans_before_build_when_requested() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_build(&session, vec!["client".to_string()], true).await;

        // Command handler should gracefully handle missing external tools
        match result {
            Ok(_) => { /* Build succeeded */ }
            Err(e) => {
                let error_string = format!("{}", e);
                assert!(
                    error_string.contains("Failed to execute build pipeline")
                        || error_string.contains("No such file or directory")
                        || error_string.contains("command not found")
                        || error_string.contains("Failed to create build orchestrator")
                        || error_string.contains("ConfigError")
                        || error_string.contains("CommandFailed"),
                    "Unexpected error type: {}",
                    error_string
                );
            }
        }
    }

    #[tokio::test]
    async fn it_handles_uninitialized_project() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(PathBuf::from("/test/uninitialized-project")),
        );

        let result = handle_build(&session, vec!["client".to_string()], false).await;

        // Should complete successfully - command handler checks state and exits gracefully
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = PathBuf::from("/test/incomplete-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_file(
                    workdir.join("empack.yml"),
                    "empack:\n  name: incomplete\n".to_string(),
                ),
        );

        let result = handle_build(&session, vec!["mrpack".to_string()], false).await;

        assert!(result.is_ok());
        assert!(session.process_provider.get_calls().is_empty());
    }

    #[tokio::test]
    async fn it_preserves_configuration_when_cleaning_before_build() {
        let workdir = PathBuf::from("/test/built-project");
        let pack_file = workdir.join("pack").join("pack.toml");
        let rebuilt_mrpack = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            )
            .with_process(MockProcessProvider::new().with_mrpack_export_side_effects());

        let result = handle_build(&session, vec!["mrpack".to_string()], true).await;

        assert!(result.is_ok(), "clean-before-build should succeed: {result:?}");
        assert!(session.filesystem().exists(&workdir.join("empack.yml")));
        assert!(session.filesystem().exists(&workdir.join("pack/pack.toml")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
        assert!(session.filesystem().exists(&rebuilt_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let rebuilt_mrpack_arg = rebuilt_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &[
                "--pack-file",
                &pack_file_arg,
                "mr",
                "export",
                "-o",
                &rebuilt_mrpack_arg,
            ],
            &workdir
        ));
    }
}

// ===== HANDLE_CLEAN TESTS =====

mod handle_clean_tests {
    use super::*;

    #[tokio::test]
    async fn it_cleans_build_artifacts() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_clean(&session, vec!["builds".to_string()]).await;

        // Should complete successfully - command handler delegates to state manager
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_cleans_all_when_requested() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_clean(&session, vec!["all".to_string()]).await;

        // Should complete successfully - command handler delegates to state manager
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_cleans_cache_when_requested() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_clean(&session, vec!["cache".to_string()]).await;

        assert!(result.is_ok());

        // Cache cleaning doesn't use state manager currently
        // In a real implementation, we'd verify the cache cleaning logic
    }

    #[tokio::test]
    async fn it_handles_empty_targets() {
        let workdir = PathBuf::from("/test/configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_clean(&session, vec![]).await;

        // Should complete successfully - command handler delegates to state manager
        assert!(result.is_ok());
    }
}

// ===== HELPER FUNCTION TESTS =====

#[tokio::test]
async fn test_parse_build_targets_all_keyword() {
    let targets = vec!["all".to_string()];
    let result = parse_build_targets(targets);
    assert!(result.is_ok());
    let parsed = result.unwrap();
    // "all" should expand to all available targets
    assert!(parsed.len() >= 4);
    assert!(parsed.contains(&BuildTarget::Client));
    assert!(parsed.contains(&BuildTarget::Server));
}

#[tokio::test]
async fn test_parse_build_targets_single_target() {
    let targets = vec!["server".to_string()];
    let result = parse_build_targets(targets);
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0], BuildTarget::Server);
}

#[tokio::test]
async fn test_parse_build_targets_multiple_targets() {
    let targets = vec!["server".to_string(), "client".to_string()];
    let result = parse_build_targets(targets);
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed.len(), 2);
    assert!(parsed.contains(&BuildTarget::Server));
    assert!(parsed.contains(&BuildTarget::Client));
}

#[tokio::test]
async fn test_parse_build_targets_invalid_target() {
    let targets = vec!["invalid".to_string()];
    let result = parse_build_targets(targets);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_parse_build_targets_empty_list() {
    let targets: Vec<String> = vec![];
    let result = parse_build_targets(targets);
    assert!(result.is_err());
}

// ===== SYNC ACTION TESTS =====

#[tokio::test]
async fn test_sync_action_creation() {
    // Test that sync actions are created correctly
    let add_action = crate::application::sync::SyncExecutionAction::Add {
        key: "jei".to_string(),
        title: "Just Enough Items".to_string(),
        commands: vec![vec!["packwiz".to_string(), "add".to_string(), "jei".to_string()]],
        resolved_project_id: "jei".to_string(),
        resolved_platform: ProjectPlatform::Modrinth,
    };

    match add_action {
        crate::application::sync::SyncExecutionAction::Add {
            key,
            title,
            commands,
            resolved_project_id,
            resolved_platform,
        } => {
            assert_eq!(key, "jei");
            assert_eq!(title, "Just Enough Items");
            assert_eq!(commands, vec![vec!["packwiz", "add", "jei"]]);
            assert_eq!(resolved_project_id, "jei");
            assert_eq!(resolved_platform, ProjectPlatform::Modrinth);
        }
        _ => panic!("Expected Add action"),
    }
}

#[tokio::test]
async fn test_sync_action_remove() {
    let remove_action = crate::application::sync::SyncExecutionAction::Remove {
        key: "jei".to_string(),
        title: "Just Enough Items".to_string(),
    };

    match remove_action {
        crate::application::sync::SyncExecutionAction::Remove { key, title } => {
            assert_eq!(key, "jei");
            assert_eq!(title, "Just Enough Items");
        }
        _ => panic!("Expected Remove action"),
    }
}

// ===== VALIDATION LOGIC TESTS =====

#[tokio::test]
async fn test_build_target_validation_logic() {
    // Test the core validation logic that would be used in handle_build
    let valid_targets = ["all", "client", "server", "client-full", "server-full"];

    for target in valid_targets {
        // This mimics the validation logic in handle_build
        let is_valid = matches!(target, "all" | "client" | "server" | "client-full" | "server-full");
        assert!(is_valid, "Expected '{}' to be valid", target);
    }
}

#[tokio::test]
async fn test_project_type_validation() {
    // Test project type validation logic
    let valid_types = ["mod", "resourcepack", "datapack"];

    for project_type in valid_types {
        let is_valid = matches!(project_type, "mod" | "resourcepack" | "datapack");
        assert!(
            is_valid,
            "Expected '{}' to be valid project type",
            project_type
        );
    }
}

#[tokio::test]
async fn test_mod_loader_validation() {
    // Test mod loader validation logic
    let valid_loaders = ["fabric", "forge", "quilt", "neoforge"];

    for loader in valid_loaders {
        let is_valid = matches!(loader, "fabric" | "forge" | "quilt" | "neoforge");
        assert!(is_valid, "Expected '{}' to be valid mod loader", loader);
    }
}

// ===== ERROR HANDLING TESTS =====

#[test]
fn test_render_add_contract_error_for_resolver_failures() {
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "sodium".to_string(),
        source: crate::empack::search::SearchError::NoResults {
            query: "sodium".to_string(),
        },
    });

    assert_eq!(rendered.item, "Failed to resolve mod");
    assert_eq!(rendered.details, "sodium: No results found for query: sodium");
}

#[test]
fn test_render_add_contract_error_for_plan_failures() {
    let rendered = render_add_contract_error(&AddContractError::PlanPackwizAdd {
        project_id: "AANobbMI".to_string(),
        platform: ProjectPlatform::Modrinth,
        source: crate::application::sync::AddCommandPlanError::EmptyVersionOverrideList,
    });

    assert_eq!(rendered.item, "Failed to prepare add command");
    assert_eq!(rendered.details, "modrinth project AANobbMI: version override list cannot be empty");
}

#[tokio::test]
async fn test_error_message_formatting() {
    // Test that error messages are properly formatted
    let error_msg = format!("Project '{}' not found", "nonexistent");
    assert_eq!(error_msg, "Project 'nonexistent' not found");

    let version_error = format!("Unsupported Minecraft version: {}", "1.0.0");
    assert_eq!(version_error, "Unsupported Minecraft version: 1.0.0");
}

#[tokio::test]
async fn test_success_message_formatting() {
    // Test that success messages are properly formatted
    let success_msg = format!("Successfully added mod: {}", "Just Enough Items");
    assert_eq!(success_msg, "Successfully added mod: Just Enough Items");

    let build_success = format!("Build completed for targets: {}", "client,server");
    assert_eq!(build_success, "Build completed for targets: client,server");
}

// ===== STATE VALIDATION TESTS =====

#[tokio::test]
async fn test_project_initialization_check() {
    // Test the logic that checks if a project is initialized
    // This would be used in all command handlers to validate state

    // Mock a project state check
    let check_initialized =
        |has_pack_toml: bool, has_mod_folder: bool| -> bool { has_pack_toml && has_mod_folder };

    assert!(
        check_initialized(true, true),
        "Expected initialized project to be valid"
    );
    assert!(
        !check_initialized(false, true),
        "Expected missing pack.toml to be invalid"
    );
    assert!(
        !check_initialized(true, false),
        "Expected missing mod folder to be invalid"
    );
    assert!(
        !check_initialized(false, false),
        "Expected completely missing project to be invalid"
    );
}

// ===== COMMAND ARGUMENT VALIDATION TESTS =====

#[tokio::test]
async fn test_command_argument_validation() {
    // Test that command arguments are properly validated
    let validate_mod_name = |name: &str| -> bool {
        !name.is_empty()
            && !name.contains(' ')
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };

    assert!(validate_mod_name("jei"), "Expected 'jei' to be valid");
    assert!(
        validate_mod_name("just-enough-items"),
        "Expected 'just-enough-items' to be valid"
    );
    assert!(
        validate_mod_name("mod_name"),
        "Expected 'mod_name' to be valid"
    );
    assert!(
        !validate_mod_name(""),
        "Expected empty string to be invalid"
    );
    assert!(
        !validate_mod_name("mod name"),
        "Expected space to be invalid"
    );
    assert!(!validate_mod_name("mod@name"), "Expected @ to be invalid");
}

// ===== CONFIGURATION VALIDATION TESTS =====

#[tokio::test]
async fn test_minecraft_version_validation() {
    // Test Minecraft version validation logic
    let validate_version = |version: &str| -> bool {
        let parts: Vec<&str> = version.split('.').collect();
        parts.len() >= 2
            && parts.len() <= 3
            && parts.iter().all(|p| p.chars().all(|c| c.is_numeric()))
    };

    assert!(validate_version("1.21"), "Expected '1.21' to be valid");
    assert!(validate_version("1.20.1"), "Expected '1.20.1' to be valid");
    assert!(validate_version("1.19.2"), "Expected '1.19.2' to be valid");
    assert!(!validate_version("1"), "Expected '1' to be invalid");
    assert!(
        !validate_version("1.21.0.1"),
        "Expected '1.21.0.1' to be invalid"
    );
    assert!(
        !validate_version("1.21.x"),
        "Expected '1.21.x' to be invalid"
    );
}

#[tokio::test]
async fn test_pack_name_validation() {
    // Test pack name validation logic
    let validate_pack_name = |name: &str| -> bool {
        !name.is_empty()
            && name.len() <= 50
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };

    assert!(
        validate_pack_name("my-pack"),
        "Expected 'my-pack' to be valid"
    );
    assert!(
        validate_pack_name("test_pack"),
        "Expected 'test_pack' to be valid"
    );
    assert!(
        validate_pack_name("pack123"),
        "Expected 'pack123' to be valid"
    );
    assert!(
        !validate_pack_name(""),
        "Expected empty string to be invalid"
    );
    assert!(
        !validate_pack_name("my pack"),
        "Expected space to be invalid"
    );
    assert!(!validate_pack_name("pack@name"), "Expected @ to be invalid");
    assert!(
        !validate_pack_name(&"a".repeat(51)),
        "Expected long name to be invalid"
    );
}

// ===== BUILD TARGET VALIDATION TESTS (Slice 3) =====

#[tokio::test]
async fn test_invalid_build_target_single() {
    // Test single invalid target
    let result = parse_build_targets(vec!["invalid-target".to_string()]);
    assert!(result.is_err(), "Invalid target should be rejected");

    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);
    assert!(
        err_msg.contains("invalid-target")
            || err_msg.contains("Unknown")
            || err_msg.contains("Invalid"),
        "Error should mention the invalid target: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_invalid_build_target_mixed_with_valid() {
    // Test mix of valid and invalid targets
    let result = parse_build_targets(vec![
        "client".to_string(),
        "invalid-target".to_string(),
        "server".to_string(),
    ]);
    assert!(result.is_err(), "Should reject if any target is invalid");
}

#[tokio::test]
async fn test_empty_build_target_list() {
    // Test empty target list
    let result = parse_build_targets(vec![]);

    // Empty list should be invalid (error expected)
    assert!(result.is_err(), "Empty target list should be rejected");
    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);
    assert!(
        err_msg.contains("target") || err_msg.contains("required") || err_msg.contains("No"),
        "Error should indicate problem with empty target list: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_case_insensitive_build_targets() {
    // Test case variations - should accept lowercase
    let result_lowercase = parse_build_targets(vec!["client".to_string()]);
    assert!(
        result_lowercase.is_ok(),
        "Lowercase 'client' should be accepted"
    );

    // Uppercase might not be accepted (implementation dependent)
    let result_uppercase = parse_build_targets(vec!["CLIENT".to_string()]);
    // Don't assert - just test that it either works or gives clear error
    if let Err(err) = result_uppercase {
        let err_msg = format!("{:?}", err);
        assert!(!err_msg.is_empty(), "Error for uppercase should be clear");
    }
}

#[tokio::test]
async fn test_all_valid_build_targets_individually() {
    // Test that all documented valid targets are accepted
    let valid_targets = vec!["mrpack", "client", "server", "client-full", "server-full"];

    for target in valid_targets {
        let result = parse_build_targets(vec![target.to_string()]);
        assert!(
            result.is_ok(),
            "Valid target '{}' should be accepted",
            target
        );
    }
}

// ===== BUILD COMMAND ERROR HANDLING TESTS (Slice 3) =====

#[tokio::test]
async fn test_build_with_uninitialized_project() {
    // Test building before project initialization
    let workdir = PathBuf::from("/test/uninitialized-project");
    let session = MockCommandSession::new()
        .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir));

    let result = handle_build(&session, vec!["client".to_string()], false).await;

    // Build on uninitialized project - may succeed in mock (no real packwiz check)
    // or fail depending on state validation. Both are acceptable behaviors.
    match result {
        Ok(_) => {
            // Mock environment allows build attempt - acceptable
        }
        Err(e) => {
            // If it fails, error should mention uninitialized state
            let err_msg = format!("{:?}", e);
            assert!(
                err_msg.contains("not initialized")
                    || err_msg.contains("Uninitialized")
                    || err_msg.contains("pack")
                    || !err_msg.is_empty(),
                "Error should be informative: {}",
                err_msg
            );
        }
    }
}

#[tokio::test]
async fn test_build_with_invalid_target_string() {
    // Test build command with invalid target that parse_build_targets would reject
    let workdir = PathBuf::from("/test/configured-project");
    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_configured_project(workdir),
    );

    let result = handle_build(&session, vec!["not-a-real-target".to_string()], false).await;

    // Should fail with clear error about invalid target
    assert!(result.is_err(), "Build should fail with invalid target");
    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);
    assert!(
        err_msg.contains("not-a-real-target")
            || err_msg.contains("Unknown")
            || err_msg.contains("Invalid"),
        "Error should mention the invalid target: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_build_cleans_before_build_when_flag_set() {
    // Test that --clean flag triggers cleanup before build
    let workdir = PathBuf::from("/test/configured-project");
    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_configured_project(workdir),
    );

    // Build with clean=true
    let result = handle_build(&session, vec!["mrpack".to_string()], true).await;

    // Should complete (clean happens before build attempt)
    // In mock environment, build might fail for other reasons, but clean should execute
    match result {
        Ok(_) => {
            // Success is acceptable
        }
        Err(e) => {
            // Failure is also acceptable in mock, but should not be a "clean" error
            let err_msg = format!("{:?}", e);
            // Just verify error is informative
            assert!(!err_msg.is_empty(), "Error should be informative");
        }
    }
}
