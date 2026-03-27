//! Mock implementations of session providers for testing
//!
//! These mocks enable comprehensive testing of command handlers without
//! requiring external dependencies or filesystem operations.

use crate::Result;
use crate::application::config::AppConfig;
use crate::application::session::{InteractiveProvider, ProcessOutput, Session, *};
use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::empack::config::ConfigManager;
use crate::empack::packwiz::{MockPackwizOps, PackwizOps};
use crate::empack::search::{ProjectInfo, ProjectResolverTrait, SearchError};
use indicatif::MultiProgress;
use reqwest::Client;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Returns a platform-appropriate absolute path root for mock/test paths.
/// On Unix: `/test`, on Windows: `C:\test`
pub fn mock_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\test")
    } else {
        PathBuf::from("/test")
    }
}

/// Default index.toml template for packwiz integration tests
const DEFAULT_INDEX_TOML: &str = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;

/// Mock filesystem provider for testing
pub struct MockFileSystemProvider {
    pub current_dir: PathBuf,
    pub installed_mods: HashSet<String>,
    pub state_manager_calls: Arc<Mutex<Vec<PathBuf>>>,
    pub config_manager_calls: Arc<Mutex<Vec<PathBuf>>>,
    /// In-memory filesystem: path -> content
    pub files: Arc<Mutex<HashMap<PathBuf, String>>>,
    /// In-memory binary filesystem: path -> bytes
    pub binary_files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    /// Track directories that exist
    pub directories: Arc<Mutex<HashSet<PathBuf>>>,
}

