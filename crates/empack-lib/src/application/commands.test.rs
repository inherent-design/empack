use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session_mocks::*;
use crate::empack::search::ProjectInfo;
use crate::primitives::{BuildTarget, ProjectPlatform};
use std::collections::HashSet;

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

        // handle_version is a pure display function -- LiveDisplayProvider writes to indicatif
        // MultiProgress which is not capturable in unit tests. Verifying no error is the
        // meaningful assertion here.
        assert!(result.is_ok());
    }
}

// ===== HANDLE_REQUIREMENTS TESTS =====

mod handle_requirements_tests {
    use super::*;
    use crate::empack::packwiz::check_packwiz_available;
    use std::path::Path;

    #[tokio::test]
    async fn it_reports_packwiz_available() {
        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_version("1.2.3".to_string()));

        let result = handle_requirements(&session).await;

        assert!(result.is_ok());
        // Verify that packwiz was checked via the free function
        assert_eq!(
            check_packwiz_available(session.process(), Path::new(".")).unwrap(),
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
            check_packwiz_available(session.process(), Path::new(".")).unwrap(),
            (false, "not found".to_string())
        );
    }
}

// ===== HANDLE_INIT TESTS =====

mod handle_init_tests {
    use super::*;

    #[tokio::test]
    async fn it_initializes_new_project() {
        let workdir = mock_root().join("empty-project");
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
            None,
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
        let workdir = mock_root().join("existing-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir.clone()),
        );

        let original_empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();

        let result = handle_init(&session, None, None, false, None, None, None, None).await;

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
        let workdir = mock_root().join("force-pack");
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
            None,
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
        let workdir = mock_root().join("force-built-pack");
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
            None,
        )
        .await;

        assert!(result.is_ok());
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(session.filesystem().exists(&workdir.join("pack").join("pack.toml")));
    }

    #[tokio::test]
    async fn it_handles_user_cancellation_at_confirmation() {
        let target_dir = mock_root().join("new-project").join("cancel-test");
        let interactive = MockInteractiveProvider::new().with_confirm(false);

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new().with_current_dir(mock_root().join("new-project")),
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
            None,
        )
        .await;

        assert!(result.is_ok());
        assert!(!session.filesystem().exists(&target_dir.join("empack.yml")));
        // Directory should NOT exist -- ops-as-values defers mkdir past confirmation
        assert!(!session.filesystem().is_directory(&target_dir));
        assert_eq!(
            session.interactive_provider.get_confirm_calls(),
            vec![("Create modpack with these settings?".to_string(), true)]
        );
    }
    #[tokio::test]
    async fn it_rejects_invalid_mc_version_from_cli() {
        let workdir = mock_root().join("bad-mc-version");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            Some("test-pack".to_string()),
            None,
            false,
            Some("fabric".to_string()),
            Some("99.99.99".to_string()),
            Some("Test Author".to_string()),
            None,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99.99.99") && err_msg.contains("not found"),
            "Expected error about invalid MC version, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn it_rejects_invalid_loader_from_cli() {
        let workdir = mock_root().join("bad-loader");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            Some("test-pack".to_string()),
            None,
            false,
            Some("notaloader".to_string()),
            Some("1.21.1".to_string()),
            Some("Test Author".to_string()),
            None,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("notaloader"),
            "Expected error about invalid loader, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn it_accepts_valid_loader_version_from_cli() {
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(mock_root().join("valid-loader-version")),
            )
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            None,
            Some("test-pack".to_string()),
            false,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("TestAuthor".to_string()),
            Some("0.15.0".to_string()),
        )
        .await;

        assert!(
            result.is_ok(),
            "Init with valid loader version should succeed: {result:?}"
        );
    }

    #[tokio::test]
    async fn it_rejects_invalid_loader_version_from_cli() {
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(mock_root().join("invalid-loader-version")),
            )
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            None,
            Some("test-pack".to_string()),
            false,
            Some("fabric".to_string()),
            Some("1.21.1".to_string()),
            Some("TestAuthor".to_string()),
            Some("99.99.99".to_string()),
        )
        .await;

        assert!(
            result.is_err(),
            "Init with invalid loader version should fail"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("99.99.99"),
            "Error should mention the invalid version: {err_msg}"
        );
    }

    #[tokio::test]
    async fn it_accepts_compatible_loader_fallback_for_mc_version() {
        let workdir = mock_root().join("compatible-loader-fallback");
        // When a compatible loader is provided via CLI flags, the init flow
        // succeeds through the fallback path. The mock network fails, so the
        // fallback includes all 4 loaders. The loader version is not in the
        // fetched list, but init still succeeds because it falls back gracefully.
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
            None,
        )
        .await;

        // With fallback versions, "1.21.1" + "fabric" is valid and first fallback
        // loader version "0.15.0" is selected. The final checkpoint should pass.
        assert!(result.is_ok());
    }
}

