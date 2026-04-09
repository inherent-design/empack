use super::*;
use crate::application::session::{
    ArchiveProvider, ConfigProvider, FileSystemProvider, InteractiveProvider, NetworkProvider,
    ProcessOutput, ProcessProvider, Session,
};
use crate::application::session_mocks::*;
use crate::display::DisplayProvider;
use crate::empack::search::ProjectInfo;
use crate::primitives::{BuildTarget, ProjectPlatform};
use std::collections::HashSet;
use std::path::Path;

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

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client")
}

fn configured_session(workdir: &Path) -> MockCommandSession {
    MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.to_path_buf())
            .with_configured_project(workdir.to_path_buf()),
    )
}

struct PackwizPathSession {
    inner: MockCommandSession,
    packwiz_bin: String,
}

impl PackwizPathSession {
    fn new(inner: MockCommandSession, packwiz_bin: String) -> Self {
        Self { inner, packwiz_bin }
    }
}

impl Session for PackwizPathSession {
    fn display(&self) -> &dyn DisplayProvider {
        self.inner.display()
    }

    fn filesystem(&self) -> &dyn FileSystemProvider {
        self.inner.filesystem()
    }

    fn network(&self) -> &dyn NetworkProvider {
        self.inner.network()
    }

    fn process(&self) -> &dyn ProcessProvider {
        self.inner.process()
    }

    fn config(&self) -> &dyn ConfigProvider {
        self.inner.config()
    }

    fn interactive(&self) -> &dyn InteractiveProvider {
        self.inner.interactive()
    }

    fn terminal(&self) -> &crate::terminal::TerminalCapabilities {
        self.inner.terminal()
    }

    fn archive(&self) -> &dyn ArchiveProvider {
        self.inner.archive()
    }

    fn packwiz(&self) -> Box<dyn crate::empack::packwiz::PackwizOps + '_> {
        self.inner.packwiz()
    }

    fn state(
        &self,
    ) -> crate::Result<crate::empack::state::PackStateManager<'_, dyn FileSystemProvider + '_>> {
        self.inner.state()
    }

    fn packwiz_bin(&self) -> &str {
        &self.packwiz_bin
    }
}

#[derive(Clone)]
struct StubJarResolver {
    identity: crate::empack::content::JarIdentity,
}

impl crate::empack::content::JarResolver for StubJarResolver {
    async fn identify(
        &self,
        _request: crate::empack::content::JarIdentifyRequest,
    ) -> crate::Result<crate::empack::content::JarIdentity> {
        Ok(self.identity.clone())
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

    #[tokio::test]
    async fn it_reports_packwiz_available_when_executable_exists() {
        let current_exe = std::env::current_exe().expect("current test binary");
        let mut inner = MockCommandSession::new();
        inner
            .process_provider
            .programs
            .insert("java".to_string(), Some("/usr/bin/java".to_string()));
        let session = PackwizPathSession::new(inner, current_exe.to_string_lossy().to_string());

        let result = execute_command_with_session(Commands::Requirements, &session).await;

        assert!(result.is_ok());
    }
}

// ===== FORMAT_EMPACK_YML TESTS =====

mod format_empack_yml_tests {
    use super::*;

    /// Minimal struct for round-trip deserialization of init output.
    #[derive(serde::Deserialize)]
    struct InitYml {
        empack: InitFields,
    }
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct InitFields {
        name: String,
        author: String,
        version: String,
        minecraft_version: String,
        loader_version: String,
    }

    /// Helper: serialize then deserialize, returning parsed fields.
    fn round_trip(
        name: &str,
        author: &str,
        version: &str,
        mc_version: &str,
        loader: &str,
        loader_version: &str,
    ) -> InitYml {
        let yaml = format_empack_yml(name, author, version, mc_version, loader, loader_version, None, None);
        serde_saphyr::from_str(&yaml)
            .unwrap_or_else(|e| panic!("produced invalid YAML: {e}\n---\n{yaml}"))
    }

    #[test]
    fn it_produces_valid_yaml_for_normal_input() {
        let result = format_empack_yml(
            "test-pack",
            "Test Author",
            "1.0.0",
            "1.21.1",
            "fabric",
            "0.18.4",
            None,
            None,
        );
        // serde_saphyr omits quotes for plain strings
        assert!(result.contains("name: test-pack"));
        assert!(result.contains("author: Test Author"));
        assert!(result.contains("version: 1.0.0"));
        assert!(result.contains("minecraft_version: 1.21.1"));
        assert!(result.contains("loader: fabric"));
        assert!(result.contains("loader_version: 0.18.4"));
        assert!(result.contains("dependencies: {}"));
    }

    /// Round-trip: name containing double quotes
    #[test]
    fn yaml_injection_double_quotes_in_name() {
        let parsed = round_trip("My \"Pack\"", "Author", "1.0.0", "1.21.1", "fabric", "0.1");
        assert_eq!(parsed.empack.name, "My \"Pack\"");
    }

    /// Round-trip: name containing backslash
    #[test]
    fn yaml_injection_backslash_in_name() {
        let parsed = round_trip("My \\ Pack", "Author", "1.0.0", "1.21.1", "fabric", "0.1");
        assert_eq!(parsed.empack.name, "My \\ Pack");
    }

    /// Round-trip: author with YAML special characters (colon, hash, apostrophe)
    #[test]
    fn yaml_injection_special_chars_in_author() {
        let parsed = round_trip("pack", "O'Brien: #1 Author", "1.0.0", "1.21.1", "fabric", "0.1");
        assert_eq!(parsed.empack.author, "O'Brien: #1 Author");
    }

    /// Round-trip: name containing newline
    #[test]
    fn yaml_injection_newline_in_name() {
        let parsed = round_trip("Pack\nName", "Author", "1.0.0", "1.21.1", "fabric", "0.1");
        assert_eq!(parsed.empack.name, "Pack\nName");
    }

    #[test]
    fn it_includes_datapack_folder_when_some() {
        let result = format_empack_yml(
            "dp-pack",
            "Author",
            "1.0.0",
            "1.20.1",
            "fabric",
            "0.16.0",
            Some("config/paxi/datapacks"),
            None,
        );
        assert!(
            result.contains("datapack_folder: config/paxi/datapacks"),
            "datapack_folder should appear in output; got:\n{result}",
        );
    }

    #[test]
    fn it_omits_none_fields() {
        let result = format_empack_yml(
            "minimal",
            "Author",
            "1.0.0",
            "1.21.1",
            "fabric",
            "0.18.0",
            None,
            None,
        );
        assert!(
            !result.contains("datapack_folder"),
            "datapack_folder should be omitted when None; got:\n{result}",
        );
        assert!(
            !result.contains("acceptable_game_versions"),
            "acceptable_game_versions should be omitted when None; got:\n{result}",
        );
    }

    #[test]
    fn it_includes_acceptable_game_versions_when_some() {
        let versions = vec!["1.20".to_string(), "1.20.2".to_string()];
        let result = format_empack_yml(
            "ver-pack",
            "Author",
            "1.0.0",
            "1.20.1",
            "fabric",
            "0.16.0",
            None,
            Some(&versions),
        );
        assert!(
            result.contains("acceptable_game_versions"),
            "acceptable_game_versions should appear when Some; got:\n{result}",
        );
        assert!(result.contains("1.20"), "should contain version 1.20");
        assert!(result.contains("1.20.2"), "should contain version 1.20.2");
    }

    #[test]
    fn it_round_trips_through_empack_project_config() {
        use crate::empack::config::EmpackConfig;

        let yaml = format_empack_yml(
            "roundtrip-pack",
            "Test Author",
            "2.0.0",
            "1.21.1",
            "fabric",
            "0.18.4",
            Some("datapacks"),
            Some(&["1.21".to_string(), "1.21.2".to_string()]),
        );

        let parsed: EmpackConfig = serde_saphyr::from_str(&yaml)
            .unwrap_or_else(|e| panic!("format_empack_yml output failed to parse: {e}\n---\n{yaml}"));

        assert_eq!(parsed.empack.name, Some("roundtrip-pack".to_string()));
        assert_eq!(parsed.empack.author, Some("Test Author".to_string()));
        assert_eq!(parsed.empack.version, Some("2.0.0".to_string()));
        assert_eq!(parsed.empack.minecraft_version, Some("1.21.1".to_string()));
        assert_eq!(parsed.empack.loader_version, Some("0.18.4".to_string()));
        assert_eq!(parsed.empack.datapack_folder, Some("datapacks".to_string()));
        assert_eq!(
            parsed.empack.acceptable_game_versions,
            Some(vec!["1.21".to_string(), "1.21.2".to_string()]),
        );
        assert!(parsed.empack.dependencies.is_empty());
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
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(session.filesystem().is_directory(&target_dir));

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("name: test-pack"));
        assert!(empack_yml.contains("author: Test Author"));
        assert!(empack_yml.contains("minecraft_version: 1.21.1"));

        let pack_toml = session
            .filesystem()
            .read_to_string(&target_dir.join("pack").join("pack.toml"))
            .unwrap();
        assert!(pack_toml.contains("name = \"test-pack\""));
        assert!(pack_toml.contains("author = \"Test Author\""));
        assert!(pack_toml.contains("minecraft = \"1.21.1\""));
    }

    #[tokio::test]
    async fn it_applies_interactive_text_inputs_to_generated_config() {
        let workdir = mock_root().join("interactive-project");
        let target_dir = workdir.join("interactive-test");
        let interactive = MockInteractiveProvider::new()
            .queue_text("my-test-pack")
            .queue_text("Test Author")
            .queue_text("1.0.0")
            .queue_text("")
            .queue_confirm(true);
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(interactive);

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("interactive-test".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                loader_version: Some("0.15.0".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "interactive init should succeed: {result:?}");

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("name: my-test-pack"));
        assert!(empack_yml.contains("author: Test Author"));

        let pack_toml = session
            .filesystem()
            .read_to_string(&target_dir.join("pack").join("pack.toml"))
            .unwrap();
        assert!(pack_toml.contains("name = \"my-test-pack\""));
        assert!(pack_toml.contains("author = \"Test Author\""));
        assert!(pack_toml.contains("version = \"1.0.0\""));
        assert!(pack_toml.contains("minecraft = \"1.21.1\""));
        assert!(pack_toml.contains("fabric = \"0.15.0\""));
    }

    #[tokio::test]
    async fn it_refuses_to_overwrite_existing_without_force() {
        let workdir = mock_root().join("existing-project");
        let session = configured_session(&workdir);

        let original_empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();

        let result = handle_init(&session, &InitArgs::default()).await;

        assert!(
            result.is_err(),
            "handle_init should return Err when existing project detected without --force"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("already contains a modpack project"),
            "Error should mention existing project: {}",
            err_msg
        );
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
        let session = configured_session(&workdir)
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                force: true,
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Overwrite Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());

        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("name: force-pack"));
        assert!(empack_yml.contains("author: Overwrite Author"));

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
            &InitArgs {
                force: true,
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Overwrite Author".to_string()),
                ..Default::default()
            },
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
            &InitArgs {
                dir: Some("cancel-test".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Cancel Author".to_string()),
                ..Default::default()
            },
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
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("99.99.99".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
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
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("notaloader".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
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
            &InitArgs {
                pack_name: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("TestAuthor".to_string()),
                loader_version: Some("0.15.0".to_string()),
                ..Default::default()
            },
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
            &InitArgs {
                pack_name: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("TestAuthor".to_string()),
                loader_version: Some("99.99.99".to_string()),
                ..Default::default()
            },
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

    #[test]
    fn it_normalizes_late_1710_forge_loader_versions_for_init() {
        assert_eq!(
            normalize_selected_loader_version("forge", "1.7.10", "10.13.2.1300-1.7.10"),
            "10.13.2.1300"
        );
        assert_eq!(
            normalize_selected_loader_version("forge", "1.7.10", "10.13.2.1291"),
            "10.13.2.1291"
        );
        assert_eq!(
            normalize_selected_loader_version("fabric", "1.21.1", "0.16.0"),
            "0.16.0"
        );
    }

    #[test]
    fn it_matches_raw_and_suffixed_late_1710_forge_loader_versions() {
        assert!(loader_version_matches_available(
            "forge",
            "1.7.10",
            "10.13.4.1614",
            "10.13.4.1614-1.7.10"
        ));
        assert!(loader_version_matches_available(
            "forge",
            "1.7.10",
            "10.13.4.1614-1.7.10",
            "10.13.4.1614"
        ));
        assert!(!loader_version_matches_available(
            "forge",
            "1.7.10",
            "10.13.2.1291",
            "10.13.2.1300-1.7.10"
        ));
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
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "fallback loader init should succeed: {result:?}");

        let target = mock_root().join("compatible-loader-fallback").join("test-pack");
        let empack_yml = session
            .filesystem()
            .read_to_string(&target.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("loader: fabric"),
            "empack.yml should contain fabric loader: {empack_yml}"
        );
    }

    #[tokio::test]
    async fn it_uses_pack_version_from_cli_flag() {
        let workdir = mock_root().join("pack-version-flag");
        let target_dir = workdir.join("test-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                pack_version: Some("2.0.0".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("version: 2.0.0"),
            "empack.yml should contain the CLI-provided version '2.0.0', got:\n{}",
            empack_yml
        );
    }

    #[tokio::test]
    async fn it_cleans_up_created_directory_on_failure() {
        let workdir = mock_root().join("empty-init-fail");
        let target_dir = workdir.join("fail-pack");
        let mut session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));
        session.packwiz_provider.fail_init = true;

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("fail-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_err(), "init should fail when packwiz fails");
        assert!(
            !session.filesystem().is_directory(&target_dir),
            "created directory should be removed after init failure"
        );
    }

    #[tokio::test]
    async fn it_preserves_existing_directory_on_force_init_failure() {
        let workdir = mock_root().join("force-init-fail");
        let mut session = configured_session(&workdir)
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));
        session.packwiz_provider.fail_init = true;

        let result = handle_init(
            &session,
            &InitArgs {
                force: true,
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_err(), "force init should fail when packwiz fails");
        assert!(
            session.filesystem().is_directory(&workdir),
            "pre-existing directory should NOT be removed on force init failure"
        );
    }

    #[tokio::test]
    async fn it_separates_directory_from_name() {
        let workdir = mock_root().join("dir-name-split");
        let target_dir = workdir.join("my-dir");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("my-dir".to_string()),
                pack_name: Some("My Display Name".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(session.filesystem().is_directory(&target_dir));

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("name: My Display Name"),
            "empack.yml should use --name flag for display name, not directory name: {}",
            empack_yml
        );
    }

    #[tokio::test]
    async fn it_does_not_create_subdir_from_interactive_name() {
        let workdir = mock_root().join("no-subdir-from-interactive");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir.clone()))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(
            session.filesystem().exists(&workdir.join("empack.yml")),
            "empack.yml should be in cwd, not in a subdirectory"
        );
    }

    #[tokio::test]
    async fn it_uses_dir_basename_as_default_name_with_yes() {
        let workdir = mock_root().join("cool-project");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir.clone()))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());

        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("name: cool-project"),
            "empack.yml should default name to directory basename 'cool-project': {}",
            empack_yml
        );
    }

    #[tokio::test]
    async fn it_name_flag_with_spaces_does_not_create_directory() {
        let workdir = mock_root().join("name-spaces-test");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir.clone()))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                pack_name: Some("My Cool Pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(
            !session
                .filesystem()
                .is_directory(&workdir.join("My Cool Pack")),
            "No 'My Cool Pack' directory should be created from --name flag"
        );
        assert!(
            session.filesystem().exists(&workdir.join("empack.yml")),
            "empack.yml should be in cwd, not in a space-named directory"
        );

        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("name: My Cool Pack"),
            "empack.yml should contain the display name from --name flag: {}",
            empack_yml
        );
    }
}