impl MockFileSystemProvider {
    pub fn new() -> Self {
        Self {
            current_dir: mock_root().join("workdir"),
            installed_mods: HashSet::new(),
            state_manager_calls: Arc::new(Mutex::new(Vec::new())),
            config_manager_calls: Arc::new(Mutex::new(Vec::new())),
            files: Arc::new(Mutex::new(HashMap::new())),
            binary_files: Arc::new(Mutex::new(HashMap::new())),
            directories: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn with_current_dir(mut self, dir: PathBuf) -> Self {
        self.current_dir = dir.clone();
        self.directories.lock().unwrap().insert(dir);
        self
    }

    pub fn with_installed_mods(mut self, mods: HashSet<String>) -> Self {
        self.installed_mods = mods;
        self
    }

    pub fn with_file(self, path: PathBuf, content: String) -> Self {
        // Add parent directory to directories set
        if let Some(parent) = path.parent() {
            self.directories
                .lock()
                .unwrap()
                .insert(parent.to_path_buf());
        }
        self.files.lock().unwrap().insert(path, content);
        self
    }

    pub fn with_files(self, files: HashMap<PathBuf, String>) -> Self {
        *self.files.lock().unwrap() = files;
        self
    }

    /// Helper method to create a typical empack project structure
    pub fn with_empack_project(
        self,
        workdir: PathBuf,
        name: &str,
        minecraft_version: &str,
        loader: &str,
    ) -> Self {
        let empack_yml = format!(
            r#"empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
  minecraft_version: "{}"
  loader: {}
  name: "{}"
  author: "Test Author"
  version: "1.0.0"
"#,
            minecraft_version, loader, name
        );

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
{} = "0.14.21"
"#,
            name, minecraft_version, loader
        );

        let index_toml = DEFAULT_INDEX_TOML;

        self.with_file(workdir.join("empack.yml"), empack_yml)
            .with_file(
                workdir.join("pack").join("pack.toml"),
                pack_toml.to_string(),
            )
            .with_file(
                workdir.join("pack").join("index.toml"),
                index_toml.to_string(),
            )
    }

    /// Standard mock setup for a configured project (empack.yml + pack.toml)
    pub fn with_configured_project(self, workdir: PathBuf) -> Self {
        let empack_yml = r#"empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
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

        let index_toml = DEFAULT_INDEX_TOML;

        self.with_file(workdir.join("empack.yml"), empack_yml.to_string())
            .with_file(
                workdir.join("pack").join("pack.toml"),
                pack_toml.to_string(),
            )
            .with_file(
                workdir.join("pack").join("index.toml"),
                index_toml.to_string(),
            )
    }

    /// Standard mock setup for a built project (configured + build artifacts)
    pub fn with_built_project(self, workdir: PathBuf) -> Self {
        let configured = self.with_configured_project(workdir.clone());

        // Add build artifacts
        let dist_dir = workdir.join("dist");
        let mrpack_content = "mock mrpack content";
        let zip_content = "mock zip content";

        configured
            .with_file(
                dist_dir.join("test-pack.mrpack"),
                mrpack_content.to_string(),
            )
            .with_file(dist_dir.join("test-pack.zip"), zip_content.to_string())
    }
}

impl Default for MockFileSystemProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystemProvider for MockFileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        Ok(self.current_dir.clone())
    }

    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        self.config_manager_calls
            .lock()
            .unwrap()
            .push(workdir.clone());
        ConfigManager::new(workdir, self)
    }

    fn read_to_string(&self, path: &std::path::Path) -> Result<String> {
        let files = self.files.lock().unwrap();
        if let Some(content) = files.get(path) {
            Ok(content.clone())
        } else {
            Err(anyhow::anyhow!("File not found: {}", path.display()))
        }
    }

    fn read_bytes(&self, path: &std::path::Path) -> Result<Vec<u8>> {
        let binary_files = self.binary_files.lock().unwrap();
        if let Some(content) = binary_files.get(path) {
            return Ok(content.clone());
        }
        drop(binary_files);
        let files = self.files.lock().unwrap();
        if let Some(content) = files.get(path) {
            return Ok(content.as_bytes().to_vec());
        }
        Err(anyhow::anyhow!("File not found: {}", path.display()))
    }

    fn write_file(&self, path: &std::path::Path, content: &str) -> Result<()> {
        self.binary_files.lock().unwrap().remove(path);
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn write_bytes(&self, path: &std::path::Path, content: &[u8]) -> Result<()> {
        self.files.lock().unwrap().remove(path);
        self.binary_files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_vec());
        Ok(())
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        // Check both files and directories
        self.files.lock().unwrap().contains_key(path)
            || self.binary_files.lock().unwrap().contains_key(path)
            || self.directories.lock().unwrap().contains(path)
            || self.is_directory(path)
    }

    fn metadata_exists(&self, path: &std::path::Path) -> bool {
        self.exists(path)
    }

    fn is_directory(&self, path: &std::path::Path) -> bool {
        // Check if the path is explicitly tracked as a directory
        let directories = self.directories.lock().unwrap();
        if directories.contains(path) {
            return true;
        }

        // Check if the path is the current directory
        if path == self.current_dir {
            return true;
        }

        // No fallback pattern matching - if it's not explicitly tracked, it doesn't exist
        false
    }

    fn create_dir_all(&self, path: &std::path::Path) -> Result<()> {
        // Track the directory creation in the mock filesystem
        self.directories.lock().unwrap().insert(path.to_path_buf());
        // Also track all parent directories as existing
        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            self.directories
                .lock()
                .unwrap()
                .insert(parent.to_path_buf());
            current = parent.to_path_buf();
        }
        Ok(())
    }

    fn get_file_list(
        &self,
        path: &std::path::Path,
    ) -> Result<HashSet<PathBuf>> {
        let files = self.files.lock().unwrap();
        let binary_files = self.binary_files.lock().unwrap();
        let directories = self.directories.lock().unwrap();
        let mut result = HashSet::new();

        // Add files that are direct children of the path
        for file_path in files.keys() {
            if file_path.parent() == Some(path) {
                result.insert(file_path.clone());
            }
        }

        for file_path in binary_files.keys() {
            if file_path.parent() == Some(path) {
                result.insert(file_path.clone());
            }
        }

        // Add directories that are direct children of the path
        for dir_path in directories.iter() {
            if dir_path.parent() == Some(path) {
                result.insert(dir_path.clone());
            }
        }

        Ok(result)
    }

    fn has_build_artifacts(
        &self,
        dist_dir: &std::path::Path,
    ) -> Result<bool> {
        let files = self.files.lock().unwrap();
        let binary_files = self.binary_files.lock().unwrap();

        for path in files.keys() {
            if path.starts_with(dist_dir)
                && let Some(extension) = path.extension()
            {
                match extension.to_str() {
                    Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                    _ => continue,
                }
            }
        }

        for path in binary_files.keys() {
            if path.starts_with(dist_dir)
                && let Some(extension) = path.extension()
            {
                match extension.to_str() {
                    Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                    _ => continue,
                }
            }
        }

        Ok(false)
    }

    fn remove_file(&self, path: &std::path::Path) -> Result<()> {
        self.files.lock().unwrap().remove(path);
        self.binary_files.lock().unwrap().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &std::path::Path) -> Result<()> {
        let mut files = self.files.lock().unwrap();
        let mut binary_files = self.binary_files.lock().unwrap();
        let mut directories = self.directories.lock().unwrap();
        let paths_to_remove: Vec<PathBuf> = files
            .keys()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect();
        let binary_paths_to_remove: Vec<PathBuf> = binary_files
            .keys()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect();
        let directories_to_remove: Vec<PathBuf> = directories
            .iter()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect();

        for path in paths_to_remove {
            files.remove(&path);
        }

        for path in binary_paths_to_remove {
            binary_files.remove(&path);
        }

        for path in directories_to_remove {
            directories.remove(&path);
        }

        Ok(())
    }

}

