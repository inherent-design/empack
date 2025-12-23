//! Hermetic test environment for E2E testing
//!
//! This module provides a TestEnvironment helper that creates isolated test environments
//! with mock executables, enabling true hermetic E2E testing without external dependencies.

use anyhow::Result;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveProcessProvider,
    NetworkProvider,
};
use empack_lib::empack::search::{ProjectInfo, ProjectResolverTrait, SearchError};
use empack_lib::primitives::ProjectPlatform;
use reqwest::Client;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tempfile::TempDir;

/// Hermetic test environment with mock executables
pub struct TestEnvironment {
    /// Temporary directory for the test environment
    pub temp_dir: TempDir,
    /// Path to the test environment root
    pub root_path: PathBuf,
    /// Path to the bin directory containing mock executables
    pub bin_path: PathBuf,
    /// Path to the work directory for test projects
    pub work_path: PathBuf,
    /// Mock executable configurations
    mock_executables: HashMap<String, MockExecutable>,
}

/// Configuration for a mock executable
#[derive(Debug, Clone)]
pub struct MockExecutable {
    /// Name of the executable
    pub name: String,
    /// Mock implementation behavior
    pub behavior: MockBehavior,
    /// Log file path for recording calls
    pub log_path: PathBuf,
}

/// Mock executable behavior configuration
#[derive(Debug, Clone)]
pub enum MockBehavior {
    /// Always succeed with empty output
    AlwaysSucceed,
    /// Always fail with error message
    AlwaysFail { error: String },
    /// Succeed with specific output
    SucceedWithOutput { stdout: String, stderr: String },
    /// Conditional behavior based on arguments
    Conditional { rules: Vec<ConditionalRule> },
}

/// Conditional rule for mock executable behavior
#[derive(Debug, Clone)]
pub struct ConditionalRule {
    /// Arguments pattern to match
    pub args_pattern: Vec<String>,
    /// Behavior when pattern matches
    pub behavior: MockBehavior,
}

impl TestEnvironment {
    /// Create a new hermetic test environment
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new()?;
        let root_path = temp_dir.path().to_path_buf();
        let bin_path = root_path.join("bin");
        let work_path = root_path.join("work");

        // Create directory structure
        fs::create_dir_all(&bin_path)?;
        fs::create_dir_all(&work_path)?;