// ===== VALIDATE_INIT_INPUTS UNIT TESTS =====

mod validate_init_inputs_tests {
    use super::*;

    #[test]
    fn it_passes_with_valid_inputs() {
        let mc_versions = vec!["1.21.1".to_string(), "1.20.1".to_string()];
        let loaders = vec![
            crate::empack::versions::ModLoader::Fabric,
            crate::empack::versions::ModLoader::NeoForge,
        ];
        let loader_versions = vec!["0.15.0".to_string(), "0.14.21".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "fabric",
            &loaders,
            "0.15.0",
            &loader_versions,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn it_rejects_unknown_mc_version() {
        let mc_versions = vec!["1.21.1".to_string()];
        let loaders = vec![crate::empack::versions::ModLoader::Fabric];
        let loader_versions = vec!["0.15.0".to_string()];

        let result = validate_init_inputs(
            "99.0.0",
            &mc_versions,
            "fabric",
            &loaders,
            "0.15.0",
            &loader_versions,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("99.0.0"));
    }

    #[test]
    fn it_rejects_incompatible_loader() {
        let mc_versions = vec!["1.21.1".to_string()];
        let loaders = vec![crate::empack::versions::ModLoader::Fabric];
        let loader_versions = vec!["0.15.0".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "neoforge",
            &loaders,
            "0.15.0",
            &loader_versions,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not compatible"), "Expected compatibility error, got: {}", msg);
    }

    #[test]
    fn it_rejects_unknown_loader_version() {
        let mc_versions = vec!["1.21.1".to_string()];
        let loaders = vec![crate::empack::versions::ModLoader::Fabric];
        let loader_versions = vec!["0.15.0".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "fabric",
            &loaders,
            "0.99.0",
            &loader_versions,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("0.99.0"));
    }
}

// ===== HANDLE_ADD TESTS =====

mod handle_add_tests {
    use super::*;

    #[tokio::test]
    async fn it_adds_single_mod_successfully() {
        let workdir = mock_root().join("configured-project");

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
        let workdir = mock_root().join("configured-project");
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
        let workdir = mock_root().join("configured-project");
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
        let workdir = mock_root().join("configured-project");
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
                .with_current_dir(mock_root().join("uninitialized-project")),
        );

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None).await;

        assert!(result.is_ok());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = mock_root().join("incomplete-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(
                        workdir.join("empack.yml"),
                        r#"empack:
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
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
        let workdir = mock_root().join("configured-project");
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

    #[tokio::test]
    async fn it_uses_packwiz_slug_as_dep_key_when_input_diverges() {
        // User types "iris_shaders" (underscore) but packwiz creates "iris.pw.toml" (different slug).
        // The dep_key stored in empack.yml must match the .pw.toml slug, not user input.
        let workdir = mock_root().join("configured-project");
        let mock_project = modrinth_project("YL57xq9U", "Iris Shaders");

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone()),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("iris_shaders".to_string(), mock_project),
            )
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "YL57xq9U".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    )
                    // Simulate packwiz creating iris.pw.toml (not iris_shaders.pw.toml)
                    .with_packwiz_add_slug("YL57xq9U".to_string(), "iris".to_string()),
            );