type ResolverCall = (Client, Option<String>);

/// Mock network provider for testing
pub struct MockNetworkProvider {
    pub client_calls: Arc<Mutex<usize>>,
    pub resolver_calls: Arc<Mutex<Vec<ResolverCall>>>,
    pub mock_resolver: Arc<MockProjectResolver>,
}

impl MockNetworkProvider {
    pub fn new() -> Self {
        Self {
            client_calls: Arc::new(Mutex::new(0)),
            resolver_calls: Arc::new(Mutex::new(Vec::new())),
            mock_resolver: Arc::new(MockProjectResolver::new()),
        }
    }

    pub fn with_project_response(self, query: String, project_info: ProjectInfo) -> Self {
        self.mock_resolver
            .responses
            .lock()
            .unwrap()
            .insert(query, Ok(project_info));
        self
    }

    pub fn with_error_response(self, query: String, error_message: String) -> Self {
        self.mock_resolver
            .responses
            .lock()
            .unwrap()
            .insert(query, Err(error_message));
        self
    }
}

impl Default for MockNetworkProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkProvider for MockNetworkProvider {
    fn http_client(&self) -> Result<Client> {
        *self.client_calls.lock().unwrap() += 1;
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))
    }

    fn project_resolver(
        &self,
        client: Client,
        curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        self.resolver_calls
            .lock()
            .unwrap()
            .push((client.clone(), curseforge_api_key.clone()));
        Box::new(self.mock_resolver.as_ref().clone())
    }
}

/// Mock project resolver for testing
#[derive(Clone)]
pub struct MockProjectResolver {
    pub responses: Arc<Mutex<HashMap<String, std::result::Result<ProjectInfo, String>>>>,
}

impl MockProjectResolver {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_response(
        self,
        query: String,
        response: std::result::Result<ProjectInfo, String>,
    ) -> Self {
        self.responses.lock().unwrap().insert(query, response);
        self
    }

    pub fn with_project_response(self, query: String, project_info: ProjectInfo) -> Self {
        self.responses
            .lock()
            .unwrap()
            .insert(query, Ok(project_info));
        self
    }

    pub fn with_error_response(self, query: String, error_message: String) -> Self {
        self.responses
            .lock()
            .unwrap()
            .insert(query, Err(error_message));
        self
    }
}

impl Default for MockProjectResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectResolverTrait for MockProjectResolver {
    fn resolve_project(
        &self,
        title: &str,
        _project_type: Option<&str>,
        _minecraft_version: Option<&str>,
        _mod_loader: Option<&str>,
        _preferred_platform: Option<crate::primitives::ProjectPlatform>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<ProjectInfo, SearchError>> + Send + '_>>
    {
        let responses = self.responses.clone();
        let query = title.to_string();

        Box::pin(async move {
            let responses = responses.lock().unwrap();
            match responses.get(&query).cloned() {
                Some(Ok(project_info)) => Ok(project_info),
                Some(Err(error_message)) => Err(SearchError::NoResults {
                    query: error_message,
                }),
                None => Err(SearchError::NoResults { query }),
            }
        })
    }
}

/// Process call record for spy pattern
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessCall {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

/// Mock process provider for testing with spy pattern
pub struct MockProcessProvider {
    pub calls: RefCell<Vec<ProcessCall>>,
    pub results: HashMap<(String, Vec<String>), std::result::Result<ProcessOutput, String>>,
    pub programs: HashMap<String, Option<String>>,
    materialize_mrpack_exports: bool,
    /// Maps project_id -> slug for simulating .pw.toml creation on packwiz add
    packwiz_add_slugs: HashMap<String, String>,
    files: Option<Arc<Mutex<HashMap<PathBuf, String>>>>,
    directories: Option<Arc<Mutex<HashSet<PathBuf>>>>,
}

impl MockProcessProvider {
    pub fn new() -> Self {
        let packwiz_path = mock_root()
            .join("bin")
            .join("packwiz")
            .to_string_lossy()
            .to_string();

        let mut programs = HashMap::new();
        programs.insert("packwiz".to_string(), Some(packwiz_path.clone()));

        let mut provider = Self {
            calls: RefCell::new(Vec::new()),
            results: HashMap::new(),
            programs,
            materialize_mrpack_exports: false,
            packwiz_add_slugs: HashMap::new(),
            files: None,
            directories: None,
        };
        // Backward compat: keep "which" result for any code still using execute("which", ...)
        provider.results.insert(
            ("which".to_string(), vec!["packwiz".to_string()]),
            Ok(ProcessOutput {
                stdout: format!("{}\n", packwiz_path),
                stderr: String::new(),
                success: true,
            }),
        );
        provider
    }

    pub fn with_packwiz_unavailable(mut self) -> Self {
        self.programs.insert("packwiz".to_string(), None);
        self.results.insert(
            ("which".to_string(), vec!["packwiz".to_string()]),
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: "packwiz not found".to_string(),
                success: false,
            }),
        );
        self
    }