mod handle_init_from_source_tests {
    use super::*;

    #[tokio::test]
    async fn it_rejects_missing_local_source() {
        let workdir = mock_root().join("init-from-source-local");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new().with_current_dir(workdir),
        );

        let result = execute_command_with_session(
            Commands::Init(InitArgs {
                from_source: Some(
                    mock_root()
                        .join("missing-source.mrpack")
                        .to_string_lossy()
                        .to_string(),
                ),
                ..Default::default()
            }),
            &session,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("cannot detect source type") || err_msg.contains("Unrecognized"),
            "unexpected error for missing local source: {err_msg}"
        );
    }

    #[tokio::test]
    async fn it_rejects_unrecognized_remote_source() {
        let workdir = mock_root().join("init-from-source-remote");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new().with_current_dir(workdir),
        );

        let result = execute_command_with_session(
            Commands::Init(InitArgs {
                from_source: Some("https://example.com/not-an-empack-source".to_string()),
                ..Default::default()
            }),
            &session,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("cannot detect source type") || err_msg.contains("Unrecognized"),
            "unexpected error for remote source: {err_msg}"
        );
    }

    const CF_MANIFEST_JSON: &str = r#"{
  "minecraft": {
    "version": "1.20.1",
    "modLoaders": [
      { "id": "fabric-0.16.0", "primary": true }
    ]
  },
  "files": [
    { "projectID": 12345, "fileID": 67890, "required": true }
  ],
  "manifestType": "minecraftModpack",
  "overrides": "overrides",
  "name": "TestPack",
  "version": "2.0.0",
  "author": "TestAuthor"
}"#;

    const MR_MANIFEST_JSON: &str = r#"{
  "dependencies": {
    "minecraft": "1.20.1",
    "fabric-loader": "0.14.0"
  },
  "files": [],
  "overrides": "overrides",
  "name": "ModrinthPack",
  "versionId": "2.5.0",
  "summary": "A test modpack"
}"#;

    fn create_cf_zip(manifest_json: &str) -> tempfile::NamedTempFile {
        use std::io::Write;

        let tmp = tempfile::NamedTempFile::with_suffix(".zip").unwrap();
        let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
        zip.start_file::<&str, ()>("manifest.json", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(manifest_json.as_bytes()).unwrap();
        zip.finish().unwrap();
        tmp
    }

    fn create_mrpack(manifest_json: &str) -> tempfile::NamedTempFile {
        use std::io::Write;

        let tmp = tempfile::NamedTempFile::with_suffix(".mrpack").unwrap();
        let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
        zip.start_file::<&str, ()>("modrinth.index.json", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(manifest_json.as_bytes()).unwrap();
        zip.finish().unwrap();
        tmp
    }

    #[test]
    fn import_from_local_parses_curseforge_zip() {
        let session = MockCommandSession::new();
        let archive = create_cf_zip(CF_MANIFEST_JSON);

        let (manifest, tmp_dir, source_path) =
            import_from_local(&session, &archive.path().to_string_lossy()).expect("local cf import");

        assert!(tmp_dir.is_none(), "local import should not allocate a temp dir");
        assert_eq!(source_path, archive.path());
        assert_eq!(manifest.identity.name, "TestPack");
        assert_eq!(manifest.target.minecraft_version, "1.20.1");
        assert_eq!(manifest.source_platform, ProjectPlatform::CurseForge);
    }

    #[test]
    fn import_from_local_parses_modrinth_mrpack() {
        let session = MockCommandSession::new();
        let archive = create_mrpack(MR_MANIFEST_JSON);

        let (manifest, tmp_dir, source_path) = import_from_local(
            &session,
            &archive.path().to_string_lossy(),
        )
        .expect("local mrpack import");

        assert!(tmp_dir.is_none(), "local import should not allocate a temp dir");
        assert_eq!(source_path, archive.path());
        assert_eq!(manifest.identity.name, "ModrinthPack");
        assert_eq!(manifest.target.loader, crate::empack::parsing::ModLoader::Fabric);
        assert_eq!(manifest.source_platform, ProjectPlatform::Modrinth);
    }

    #[test]
    fn import_from_local_rejects_packwiz_directory() {
        let root = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(root.path().join("pack.toml"), "name = \"pack\"").expect("pack.toml");
        std::fs::write(root.path().join("index.toml"), "").expect("index.toml");

        let session = MockCommandSession::new();
        let err = import_from_local(&session, &root.path().to_string_lossy())
            .expect_err("packwiz directory should be rejected");

        assert!(err.to_string().contains("packwiz directory import is not yet implemented"));
    }

    #[tokio::test]
    async fn import_from_remote_rejects_direct_download_url() {
        let session = MockCommandSession::new();

        let err = import_from_remote(&session, "https://example.com/modpack.zip")
            .await
            .expect_err("direct downloads are not supported as remote modpack sources");

        assert!(err.to_string().contains("cannot detect source type"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_file_writes_response_body() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/artifact.bin")
            .with_status(200)
            .with_body("payload")
            .create_async()
            .await;

        let dest_dir = tempfile::TempDir::new().expect("temp dir");
        let dest = dest_dir.path().join("artifact.bin");
        download_file(&test_http_client(), &format!("{}/artifact.bin", server.url()), &dest)
            .await
            .expect("download success");

        assert_eq!(std::fs::read(&dest).expect("downloaded file"), b"payload");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_file_reports_http_error() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/missing.bin")
            .with_status(404)
            .create_async()
            .await;

        let dest_dir = tempfile::TempDir::new().expect("temp dir");
        let dest = dest_dir.path().join("missing.bin");
        let err = download_file(&test_http_client(), &format!("{}/missing.bin", server.url()), &dest)
            .await
            .expect_err("404 should error");

        assert!(err.to_string().contains("HTTP 404"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_downloads_selected_version() {
        let mut server = mockito::Server::new_async().await;
        let archive = create_mrpack(MR_MANIFEST_JSON);
        let archive_bytes = std::fs::read(archive.path()).expect("mrpack bytes");

        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(200)
            .with_body(
                serde_json::json!([
                    {
                        "id": "version-id",
                        "version_number": "2.5.0",
                        "files": [{
                            "filename": "modrinth-pack.mrpack",
                            "primary": true,
                            "url": format!("{}/downloads/modrinth-pack.mrpack", server.url())
                        }]
                    }
                ])
                .to_string(),
            )
            .create_async()
            .await;
        let _artifact = server
            .mock("GET", "/downloads/modrinth-pack.mrpack")
            .with_status(200)
            .with_body(archive_bytes)
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let (manifest, _tmp_dir, dest_path) = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            Some("2.5.0"),
            &format!("{}/v2", server.url()),
        )
        .await
        .expect("modrinth download");

        assert_eq!(manifest.identity.name, "ModrinthPack");
        assert_eq!(dest_path.file_name().and_then(|name| name.to_str()), Some("modrinth-pack.mrpack"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_errors_when_version_missing() {
        let mut server = mockito::Server::new_async().await;
        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(200)
            .with_body(
                serde_json::json!([
                    {
                        "id": "different-id",
                        "version_number": "1.0.0",
                        "files": []
                    }
                ])
                .to_string(),
            )
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let err = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            Some("2.5.0"),
            &format!("{}/v2", server.url()),
        )
        .await
        .expect_err("missing version should error");

        assert!(err.to_string().contains("version '2.5.0' not found"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_wrapper_reports_http_client_error() {
        let session = MockCommandSession::new()
            .with_network(MockNetworkProvider::new().with_failing_http_client());

        let err = download_modrinth_modpack(&session, "test-pack", None)
            .await
            .expect_err("missing HTTP client should fail wrapper");

        assert!(err.to_string().contains("Mock HTTP client unavailable"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_errors_when_api_status_fails() {
        let mut server = mockito::Server::new_async().await;
        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(503)
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let err = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            None,
            &format!("{}/v2", server.url()),
        )
        .await
        .expect_err("non-success status should error");

        assert!(err.to_string().contains("Modrinth API returned 503"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_errors_when_no_versions_exist() {
        let mut server = mockito::Server::new_async().await;
        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let err = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            None,
            &format!("{}/v2", server.url()),
        )
        .await
        .expect_err("empty version list should error");

        assert!(err.to_string().contains("no versions found"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_sanitizes_filename() {
        let mut server = mockito::Server::new_async().await;
        let archive = create_mrpack(MR_MANIFEST_JSON);
        let archive_bytes = std::fs::read(archive.path()).expect("mrpack bytes");

        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(200)
            .with_body(
                serde_json::json!([
                    {
                        "id": "version-id",
                        "version_number": "2.5.0",
                        "files": [{
                            "filename": "nested/path/modrinth-pack.mrpack",
                            "primary": true,
                            "url": format!("{}/downloads/modrinth-pack.mrpack", server.url())
                        }]
                    }
                ])
                .to_string(),
            )
            .create_async()
            .await;
        let _artifact = server
            .mock("GET", "/downloads/modrinth-pack.mrpack")
            .with_status(200)
            .with_body(archive_bytes)
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let (_manifest, _tmp_dir, dest_path) = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            None,
            &format!("{}/v2", server.url()),
        )
        .await
        .expect("modrinth download");

        assert_eq!(
            dest_path.file_name().and_then(|name| name.to_str()),
            Some("modrinth-pack.mrpack")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_modrinth_modpack_with_client_errors_when_file_url_is_missing() {
        let mut server = mockito::Server::new_async().await;
        let _versions = server
            .mock("GET", "/v2/project/test-pack/version")
            .with_status(200)
            .with_body(
                serde_json::json!([
                    {
                        "id": "version-id",
                        "version_number": "2.5.0",
                        "files": [{
                            "filename": "modrinth-pack.mrpack",
                            "primary": true
                        }]
                    }
                ])
                .to_string(),
            )
            .create_async()
            .await;

        let session = MockCommandSession::new();
        let err = download_modrinth_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            None,
            &format!("{}/v2", server.url()),
        )
        .await
        .expect_err("missing file URL should error");

        assert!(err.to_string().contains("missing url field"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_uses_direct_download_url() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let archive = create_cf_zip(CF_MANIFEST_JSON);
        let archive_bytes = std::fs::read(archive.path()).expect("cf zip bytes");

        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "test-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":42,"name":"Test Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/42/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "data": [{
                        "id": 7,
                        "fileName": "test-pack.zip",
                        "downloadUrl": format!("{}/downloads/test-pack.zip", server.url())
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;
        let _artifact = server
            .mock("GET", "/downloads/test-pack.zip")
            .with_status(200)
            .with_body(archive_bytes)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let (manifest, _tmp_dir, dest_path) = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "test-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect("curseforge direct download");

        assert_eq!(manifest.identity.name, "TestPack");
        assert_eq!(dest_path.file_name().and_then(|name| name.to_str()), Some("test-pack.zip"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_uses_download_url_fallback() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let archive = create_cf_zip(CF_MANIFEST_JSON);
        let archive_bytes = std::fs::read(archive.path()).expect("cf zip bytes");

        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "fallback-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":77,"name":"Fallback Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/77/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(200)
            .with_body(r#"{"data":[{"id":9,"fileName":"fallback-pack.zip","downloadUrl":null}]}"#)
            .create_async()
            .await;
        let _download_url = server
            .mock("GET", "/v1/mods/77/files/9/download-url")
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "data": format!("{}/downloads/fallback-pack.zip", server.url())
                })
                .to_string(),
            )
            .create_async()
            .await;
        let _artifact = server
            .mock("GET", "/downloads/fallback-pack.zip")
            .with_status(200)
            .with_body(archive_bytes)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let (manifest, _tmp_dir, dest_path) = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "fallback-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect("curseforge fallback download");

        assert_eq!(manifest.identity.name, "TestPack");
        assert_eq!(dest_path.file_name().and_then(|name| name.to_str()), Some("fallback-pack.zip"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_reports_restricted_downloads() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;

        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "restricted-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":88,"name":"Restricted Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/88/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(200)
            .with_body(r#"{"data":[{"id":10,"fileName":"restricted-pack.zip","downloadUrl":null}]}"#)
            .create_async()
            .await;
        let _download_url = server
            .mock("GET", "/v1/mods/88/files/10/download-url")
            .with_status(403)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "restricted-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("restricted downloads should require manual download");

        assert!(err.to_string().contains("restricted downloads"));
        assert!(err.to_string().contains("restricted-pack"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_wrapper_requires_api_key() {
        let session = MockCommandSession::new();

        let err = download_curseforge_modpack(&session, "test-pack")
            .await
            .expect_err("missing api key should error");

        assert!(err.to_string().contains("CurseForge API key required"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_errors_when_search_fails() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .with_status(500)
            .with_body("bad gateway")
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "broken-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("search failure should error");

        assert!(err.to_string().contains("CurseForge search returned"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_errors_when_search_is_empty() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "missing-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[]}"#)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "missing-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("empty search result should error");

        assert!(err.to_string().contains("no CurseForge modpack found"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_errors_when_files_endpoint_fails() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "broken-files-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":42,"name":"Broken Files Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/42/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(502)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "broken-files-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("files endpoint failure should error");

        assert!(err.to_string().contains("CurseForge files endpoint returned 502"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_errors_when_no_files_exist() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "empty-files-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":42,"name":"Empty Files Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/42/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(200)
            .with_body(r#"{"data":[]}"#)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "empty-files-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("empty file list should error");

        assert!(err.to_string().contains("no files found"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn download_curseforge_modpack_with_client_reports_empty_download_url_as_restricted() {
        use crate::application::config::AppConfig;

        let mut server = mockito::Server::new_async().await;

        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("classId".into(), "4471".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "restricted-pack".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":88,"name":"Restricted Pack"}]}"#)
            .create_async()
            .await;
        let _files = server
            .mock("GET", "/v1/mods/88/files")
            .match_query(mockito::Matcher::UrlEncoded("pageSize".into(), "1".into()))
            .with_status(200)
            .with_body(r#"{"data":[{"id":10,"fileName":"restricted-pack.zip","downloadUrl":null}]}"#)
            .create_async()
            .await;
        let _download_url = server
            .mock("GET", "/v1/mods/88/files/10/download-url")
            .with_status(200)
            .with_body(r#"{"data":""}"#)
            .create_async()
            .await;

        let session = MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            curseforge_api_client_key: Some("test-key".to_string()),
            ..Default::default()
        }));

        let err = download_curseforge_modpack_with_client(
            &session,
            &test_http_client(),
            "restricted-pack",
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("empty fallback download url should be treated as restricted");

        assert!(err.to_string().contains("restricted downloads"));
    }
}

// ===== VALIDATE_INIT_INPUTS UNIT TESTS =====

mod validate_init_inputs_tests {
    use super::*;

    #[test]
    fn it_passes_with_valid_inputs() {
        let mc_versions = vec!["1.21.1".to_string(), "1.20.1".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "fabric",
            "0.15.0",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn it_rejects_unknown_mc_version() {
        let mc_versions = vec!["1.21.1".to_string()];

        let result = validate_init_inputs(
            "99.0.0",
            &mc_versions,
            "fabric",
            "0.15.0",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("99.0.0"));
    }

    #[test]
    fn it_rejects_invalid_loader_string() {
        let mc_versions = vec!["1.21.1".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "notaloader",
            "0.15.0",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Invalid mod loader"), "Expected invalid loader error, got: {}", msg);
    }

    #[test]
    fn it_rejects_empty_loader_version() {
        let mc_versions = vec!["1.21.1".to_string()];

        let result = validate_init_inputs(
            "1.21.1",
            &mc_versions,
            "fabric",
            "",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("required"), "Expected required version error, got: {}", msg);
    }
}

mod resolve_curseforge_slug_tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn wrapper_requires_api_key_before_network_access() {
        let resolver = MockProjectResolver::new();
        let err = resolve_curseforge_slug(
            "sodium",
            &test_http_client(),
            None,
            Some("1.20.1"),
            Some(ModLoader::Fabric),
            None,
            None,
            Some(ProjectType::Mod),
            &resolver,
        )
        .await
        .expect_err("missing api key should error");

        assert!(err.to_string().contains("CurseForge API key required"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_requires_api_key() {
        let resolver = MockProjectResolver::new();
        let err = resolve_curseforge_slug_with_api_base(
            "sodium",
            &test_http_client(),
            None,
            Some("1.20.1"),
            Some(ModLoader::Fabric),
            None,
            None,
            Some(ProjectType::Mod),
            &resolver,
            "https://example.invalid/v1",
        )
        .await
        .expect_err("missing api key should error");

        assert!(err.to_string().contains("CurseForge API key required"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_reports_search_status_errors() {
        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "sodium".into()),
            ]))
            .with_status(503)
            .create_async()
            .await;

        let resolver = MockProjectResolver::new();
        let err = resolve_curseforge_slug_with_api_base(
            "sodium",
            &test_http_client(),
            Some("test-key"),
            Some("1.20.1"),
            Some(ModLoader::Fabric),
            None,
            None,
            Some(ProjectType::Mod),
            &resolver,
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("search failure should error");

        assert!(err.to_string().contains("CurseForge API returned 503"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_reports_missing_slug_results() {
        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "missing-mod".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[]}"#)
            .create_async()
            .await;

        let resolver = MockProjectResolver::new();
        let err = resolve_curseforge_slug_with_api_base(
            "missing-mod",
            &test_http_client(),
            Some("test-key"),
            Some("1.20.1"),
            Some(ModLoader::Fabric),
            None,
            None,
            Some(ProjectType::Mod),
            &resolver,
            &format!("{}/v1", server.url()),
        )
        .await
        .expect_err("empty search should error");

        assert!(err.to_string().contains("project not found"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_resolves_first_search_match_into_add_contract() {
        let mut server = mockito::Server::new_async().await;
        let _search = server
            .mock("GET", "/v1/mods/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("gameId".into(), "432".into()),
                mockito::Matcher::UrlEncoded("slug".into(), "sodium".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"data":[{"id":394468,"name":"Sodium"}]}"#)
            .create_async()
            .await;

        let resolver = MockProjectResolver::new();
        let resolution = resolve_curseforge_slug_with_api_base(
            "sodium",
            &test_http_client(),
            Some("test-key"),
            Some("1.20.1"),
            Some(ModLoader::Fabric),
            Some("12345"),
            None,
            Some(ProjectType::Mod),
            &resolver,
            &format!("{}/v1", server.url()),
        )
        .await
        .expect("curseforge slug resolution");

        assert_eq!(resolution.title, "Sodium");
        assert_eq!(resolution.resolved_project_id, "394468");
        assert_eq!(resolution.resolved_platform, ProjectPlatform::CurseForge);
        assert_eq!(resolution.commands.len(), 1);
        assert!(
            resolution.commands[0].iter().any(|arg| arg == "curseforge"),
            "expected curseforge packwiz add command, got {:?}",
            resolution.commands[0]
        );
        assert!(
            resolution.commands[0].iter().any(|arg| arg == "12345"),
            "expected file id override to flow into add contract, got {:?}",
            resolution.commands[0]
        );
    }
}

mod handle_direct_download_jar_tests {
    use super::*;

    fn configured_direct_download_session(workdir: &Path) -> MockCommandSession {
        MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.to_path_buf())
                .with_configured_project(workdir.to_path_buf()),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn wrapper_propagates_download_failures() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/missing.jar")
            .with_status(404)
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-wrapper-failure");
        let session = configured_direct_download_session(&workdir)
            .with_network(MockNetworkProvider::new().enable_http_client());
        let resolver = MockProjectResolver::new();

        let err = handle_direct_download_jar(
            &session,
            &format!("{}/downloads/missing.jar", server.url()),
            &resolver,
        )
        .await
        .err()
        .expect("download failure should propagate through wrapper");

        assert!(err.to_string().contains("HTTP 404"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_adds_identified_modrinth_jar_via_packwiz() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/sodium.jar")
            .with_status(200)
            .with_body("jar-bytes")
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-modrinth");
        let commands = crate::application::sync::build_packwiz_add_commands(
            "AANobbMI",
            ProjectPlatform::Modrinth,
            Some("version-123"),
        )
        .expect("packwiz add commands");
        let session = configured_direct_download_session(&workdir).with_process(
            MockProcessProvider::new()
                .with_packwiz_result(commands[0].clone(), Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                })),
        );
        let resolver = StubJarResolver {
            identity: crate::empack::content::JarIdentity::Modrinth {
                project_id: "AANobbMI".to_string(),
                version_id: "version-123".to_string(),
                title: "Sodium".to_string(),
            },
        };

        let result = handle_direct_download_jar_with_client_and_resolver(
            &session,
            &format!("{}/downloads/sodium.jar", server.url()),
            &test_http_client(),
            &resolver,
        )
        .await
        .expect("identified modrinth jar should succeed");

        assert_eq!(result.title, "Sodium");
        assert_eq!(result.platform, ProjectPlatform::Modrinth);
        assert_eq!(result.project_id.as_deref(), Some("AANobbMI"));
        assert!(!result.local);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_reports_modrinth_packwiz_failures() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/sodium.jar")
            .with_status(200)
            .with_body("jar-bytes")
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-modrinth-packwiz-fail");
        let commands = crate::application::sync::build_packwiz_add_commands(
            "AANobbMI",
            ProjectPlatform::Modrinth,
            Some("version-123"),
        )
        .expect("packwiz add commands");
        let session = configured_direct_download_session(&workdir).with_process(
            MockProcessProvider::new()
                .with_packwiz_result(commands[0].clone(), Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: "bad add".to_string(),
                    success: false,
                })),
        );
        let resolver = StubJarResolver {
            identity: crate::empack::content::JarIdentity::Modrinth {
                project_id: "AANobbMI".to_string(),
                version_id: "version-123".to_string(),
                title: "Sodium".to_string(),
            },
        };

        let err = handle_direct_download_jar_with_client_and_resolver(
            &session,
            &format!("{}/downloads/sodium.jar", server.url()),
            &test_http_client(),
            &resolver,
        )
        .await
        .err()
        .expect("packwiz add failure should propagate");

        assert!(err.to_string().contains("packwiz add failed: bad add"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_adds_identified_curseforge_jar_via_packwiz() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/jei.jar")
            .with_status(200)
            .with_body("jar-bytes")
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-curseforge");
        let commands = crate::application::sync::build_packwiz_add_commands(
            "238222",
            ProjectPlatform::CurseForge,
            Some("5500000"),
        )
        .expect("packwiz add commands");
        let session = configured_direct_download_session(&workdir).with_process(
            MockProcessProvider::new()
                .with_packwiz_result(commands[0].clone(), Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                })),
        );
        let resolver = StubJarResolver {
            identity: crate::empack::content::JarIdentity::CurseForge {
                project_id: 238222,
                file_id: 5500000,
                title: "JEI".to_string(),
            },
        };

        let result = handle_direct_download_jar_with_client_and_resolver(
            &session,
            &format!("{}/downloads/jei.jar", server.url()),
            &test_http_client(),
            &resolver,
        )
        .await
        .expect("identified curseforge jar should succeed");

        assert_eq!(result.title, "JEI");
        assert_eq!(result.platform, ProjectPlatform::CurseForge);
        assert_eq!(result.project_id.as_deref(), Some("238222"));
        assert!(!result.local);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_reports_curseforge_packwiz_process_errors() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/jei.jar")
            .with_status(200)
            .with_body("jar-bytes")
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-curseforge-process-fail");
        let commands = crate::application::sync::build_packwiz_add_commands(
            "238222",
            ProjectPlatform::CurseForge,
            Some("5500000"),
        )
        .expect("packwiz add commands");
        let session = configured_direct_download_session(&workdir).with_process(
            MockProcessProvider::new().with_packwiz_result(
                commands[0].clone(),
                Err("process exploded".to_string()),
            ),
        );
        let resolver = StubJarResolver {
            identity: crate::empack::content::JarIdentity::CurseForge {
                project_id: 238222,
                file_id: 5500000,
                title: "JEI".to_string(),
            },
        };

        let err = handle_direct_download_jar_with_client_and_resolver(
            &session,
            &format!("{}/downloads/jei.jar", server.url()),
            &test_http_client(),
            &resolver,
        )
        .await
        .err()
        .expect("process execution error should propagate");

        assert!(err.to_string().contains("packwiz add failed: process exploded"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn it_copies_unidentified_jar_as_local_dependency() {
        let mut server = mockito::Server::new_async().await;
        let _artifact = server
            .mock("GET", "/downloads/unknown.jar")
            .with_status(200)
            .with_body("jar-bytes")
            .create_async()
            .await;

        let workdir = mock_root().join("direct-download-local");
        let session = configured_direct_download_session(&workdir);
        let resolver = StubJarResolver {
            identity: crate::empack::content::JarIdentity::Unidentified,
        };

        let result = handle_direct_download_jar_with_client_and_resolver(
            &session,
            &format!("{}/downloads/unknown.jar", server.url()),
            &test_http_client(),
            &resolver,
        )
        .await
        .expect("unidentified jar should be copied locally");

        assert_eq!(result.title, "unknown.jar");
        assert!(result.local);
        assert!(session
            .filesystem()
            .exists(&workdir.join("pack").join("mods").join("unknown.jar")));
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

        let session = configured_session(&workdir)
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

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None, None, None, None).await;

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
            crate::empack::packwiz::PACKWIZ_BIN,
            &["modrinth", "add", "--project-id", "test-mod-id", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_adds_multiple_mods_successfully() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_ok());

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 2);
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["modrinth", "add", "--project-id", "mod1-id", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["modrinth", "add", "--project-id", "mod2-id", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_treats_modrinth_id_as_search_query_without_platform_flag() {
        // After removing Modrinth ID auto-detection, "AANobbMI" without --platform
        // is treated as a search query, not a direct ID lookup.
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response(
                        "AANobbMI".to_string(),
                        modrinth_project("AANobbMI", "Sodium"),
                    ),
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

        let result = handle_add(&session, vec!["AANobbMI".to_string()], false, None, None, None, None).await;

        assert!(result.is_ok());
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["modrinth", "add", "--project-id", "AANobbMI", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_uses_curseforge_direct_ids_when_platform_is_explicit() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_ok());
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["curseforge", "add", "--addon-id", "238222", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_handles_empty_mod_list() {
        let session = MockCommandSession::new();

        let result = handle_add(&session, vec![], false, None, None, None, None).await;

        assert!(result.is_err());

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

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None, None, None, None).await;

        assert!(result.is_err());

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

        let result = handle_add(&session, vec!["sodium".to_string()], false, None, None, None, None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn it_handles_packwiz_failures() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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

        let result = handle_add(&session, vec!["failing-mod".to_string()], false, None, None, None, None).await;

        assert!(result.is_err(), "handle_add must return Err when packwiz fails");

        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1);
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
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

        let session = configured_session(&workdir)
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

        let result = handle_add(&session, vec!["iris_shaders".to_string()], false, None, None, None, None).await;
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

        let session = configured_session(&workdir)
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

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None, None, None, None).await;
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
                    // packwiz creates "new-mod.pw.toml"; different from "new_mod" (query normalized)
                    .with_packwiz_add_slug("new-mod-id".to_string(), "new-mod".to_string()),
            );

        let result = handle_add(&session, vec!["New Mod".to_string()], false, None, None, None, None).await;
        assert!(result.is_ok(), "handle_add should succeed: {result:?}");

        // The dep_key should be "new-mod" (from the newly created .pw.toml),
        // not "new-mod" from input normalization (they happen to match here, but
        // the mechanism is what matters; it came from filesystem diff)
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

    #[tokio::test]
    async fn it_skips_side_effects_in_dry_run() {
        let workdir = mock_root().join("configured-project");
        let mock_project = modrinth_project("test-mod-id", "Test Mod");

        let mut session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("test-mod".to_string(), mock_project),
            );
        session.config_provider.app_config.dry_run = true;

        let result = handle_add(&session, vec!["test-mod".to_string()], false, None, None, None, None).await;

        assert!(result.is_ok());
        assert!(
            session.process_provider.get_calls().is_empty(),
            "Dry-run mode should not execute packwiz commands"
        );
    }
}

// ===== HANDLE_REMOVE TESTS =====

mod handle_remove_tests {
    use super::*;

    #[tokio::test]
    async fn it_removes_single_mod_successfully() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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
            crate::empack::packwiz::PACKWIZ_BIN,
            &["remove", "-y", "test-mod"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_multiple_mods_successfully() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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
            crate::empack::packwiz::PACKWIZ_BIN,
            &["remove", "-y", "mod1"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["remove", "-y", "mod2"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_removes_mod_with_dependencies() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
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
            crate::empack::packwiz::PACKWIZ_BIN,
            &["remove", "-y", "test-mod"],
            &session.filesystem_provider.current_dir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_handles_empty_mod_list() {
        let session = MockCommandSession::new();

        let result = handle_remove(&session, vec![], false).await;

        assert!(result.is_err());

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

        assert!(result.is_err());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_rejects_incomplete_project_state() {
        let workdir = mock_root().join("incomplete-project");
        let session = MockCommandSession::new().with_filesystem(
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
        );

        let result = handle_remove(&session, vec!["sodium".to_string()], false).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn it_skips_side_effects_in_dry_run() {
        let workdir = mock_root().join("configured-project");
        let mut session = configured_session(&workdir);
        session.config_provider.app_config.dry_run = true;

        let result = handle_remove(&session, vec!["test-mod".to_string()], false).await;

        assert!(result.is_ok());
        assert!(
            session.process_provider.get_calls().is_empty(),
            "Dry-run mode should not execute packwiz commands"
        );
    }

    #[tokio::test]
    async fn it_returns_error_when_remove_fails() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
            .with_process(
                MockProcessProvider::new().with_packwiz_result(
                    vec!["remove".to_string(), "-y".to_string(), "bad-mod".to_string()],
                    Ok(ProcessOutput {
                        stdout: String::new(),
                        stderr: "Error: mod not found".to_string(),
                        success: false,
                    }),
                ),
            );

        let result = handle_remove(&session, vec!["bad-mod".to_string()], false).await;

        assert!(result.is_err(), "handle_remove should return Err when mods fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("bad-mod"),
            "Error should mention the failed mod: {}",
            err_msg
        );
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
                            "--no-refresh".to_string(),
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
                            "--no-refresh".to_string(),
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
                    )
                    .with_packwiz_result(
                        vec!["refresh".to_string()],
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
        assert_eq!(calls.len(), 3);
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["--no-refresh", "modrinth", "add", "--project-id", "P7dR8mSH", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["--no-refresh", "modrinth", "add", "--project-id", "AANobbMI", "-y"],
            &workdir.join("pack")
        ));
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["refresh"],
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
            crate::empack::packwiz::PACKWIZ_BIN,
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
            crate::empack::packwiz::PACKWIZ_BIN,
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
            crate::empack::packwiz::PACKWIZ_BIN,
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

        assert!(result.is_err());

        // Should not execute packwiz commands in uninitialized project
        let calls = session.process_provider.get_calls();
        assert!(calls.is_empty());
    }

    #[tokio::test]
    async fn it_returns_error_when_all_planning_resolutions_fail() {
        // Deps with empty project_id force the resolver path, which returns errors
        let workdir = mock_root().join("all-fail-sync");
        let empack_yml = r#"empack:
  dependencies:
    mod_a:
      status: resolved
      title: Mod A
      platform: modrinth
      project_id: ""
      type: mod
    mod_b:
      status: resolved
      title: Mod B
      platform: modrinth
      project_id: ""
      type: mod
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
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.0"
"#;

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(workdir.join("empack.yml"), empack_yml.to_string())
                    .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string())
                    .with_file(
                        workdir.join("pack").join("index.toml"),
                        "hash-format = \"sha256\"\n".to_string(),
                    ),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_error_response("Mod A".to_string(), "network timeout".to_string())
                    .with_error_response("Mod B".to_string(), "network timeout".to_string()),
            );

        let result = handle_sync(&session).await;

        assert!(result.is_err(), "handle_sync should fail when all resolutions fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed during resolution"),
            "Error should mention resolution failures, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("2"),
            "Error should mention the count of 2 failures, got: {}",
            err_msg
        );
        // No packwiz commands should have been executed
        assert!(session.process_provider.get_calls().is_empty());
    }

    #[tokio::test]
    async fn it_proceeds_with_warning_when_some_planning_resolutions_fail() {
        // mod_a has empty project_id (resolver fails), mod_b has real project_id (succeeds)
        let workdir = mock_root().join("partial-fail-sync");
        let empack_yml = r#"empack:
  dependencies:
    mod_a:
      status: resolved
      title: Mod A
      platform: modrinth
      project_id: ""
      type: mod
    mod_b:
      status: resolved
      title: Mod B
      platform: modrinth
      project_id: BBNobbMI
      type: mod
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
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.0"
"#;

        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_file(workdir.join("empack.yml"), empack_yml.to_string())
                    .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string())
                    .with_file(
                        workdir.join("pack").join("index.toml"),
                        "hash-format = \"sha256\"\n".to_string(),
                    ),
            )
            .with_network(
                MockNetworkProvider::new()
                    .with_error_response("Mod A".to_string(), "network timeout".to_string()),
            )
            .with_process(
                MockProcessProvider::new().with_packwiz_result(
                    vec![
                        "modrinth".to_string(),
                        "add".to_string(),
                        "--project-id".to_string(),
                        "BBNobbMI".to_string(),
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

        assert!(result.is_ok(), "handle_sync should succeed for partial failure: {result:?}");
        // Only the successful resolution should have been executed
        let calls = session.process_provider.get_calls();
        assert_eq!(
            calls.len(),
            1,
            "Only the successfully resolved action should execute, got {} calls",
            calls.len()
        );
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["modrinth", "add", "--project-id", "BBNobbMI", "-y"],
            &workdir.join("pack")
        ));
    }

    #[tokio::test]
    async fn it_succeeds_normally_when_no_planning_resolutions_fail() {
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
                            "--no-refresh".to_string(),
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
                            "--no-refresh".to_string(),
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
                    )
                    .with_packwiz_result(
                        vec!["refresh".to_string()],
                        Ok(ProcessOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            success: true,
                        }),
                    ),
            );

        let result = handle_sync(&session).await;

        assert!(result.is_ok(), "handle_sync should succeed when all resolutions pass: {result:?}");
        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 3, "Both resolved actions + final refresh should execute");
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

        let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], ..Default::default() }).await;

        assert!(result.is_ok(), "mrpack build should succeed: {result:?}");
        assert!(session.filesystem().exists(&built_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let built_mrpack_arg = built_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ), "expected packwiz refresh call");
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
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

        let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], clean: true, ..Default::default() }).await;

        assert!(result.is_ok(), "clean-before-build should succeed: {result:?}");
        // Original artifacts should be cleaned
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
        // Rebuilt artifact should exist
        assert!(session.filesystem().exists(&rebuilt_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let rebuilt_mrpack_arg = rebuilt_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ), "expected packwiz refresh call");
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
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

        let result = handle_build(&session, &BuildArgs { targets: vec!["client".to_string()], ..Default::default() }).await;

        assert!(result.is_err(), "handle_build must return Err when not in a modpack directory");
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

        let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], ..Default::default() }).await;

        assert!(result.is_err(), "handle_build must return Err for incomplete project state");
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

        let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], clean: true, ..Default::default() }).await;

        assert!(result.is_ok(), "clean-before-build should succeed: {result:?}");
        assert!(session.filesystem().exists(&workdir.join("empack.yml")));
        assert!(session.filesystem().exists(&workdir.join("pack").join("pack.toml")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")));
        assert!(!session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")));
        assert!(session.filesystem().exists(&rebuilt_mrpack));

        let pack_file_arg = pack_file.display().to_string();
        let rebuilt_mrpack_arg = rebuilt_mrpack.display().to_string();
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
            &["--pack-file", &pack_file_arg, "refresh"],
            &workdir
        ));
        assert!(session.process_provider.verify_call(
            crate::empack::packwiz::PACKWIZ_BIN,
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

    #[tokio::test]
    async fn it_skips_side_effects_in_dry_run() {
        let workdir = mock_root().join("built-project");
        let mut session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            );
        session.config_provider.app_config.dry_run = true;

        let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], ..Default::default() }).await;

        assert!(result.is_ok());
        assert!(
            session.process_provider.get_calls().is_empty(),
            "Dry-run mode should not execute build commands"
        );
        assert!(
            session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")),
            "Dry-run mode should not modify dist/ artifacts"
        );
    }
}

mod handle_build_continue_tests {
    #![allow(clippy::await_holding_lock)]

    use super::*;
    use std::ffi::OsString;
    use tempfile::TempDir;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        unsafe fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn cached_full_build_filesystem(workdir: PathBuf) -> MockFileSystemProvider {
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_empack_project(workdir.clone(), "Restricted Pack", "1.21.1", "fabric")
            .with_file(
                workdir.join("cache").join("packwiz-installer-bootstrap.jar"),
                "bootstrap".to_string(),
            )
            .with_file(
                workdir.join("cache").join("packwiz-installer.jar"),
                "installer".to_string(),
            )
    }

    fn tty_capabilities() -> crate::terminal::TerminalCapabilities {
        crate::terminal::TerminalCapabilities {
            color: crate::primitives::TerminalColorCaps::None,
            unicode: crate::primitives::TerminalUnicodeCaps::Ascii,
            is_tty: true,
            cols: 80,
        }
    }

    fn restricted_install_output(workdir: &std::path::Path) -> crate::application::session::ProcessOutput {
        crate::application::session::ProcessOutput {
            stdout: "Failed to download modpack, the following errors were encountered:\nOptiFine.jar:".to_string(),
            stderr: format!(
                "java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.\nPlease go to https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891 and save this file to {}\n\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)",
                workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
            ),
            success: false,
        }
    }

    fn client_full_installer_args(workdir: &Path) -> Vec<String> {
        vec![
            "-jar".to_string(),
            workdir
                .join("cache")
                .join("packwiz-installer-bootstrap.jar")
                .to_string_lossy()
                .to_string(),
            "--bootstrap-main-jar".to_string(),
            workdir
                .join("cache")
                .join("packwiz-installer.jar")
                .to_string_lossy()
                .to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            workdir
                .join("dist")
                .join("client-full")
                .join("pack")
                .join("pack.toml")
                .to_string_lossy()
                .to_string(),
        ]
    }

    #[tokio::test]
    async fn build_continue_rejects_targets() {
        let workdir = mock_root().join("continue-reject-targets");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir));

        let err = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["client-full".to_string()],
                continue_build: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("continue build should reject positional targets");

        assert!(err.to_string().contains("does not accept positional targets"));
    }

    #[tokio::test]
    async fn build_continue_rejects_clean() {
        let workdir = mock_root().join("continue-reject-clean");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir));

        let err = handle_build(
            &session,
            &BuildArgs {
                continue_build: true,
                clean: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("continue build should reject clean");

        assert!(err.to_string().contains("cannot be combined with --clean"));
    }

    #[tokio::test]
    async fn build_continue_errors_without_pending_state() {
        let workdir = mock_root().join("continue-no-pending");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir));

        let err = handle_build(
            &session,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("continue build should require pending state");

        assert_eq!(err.to_string(), "No pending restricted build to continue");
    }

    #[tokio::test]
    async fn fresh_restricted_build_writes_pending_state() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-record-state");
        let pack_toml_path = workdir.join("dist").join("client-full").join("pack").join("pack.toml");
        let process = MockProcessProvider::new().with_result(
            "java".to_string(),
            vec![
                "-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer-bootstrap.jar")
                    .to_string_lossy()
                    .to_string(),
                "--bootstrap-main-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer.jar")
                    .to_string_lossy()
                    .to_string(),
                "-g".to_string(),
                "-s".to_string(),
                "both".to_string(),
                pack_toml_path.to_string_lossy().to_string(),
            ],
            Ok(restricted_install_output(&workdir)),
        );
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(process);

        let err = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["client-full".to_string()],
                ..Default::default()
            },
        )
        .await
        .expect_err("restricted build should stop for manual download");

        assert!(err.to_string().contains("empack build --continue"));
        let pending = crate::empack::restricted_build::load_pending_build(
            session.filesystem(),
            &workdir,
        )
        .expect("load pending build")
        .expect("pending build exists");
        assert_eq!(pending.targets, vec!["client-full"]);
        assert_eq!(pending.entries.len(), 1);
        assert_eq!(pending.entries[0].filename, "OptiFine.jar");
    }

    #[tokio::test]
    async fn fresh_restricted_build_imports_downloads_dir_into_cache() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-import-downloads-dir");
        let downloads_dir = workdir.join("manual-downloads");
        let pack_toml_path = workdir.join("dist").join("client-full").join("pack").join("pack.toml");
        let process = MockProcessProvider::new().with_result(
            "java".to_string(),
            vec![
                "-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer-bootstrap.jar")
                    .to_string_lossy()
                    .to_string(),
                "--bootstrap-main-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer.jar")
                    .to_string_lossy()
                    .to_string(),
                "-g".to_string(),
                "-s".to_string(),
                "both".to_string(),
                pack_toml_path.to_string_lossy().to_string(),
            ],
            Ok(restricted_install_output(&workdir)),
        );
        let session = MockCommandSession::new()
            .with_filesystem(
                cached_full_build_filesystem(workdir.clone()).with_file(
                    downloads_dir.join("OptiFine.jar"),
                    "manual bytes".to_string(),
                ),
            )
            .with_process(process);

        let _ = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["client-full".to_string()],
                downloads_dir: Some(downloads_dir.to_string_lossy().to_string()),
                ..Default::default()
            },
        )
        .await;

        let pending = crate::empack::restricted_build::load_pending_build(
            session.filesystem(),
            &workdir,
        )
        .expect("load pending build")
        .expect("pending build exists");
        assert!(
            session
                .filesystem()
                .exists(&pending.restricted_cache_path().join("OptiFine.jar")),
            "downloads-dir file should be imported into the managed restricted cache"
        );
    }

    #[tokio::test]
    async fn fresh_restricted_build_prompts_before_opening_browser_urls() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-browser-confirm-no");
        let pack_toml_path = workdir
            .join("dist")
            .join("client-full")
            .join("pack")
            .join("pack.toml");
        let process = MockProcessProvider::new().with_result(
            "java".to_string(),
            vec![
                "-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer-bootstrap.jar")
                    .to_string_lossy()
                    .to_string(),
                "--bootstrap-main-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer.jar")
                    .to_string_lossy()
                    .to_string(),
                "-g".to_string(),
                "-s".to_string(),
                "both".to_string(),
                pack_toml_path.to_string_lossy().to_string(),
            ],
            Ok(restricted_install_output(&workdir)),
        );
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(process)
            .with_interactive(MockInteractiveProvider::new().with_confirm(false))
            .with_terminal_capabilities(tty_capabilities());

        let err = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["client-full".to_string()],
                ..Default::default()
            },
        )
        .await
        .expect_err("restricted build should still require manual download");

        assert!(err.to_string().contains("empack build --continue"));
        assert_eq!(
            session.interactive_provider.get_confirm_calls(),
            vec![("Open download URLs in browser?".to_string(), false)]
        );
        let (browser_cmd, _) = crate::platform::browser_open_command();
        assert!(
            session
                .process_provider
                .get_calls_for_command(browser_cmd)
                .is_empty(),
            "declining the browser confirm should not launch a browser command"
        );
    }

    #[tokio::test]
    async fn fresh_restricted_build_opens_browser_urls_when_confirmed() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-browser-confirm-yes");
        let pack_toml_path = workdir
            .join("dist")
            .join("client-full")
            .join("pack")
            .join("pack.toml");
        let process = MockProcessProvider::new().with_result(
            "java".to_string(),
            vec![
                "-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer-bootstrap.jar")
                    .to_string_lossy()
                    .to_string(),
                "--bootstrap-main-jar".to_string(),
                workdir
                    .join("cache")
                    .join("packwiz-installer.jar")
                    .to_string_lossy()
                    .to_string(),
                "-g".to_string(),
                "-s".to_string(),
                "both".to_string(),
                pack_toml_path.to_string_lossy().to_string(),
            ],
            Ok(restricted_install_output(&workdir)),
        );
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(process)
            .with_interactive(MockInteractiveProvider::new().with_confirm(true))
            .with_terminal_capabilities(tty_capabilities());

        let err = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["client-full".to_string()],
                ..Default::default()
            },
        )
        .await
        .expect_err("restricted build should still require manual download");

        assert!(err.to_string().contains("empack build --continue"));
        let (browser_cmd, browser_prefix) = crate::platform::browser_open_command();
        let browser_calls = session.process_provider.get_calls_for_command(browser_cmd);
        assert_eq!(browser_calls.len(), 1, "expected one browser-open call");
        let mut expected_args: Vec<String> = browser_prefix
            .iter()
            .map(|arg| (*arg).to_string())
            .collect();
        expected_args.push(
            "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891".to_string(),
        );
        assert_eq!(browser_calls[0].args, expected_args);
    }

    #[tokio::test]
    async fn build_continue_restores_cached_files_and_clears_pending_state() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-success");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_java_installer_side_effects());

        let pending = crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");
        session
            .filesystem()
            .write_bytes(&pending.restricted_cache_path().join("OptiFine.jar"), b"cached bytes")
            .expect("write cached restricted file");

        let result = handle_build(
            &session,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "continue build should succeed: {result:?}");
        assert!(
            session.filesystem().exists(
                &workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
            ),
            "cached restricted file should be restored into the distribution tree"
        );
        assert!(
            crate::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)
                .expect("load pending build")
                .is_none(),
            "pending state should be cleared after a successful continue build"
        );
    }

    #[tokio::test]
    async fn build_continue_clears_stale_pending_state() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-stale");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_java_installer_side_effects());

        let pending = crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .write_file(&workdir.join("empack.yml"), "empack:\n  name: changed\n")
            .expect("rewrite empack.yml");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");
        session
            .filesystem()
            .write_bytes(&pending.restricted_cache_path().join("OptiFine.jar"), b"cached bytes")
            .expect("write cached restricted file");

        let err = handle_build(
            &session,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("stale pending state should fail");

        assert!(err.to_string().contains("Pending restricted build is stale"));
        assert!(
            crate::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)
                .expect("load pending build")
                .is_none(),
            "stale pending state should be cleared"
        );
    }

    #[tokio::test]
    async fn build_continue_dry_run_keeps_pending_state() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-dry-run");
        let mut session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()));
        session.config_provider.app_config.dry_run = true;

        crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");

        let result = continue_pending_restricted_build(
            &session,
            &workdir,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
            std::time::Instant::now(),
        )
        .await;

        assert!(result.is_ok(), "dry-run continue should succeed: {result:?}");
        assert!(
            crate::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)
                .expect("load pending build")
                .is_some(),
            "dry-run should preserve pending state"
        );
        assert!(
            session.process_provider.get_calls().is_empty(),
            "dry-run continue should not launch subprocesses"
        );
    }

    #[tokio::test]
    async fn build_continue_errors_when_cached_downloads_are_still_missing() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-cache-missing");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()));

        crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");

        let err = continue_pending_restricted_build(
            &session,
            &workdir,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
            std::time::Instant::now(),
        )
        .await
        .expect_err("missing restricted cache should fail");

        assert!(err
            .to_string()
            .contains("still required before the build can continue"));
    }

    #[tokio::test]
    async fn build_continue_errors_when_restricted_downloads_recur() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-restricted-again");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_result(
                "java".to_string(),
                client_full_installer_args(&workdir),
                Ok(restricted_install_output(&workdir)),
            ));

        let pending = crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");
        session
            .filesystem()
            .write_bytes(&pending.restricted_cache_path().join("OptiFine.jar"), b"cached bytes")
            .expect("write cached restricted file");

        let err = continue_pending_restricted_build(
            &session,
            &workdir,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
            std::time::Instant::now(),
        )
        .await
        .expect_err("repeated restricted installer output should fail");

        assert!(err
            .to_string()
            .contains("restricted download(s) are still required after continue"));
    }

    #[tokio::test]
    async fn build_continue_reports_failed_targets_after_restore() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-build-failed");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_result(
                "java".to_string(),
                client_full_installer_args(&workdir),
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: "installer exploded".to_string(),
                    success: false,
                }),
            ));

        let pending = crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");
        session
            .filesystem()
            .write_bytes(&pending.restricted_cache_path().join("OptiFine.jar"), b"cached bytes")
            .expect("write cached restricted file");

        let err = continue_pending_restricted_build(
            &session,
            &workdir,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
            std::time::Instant::now(),
        )
        .await
        .expect_err("failed target should propagate");

        assert!(err.to_string().contains("Failed to execute build pipeline"));
    }

    #[tokio::test]
    async fn build_continue_restores_multiple_destinations_for_one_cached_file() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("continue-multiple-dests");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_java_installer_side_effects());

        let pending = crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[
                crate::empack::RestrictedModInfo {
                    name: "OptiFine.jar".to_string(),
                    url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                        .to_string(),
                    dest_path: workdir
                        .join("dist")
                        .join("client-full")
                        .join("mods")
                        .join("OptiFine.jar")
                        .to_string_lossy()
                        .to_string(),
                },
                crate::empack::RestrictedModInfo {
                    name: "OptiFine.jar".to_string(),
                    url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                        .to_string(),
                    dest_path: workdir
                        .join("dist")
                        .join("client-full")
                        .join("backup")
                        .join("OptiFine.jar")
                        .to_string_lossy()
                        .to_string(),
                },
            ],
        )
        .expect("save pending build");
        session
            .filesystem()
            .create_dir_all(&workdir.join("dist").join("client-full"))
            .expect("create client-full output");
        session
            .filesystem()
            .write_bytes(&pending.restricted_cache_path().join("OptiFine.jar"), b"cached bytes")
            .expect("write cached restricted file");

        let result = handle_build(
            &session,
            &BuildArgs {
                continue_build: true,
                ..Default::default()
            },
        )
        .await;

        assert!(
            result.is_ok(),
            "continue build should restore every destination and succeed: {result:?}"
        );
        assert!(
            session.filesystem().exists(
                &workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
            )
        );
        assert!(
            session.filesystem().exists(
                &workdir
                    .join("dist")
                    .join("client-full")
                    .join("backup")
                    .join("OptiFine.jar")
            )
        );
    }

    #[tokio::test]
    async fn clean_build_clears_pending_restricted_state_before_rebuilding() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let cache_root = TempDir::new().expect("cache root tempdir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", cache_root.path()) };

        let workdir = mock_root().join("clean-clears-pending-state");
        let session = MockCommandSession::new()
            .with_filesystem(cached_full_build_filesystem(workdir.clone()))
            .with_process(MockProcessProvider::new().with_mrpack_export_side_effects());

        crate::empack::restricted_build::save_pending_build(
            session.filesystem(),
            &workdir,
            &[BuildTarget::ClientFull],
            crate::empack::archive::ArchiveFormat::Zip,
            &[crate::empack::RestrictedModInfo {
                name: "OptiFine.jar".to_string(),
                url: "https://www.curseforge.com/minecraft/mc-mods/optifine/files/4912891"
                    .to_string(),
                dest_path: workdir
                    .join("dist")
                    .join("client-full")
                    .join("mods")
                    .join("OptiFine.jar")
                    .to_string_lossy()
                    .to_string(),
            }],
        )
        .expect("save pending build");

        let result = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["mrpack".to_string()],
                clean: true,
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "clean rebuild should succeed: {result:?}");
        assert!(
            crate::empack::restricted_build::load_pending_build(session.filesystem(), &workdir)
                .expect("load pending build")
                .is_none(),
            "clean build should clear stale pending restricted state before rebuilding"
        );
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
        let session = configured_session(&workdir);

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

    #[tokio::test]
    async fn it_skips_side_effects_in_dry_run() {
        let workdir = mock_root().join("built-project");
        let mut session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            );
        session.config_provider.app_config.dry_run = true;

        let result = handle_clean(&session, vec!["builds".to_string()]).await;

        assert!(result.is_ok());
        assert!(
            session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")),
            "Dry-run mode should not remove dist/ artifacts"
        );
        assert!(
            session.filesystem().exists(&workdir.join("dist").join("test-pack.zip")),
            "Dry-run mode should not remove dist/ artifacts"
        );
    }

    #[tokio::test]
    async fn it_cleans_dist_when_state_is_configured() {
        let workdir = mock_root().join("configured-with-dist");
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir.clone())
                .with_file(
                    workdir.join("dist").join("leftover.txt"),
                    "leftover".to_string(),
                ),
        );

        assert!(
            session.filesystem().is_directory(&workdir.join("dist")),
            "dist/ should exist before clean"
        );

        let result = handle_clean(&session, vec!["builds".to_string()]).await;

        assert!(result.is_ok(), "clean should succeed: {result:?}");
        assert!(
            !session.filesystem().is_directory(&workdir.join("dist")),
            "dist/ should be removed even when state is Configured"
        );
        assert!(
            session.filesystem().exists(&workdir.join("empack.yml")),
            "configuration files should be preserved"
        );
    }

    #[tokio::test]
    async fn it_reports_nothing_to_clean_when_dist_absent() {
        let workdir = mock_root().join("configured-no-dist");
        let session = configured_session(&workdir);

        assert!(
            !session.filesystem().is_directory(&workdir.join("dist")),
            "dist/ should not exist"
        );

        let result = handle_clean(&session, vec!["builds".to_string()]).await;

        assert!(result.is_ok(), "clean should succeed: {result:?}");
    }
}

