//! Cross-platform mock test session builder
//!
//! Provides `MockSessionBuilder` for creating in-memory test sessions backed
//! entirely by mock providers. No shell scripts, no real filesystem.

use anyhow::Result;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{NetworkProvider, ProcessOutput};
use empack_lib::application::session_mocks::{
    MockCommandSession, MockConfigProvider, MockFileSystemProvider, MockInteractiveProvider,
    MockNetworkProvider as LibMockNetworkProvider, MockProcessProvider, mock_root,
};
use empack_lib::empack::search::{ProjectInfo, ProjectResolverTrait, SearchError};
use empack_lib::primitives::ProjectPlatform;
use empack_lib::terminal::TerminalCapabilities;
use reqwest::Client;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Builder for creating cross-platform mock test sessions.
///
/// Produces a `MockCommandSession` backed entirely by in-memory providers,
/// enabling tests to run on any platform without shell scripts or real
/// filesystem operations.
pub struct MockSessionBuilder {
    filesystem: MockFileSystemProvider,
    process: MockProcessProvider,
    network: LibMockNetworkProvider,
    config: AppConfig,
    interactive: Option<MockInteractiveProvider>,
    terminal_capabilities: Option<TerminalCapabilities>,
}

impl Default for MockSessionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSessionBuilder {
    pub fn new() -> Self {
        Self {
            filesystem: MockFileSystemProvider::new(),
            process: MockProcessProvider::new()
                .with_mrpack_export_side_effects()
                .with_java_installer_side_effects(),
            network: LibMockNetworkProvider::new().with_failing_http_client(),
            config: AppConfig::default(),
            interactive: None,
            terminal_capabilities: None,
        }
    }

    pub fn with_empack_project(mut self, name: &str, mc_version: &str, loader: &str) -> Self {
        let workdir = mock_root().join("workdir");
        self.filesystem = self
            .filesystem
            .with_current_dir(workdir.clone())
            .with_empack_project(workdir.clone(), name, mc_version, loader);
        self.config.workdir = Some(workdir);
        self
    }

    pub fn with_yes_flag(mut self) -> Self {
        self.config.yes = true;
        self
    }

    pub fn with_dry_run_flag(mut self) -> Self {
        self.config.dry_run = true;
        self
    }

    pub fn with_mock_search_result(mut self, query: &str, project_info: ProjectInfo) -> Self {
        self.network = self
            .network
            .with_project_response(query.to_string(), project_info);
        self
    }

    pub fn with_pre_cached_jars(mut self) -> Self {
        let workdir = self
            .config
            .workdir
            .clone()
            .unwrap_or_else(|| mock_root().join("workdir"));
        let cache_dir = workdir.join("cache");
        self.filesystem = self
            .filesystem
            .with_file(
                cache_dir.join("packwiz-installer-bootstrap.jar"),
                "mock-bootstrap-jar".to_string(),
            )
            .with_file(
                cache_dir.join("packwiz-installer.jar"),
                "mock-installer-jar".to_string(),
            );
        self
    }

    pub fn with_packwiz_result(
        mut self,
        args: Vec<String>,
        result: std::result::Result<ProcessOutput, String>,
    ) -> Self {
        self.process = self.process.with_packwiz_result(args, result);
        self
    }

    pub fn with_packwiz_add_slug(mut self, project_id: String, slug: String) -> Self {
        self.process = self.process.with_packwiz_add_slug(project_id, slug);
        self
    }

    pub fn with_installed_mods(mut self, mods: std::collections::HashSet<String>) -> Self {
        self.filesystem = self.filesystem.with_installed_mods(mods);
        self
    }

    pub fn with_file(mut self, path: PathBuf, content: String) -> Self {
        self.filesystem = self.filesystem.with_file(path, content);
        self
    }

    pub fn with_deferred_file(
        mut self,
        directory: PathBuf,
        filename: String,
        content: String,
    ) -> Self {
        self.filesystem = self
            .filesystem
            .with_deferred_file(directory, filename, content);
        self
    }