    pub fn with_packwiz_version(mut self, version: String) -> Self {
        let packwiz_path = mock_root()
            .join("bin")
            .join("packwiz")
            .to_string_lossy()
            .to_string();

        // Ensure packwiz is available via find_program
        self.programs.insert(
            "packwiz".to_string(),
            Some(packwiz_path.clone()),
        );
        // Backward compat: keep "which" result
        self.results.insert(
            ("which".to_string(), vec!["packwiz".to_string()]),
            Ok(ProcessOutput {
                stdout: format!("{}\n", packwiz_path),
                stderr: String::new(),
                success: true,
            }),
        );
        // Mock go version -m output for version detection (matches real format with leading tabs)
        self.results.insert(
            (
                "go".to_string(),
                vec![
                    "version".to_string(),
                    "-m".to_string(),
                    packwiz_path.clone(),
                ],
            ),
            Ok(ProcessOutput {
                stdout: format!(
                    "{}: go1.21.0\n\tpath\tgithub.com/packwiz/packwiz\n\tmod\tgithub.com/packwiz/packwiz\t{}\th1:abc123=\n",
                    packwiz_path, version
                ),
                stderr: String::new(),
                success: true,
            }),
        );
        self
    }

    pub fn with_result(
        mut self,
        command: String,
        args: Vec<String>,
        result: std::result::Result<ProcessOutput, String>,
    ) -> Self {
        self.results.insert((command, args), result);
        self
    }

    pub fn with_packwiz_result(
        mut self,
        args: Vec<String>,
        result: std::result::Result<ProcessOutput, String>,
    ) -> Self {
        self.results.insert(("packwiz".to_string(), args), result);
        self
    }

    pub fn with_mrpack_export_side_effects(mut self) -> Self {
        self.materialize_mrpack_exports = true;
        self
    }

    /// Register a side effect: when packwiz add succeeds for `project_id`,
    /// create `mods/{slug}.pw.toml` in the mock filesystem.
    pub fn with_packwiz_add_slug(mut self, project_id: String, slug: String) -> Self {
        self.packwiz_add_slugs.insert(project_id, slug);
        self
    }

    fn connect_filesystem(&mut self, filesystem: &MockFileSystemProvider) {
        self.files = Some(filesystem.files.clone());
        self.directories = Some(filesystem.directories.clone());
    }