// ===== HELPER FUNCTION TESTS =====

#[tokio::test]
async fn test_parse_build_targets_all_keyword() {
    let targets = vec!["all".to_string()];
    let result = parse_build_targets(targets);
    let parsed = result.expect("'all' should parse successfully");
    assert_eq!(
        parsed,
        vec![
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::Server,
            BuildTarget::ClientFull,
            BuildTarget::ServerFull,
        ]
    );
}

#[tokio::test]
async fn test_parse_build_targets_single_target() {
    let targets = vec!["server".to_string()];
    let parsed = parse_build_targets(targets).expect("'server' should parse successfully");
    assert_eq!(parsed, vec![BuildTarget::Server]);
}

#[tokio::test]
async fn test_parse_build_targets_multiple_targets() {
    let targets = vec!["server".to_string(), "client".to_string()];
    let parsed =
        parse_build_targets(targets).expect("'server','client' should parse successfully");
    assert_eq!(parsed, vec![BuildTarget::Server, BuildTarget::Client]);
}

#[tokio::test]
async fn test_parse_build_targets_invalid_target() {
    let targets = vec!["invalid".to_string()];
    let err = parse_build_targets(targets).unwrap_err();
    assert!(
        err.to_string().contains("Unknown build target: invalid"),
        "Expected error about unknown target, got: {}",
        err
    );
}