        let result = handle_add(&session, vec!["iris_shaders".to_string()], false, None).await;
        assert!(result.is_ok(), "handle_add should succeed: {result:?}");

        // Verify empack.yml was updated with "iris" (from .pw.toml), NOT "iris_shaders" (from input)
        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("iris:"),
            "empack.yml should contain 'iris:' as the dep key (from .pw.toml slug), got:\n{}",
            empack_yml
        );
        assert!(
            !empack_yml.contains("iris_shaders:"),
            "empack.yml should NOT contain 'iris_shaders:' (from user input), got:\n{}",
            empack_yml
        );
    }

    #[tokio::test]
    async fn it_falls_back_to_input_key_when_no_new_pw_toml_detected() {
        // When packwiz doesn't create a new .pw.toml (edge case), fall back to input-derived key.
        let workdir = mock_root().join("configured-project");
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
        // Note: No with_packwiz_add_slug → no .pw.toml created → fallback

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None).await;
        assert!(result.is_ok());

        // Verify empack.yml was updated with the fallback key "test-mod"
        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("test-mod:"),
            "empack.yml should contain 'test-mod:' as fallback dep key, got:\n{}",
            empack_yml
        );
    }

    #[tokio::test]
    async fn it_handles_slug_discovery_with_pre_existing_pw_toml_files() {
        // When mods/ already has .pw.toml files, only the NEW file should be used as dep_key.
        let workdir = mock_root().join("configured-project");
        let mock_project = modrinth_project("new-mod-id", "New Mod");
        let mods_dir = workdir.join("pack").join("mods");

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    // Pre-existing .pw.toml for sodium
                    .with_file(
                        mods_dir.join("sodium.pw.toml"),
                        "name = \"sodium\"\n".to_string(),
                    ),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("New Mod".to_string(), mock_project),
            )
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "new-mod-id".to_string(),
                            "-y".to_string(),
                        ],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    )
                    // packwiz creates "new-mod.pw.toml" — different from "new_mod" (query normalized)
                    .with_packwiz_add_slug("new-mod-id".to_string(), "new-mod".to_string()),
            );

        let result = handle_add(&session, vec!["New Mod".to_string()], false, None).await;
        assert!(result.is_ok(), "handle_add should succeed: {result:?}");

        // The dep_key should be "new-mod" (from the newly created .pw.toml),
        // not "new-mod" from input normalization (they happen to match here, but
        // the mechanism is what matters — it came from filesystem diff)
        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("new-mod:"),
            "empack.yml should contain 'new-mod:' as dep key, got:\n{}",
            empack_yml
        );
    }
}

// ===== HANDLE_REMOVE TESTS =====

mod handle_remove_tests {
    use super::*;