    fn maybe_materialize_mrpack_export(
        &self,
        command: &str,
        args: &[&str],
        output: &ProcessOutput,
    ) {
        if command != "packwiz" || !self.materialize_mrpack_exports || !output.success {
            return;
        }

        if !args.contains(&"mr") || !args.contains(&"export") {
            return;
        }

        let Some(output_index) = args.iter().position(|arg| *arg == "-o") else {
            return;
        };
        let Some(output_path) = args.get(output_index + 1) else {
            return;
        };

        let (Some(files), Some(directories)) = (&self.files, &self.directories) else {
            return;
        };

        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            directories.lock().unwrap().insert(parent.to_path_buf());
        }
        files
            .lock()
            .unwrap()
            .insert(output_path, "mock mrpack artifact".to_string());
    }

    /// When a `packwiz {platform} add --project-id {id}` command succeeds and we
    /// have a registered slug for that project_id, create `{workdir}/mods/{slug}.pw.toml`
    /// in the mock filesystem. This simulates what real packwiz does.
    fn maybe_materialize_pw_toml(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &std::path::Path,
        output: &ProcessOutput,
    ) {
        if command != "packwiz" || self.packwiz_add_slugs.is_empty() || !output.success {
            return;
        }

        if !args.contains(&"add") {
            return;
        }

        // Extract project_id from --project-id or --addon-id flag
        let project_id = args
            .iter()
            .position(|arg| *arg == "--project-id" || *arg == "--addon-id")
            .and_then(|i| args.get(i + 1));

        let Some(project_id) = project_id else {
            return;
        };

        let Some(slug) = self.packwiz_add_slugs.get(*project_id) else {
            return;
        };

        let (Some(files), Some(directories)) = (&self.files, &self.directories) else {
            return;
        };

        // packwiz runs inside {workdir}/pack, creates files at {workdir}/pack/mods/{slug}.pw.toml
        // but handle_add passes working_dir as workdir.join("pack"), so mods/ is relative to that
        let mods_dir = working_dir.join("mods");
        directories.lock().unwrap().insert(mods_dir.clone());
        let pw_toml_path = mods_dir.join(format!("{}.pw.toml", slug));
        files
            .lock()
            .unwrap()
            .insert(pw_toml_path, format!("name = \"{}\"\n", slug));
    }

    /// Get all recorded process calls for verification
    pub fn get_calls(&self) -> Vec<ProcessCall> {
        self.calls.borrow().clone()
    }

    /// Get calls for a specific command
    pub fn get_calls_for_command(&self, command: &str) -> Vec<ProcessCall> {
        self.calls
            .borrow()
            .iter()
            .filter(|call| call.command == command)
            .cloned()
            .collect()
    }

    /// Verify that a specific command was called with expected arguments
    pub fn verify_call(&self, command: &str, args: &[&str], working_dir: &std::path::Path) -> bool {
        let expected_call = ProcessCall {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            working_dir: working_dir.to_path_buf(),
        };

        self.calls.borrow().contains(&expected_call)
    }
}

impl Default for MockProcessProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessProvider for MockProcessProvider {
    fn execute(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &std::path::Path,
    ) -> Result<ProcessOutput> {
        // Record the call for spy pattern verification
        let call = ProcessCall {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            working_dir: working_dir.to_path_buf(),
        };
        self.calls.borrow_mut().push(call);

        // Check if we have a specific result for this command
        let key = (
            command.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        );
        let output = if let Some(result) = self.results.get(&key) {
            match result {
                Ok(output) => Ok(output.clone()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            }
        } else {
            // Default behavior: succeed with empty output
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            })
        }?;

        self.maybe_materialize_mrpack_export(command, args, &output);
        self.maybe_materialize_pw_toml(command, args, working_dir, &output);

        Ok(output)
    }

    fn find_program(&self, program: &str) -> Option<String> {
        self.programs.get(program).cloned().flatten()
    }
}

/// Mock config provider for testing
pub struct MockConfigProvider {
    pub app_config: AppConfig,
}

impl MockConfigProvider {
    pub fn new(app_config: AppConfig) -> Self {
        Self { app_config }
    }
}

impl ConfigProvider for MockConfigProvider {
    fn app_config(&self) -> &AppConfig {
        &self.app_config
    }
}

/// Typed response for the queue-based mock interactive provider.
///
/// Each variant corresponds to one `InteractiveProvider` trait method.
/// Queued responses are consumed in FIFO order; when the front element
/// matches the expected type it is popped and returned. When the queue
/// is empty or the front element is the wrong type, the provider falls
/// back to yes_mode, then the static response, then the default value.
#[derive(Debug, Clone, PartialEq)]
pub enum MockResponse {
    Text(String),
    Confirm(bool),
    Select(usize),
    FuzzySelect(Option<usize>),
}

/// Mock interactive provider for testing
pub struct MockInteractiveProvider {
    yes_mode: bool,
    pub text_input_calls: Arc<Mutex<Vec<(String, String)>>>, // (prompt, default)
    pub confirm_calls: Arc<Mutex<Vec<(String, bool)>>>,      // (prompt, default)
    pub select_calls: Arc<Mutex<Vec<String>>>,               // prompt
    pub fuzzy_select_calls: Arc<Mutex<Vec<String>>>,         // prompt
    pub text_input_response: Arc<Mutex<Option<String>>>,
    pub confirm_response: Arc<Mutex<Option<bool>>>,
    pub select_response: Arc<Mutex<Option<usize>>>,
    pub fuzzy_select_response: Arc<Mutex<Option<usize>>>,
    pub response_queue: Arc<Mutex<VecDeque<MockResponse>>>,
}