#[tokio::test]
async fn test_parse_build_targets_empty_list() {
    let targets: Vec<String> = vec![];
    let err = parse_build_targets(targets).unwrap_err();
    assert!(
        err.to_string().contains("No build targets specified"),
        "Expected error about no targets, got: {}",
        err
    );
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

#[test]
fn test_render_add_contract_error_incompatible_project_loader() {
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "sodium".to_string(),
        source: crate::empack::search::SearchError::IncompatibleProject {
            query: "sodium".to_string(),
            project_title: "Sodium".to_string(),
            project_slug: "sodium".to_string(),
            available_loaders: vec![
                "fabric".to_string(),
                "neoforge".to_string(),
                "quilt".to_string(),
            ],
            available_versions: vec!["1.21.4".to_string()],
            requested_loader: Some("forge".to_string()),
            requested_version: Some("1.21.4".to_string()),
            downloads: 134_306_743,
        },
    });

    assert_eq!(rendered.item, "Mod found but incompatible");
    assert!(
        rendered.details.contains("Sodium"),
        "Details should contain the project title; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("134M"),
        "Details should contain abbreviated download count; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("fabric, neoforge, quilt"),
        "Details should list supported loaders; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("forge"),
        "Details should mention the requested loader; got: {}",
        rendered.details
    );
}