    /// Pre-populate `srv.jar` stubs in server and server-full dist directories
    /// via deferred files so the build orchestrator skips the real HTTP download.
    pub fn with_server_jar_stub(self) -> Self {
        let workdir = self
            .config
            .workdir
            .clone()
            .unwrap_or_else(|| mock_root().join("workdir"));
        let dist = workdir.join("dist");
        self.with_deferred_file(
            dist.join("server"),
            "srv.jar".to_string(),
            "mock-server-jar".to_string(),
        )
        .with_deferred_file(
            dist.join("server-full"),
            "srv.jar".to_string(),
            "mock-server-jar".to_string(),
        )
    }

    pub fn with_mock_http_client(mut self) -> Self {
        self.network = self.network.enable_http_client();
        self
    }

    pub fn with_interactive(mut self, interactive: MockInteractiveProvider) -> Self {
        self.interactive = Some(interactive);
        self
    }

    pub fn with_terminal_capabilities(mut self, caps: TerminalCapabilities) -> Self {
        self.terminal_capabilities = Some(caps);
        self
    }

    pub fn build(self) -> MockCommandSession {
        let interactive = self
            .interactive
            .unwrap_or_else(|| MockInteractiveProvider::new().with_yes_mode(self.config.yes));

        let mut session = MockCommandSession::new()
            .with_filesystem(self.filesystem)
            .with_process(self.process)
            .with_network(self.network)
            .with_config(MockConfigProvider::new(self.config))
            .with_interactive(interactive);

        if let Some(caps) = self.terminal_capabilities {
            session = session.with_terminal_capabilities(caps);
        }

        session
    }
}

/// Mock network provider for hermetic testing
pub struct MockNetworkProvider {
    /// Mock project search results
    search_results: HashMap<String, ProjectInfo>,
    /// Whether tests may construct a reqwest client without performing live IO.
    allow_http_client: bool,
}

impl MockNetworkProvider {
    /// Create a new mock network provider
    pub fn new() -> Self {
        Self {
            search_results: HashMap::new(),
            allow_http_client: false,
        }
    }

    pub fn enable_http_client(&mut self) {
        self.allow_http_client = true;
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

impl Default for MockNetworkProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkProvider for MockNetworkProvider {
    fn http_client(&self) -> Result<Client> {
        if self.allow_http_client {
            return Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create mock HTTP client: {e}"));
        }

        // Return error to force fallback to hardcoded versions in tests
        // This prevents real network calls and makes tests deterministic
        Err(anyhow::anyhow!("Mock HTTP client unavailable (test mode)"))
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
        _preferred_platform: Option<ProjectPlatform>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_session_builder_creates_project() {
        let session = MockSessionBuilder::new()
            .with_empack_project("test-pack", "1.21.4", "fabric")
            .with_yes_flag()
            .build();

        let workdir = session.filesystem().current_dir().unwrap();
        assert!(
            session.filesystem().exists(&workdir.join("empack.yml")),
            "empack.yml should exist in mock filesystem"
        );
        assert!(
            session
                .filesystem()
                .exists(&workdir.join("pack").join("pack.toml")),
            "pack/pack.toml should exist in mock filesystem"
        );
        assert!(
            session
                .filesystem()
                .exists(&workdir.join("pack").join("index.toml")),
            "pack/index.toml should exist in mock filesystem"
        );

        let empack_yml = session
            .filesystem()
            .read_to_string(&workdir.join("empack.yml"))
            .unwrap();
        assert!(empack_yml.contains("minecraft_version: \"1.21.4\""));
        assert!(empack_yml.contains("loader: fabric"));
        assert!(empack_yml.contains("name: \"test-pack\""));
    }

    #[test]
    fn test_mock_session_builder_pre_cached_jars() {
        let session = MockSessionBuilder::new()
            .with_empack_project("test-pack", "1.21.4", "fabric")
            .with_pre_cached_jars()
            .build();

        let cache_dir = mock_root().join("workdir").join("cache");
        assert!(
            session
                .filesystem()
                .exists(&cache_dir.join("packwiz-installer-bootstrap.jar"))
        );
        assert!(
            session
                .filesystem()
                .exists(&cache_dir.join("packwiz-installer.jar"))
        );
    }

    #[test]
    fn test_mock_session_builder_dry_run() {
        let session = MockSessionBuilder::new().with_dry_run_flag().build();

        assert!(session.config().app_config().dry_run);
    }
}