impl MockInteractiveProvider {
    pub fn new() -> Self {
        Self {
            yes_mode: false,
            text_input_calls: Arc::new(Mutex::new(Vec::new())),
            confirm_calls: Arc::new(Mutex::new(Vec::new())),
            select_calls: Arc::new(Mutex::new(Vec::new())),
            fuzzy_select_calls: Arc::new(Mutex::new(Vec::new())),
            text_input_response: Arc::new(Mutex::new(None)),
            confirm_response: Arc::new(Mutex::new(None)),
            select_response: Arc::new(Mutex::new(None)),
            fuzzy_select_response: Arc::new(Mutex::new(None)),
            response_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn with_yes_mode(mut self, yes_mode: bool) -> Self {
        self.yes_mode = yes_mode;
        self
    }

    pub fn with_text_input(self, response: String) -> Self {
        *self.text_input_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_confirm(self, response: bool) -> Self {
        *self.confirm_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_select(self, response: usize) -> Self {
        *self.select_response.lock().unwrap() = Some(response);
        self
    }

    pub fn with_fuzzy_select(self, response: usize) -> Self {
        *self.fuzzy_select_response.lock().unwrap() = Some(response);
        self
    }

    pub fn queue_text(self, response: &str) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Text(response.to_string()));
        self
    }

    pub fn queue_confirm(self, response: bool) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Confirm(response));
        self
    }

    pub fn queue_select(self, index: usize) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::Select(index));
        self
    }

    pub fn queue_fuzzy_select(self, index: Option<usize>) -> Self {
        self.response_queue
            .lock()
            .unwrap()
            .push_back(MockResponse::FuzzySelect(index));
        self
    }

    /// Get recorded text input calls
    pub fn get_text_input_calls(&self) -> Vec<(String, String)> {
        self.text_input_calls.lock().unwrap().clone()
    }

    /// Get recorded confirm calls
    pub fn get_confirm_calls(&self) -> Vec<(String, bool)> {
        self.confirm_calls.lock().unwrap().clone()
    }

    /// Get recorded select calls
    pub fn get_select_calls(&self) -> Vec<String> {
        self.select_calls.lock().unwrap().clone()
    }

    /// Get recorded fuzzy select calls
    pub fn get_fuzzy_select_calls(&self) -> Vec<String> {
        self.fuzzy_select_calls.lock().unwrap().clone()
    }
}

impl Default for MockInteractiveProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveProvider for MockInteractiveProvider {
    fn text_input(&self, prompt: &str, default: String) -> Result<String> {
        self.text_input_calls
            .lock()
            .unwrap()
            .push((prompt.to_string(), default.clone()));

        // Queue takes priority: pop if front element is the matching type
        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Text(_)) = queue.front() {
                if let Some(MockResponse::Text(value)) = queue.pop_front() {
                    return Ok(value);
                }
            }
        }

        if self.yes_mode {
            return Ok(default);
        }

        if let Some(response) = self.text_input_response.lock().unwrap().clone() {
            Ok(response)
        } else {
            Ok(default)
        }
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        self.confirm_calls
            .lock()
            .unwrap()
            .push((prompt.to_string(), default));

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Confirm(_)) = queue.front() {
                if let Some(MockResponse::Confirm(value)) = queue.pop_front() {
                    return Ok(value);
                }
            }
        }

        if self.yes_mode {
            return Ok(default);
        }

        if let Some(response) = *self.confirm_response.lock().unwrap() {
            Ok(response)
        } else {
            Ok(default)
        }
    }

    fn select(&self, prompt: &str, _options: &[&str]) -> Result<usize> {
        self.select_calls.lock().unwrap().push(prompt.to_string());

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::Select(_)) = queue.front() {
                if let Some(MockResponse::Select(value)) = queue.pop_front() {
                    return Ok(value);
                }
            }
        }

        if self.yes_mode {
            return Ok(0);
        }

        if let Some(response) = *self.select_response.lock().unwrap() {
            Ok(response)
        } else {
            Ok(0)
        }
    }

    fn fuzzy_select(&self, prompt: &str, _options: &[String]) -> Result<Option<usize>> {
        self.fuzzy_select_calls
            .lock()
            .unwrap()
            .push(prompt.to_string());

        {
            let mut queue = self.response_queue.lock().unwrap();
            if let Some(MockResponse::FuzzySelect(_)) = queue.front() {
                if let Some(MockResponse::FuzzySelect(value)) = queue.pop_front() {
                    return Ok(value);
                }
            }
        }

        if self.yes_mode {
            return Ok(Some(0));
        }

        if let Some(response) = *self.fuzzy_select_response.lock().unwrap() {
            Ok(Some(response))
        } else {
            Ok(Some(0))
        }
    }
}