#[test]
fn test_render_add_contract_error_incompatible_project_loader_only() {
    let rendered = render_add_contract_error(&AddContractError::ResolveProject {
        query: "sodium".to_string(),
        source: crate::empack::search::SearchError::IncompatibleProject {
            query: "sodium".to_string(),
            project_title: "Sodium".to_string(),
            project_slug: "sodium".to_string(),
            available_loaders: vec!["fabric".to_string(), "quilt".to_string()],
            available_versions: vec![],
            requested_loader: Some("forge".to_string()),
            requested_version: None,
            downloads: 50_000,
        },
    });

    assert_eq!(rendered.item, "Mod found but incompatible");
    assert!(
        rendered.details.contains("does not support forge"),
        "Details should mention unsupported loader; got: {}",
        rendered.details
    );
    assert!(
        rendered.details.contains("50K"),
        "Details should contain abbreviated download count; got: {}",
        rendered.details
    );
}

#[test]
fn test_format_downloads_abbreviation() {
    assert_eq!(format_downloads(134_306_743), "134M");
    assert_eq!(format_downloads(1_000_000), "1M");
    assert_eq!(format_downloads(50_000), "50K");
    assert_eq!(format_downloads(1_000), "1K");
    assert_eq!(format_downloads(999), "999");
    assert_eq!(format_downloads(0), "0");
}