        Ok(Self {
            temp_dir,
            root_path,
            bin_path,
            work_path,
            mock_executables: HashMap::new(),
        })
    }

    /// Add a mock executable to the environment
    pub fn add_mock_executable(&mut self, name: &str, behavior: MockBehavior) -> Result<()> {
        let log_path = self.root_path.join(format!("{}.log", name));
        let executable_path = self.bin_path.join(name);

        let mock_executable = MockExecutable {
            name: name.to_string(),
            behavior: behavior.clone(),
            log_path: log_path.clone(),
        };

        self.mock_executables
            .insert(name.to_string(), mock_executable);

        // Create the mock executable script
        let script_content = self.generate_mock_script(name, &behavior, &log_path)?;
        fs::write(&executable_path, script_content)?;

        // Make it executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&executable_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&executable_path, perms)?;
        }

        Ok(())
    }

    /// Generate mock script content for an executable
    fn generate_mock_script(
        &self,
        name: &str,
        behavior: &MockBehavior,
        log_path: &Path,
    ) -> Result<String> {
        let log_path_str = log_path.to_str().unwrap();

        let behavior_code = match behavior {
            MockBehavior::AlwaysSucceed => "exit 0".to_string(),
            MockBehavior::AlwaysFail { error } => {
                format!("echo '{}' >&2\nexit 1", error)
            }
            MockBehavior::SucceedWithOutput { stdout, stderr } => {
                let mut code = String::new();
                if !stdout.is_empty() {
                    code.push_str(&format!("echo '{}'\n", stdout));
                }
                if !stderr.is_empty() {
                    code.push_str(&format!("echo '{}' >&2\n", stderr));
                }
                code.push_str("exit 0");
                code
            }
            MockBehavior::Conditional { rules } => {
                let mut code = String::new();
                for rule in rules {
                    let pattern = rule.args_pattern.join(" ");
                    code.push_str(&format!("if [ \"$*\" = \"{}\" ]; then\n", pattern));
                    code.push_str(&format!(
                        "  {}\n",
                        self.generate_behavior_code(&rule.behavior)
                    ));
                    code.push_str("fi\n");
                }
                code.push_str("exit 0"); // Default success
                code
            }
        };

        let script = format!(
            r#"#!/bin/bash
# Mock executable: {}
# Log all calls to: {}

# Log the call
echo "$(date '+%Y-%m-%d %H:%M:%S') {} $*" >> "{}"

# Execute behavior
{}
"#,
            name, log_path_str, name, log_path_str, behavior_code
        );

        Ok(script)
    }

    /// Generate behavior code for conditional rules
    fn generate_behavior_code(&self, behavior: &MockBehavior) -> String {
        match behavior {
            MockBehavior::AlwaysSucceed => "exit 0".to_string(),
            MockBehavior::AlwaysFail { error } => format!("echo '{}' >&2; exit 1", error),
            MockBehavior::SucceedWithOutput { stdout, stderr } => {
                let mut code = String::new();
                if !stdout.is_empty() {
                    code.push_str(&format!("echo '{}'; ", stdout));
                }
                if !stderr.is_empty() {
                    code.push_str(&format!("echo '{}' >&2; ", stderr));
                }
                code.push_str("exit 0");
                code
            }
            MockBehavior::Conditional { .. } => "exit 0".to_string(), // Nested conditionals not supported
        }
    }

    /// Get the PATH environment variable for this test environment
    pub fn get_path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_path.to_str().unwrap(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// Get the log contents for a mock executable
    pub fn get_mock_log(&self, executable_name: &str) -> Result<String> {
        let log_path = self.root_path.join(format!("{}.log", executable_name));
        if log_path.exists() {
            Ok(fs::read_to_string(log_path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Verify that a mock executable was called with specific arguments
    pub fn verify_mock_call(&self, executable_name: &str, args: &[&str]) -> Result<bool> {
        let log_content = self.get_mock_log(executable_name)?;
        let expected_call = format!("{} {}", executable_name, args.join(" "));
        Ok(log_content.contains(&expected_call))
    }

    /// Get all calls made to a mock executable
    pub fn get_mock_calls(&self, executable_name: &str) -> Result<Vec<String>> {
        let log_content = self.get_mock_log(executable_name)?;
        let calls: Vec<String> = log_content
            .lines()
            .filter_map(|line| {
                // Extract the command part after the timestamp
                let parts: Vec<&str> = line.split(' ').collect();
                if parts.len() > 2 {
                    Some(parts[2..].join(" "))
                } else {
                    None
                }
            })
            .collect();
        Ok(calls)
    }

    /// Initialize an empack project in the work directory
    pub fn init_empack_project(
        &self,
        project_name: &str,
        minecraft_version: &str,
        loader: &str,
    ) -> Result<PathBuf> {
        let project_path = self.work_path.join(project_name);
        fs::create_dir_all(&project_path)?;
        fs::create_dir_all(project_path.join("pack"))?;

        // Create empack.yml
        let empack_yml = format!(
            r#"empack:
  dependencies:
    - fabric_api: "Fabric API|mod"
    - sodium: "Sodium|mod"
  minecraft_version: "{}"
  loader: {}
  name: "{}"
  author: "Test Author"
  version: "1.0.0"
"#,
            minecraft_version, loader, project_name
        );
        fs::write(project_path.join("empack.yml"), empack_yml)?;

        // Create pack.toml
        let pack_toml = format!(
            r#"name = "{}"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{}"
{} = "0.15.0"
"#,
            project_name, minecraft_version, loader
        );
        fs::write(project_path.join("pack").join("pack.toml"), pack_toml)?;

        // Create index.toml
        let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
        fs::write(project_path.join("pack").join("index.toml"), index_toml)?;

        Ok(project_path)
    }
}

/// Builder for creating hermetic test sessions with coordinated mock providers
pub struct HermeticSessionBuilder {
    test_env: TestEnvironment,
    app_config: AppConfig,
    network_provider: MockNetworkProvider,
}

impl HermeticSessionBuilder {
    /// Create a new hermetic session builder
    pub fn new() -> Result<Self> {
        let test_env = TestEnvironment::new()?;
        let app_config = AppConfig::default();
        let network_provider = MockNetworkProvider::new();

        Ok(Self {
            test_env,
            app_config,
            network_provider,
        })
    }

    /// Add a mock executable to the test environment
    pub fn with_mock_executable(mut self, name: &str, behavior: MockBehavior) -> Result<Self> {
        self.test_env.add_mock_executable(name, behavior)?;
        Ok(self)
    }

    /// Set the working directory for the app config
    pub fn with_workdir(mut self, workdir: PathBuf) -> Self {
        self.app_config.workdir = Some(workdir);
        self
    }

    /// Add a mock mod to the network provider
    pub fn with_mock_mod(mut self, name: &str, project_id: &str) -> Self {
        self.network_provider.add_mock_mod(name, project_id);
        self
    }

    /// Add a mock search result to the network provider
    pub fn with_mock_search_result(mut self, query: &str, project_info: ProjectInfo) -> Self {
        self.network_provider.add_search_result(query, project_info);
        self
    }

    /// Initialize an empack project in the test environment
    pub fn with_empack_project(
        mut self,
        project_name: &str,
        minecraft_version: &str,
        loader: &str,
    ) -> Result<Self> {
        let project_path =
            self.test_env
                .init_empack_project(project_name, minecraft_version, loader)?;
        self.app_config.workdir = Some(project_path);
        Ok(self)
    }

    /// Build the hermetic session with all configured providers
    pub fn build(
        self,
    ) -> Result<(
        CommandSession<
            LiveFileSystemProvider,
            MockNetworkProvider,
            LiveProcessProvider,
            LiveConfigProvider,
        >,
        TestEnvironment,
    )> {
        // Create session with coordinated mock providers
        let session = CommandSession::new_with_providers(
            LiveFileSystemProvider,
            self.network_provider,
            LiveProcessProvider::new_for_test(Some(
                self.test_env.bin_path.to_string_lossy().to_string(),
            )),
            LiveConfigProvider::new(self.app_config),
        );

        Ok((session, self.test_env))
    }

    /// Get a reference to the test environment
    pub fn test_env(&self) -> &TestEnvironment {
        &self.test_env
    }

    /// Get a mutable reference to the test environment
    pub fn test_env_mut(&mut self) -> &mut TestEnvironment {
        &mut self.test_env
    }
}

/// Mock network provider for hermetic testing
pub struct MockNetworkProvider {
    /// Mock project search results
    search_results: HashMap<String, ProjectInfo>,
}

impl MockNetworkProvider {
    /// Create a new mock network provider
    pub fn new() -> Self {
        Self {
            search_results: HashMap::new(),
        }
    }

    /// Add a mock search result for a query
    pub fn add_search_result(&mut self, query: &str, project_info: ProjectInfo) {
        self.search_results.insert(query.to_string(), project_info);
    }

    /// Add a mock mod result with reasonable defaults
    pub fn add_mock_mod(&mut self, name: &str, project_id: &str) {
        self.search_results.insert(
            name.to_string(),
            ProjectInfo {
                platform: ProjectPlatform::Modrinth,
                project_id: project_id.to_string(),
                title: name.to_string(),
                downloads: 1000,
                confidence: 100,
                project_type: "mod".to_string(),
            },
        );
    }
}

impl NetworkProvider for MockNetworkProvider {
    fn http_client(&self) -> Result<Client> {
        // Return a client that won't actually be used in mocked scenarios
        Ok(Client::new())
    }

    fn project_resolver(
        &self,
        _client: Client,
        _curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        Box::new(MockProjectResolver {
            search_results: self.search_results.clone(),
        })
    }
}

/// Mock project resolver that returns predefined results
pub struct MockProjectResolver {
    search_results: HashMap<String, ProjectInfo>,
}

impl ProjectResolverTrait for MockProjectResolver {
    fn resolve_project(
        &self,
        title: &str,
        _project_type: Option<&str>,
        _minecraft_version: Option<&str>,
        _mod_loader: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectInfo, SearchError>> + Send + '_>> {
        let result = if let Some(project_info) = self.search_results.get(title) {
            Ok(project_info.clone())
        } else {
            Err(SearchError::NoResults {
                query: title.to_string(),
            })
        };

        Box::pin(async move { result })
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Cleanup is handled by TempDir automatically
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_creation() {
        let env = TestEnvironment::new().expect("Failed to create test environment");
        assert!(env.root_path.exists());
        assert!(env.bin_path.exists());
        assert!(env.work_path.exists());
    }

    #[test]
    fn test_mock_executable_creation() {
        let mut env = TestEnvironment::new().expect("Failed to create test environment");

        env.add_mock_executable("test-cmd", MockBehavior::AlwaysSucceed)
            .expect("Failed to add mock executable");

        let executable_path = env.bin_path.join("test-cmd");
        assert!(executable_path.exists());

        // Verify the executable is actually executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&executable_path).unwrap();
            assert!(metadata.permissions().mode() & 0o111 != 0);
        }
    }

    #[test]
    fn test_mock_executable_logging() {
        let mut env = TestEnvironment::new().expect("Failed to create test environment");

        env.add_mock_executable("test-cmd", MockBehavior::AlwaysSucceed)
            .expect("Failed to add mock executable");

        // In a real test, we would execute the mock command here
        // For now, we just verify the log file path is correct
        let log_path = env.root_path.join("test-cmd.log");
        assert!(!log_path.exists()); // Should not exist until command is run
    }

    #[test]
    fn test_empack_project_initialization() {
        let env = TestEnvironment::new().expect("Failed to create test environment");

        let project_path = env
            .init_empack_project("test-pack", "1.21.1", "fabric")
            .expect("Failed to initialize empack project");

        assert!(project_path.exists());
        assert!(project_path.join("empack.yml").exists());
        assert!(project_path.join("pack").join("pack.toml").exists());
        assert!(project_path.join("pack").join("index.toml").exists());

        // Verify content
        let empack_yml = fs::read_to_string(project_path.join("empack.yml")).unwrap();
        assert!(empack_yml.contains("minecraft_version: \"1.21.1\""));
        assert!(empack_yml.contains("loader: fabric"));
    }

    #[test]
    fn test_path_env_generation() {
        let env = TestEnvironment::new().expect("Failed to create test environment");
        let path_env = env.get_path_env();

        assert!(path_env.starts_with(env.bin_path.to_str().unwrap()));
        assert!(path_env.contains(":"));
    }
}