/// Mock command session for testing
pub struct MockCommandSession {
    pub multi_progress: Arc<MultiProgress>,
    pub display_provider: LiveDisplayProvider,
    pub filesystem_provider: MockFileSystemProvider,
    pub network_provider: MockNetworkProvider,
    pub process_provider: MockProcessProvider,
    pub config_provider: MockConfigProvider,
    pub interactive_provider: MockInteractiveProvider,
    pub packwiz_provider: MockPackwizOps,
}

impl MockCommandSession {
    pub fn new() -> Self {
        // Initialize display system for tests
        use crate::display::Display;
        use crate::terminal::capabilities::TerminalCapabilities;
        let capabilities = TerminalCapabilities::detect_from_config(&AppConfig::default())
            .expect("Failed to detect terminal capabilities for testing");
        Display::init_or_get(capabilities);

        let multi_progress = Arc::new(MultiProgress::new());
        let display_provider = LiveDisplayProvider::new_with_arc(multi_progress.clone());

        let filesystem_provider = MockFileSystemProvider::new();
        let packwiz_provider = MockPackwizOps::new()
            .with_current_dir(filesystem_provider.current_dir.clone())
            .with_filesystem(filesystem_provider.files.clone());

        let mut session = Self {
            multi_progress,
            display_provider,
            filesystem_provider,
            network_provider: MockNetworkProvider::new(),
            process_provider: MockProcessProvider::new(),
            config_provider: MockConfigProvider::new(AppConfig::default()),
            interactive_provider: MockInteractiveProvider::new(),
            packwiz_provider,
        };

        session.sync_process_provider();
        session
    }

    pub fn with_filesystem(mut self, filesystem: MockFileSystemProvider) -> Self {
        self.packwiz_provider = MockPackwizOps::new()
            .with_current_dir(filesystem.current_dir.clone())
            .with_installed_mods(filesystem.installed_mods.clone())
            .with_filesystem(filesystem.files.clone());
        self.filesystem_provider = filesystem;
        self.sync_process_provider();
        self
    }

    pub fn with_network(mut self, network: MockNetworkProvider) -> Self {
        self.network_provider = network;
        self
    }

    pub fn with_process(mut self, process: MockProcessProvider) -> Self {
        self.process_provider = process;
        self.sync_process_provider();
        self
    }

    pub fn with_config(mut self, config: MockConfigProvider) -> Self {
        self.config_provider = config;
        self
    }

    pub fn with_interactive(mut self, interactive: MockInteractiveProvider) -> Self {
        self.interactive_provider = interactive;
        self
    }

    pub fn with_packwiz(mut self, packwiz: MockPackwizOps) -> Self {
        self.packwiz_provider = packwiz;
        self
    }

    fn sync_process_provider(&mut self) {
        self.process_provider
            .connect_filesystem(&self.filesystem_provider);
    }

    /// Get the display provider for this session
    pub fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }

    /// Get the filesystem provider for this session
    pub fn filesystem(&self) -> &dyn FileSystemProvider {
        &self.filesystem_provider
    }

    /// Get the network provider for this session
    pub fn network(&self) -> &dyn NetworkProvider {
        &self.network_provider
    }

    /// Get the process provider for this session
    pub fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }

    /// Get the config provider for this session
    pub fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }

    /// Get the interactive provider for this session
    pub fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }
}