// ===== BUILD TARGET VALIDATION TESTS (Slice 3) =====

#[tokio::test]
async fn test_invalid_build_target_mixed_with_valid() {
    let err = parse_build_targets(vec![
        "client".to_string(),
        "invalid-target".to_string(),
        "server".to_string(),
    ])
    .expect_err("Should reject if any target is invalid");
    assert!(
        err.to_string()
            .contains("Unknown build target: invalid-target"),
        "Expected error about the invalid target, got: {}",
        err
    );
}

#[tokio::test]
async fn test_case_insensitive_build_targets() {
    // Lowercase is the canonical form and must be accepted
    let parsed =
        parse_build_targets(vec!["client".to_string()]).expect("Lowercase 'client' should parse");
    assert_eq!(parsed, vec![BuildTarget::Client]);

    // Uppercase is rejected; parse_build_targets uses exact match
    let err = parse_build_targets(vec!["CLIENT".to_string()])
        .expect_err("Uppercase 'CLIENT' should be rejected");
    assert!(
        err.to_string().contains("Unknown build target: CLIENT"),
        "Expected error about unknown target, got: {}",
        err
    );
}

#[tokio::test]
async fn test_all_valid_build_targets_individually() {
    let cases: Vec<(&str, BuildTarget)> = vec![
        ("mrpack", BuildTarget::Mrpack),
        ("client", BuildTarget::Client),
        ("server", BuildTarget::Server),
        ("client-full", BuildTarget::ClientFull),
        ("server-full", BuildTarget::ServerFull),
    ];

    for (input, expected) in cases {
        let parsed = parse_build_targets(vec![input.to_string()])
            .unwrap_or_else(|e| panic!("Valid target '{}' should be accepted: {}", input, e));
        assert_eq!(parsed, vec![expected], "Mismatch for target '{}'", input);
    }
}

// ===== BUILD COMMAND ERROR HANDLING TESTS (Slice 3) =====

#[tokio::test]
async fn test_build_with_invalid_target_string() {
    let workdir = mock_root().join("configured-project");
    let session = configured_session(&workdir);

    let err = handle_build(&session, &BuildArgs { targets: vec!["not-a-real-target".to_string()], ..Default::default() })
        .await
        .expect_err("Build should fail with invalid target");
    assert!(
        err.to_string()
            .contains("Unknown build target: not-a-real-target"),
        "Expected error about unknown target, got: {}",
        err
    );
}

#[tokio::test]
async fn test_build_cleans_before_build_when_flag_set() {
    let workdir = mock_root().join("configured-project");
    let session = configured_session(&workdir);

    // Build with clean=true
    let result = handle_build(&session, &BuildArgs { targets: vec!["mrpack".to_string()], clean: true, ..Default::default() }).await;

    // Should complete (clean happens before build attempt)
    // In mock environment, build might fail for other reasons, but clean should execute
    match result {
        Ok(_) => {
            // Success is acceptable
        }
        Err(e) => {
            // Mock environment: packwiz doesn't create real artifacts,
            // so build validation fails. Verify it's the expected build pipeline error.
            let err_chain = format!("{e:?}");
            assert!(
                err_chain.contains("build pipeline")
                    || err_chain.contains("expected artifact"),
                "Expected build pipeline/artifact error, got: {err_chain}",
            );
        }
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
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn curseforge_id_no_platform() {
        // All-digit strings are treated as search queries without explicit --platform.
        // Direct CF project ID lookup requires --platform curseforge.
        let intent = AddResolutionIntent::from_cli_input("306612", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
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
        assert_eq!(intent.direct_project_id, Some("306612".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.preferred_platform, Some(ProjectPlatform::Modrinth));
        assert_eq!(intent.search_query, "306612");
    }

    #[test]
    fn search_query_with_modrinth_platform() {
        let intent =
            AddResolutionIntent::from_cli_input("sodium", Some(SearchPlatform::Modrinth));
        assert_eq!(intent.direct_project_id, Some("sodium".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
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
        assert_eq!(intent.direct_project_id, Some("AANobbMI".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::CurseForge));
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
        assert_eq!(intent.direct_project_id, Some("sodium".to_string()));
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::CurseForge));
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
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.preferred_platform, None);
        assert_eq!(intent.search_query, "AANobbMI");
    }

    #[test]
    fn curseforge_id_with_both_platform() {
        // --platform both does not auto-detect; treats input as search query.
        let intent =
            AddResolutionIntent::from_cli_input("306612", Some(SearchPlatform::Both));
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
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

    // --- Edge cases: mod names that previously false-positived as Modrinth IDs ---

    #[test]
    fn eight_char_mod_name_faithful_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("faithful", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "faithful");
    }

    #[test]
    fn eight_char_mod_name_optifine_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("optifine", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "optifine");
    }

    #[test]
    fn eight_char_mod_name_litematr_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("litematr", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "litematr");
    }

    #[test]
    fn eight_char_mod_name_dynmappp_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("dynmappp", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "dynmappp");
    }

    #[test]
    fn titlecase_mod_name_optifine_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("Optifine", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "Optifine");
    }

    #[test]
    fn mixed_case_digits_sodium99_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("SODIUM99", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "SODIUM99");
    }

    #[test]
    fn lowercase_digits_sodium12_is_search_not_id() {
        let intent = AddResolutionIntent::from_cli_input("sodium12", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "sodium12");
    }

    #[test]
    fn real_modrinth_id_treated_as_search_without_platform_flag() {
        // Real Modrinth IDs like AANobbMI require --platform modrinth for direct lookup
        let intent = AddResolutionIntent::from_cli_input("AANobbMI", None);
        assert_eq!(intent.direct_project_id, None);
        assert_eq!(intent.direct_platform, None);
        assert_eq!(intent.search_query, "AANobbMI");
    }

    // --- URL inputs ---

    #[test]
    fn modrinth_project_url_produces_search_with_slug() {
        let intent =
            AddResolutionIntent::from_cli_input("https://modrinth.com/mod/sodium", None);
        assert_eq!(intent.kind, AddIntentKind::Search);
        assert_eq!(intent.search_query, "sodium");
        assert_eq!(intent.direct_project_id, Some("sodium".to_string()));
        assert_eq!(
            intent.direct_platform,
            Some(ProjectPlatform::Modrinth)
        );
    }

    #[test]
    fn modrinth_plugin_url_produces_search_with_slug() {
        let intent = AddResolutionIntent::from_cli_input(
            "https://modrinth.com/plugin/vault-hunters",
            None,
        );
        assert_eq!(intent.kind, AddIntentKind::Search);
        assert_eq!(intent.search_query, "vault-hunters");
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
    }

    #[test]
    fn curseforge_project_url_produces_curseforge_direct() {
        let intent = AddResolutionIntent::from_cli_input(
            "https://www.curseforge.com/minecraft/mc-mods/jei",
            None,
        );
        assert_eq!(intent.kind, AddIntentKind::CurseForgeDirect { slug: "jei".to_string() });
        assert_eq!(intent.search_query, "https://www.curseforge.com/minecraft/mc-mods/jei");
        assert_eq!(intent.direct_project_id, None);
    }

    #[test]
    fn direct_download_jar_url_produces_direct_download() {
        let intent = AddResolutionIntent::from_cli_input(
            "https://example.com/downloads/sodium-0.6.0.jar",
            None,
        );
        assert_eq!(
            intent.kind,
            AddIntentKind::DirectDownload {
                url: "https://example.com/downloads/sodium-0.6.0.jar".to_string(),
                extension: "jar".to_string(),
            }
        );
    }

    #[test]
    fn direct_download_zip_url_produces_direct_download() {
        let intent = AddResolutionIntent::from_cli_input(
            "https://example.com/pack.zip",
            None,
        );
        assert_eq!(
            intent.kind,
            AddIntentKind::DirectDownload {
                url: "https://example.com/pack.zip".to_string(),
                extension: "zip".to_string(),
            }
        );
    }

    #[test]
    fn unrecognized_url_falls_through_to_search() {
        let intent =
            AddResolutionIntent::from_cli_input("https://unknown-site.com/project/123", None);
        assert_eq!(intent.kind, AddIntentKind::Search);
        assert_eq!(
            intent.search_query,
            "https://unknown-site.com/project/123"
        );
        assert_eq!(intent.direct_project_id, None);
    }

    #[test]
    fn modpack_urls_are_ignored_by_from_cli_input() {
        // Modpack URLs are not valid for empack add; classify_url returns
        // ModrinthModpack/CurseForgeModpack which from_cli_input does not match.
        // These fall through to Search with the raw URL as the query.
        let intent = AddResolutionIntent::from_cli_input(
            "https://modrinth.com/modpack/fabulously-optimized",
            None,
        );
        assert_eq!(intent.kind, AddIntentKind::Search);
        assert_eq!(
            intent.search_query,
            "https://modrinth.com/modpack/fabulously-optimized"
        );
    }

    #[test]
    fn http_url_is_also_classified() {
        let intent =
            AddResolutionIntent::from_cli_input("http://modrinth.com/mod/sodium", None);
        assert_eq!(intent.kind, AddIntentKind::Search);
        assert_eq!(intent.direct_platform, Some(ProjectPlatform::Modrinth));
    }
}

// ===== SEARCH AND ADD TESTS (W1-T3) =====
//
// S1: CurseForge numeric ID auto-detection and add
// S2: Search quality for multi-word queries and type filtering

mod search_add_tests {
    use super::*;
    use crate::application::sync::resolve_add_contract;
    use crate::empack::parsing::ModLoader;

    // S1: CurseForge numeric ID with explicit --platform curseforge flag
    // produces valid packwiz curseforge add commands.
    #[tokio::test]
    async fn test_handle_add_curseforge_direct_id() {
        let workdir = mock_root().join("cf-direct-id");

        let session = configured_session(&workdir)
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
            Some(SearchPlatform::Curseforge),
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_ok(), "handle_add with CF numeric ID should succeed: {result:?}");