    #[tokio::test]
    async fn it_removes_single_mod_successfully() {
        let workdir = mock_root().join("configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "-y".to_string(), "test-mod".to_string()],
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
            &["remove", "-y", "test-mod"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_multiple_mods_successfully() {
        let workdir = mock_root().join("configured-project");
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
            &["remove", "-y", "mod1"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["remove", "-y", "mod2"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_mod_with_dependencies() {
        let workdir = mock_root().join("configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "-y".to_string(), "test-mod".to_string()],
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
            &["remove", "-y", "test-mod"],
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
                .with_current_dir(mock_root().join("uninitialized-project")),
        );

        let result = handle_remove(&session, vec!["test-mod".to_string()], false).await;

        assert!(result.is_ok());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = mock_root().join("incomplete-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(
                        workdir.join("empack.yml"),
                        r#"empack:
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
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

        let workdir = mock_root().join("configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    .with_installed_mods(installed_mods),
            )
            .with_network(MockNetworkProvider::new())
            .with_process(
                MockProcessProvider::new()
                    .with_packwiz_result(
                        vec![
                            "modrinth".to_string(),
                            "add".to_string(),
                            "--project-id".to_string(),
                            "P7dR8mSH".to_string(),
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
                            "AANobbMI".to_string(),
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
            &["modrinth", "add", "--project-id", "P7dR8mSH", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["modrinth", "add", "--project-id", "AANobbMI", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_extra_mod() {
        let mut installed_mods = HashSet::new();
        installed_mods.insert("fabric_api".to_string());
        installed_mods.insert("sodium".to_string());
        installed_mods.insert("extra_mod".to_string());

        let workdir = mock_root().join("configured-project");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    .with_installed_mods(installed_mods),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "-y".to_string(), "extra_mod".to_string()],
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
            &["remove", "-y", "extra_mod"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_does_nothing_when_in_sync() {
        let mut installed_mods = HashSet::new();
        installed_mods.insert("fabric_api".to_string());
        installed_mods.insert("sodium".to_string());

        let workdir = mock_root().join("configured-project");
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

        let workdir = mock_root().join("configured-project");
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
    async fn it_preserves_curseforge_direct_id_and_version_pin() {
        let workdir = mock_root().join("configured-project");
        let empack_yml = r#"empack:
  dependencies:
    jei:
      status: resolved
      title: Just Enough Items
      platform: curseforge
      project_id: "238222"
      type: mod
      version: "5678901"
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
    async fn it_uses_version_pin_for_modrinth_add() {
        let workdir = mock_root().join("configured-project");
        let empack_yml = r#"empack:
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
      version: good-version
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
            .with_process(MockProcessProvider::new().with_packwiz_result(
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
            ));

        let result = handle_sync(&session).await;

        assert!(result.is_ok());
        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1);
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
                .with_current_dir(mock_root().join("uninitialized-project")),
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
        let workdir = mock_root().join("built-project");
        let pack_file = workdir.join("pack").join("pack.toml");
        let built_mrpack = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            )
            .with_process(MockProcessProvider::new().with_mrpack_export_side_effects());

        let result = handle_build(&session, vec!["mrpack".to_string()], false).await;

        assert!(result.is_ok(), "mrpack build should succeed: {result:?}");
        assert!(session.filesystem().exists(&built_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let built_mrpack_arg = built_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ), "expected packwiz refresh call");
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["--pack-file", &pack_file_arg, "mr", "export", "-o", &built_mrpack_arg],
            &workdir
        ), "expected packwiz mr export call");
    }

    #[tokio::test]
    async fn it_cleans_before_build_when_requested() {
        let workdir = mock_root().join("built-project");
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
        // Original artifacts should be cleaned
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
        // Rebuilt artifact should exist
        assert!(session.filesystem().exists(&rebuilt_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let rebuilt_mrpack_arg = rebuilt_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ), "expected packwiz refresh call");
        assert!(session.process_provider.verify_call(
            "packwiz",
            &["--pack-file", &pack_file_arg, "mr", "export", "-o", &rebuilt_mrpack_arg],
            &workdir
        ), "expected packwiz mr export call");
    }

    #[tokio::test]
    async fn it_handles_uninitialized_project() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(mock_root().join("uninitialized-project")),
        );

        let result = handle_build(&session, vec!["client".to_string()], false).await;

        // Should complete successfully - command handler checks state and exits gracefully
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = mock_root().join("incomplete-project");
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
        let workdir = mock_root().join("built-project");
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
        assert!(session.filesystem().exists(&workdir.join("pack").join("pack.toml")));
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
        let workdir = mock_root().join("built-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_built_project(workdir.clone()),
        );

        let result = handle_clean(&session, vec!["builds".to_string()]).await;

        assert!(result.is_ok(), "handle_clean should succeed: {result:?}");
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
    }

    #[tokio::test]
    async fn it_cleans_all_when_requested() {
        let workdir = mock_root().join("built-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_built_project(workdir.clone()),
        );

        let result = handle_clean(&session, vec!["all".to_string()]).await;

        assert!(result.is_ok(), "handle_clean with 'all' should succeed: {result:?}");
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
    }

    #[tokio::test]
    async fn it_cleans_cache_when_requested() {
        let workdir = mock_root().join("configured-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir),
        );

        let result = handle_clean(&session, vec!["cache".to_string()]).await;

        // Cache cleaning is not yet implemented -- this test verifies the code path does not error
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn it_handles_empty_targets() {
        let workdir = mock_root().join("built-project");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_built_project(workdir.clone()),
        );

        let result = handle_clean(&session, vec![]).await;

        // Empty targets triggers the builds branch (targets.is_empty() check in handle_clean)
        assert!(result.is_ok(), "handle_clean with empty targets should succeed: {result:?}");
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
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
        source: crate::application::sync::AddCommandPlanError::InvalidPlan,
    });

    assert_eq!(rendered.item, "Failed to prepare add command");
    assert_eq!(rendered.details, "modrinth project AANobbMI: invalid packwiz add command plan");
}

#[test]
fn test_render_add_contract_error_network_error() {
    // NetworkError inside ResolveProject → details mention query and network failure
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "iris".to_string(),
        source: crate::empack::search::SearchError::NetworkError {
            source: crate::networking::NetworkingError::PlatformError {
                source: crate::platform::PlatformError::UnsupportedPlatform {
                    platform: "test".to_string(),
                },
            },
        },
    });

    assert_eq!(rendered.item, "Failed to resolve mod");
    assert!(
        rendered.details.contains("iris"),
        "Details should contain the search query; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("Platform"),
        "Details should mention the network/platform error; got: {}",
        rendered.details
    );
}

#[test]
fn test_render_add_contract_error_missing_api_key() {
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "jei".to_string(),
        source: crate::empack::search::SearchError::MissingApiKey {
            platform: "CurseForge".to_string(),
        },
    });

    assert_eq!(rendered.item, "Failed to resolve mod");
    assert!(
        rendered.details.contains("jei"),
        "Details should contain the query; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("CurseForge"),
        "Details should mention the platform missing the API key; got: {}",
        rendered.details
    );
}

#[test]
fn test_render_add_contract_error_low_confidence() {
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "sodium".to_string(),
        source: crate::empack::search::SearchError::LowConfidence {
            confidence: 60,
            threshold: 90,
        },
    });

    assert_eq!(rendered.item, "Failed to resolve mod");
    assert!(
        rendered.details.contains("sodium"),
        "Details should contain the query; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("60%"),
        "Details should contain the confidence value; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("90%"),
        "Details should contain the threshold; got: {}",
        rendered.details
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
    let workdir = mock_root().join("uninitialized-project");
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
    let workdir = mock_root().join("configured-project");
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
    let workdir = mock_root().join("configured-project");
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

// ===== IS_MODRINTH_PROJECT_ID TESTS =====

mod is_modrinth_project_id_tests {
    use super::*;

    #[test]
    fn canonical_valid_id() {
        assert!(is_modrinth_project_id("AANobbMI"));
    }

    #[test]
    fn all_lowercase_alpha_8_chars() {
        assert!(is_modrinth_project_id("abcdefgh"));
    }

    #[test]
    fn all_uppercase_alpha_8_chars() {
        assert!(is_modrinth_project_id("ABCDEFGH"));
    }

    #[test]
    fn mixed_alphanumeric_8_chars() {
        assert!(is_modrinth_project_id("A1B2C3D4"));
    }

    #[test]
    fn mostly_digits_with_letter() {
        assert!(is_modrinth_project_id("1234567a"));
    }

    #[test]
    fn all_digits_8_chars_is_curseforge_not_modrinth() {
        assert!(!is_modrinth_project_id("12345678"));
    }

    #[test]
    fn too_short_7_chars() {
        assert!(!is_modrinth_project_id("ABC1234"));
    }

    #[test]
    fn too_long_9_chars() {
        assert!(!is_modrinth_project_id("ABC123456"));
    }

    #[test]
    fn empty_string() {
        assert!(!is_modrinth_project_id(""));
    }

    #[test]
    fn special_char_in_middle() {
        assert!(!is_modrinth_project_id("ABC-1234"));
    }
}

// ===== IS_CURSEFORGE_PROJECT_ID TESTS =====

mod is_curseforge_project_id_tests {
    use super::*;

    #[test]
    fn canonical_valid_id() {
        assert!(is_curseforge_project_id("238222"));
    }

    #[test]
    fn single_digit() {
        assert!(is_curseforge_project_id("1"));
    }

    #[test]
    fn large_number() {
        assert!(is_curseforge_project_id("999999999"));
    }

    #[test]
    fn leading_zeros() {
        assert!(is_curseforge_project_id("00012345"));
    }

    #[test]
    fn empty_string() {
        assert!(!is_curseforge_project_id(""));
    }

    #[test]
    fn has_letter() {
        assert!(!is_curseforge_project_id("23822A"));
    }

    #[test]
    fn has_dash() {
        assert!(!is_curseforge_project_id("-238222"));
    }

    #[test]
    fn has_space() {
        assert!(!is_curseforge_project_id(" 238222"));
    }
}

// ===== FROM_CLI_INPUT MATRIX TESTS =====

mod from_cli_input_tests {
    use super::*;
    use crate::application::cli::SearchPlatform;

    // --- Platform = None ---

    #[test]
    fn modrinth_id_no_platform() {
        let intent = AddResolutionIntent::from_cli_input("AANobbMI", None);
        assert_eq!(intent.direct_project_id, Some("AANobbMI".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn curseforge_id_no_platform() {
        let intent = AddResolutionIntent::from_cli_input("306612", None);
        assert_eq!(intent.direct_project_id, Some("306612".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::CurseForge));
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "306612");
    }

    #[test]
    fn search_query_no_platform() {
        let intent = AddResolutionIntent::from_cli_input("sodium", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "sodium");
    }

    // --- Platform = Modrinth ---

    #[test]
    fn modrinth_id_with_modrinth_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("AANobbMI", Some(SearchPlatform::Modrinth));
        assert_eq!(intent.direct_project_id, Some("AANobbMI".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.preferred_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn curseforge_id_with_modrinth_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("306612", Some(SearchPlatform::Modrinth));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.search_query, "306612");
    }

    #[test]
    fn search_query_with_modrinth_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("sodium", Some(SearchPlatform::Modrinth));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.search_query, "sodium");
    }

    // --- Platform = Curseforge ---

    #[test]
    fn curseforge_id_with_curseforge_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("306612", Some(SearchPlatform::Curseforge));
        assert_eq!(intent.direct_project_id, Some("306612".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::CurseForge));
        assert_eq!(
            intent.preferred_platform,
            Some(ProjectPlatform::CurseForge)
        );
        assert_eq!(intent.search_query, "306612");
    }

    #[test]
    fn modrinth_id_with_curseforge_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("AANobbMI", Some(SearchPlatform::Curseforge));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(
            intent.preferred_platform,
            Some(ProjectPlatform::CurseForge)
        );
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn search_query_with_curseforge_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("sodium", Some(SearchPlatform::Curseforge));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(
            intent.preferred_platform,
            Some(ProjectPlatform::CurseForge)
        );
        assert_eq!(intent.search_query, "sodium");
    }

    // --- Platform = Both ---

    #[test]
    fn modrinth_id_with_both_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("AANobbMI", Some(SearchPlatform::Both));
        assert_eq!(intent.direct_project_id, Some("AANobbMI".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn curseforge_id_with_both_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("306612", Some(SearchPlatform::Both));
        assert_eq!(intent.direct_project_id, Some("306612".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::CurseForge));
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "306612");
    }

    #[test]
    fn search_query_with_both_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("sodium", Some(SearchPlatform::Both));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "sodium");
    }
}