impl Default for MockCommandSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Session for MockCommandSession {
    fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }

    fn filesystem(&self) -> &dyn FileSystemProvider {
        &self.filesystem_provider
    }

    fn network(&self) -> &dyn NetworkProvider {
        &self.network_provider
    }

    fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }

    fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }

    fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }

    fn packwiz(&self) -> Box<dyn PackwizOps + '_> {
        let mut mock = MockPackwizOps::new()
            .with_current_dir(self.packwiz_provider.current_dir.clone())
            .with_installed_mods(self.packwiz_provider.installed_mods.clone())
            .with_filesystem(self.packwiz_provider.filesystem.clone());
        mock.fail_init = self.packwiz_provider.fail_init;
        Box::new(mock)
    }

    fn state(&self) -> crate::Result<crate::empack::state::PackStateManager<'_, dyn FileSystemProvider + '_>> {
        let workdir = self.filesystem().current_dir()?;
        Ok(crate::empack::state::PackStateManager::new(workdir, self.filesystem()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_mock_filesystem_provider() {
        let provider = MockFileSystemProvider::new()
            .with_current_dir(PathBuf::from("/custom/path"));

        assert_eq!(
            provider.current_dir().unwrap(),
            PathBuf::from("/custom/path")
        );
    }

    #[test]
    fn test_mock_packwiz_ops() {
        let mut mods = HashSet::new();
        mods.insert("test_mod".to_string());

        let packwiz = MockPackwizOps::new()
            .with_current_dir(PathBuf::from("/custom/path"))
            .with_installed_mods(mods.clone());

        assert_eq!(
            packwiz
                .get_installed_mods(&PathBuf::from("/custom/path"))
                .unwrap(),
            mods
        );
    }

    #[test]
    fn test_mock_process_provider() {
        use crate::empack::packwiz::{check_packwiz_available, get_packwiz_version};

        let working_dir = mock_root().join("workdir");
        let provider = MockProcessProvider::new()
            .with_packwiz_version("2.0.0".to_string())
            .with_result(
                "packwiz".to_string(),
                vec!["add".to_string(), "test-mod".to_string()],
                Err("Mock error".to_string()),
            );

        assert_eq!(
            check_packwiz_available(&provider, &working_dir).unwrap(),
            (true, "2.0.0".to_string())
        );
        let packwiz_path = mock_root()
            .join("bin")
            .join("packwiz")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            get_packwiz_version(&provider, &packwiz_path, &working_dir).unwrap(),
            "2.0.0"
        );

        // Test successful command (uses default behavior)
        let result = provider.execute("packwiz", &["list"], &working_dir);
        assert!(result.is_ok());
        assert!(result.unwrap().success);

        // Test command with specific result
        let result = provider.execute("packwiz", &["add", "test-mod"], &working_dir);
        assert!(result.is_err());

        // Test spy pattern - verify packwiz calls were recorded
        // (go version calls from check_packwiz_available are also recorded but filtered out)
        let packwiz_calls = provider.get_calls_for_command("packwiz");
        assert_eq!(packwiz_calls.len(), 2);
        assert_eq!(packwiz_calls[0].args, vec!["list"]);
        assert_eq!(packwiz_calls[1].args, vec!["add", "test-mod"]);

        // Test verification helper
        assert!(provider.verify_call("packwiz", &["list"], &working_dir));
        assert!(provider.verify_call("packwiz", &["add", "test-mod"], &working_dir));
        assert!(!provider.verify_call("packwiz", &["remove", "test-mod"], &working_dir));
    }

    #[test]
    fn test_mock_command_session() {
        use crate::empack::packwiz::check_packwiz_available;
        use std::path::Path;

        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_unavailable());

        assert_eq!(
            check_packwiz_available(session.process(), Path::new(".")).unwrap(),
            (false, "not found".to_string())
        );
    }

    #[test]
    fn test_mock_interactive_queue_responses() {
        let provider = MockInteractiveProvider::new()
            .queue_text("queued-name")
            .queue_confirm(false)
            .queue_select(2);

        // Queued responses are returned in FIFO order
        assert_eq!(
            provider.text_input("Name?", "default".to_string()).unwrap(),
            "queued-name"
        );
        assert_eq!(provider.confirm("Continue?", true).unwrap(), false);
        assert_eq!(
            provider.select("Pick one:", &["a", "b", "c"]).unwrap(),
            2
        );

        // Queue is now empty — falls back to default behavior
        assert_eq!(
            provider
                .text_input("Another?", "fallback".to_string())
                .unwrap(),
            "fallback"
        );
        assert_eq!(provider.confirm("Sure?", true).unwrap(), true);
        assert_eq!(provider.select("Again:", &["x", "y"]).unwrap(), 0);
        assert_eq!(
            provider
                .fuzzy_select("Search:", &["a".to_string(), "b".to_string()])
                .unwrap(),
            Some(0)
        );
    }
}