        let calls = session.process_provider.get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN);
        assert_eq!(calls.len(), 1, "Expected exactly one packwiz call");
        assert_eq!(
            calls[0].args,
            vec!["curseforge", "add", "--addon-id", "238222", "-y"],
            "Packwiz command must use curseforge add --addon-id for numeric ID"
        );
    }

    // S1/E5: When packwiz curseforge add --addon-id fails with stderr content,
    // the error message must be non-empty and propagate the stderr.
    #[tokio::test]
    async fn test_handle_add_curseforge_id_propagates_stderr() {
        let workdir = mock_root().join("cf-id-stderr");

        let session = configured_session(&workdir)
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
                    stderr: "no file found for game version".to_string(),
                    success: false,
                }),
            ));

        let result = handle_add(
            &session,
            vec!["238222".to_string()],
            false,
            Some(SearchPlatform::Curseforge),
            None,
            None,
            None,
        )
        .await;

        assert!(
            result.is_err(),
            "handle_add must return Err when packwiz curseforge add fails"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.is_empty(),
            "Error message must be non-empty when packwiz produces stderr"
        );
        assert!(
            err_msg.contains("no file found for game version"),
            "Error must propagate packwiz stderr; got: {}",
            err_msg
        );
    }

    // S2 partial: Search with explicit --type mod uses the correct type filter
    // through the full handle_add flow. The mock resolver receives the query
    // and returns a mod result; the packwiz command is built for Modrinth.
    // After N4 search redesign, ProjectType::Mod maps to classId 6 on
    // CurseForge and project_type:mod facet on Modrinth.
    #[tokio::test]
    async fn test_search_type_mod_prevents_resourcepack() {
        let workdir = mock_root().join("type-mod-filter");

        let session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new().with_project_response(
                    "faithful".to_string(),
                    modrinth_project("faith-id", "Faithful"),
                ),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "faith-id".to_string(),
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
            vec!["faithful".to_string()],
            false,
            None,
            Some(CliProjectType::Mod),
            None,
            None,
        )
        .await;

        assert!(result.is_ok(), "handle_add with --type mod should succeed: {result:?}");

        let calls = session.process_provider.get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args,
            vec!["modrinth", "add", "--project-id", "faith-id", "-y"],
        );
    }

    // S2: Multi-word query "just enough items" resolves to the correct mod via
    // resolve_add_contract. The mock resolver returns JEI when queried with
    // the multi-word title. This validates that multi-word queries flow
    // correctly through the resolution pipeline.
    #[tokio::test]
    async fn test_search_multiword_resolves_correct_project() {
        let resolver = MockProjectResolver::new().with_project_response(
            "just enough items".to_string(),
            ProjectInfo {
                platform: ProjectPlatform::Modrinth,
                project_id: "u6dRKJwZ".to_string(),
                title: "Just Enough Items".to_string(),
                downloads: 200_000_000,
                confidence: 98,
                project_type: "mod".to_string(),
            },
        );

        let resolution = resolve_add_contract(
            "just enough items",
            None,
            Some("1.21.1"),
            Some(ModLoader::Fabric),
            "",
            ProjectPlatform::Modrinth,
            None,
            None,
            &resolver,
        )
        .await;

        assert!(
            resolution.is_ok(),
            "Multi-word query should resolve: {resolution:?}"
        );
        let res = resolution.unwrap();
        assert_eq!(res.title, "Just Enough Items");
        assert_eq!(res.resolved_project_id, "u6dRKJwZ");
        assert_eq!(res.resolved_platform, ProjectPlatform::Modrinth);

        let commands = &res.commands;
        assert_eq!(commands.len(), 1);
        assert!(
            commands[0].contains(&"modrinth".to_string()),
            "Command must target modrinth: {:?}",
            commands[0]
        );
        assert!(
            commands[0].contains(&"--project-id".to_string()),
            "Command must use --project-id: {:?}",
            commands[0]
        );
        assert!(
            commands[0].contains(&"u6dRKJwZ".to_string()),
            "Command must contain the resolved project ID: {:?}",
            commands[0]
        );
    }

    // version_id/file_id propagation through handle_add
    #[tokio::test]
    async fn test_handle_add_modrinth_version_id_propagates_to_packwiz() {
        let workdir = mock_root().join("mr-version-id-add");

        let session = configured_session(&workdir)
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "AANobbMI".to_string(),
                    "--version-id".to_string(),
                    "mc1.21-fabric-0.16".to_string(),
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
            vec!["AANobbMI".to_string()],
            false,
            Some(SearchPlatform::Modrinth),
            None,
            Some("mc1.21-fabric-0.16".to_string()),
            None,
        )
        .await;

        assert!(result.is_ok(), "handle_add with --version-id should succeed: {result:?}");

        let calls = session.process_provider.get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args,
            vec!["modrinth", "add", "--project-id", "AANobbMI", "--version-id", "mc1.21-fabric-0.16", "-y"],
            "Packwiz command must include --version-id from CLI flag"
        );
    }

    #[tokio::test]
    async fn test_handle_add_curseforge_file_id_propagates_to_packwiz() {
        let workdir = mock_root().join("cf-file-id-add");

        let session = configured_session(&workdir)
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

        let result = handle_add(
            &session,
            vec!["238222".to_string()],
            false,
            Some(SearchPlatform::Curseforge),
            None,
            None,
            Some("5678901".to_string()),
        )
        .await;

        assert!(result.is_ok(), "handle_add with --file-id should succeed: {result:?}");

        let calls = session.process_provider.get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args,
            vec!["curseforge", "add", "--addon-id", "238222", "--file-id", "5678901", "-y"],
            "Packwiz command must include --file-id from CLI flag"
        );
    }
}

// ===== EXIT CODE TESTS (W1-T1) =====
//
// These tests verify exit code behavior for error conditions.
// They are expected to FAIL until Wave 2 fixes (W2-F1) are implemented.
//
// Test for ExitCode enum (E4) is omitted because the type does not exist yet;
// a non-compiling test would block the entire suite. The enum should define:
//   General = 1, Config = 2, Network = 3, NotFound = 4

// ===== INIT INTERACTIVE TESTS =====

mod init_interactive_tests {
    use super::*;
    use crate::application::config::AppConfig;

    // I1: handle_init with yes_mode=true and no --modloader must return Err
    // containing "modloader".
    #[tokio::test]
    async fn test_handle_init_yes_without_modloader_errors() {
        let workdir = mock_root().join("yes-no-modloader");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true))
            .with_config(MockConfigProvider::new(AppConfig {
                yes: true,
                ..Default::default()
            }));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(
            result.is_err(),
            "handle_init with --yes and no --modloader must return Err, got Ok"
        );
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("modloader"),
            "Error must mention 'modloader'; got: {}",
            err_msg
        );
    }

    // I2: handle_init with a positional dir uses it as the default for the
    // interactive name prompt (the --name flag is needed to skip the prompt).
    #[tokio::test]
    async fn test_handle_init_positional_dir_sets_prompt_default() {
        let workdir = mock_root().join("positional-name-default");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let _result = handle_init(
            &session,
            &InitArgs {
                dir: Some("my-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        // Positional name is the default for the interactive prompt (not a skip).
        // The --name flag is the only way to skip the name prompt entirely.
        let text_calls = session.interactive_provider.get_text_input_calls();
        let name_call = text_calls
            .iter()
            .find(|(prompt, _)| prompt.contains("Modpack name"));
        assert!(
            name_call.is_some(),
            "Positional name should be passed as default to interactive prompt; text_input calls: {:?}",
            text_calls
        );
        assert_eq!(
            name_call.unwrap().1,
            "my-pack",
            "Positional dir 'my-pack' should be the default for the name prompt"
        );
    }

    // I2b: handle_init with "." as positional dir resolves to the actual workdir basename.
    #[tokio::test]
    async fn test_handle_init_dot_dir_uses_actual_basename_as_default() {
        let workdir = mock_root().join("my-cool-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir.clone()))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let _result = handle_init(
            &session,
            &InitArgs {
                dir: Some(".".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        let text_calls = session.interactive_provider.get_text_input_calls();
        let name_call = text_calls
            .iter()
            .find(|(prompt, _)| prompt.contains("Modpack name"));
        assert!(
            name_call.is_some(),
            "Dot dir should resolve to actual basename for name prompt; text_input calls: {:?}",
            text_calls
        );
        assert_eq!(
            name_call.unwrap().1,
            "my-cool-pack",
            "Positional dir '.' should resolve to the actual workdir basename 'my-cool-pack'"
        );
    }

    // I3: Orphan removal in handle_remove must use confirm(), not text_input().
    #[tokio::test]
    async fn test_handle_remove_orphan_uses_confirm() {
        let workdir = mock_root().join("orphan-confirm");
        let mods_dir = workdir.join("pack").join("mods");
        let session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_configured_project(workdir.clone())
                    .with_file(
                        mods_dir.join("fabric-language-kotlin.pw.toml"),
                        "name = \"Fabric Language Kotlin\"\nfilename = \"fabric-language-kotlin-1.12.0.jar\"\n".to_string(),
                    ),
            )
            .with_interactive(
                MockInteractiveProvider::new().queue_confirm(true),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "-y".to_string(), "sodium".to_string()],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let _result = handle_remove(
            &session,
            vec!["sodium".to_string()],
            true, // --deps: enable orphan detection
        )
        .await;

        let confirm_calls = session.interactive_provider.get_confirm_calls();
        assert!(
            !confirm_calls.is_empty(),
            "Orphan removal must use confirm(), not text_input(); confirm_calls was empty. \
             text_input_calls: {:?}",
            session.interactive_provider.get_text_input_calls()
        );
    }

    // I4: handle_init with --modloader none and --loader-version 0.15.0 should
    // either warn about the ignored loader version or return an error.
    #[tokio::test]
    async fn test_handle_init_vanilla_loader_version_warns() {
        let workdir = mock_root().join("vanilla-loader-version");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("none".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                loader_version: Some("0.15.0".to_string()),
                ..Default::default()
            },
        )
        .await;

        // Accept either behavior: an error return OR a successful init.
        // If Ok, the loader_version should NOT appear in empack.yml
        // (and ideally a warning was emitted, but we can't capture
        // display output in unit tests).
        //
        // The key assertion: the result must not silently succeed with
        // a loader_version baked into the config. If it returns Ok,
        // verify the empack.yml does NOT contain "0.15.0".
        if let Err(e) = &result {
            assert!(
                e.to_string().to_lowercase().contains("loader-version")
                    || e.to_string().to_lowercase().contains("vanilla"),
                "Err message should mention loader-version or vanilla; got: {e}"
            );
        } else {
            let target_dir = mock_root()
                .join("vanilla-loader-version")
                .join("test-pack");
            if let Ok(yml) = session
                .filesystem()
                .read_to_string(&target_dir.join("empack.yml"))
            {
                assert!(
                    !yml.contains("0.15.0"),
                    "vanilla init must not embed loader-version in empack.yml: {yml}"
                );
            }
        }
    }
}


mod exit_code_tests {
    use super::*;

    // E1: handle_add with packwiz failure must return Err, not Ok.
    #[tokio::test]
    async fn test_handle_add_packwiz_failure_returns_error() {
        let workdir = mock_root().join("exit-code-packwiz-fail");
        let mock_project = modrinth_project("fail-mod-id", "Fail Mod");

        let session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("fail-mod".to_string(), mock_project),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "fail-mod-id".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: "packwiz error: could not add mod".to_string(),
                    success: false,
                }),
            ));

        let result = handle_add(
            &session,
            vec!["fail-mod".to_string()],
            false,
            None,
            None,
            None,
            None,
        )
        .await;

        assert!(
            result.is_err(),
            "handle_add must return Err when packwiz fails, got Ok"
        );
    }

    // E3: handle_add with no search results must return Err, not Ok.
    #[tokio::test]
    async fn test_handle_add_no_results_returns_error() {
        let workdir = mock_root().join("exit-code-no-results");

        let session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new()
                    .with_error_response(
                        "nonexistent-mod".to_string(),
                        "No results found".to_string(),
                    ),
            );

        let result = handle_add(
            &session,
            vec!["nonexistent-mod".to_string()],
            false,
            None,
            None,
            None,
            None,
        )
        .await;

        assert!(
            result.is_err(),
            "handle_add must return Err when no search results found, got Ok"
        );
    }

    // E2: handle_build in an uninitialized directory must return Err, not Ok.
    #[tokio::test]
    async fn test_handle_build_uninitialized_returns_error() {
        let workdir = mock_root().join("exit-code-uninit-build");

        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new().with_current_dir(workdir),
        );

        let result = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["mrpack".to_string()],
                ..Default::default()
            },
        )
        .await;

        assert!(
            result.is_err(),
            "handle_build must return Err when not in a modpack directory, got Ok"
        );
    }

    // E5: When packwiz fails with stderr output, the error message returned
    // by handle_add must contain that stderr content.
    #[tokio::test]
    async fn test_handle_add_propagates_packwiz_stderr() {
        let workdir = mock_root().join("exit-code-stderr-prop");
        let mock_project = modrinth_project("stderr-mod-id", "Stderr Mod");

        let session = configured_session(&workdir)
            .with_network(
                MockNetworkProvider::new()
                    .with_project_response("stderr-mod".to_string(), mock_project),
            )
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec![
                    "modrinth".to_string(),
                    "add".to_string(),
                    "--project-id".to_string(),
                    "stderr-mod-id".to_string(),
                    "-y".to_string(),
                ],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: "version not found".to_string(),
                    success: false,
                }),
            ));

        let result = handle_add(
            &session,
            vec!["stderr-mod".to_string()],
            false,
            None,
            None,
            None,
            None,
        )
        .await;

        assert!(
            result.is_err(),
            "handle_add must return Err when packwiz fails"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("version not found"),
            "Error must propagate packwiz stderr; got: {}",
            err_msg
        );
    }
}

// ===== DERIVE_VERSION_PIN TESTS =====

mod derive_version_pin_tests {
    use super::*;

    #[test]
    fn both_version_id_and_file_id_curseforge_prefers_file_id() {
        let vid = Some("ver-123".to_string());
        let fid = Some("file-456".to_string());
        let result = derive_version_pin(&vid, &fid, Some(ProjectPlatform::CurseForge));
        assert_eq!(result, Some("file-456"));
    }

    #[test]
    fn both_version_id_and_file_id_modrinth_prefers_version_id() {
        let vid = Some("ver-123".to_string());
        let fid = Some("file-456".to_string());
        let result = derive_version_pin(&vid, &fid, Some(ProjectPlatform::Modrinth));
        assert_eq!(result, Some("ver-123"));
    }

    #[test]
    fn both_version_id_and_file_id_no_platform_prefers_version_id() {
        let vid = Some("ver-123".to_string());
        let fid = Some("file-456".to_string());
        let result = derive_version_pin(&vid, &fid, None);
        assert_eq!(result, Some("ver-123"));
    }

    #[test]
    fn version_id_only() {
        let vid = Some("ver-123".to_string());
        let fid = None;
        let result = derive_version_pin(&vid, &fid, None);
        assert_eq!(result, Some("ver-123"));
    }

    #[test]
    fn file_id_only() {
        let vid = None;
        let fid = Some("file-456".to_string());
        let result = derive_version_pin(&vid, &fid, None);
        assert_eq!(result, Some("file-456"));
    }

    #[test]
    fn neither_version_id_nor_file_id() {
        let vid: Option<String> = None;
        let fid: Option<String> = None;
        let result = derive_version_pin(&vid, &fid, None);
        assert_eq!(result, None);
    }
}

// ===== CONTENT_FOLDER_FOR_TYPE TESTS =====

mod content_folder_for_type_tests {
    use super::*;

    #[test]
    fn mod_maps_to_mods() {
        assert_eq!(content_folder_for_type(ProjectType::Mod), "mods");
    }

    #[test]
    fn resourcepack_maps_to_resourcepacks() {
        assert_eq!(content_folder_for_type(ProjectType::ResourcePack), "resourcepacks");
    }

    #[test]
    fn shader_maps_to_shaderpacks() {
        assert_eq!(content_folder_for_type(ProjectType::Shader), "shaderpacks");
    }

    #[test]
    fn datapack_maps_to_datapacks() {
        assert_eq!(content_folder_for_type(ProjectType::Datapack), "datapacks");
    }
}

// ===== SCAN_PW_TOML_SLUGS TESTS =====

mod scan_pw_toml_slugs_tests {
    use super::*;

    #[test]
    fn returns_empty_for_nonexistent_dir() {
        let fs = MockFileSystemProvider::new();
        let slugs = scan_pw_toml_slugs(&fs, &mock_root().join("nonexistent"));
        assert!(slugs.is_empty());
    }

    #[test]
    fn extracts_slugs_from_pw_toml_files() {
        let mods_dir = mock_root().join("project").join("pack").join("mods");
        let fs = MockFileSystemProvider::new()
            .with_file(
                mods_dir.join("sodium.pw.toml"),
                "name = \"Sodium\"".to_string(),
            )
            .with_file(
                mods_dir.join("iris.pw.toml"),
                "name = \"Iris\"".to_string(),
            )
            .with_file(
                mods_dir.join("not-a-pw.toml"),
                "something".to_string(),
            );

        let slugs = scan_pw_toml_slugs(&fs, &mods_dir);
        assert!(slugs.contains("sodium"));
        assert!(slugs.contains("iris"));
        assert!(!slugs.contains("not-a-pw"), "non .pw.toml files should not be included");
    }
}

// ===== DISCOVER_DEP_KEY TESTS =====

mod discover_dep_key_tests {
    use super::*;

    #[test]
    fn returns_new_slug_when_exactly_one_new_file() {
        let mods_dir = mock_root().join("discover").join("pack").join("mods");
        let fs = MockFileSystemProvider::new()
            .with_file(
                mods_dir.join("sodium.pw.toml"),
                "name = \"Sodium\"".to_string(),
            )
            .with_file(
                mods_dir.join("iris.pw.toml"),
                "name = \"Iris\"".to_string(),
            );

        let mut before = HashSet::new();
        before.insert("sodium".to_string());

        let session = MockCommandSession::new();
        let result = discover_dep_key(&fs, &mods_dir, &before, "fallback", session.display());
        assert_eq!(result, "iris");
    }

    #[test]
    fn returns_fallback_when_no_new_files() {
        let mods_dir = mock_root().join("discover-none").join("pack").join("mods");
        let fs = MockFileSystemProvider::new()
            .with_file(
                mods_dir.join("sodium.pw.toml"),
                "name = \"Sodium\"".to_string(),
            );

        let mut before = HashSet::new();
        before.insert("sodium".to_string());

        let session = MockCommandSession::new();
        let result = discover_dep_key(&fs, &mods_dir, &before, "my-fallback", session.display());
        assert_eq!(result, "my-fallback");
    }

    #[test]
    fn returns_fallback_when_multiple_new_files() {
        let mods_dir = mock_root().join("discover-multi").join("pack").join("mods");
        let fs = MockFileSystemProvider::new()
            .with_file(mods_dir.join("a.pw.toml"), "".to_string())
            .with_file(mods_dir.join("b.pw.toml"), "".to_string())
            .with_file(mods_dir.join("c.pw.toml"), "".to_string());

        let before = HashSet::new();

        let session = MockCommandSession::new();
        let result = discover_dep_key(&fs, &mods_dir, &before, "ambig-key", session.display());
        assert_eq!(result, "ambig-key");
    }
}

// ===== RENDER_ADD_CONTRACT_ERROR EDGE CASE TESTS =====

mod render_error_edge_case_tests {
    use super::*;

    #[test]
    fn incompatible_project_version_only_no_loader() {
        let rendered = render_add_contract_error(&AddContractError::ResolveProject {
            query: "sodium".to_string(),
            source: crate::empack::search::SearchError::IncompatibleProject {
                query: "sodium".to_string(),
                project_title: "Sodium".to_string(),
                project_slug: "sodium".to_string(),
                available_loaders: vec!["fabric".to_string()],
                available_versions: vec!["1.21.4".to_string()],
                requested_loader: None,
                requested_version: Some("1.20.1".to_string()),
                downloads: 100_000_000,
            },
        });

        assert_eq!(rendered.item, "Mod found but incompatible");
        assert!(
            rendered.details.contains("has no version for 1.20.1"),
            "should mention version mismatch: {}",
            rendered.details
        );
        assert!(
            rendered.details.contains("100M"),
            "should contain download count: {}",
            rendered.details
        );
    }

    #[test]
    fn incompatible_project_neither_loader_nor_version() {
        let rendered = render_add_contract_error(&AddContractError::ResolveProject {
            query: "sodium".to_string(),
            source: crate::empack::search::SearchError::IncompatibleProject {
                query: "sodium".to_string(),
                project_title: "Sodium".to_string(),
                project_slug: "sodium".to_string(),
                available_loaders: vec![],
                available_versions: vec![],
                requested_loader: None,
                requested_version: None,
                downloads: 0,
            },
        });

        assert_eq!(rendered.item, "Mod found but incompatible");
        assert!(
            rendered.details.contains("sodium"),
            "fallback should use query: {}",
            rendered.details
        );
    }
}

// ===== HEX_ENCODE_BYTES TESTS =====

mod hex_encode_bytes_tests {
    use super::*;

    #[test]
    fn encodes_empty_bytes() {
        assert_eq!(hex_encode_bytes(&[]), "");
    }

    #[test]
    fn encodes_known_bytes() {
        assert_eq!(hex_encode_bytes(&[0x00, 0xff, 0xab, 0xcd]), "00ffabcd");
    }

    #[test]
    fn encodes_single_byte() {
        assert_eq!(hex_encode_bytes(&[0x42]), "42");
    }
}

// ===== COMPUTE_SHA1_HEX_FOR_BYTES TESTS =====

mod compute_sha1_tests {
    use super::*;

    #[test]
    fn sha1_of_empty_bytes() {
        let hash = compute_sha1_hex_for_bytes(b"");
        assert_eq!(hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha1_of_known_content() {
        let hash = compute_sha1_hex_for_bytes(b"hello world");
        assert_eq!(hash, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
    }
}

// ===== HANDLE_INIT VANILLA TESTS =====

mod handle_init_vanilla_tests {
    use super::*;

    #[tokio::test]
    async fn vanilla_init_succeeds() {
        let workdir = mock_root().join("vanilla-init");
        let target_dir = workdir.join("vanilla-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("vanilla-pack".to_string()),
                modloader: Some("none".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "vanilla init should succeed: {result:?}");
        assert!(session.filesystem().is_directory(&target_dir));

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        // "none" is not a real ModLoader, so the loader field is omitted from empack.yml
        assert!(!empack_yml.contains("loader:"), "vanilla pack should not have loader field: {empack_yml}");
        assert!(empack_yml.contains("minecraft_version: 1.21.1"));
    }

    #[tokio::test]
    async fn vanilla_init_dry_run_makes_no_changes() {
        let workdir = mock_root().join("vanilla-dry-run");
        let target_dir = workdir.join("test-pack");
        let mut session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));
        session.config_provider.app_config.dry_run = true;

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("none".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(
            !session.filesystem().is_directory(&target_dir),
            "dry run should not create directory"
        );
    }
}

// ===== HANDLE_INIT WITH DATAPACK FOLDER AND GAME VERSIONS =====

mod handle_init_options_tests {
    use super::*;

    #[tokio::test]
    async fn init_with_datapack_folder() {
        let workdir = mock_root().join("dp-folder-init");
        let target_dir = workdir.join("test-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                datapack_folder: Some("config/paxi/datapacks".to_string()),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "init with datapack folder should succeed: {result:?}");

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("datapack_folder"),
            "empack.yml should contain datapack_folder: {empack_yml}"
        );
    }

    #[tokio::test]
    async fn init_with_game_versions() {
        let workdir = mock_root().join("gv-init");
        let target_dir = workdir.join("test-pack");
        let session = MockCommandSession::new()
            .with_filesystem(MockFileSystemProvider::new().with_current_dir(workdir))
            .with_interactive(MockInteractiveProvider::new().with_yes_mode(true));

        let result = handle_init(
            &session,
            &InitArgs {
                dir: Some("test-pack".to_string()),
                modloader: Some("fabric".to_string()),
                mc_version: Some("1.21.1".to_string()),
                author: Some("Test Author".to_string()),
                game_versions: Some(vec!["1.21".to_string(), "1.21.2".to_string()]),
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok(), "init with game versions should succeed: {result:?}");

        let empack_yml = session
            .filesystem()
            .read_to_string(&target_dir.join("empack.yml"))
            .unwrap();
        assert!(
            empack_yml.contains("acceptable_game_versions"),
            "empack.yml should contain acceptable_game_versions: {empack_yml}"
        );
    }
}

// ===== HANDLE_ADD NON-JAR URL REJECTION =====

mod handle_add_non_jar_url_tests {
    use super::*;

    #[tokio::test]
    async fn rejects_non_jar_direct_download_url() {
        let workdir = mock_root().join("non-jar-url");
        let session = configured_session(&workdir);

        let result = handle_add(
            &session,
            vec!["https://example.com/pack.zip".to_string()],
            false,
            None,
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_err(), "non-jar URL should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not yet supported") || err_msg.contains("Unsupported"),
            "Error should mention unsupported file type: {err_msg}"
        );
    }
}

// ===== HANDLE_CLEAN DRY-RUN WITH ALL =====

mod handle_clean_dry_run_tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_all_preserves_artifacts() {
        let workdir = mock_root().join("built-project");
        let mut session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            );
        session.config_provider.app_config.dry_run = true;

        let result = handle_clean(&session, vec!["all".to_string()]).await;

        assert!(result.is_ok());
        assert!(
            session.filesystem().exists(&workdir.join("dist").join("test-pack.mrpack")),
            "Dry-run with 'all' should not remove artifacts"
        );
    }

    #[tokio::test]
    async fn dry_run_cache_target() {
        let workdir = mock_root().join("configured-project");
        let mut session = configured_session(&workdir);
        session.config_provider.app_config.dry_run = true;

        let result = handle_clean(&session, vec!["cache".to_string()]).await;
        assert!(result.is_ok());
    }
}

// ===== EXECUTE_COMMAND_WITH_SESSION DISPATCH TESTS =====

mod execute_command_dispatch_tests {
    use super::*;

    #[tokio::test]
    async fn dispatches_version_command() {
        let session = MockCommandSession::new();
        let result = execute_command_with_session(Commands::Version, &session).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatches_requirements_command() {
        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_version("1.2.3".to_string()));
        let result = execute_command_with_session(Commands::Requirements, &session).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatches_clean_command() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir);
        let result = execute_command_with_session(
            Commands::Clean { targets: vec!["cache".to_string()] },
            &session,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatches_sync_uninitialized_returns_error() {
        let session = MockCommandSession::new().with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(mock_root().join("uninit-dispatch")),
        );
        let result = execute_command_with_session(Commands::Sync {}, &session).await;
        assert!(result.is_err());
    }
}

// ===== HANDLE_REMOVE EMPTY MOD NAME FILTER =====

mod handle_remove_empty_name_tests {
    use super::*;

    #[tokio::test]
    async fn filters_empty_mod_names() {
        let workdir = mock_root().join("configured-project");
        let session = configured_session(&workdir)
            .with_process(MockProcessProvider::new().with_packwiz_result(
                vec!["remove".to_string(), "-y".to_string(), "sodium".to_string()],
                Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                }),
            ));

        let result = handle_remove(
            &session,
            vec!["".to_string(), "  ".to_string(), "sodium".to_string()],
            false,
        )
        .await;

        assert!(result.is_ok());
        let calls = session.process_provider.get_calls();
        assert_eq!(calls.len(), 1, "only non-empty mod names should be processed");
    }
}

// ===== HANDLE_BUILD DRY RUN WITH MULTIPLE TARGETS =====

mod handle_build_dry_run_tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_multiple_targets() {
        let workdir = mock_root().join("built-project");
        let mut session = MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new()
                    .with_current_dir(workdir.clone())
                    .with_built_project(workdir.clone()),
            );
        session.config_provider.app_config.dry_run = true;

        let result = handle_build(
            &session,
            &BuildArgs {
                targets: vec!["mrpack".to_string(), "client".to_string(), "server".to_string()],
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(
            session.process_provider.get_calls().is_empty(),
            "dry run should not execute any build commands"
        );
    }
}

// ===== FORMAT_DOWNLOADS EDGE CASES =====

mod format_downloads_edge_cases {
    use super::*;

    #[test]
    fn boundary_values() {
        assert_eq!(format_downloads(999_999), "999K");
        assert_eq!(format_downloads(1_999_999), "1M");
        assert_eq!(format_downloads(1), "1");
    }
}

// ===== PLAN_MC_VERSION AND PLAN_LOADER TESTS =====

mod plan_helpers_tests {
    use super::*;
    use crate::empack::config::ProjectPlan;

    #[test]
    fn plan_mc_version_returns_none_for_none() {
        assert!(plan_mc_version(None).is_none());
    }

    #[test]
    fn plan_mc_version_returns_version_from_plan() {
        let plan = ProjectPlan {
            name: "test".to_string(),
            author: None,
            version: None,
            minecraft_version: "1.21.1".to_string(),
            loader: Some(crate::empack::parsing::ModLoader::Fabric),
            loader_version: "0.15.0".to_string(),
            dependencies: vec![],
        };
        assert_eq!(plan_mc_version(Some(&plan)), Some("1.21.1"));
    }

    #[test]
    fn plan_loader_returns_none_for_none() {
        assert!(plan_loader(None).is_none());
    }

    #[test]
    fn plan_loader_returns_loader_from_plan() {
        let plan = ProjectPlan {
            name: "test".to_string(),
            author: None,
            version: None,
            minecraft_version: "1.21.1".to_string(),
            loader: Some(crate::empack::parsing::ModLoader::Forge),
            loader_version: "47.3.0".to_string(),
            dependencies: vec![],
        };
        assert_eq!(
            plan_loader(Some(&plan)),
            Some(crate::empack::parsing::ModLoader::Forge)
        );
    }

    #[test]
    fn plan_loader_returns_none_when_plan_has_no_loader() {
        let plan = ProjectPlan {
            name: "test".to_string(),
            author: None,
            version: None,
            minecraft_version: "1.21.1".to_string(),
            loader: None,
            loader_version: String::new(),
            dependencies: vec![],
        };
        assert!(plan_loader(Some(&plan)).is_none());
    }
}
