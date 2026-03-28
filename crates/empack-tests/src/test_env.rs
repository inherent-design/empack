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

type HermeticCommandSession = CommandSession<
    LiveFileSystemProvider,
    MockNetworkProvider,
    LiveProcessProvider,
    LiveConfigProvider,
    empack_lib::application::session_mocks::MockInteractiveProvider,
>;

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

const MOCK_CALL_SEPARATOR: char = '\u{1f}';

/// Structured mock invocation captured from a hermetic executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockInvocation {
    pub executable: String,
    pub args: Vec<String>,
}

impl MockInvocation {
    pub fn render(&self) -> String {
        std::iter::once(self.executable.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn contains_args(&self, expected: &[&str]) -> bool {
        if expected.is_empty() {
            return true;
        }

        self.args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }
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

    /// Escape a string for use inside single-quoted shell strings.
    fn escape_shell_single_quote(s: &str) -> String {
        s.replace('\'', "'\\''")
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
                let escaped = Self::escape_shell_single_quote(error);
                format!("echo '{}' >&2\nexit 1", escaped)
            }
            MockBehavior::SucceedWithOutput { stdout, stderr } => {
                let mut code = String::new();

                // Special handling for packwiz init - create pack.toml and index.toml
                if name == "packwiz" && stdout.contains("Initialized") {
                    code.push_str(
                        r#"
# Create pack.toml and index.toml if 'init' command detected
if [[ "$1" == "init" ]]; then
  # Extract parameters from command line
  NAME="mock-pack"
  AUTHOR="Test Author"
  VERSION="1.0.0"
  MC_VERSION="1.21.1"
  MODLOADER="fabric"
  LOADER_VERSION="0.15.0"

  # Parse arguments (simple extraction)
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --name)
        NAME="$2"
        shift 2
        ;;
      --author)
        AUTHOR="$2"
        shift 2
        ;;
      --version)
        VERSION="$2"
        shift 2
        ;;
      --mc-version)
        MC_VERSION="$2"
        shift 2
        ;;
      --modloader)
        MODLOADER="$2"
        shift 2
        ;;
      --fabric-version|--neoforge-version|--forge-version|--quilt-version)
        LOADER_VERSION="$2"
        shift 2
        ;;
      *)
        shift
        ;;
    esac
  done

  # Create pack.toml
  cat > pack.toml <<PACKTOML
name = "$NAME"
author = "$AUTHOR"
version = "$VERSION"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "$MC_VERSION"
$MODLOADER = "$LOADER_VERSION"
PACKTOML

  # Create index.toml
  cat > index.toml <<'INDEXTOML'
hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
INDEXTOML
fi
"#,
                    );
                }

                if name == "packwiz" {
                    code.push_str(
                        r#"
# Create mock mod metadata when add commands are requested.
# Args may be prefixed with --pack-file <path>, so scan all args
# instead of assuming $1/$2 positions.
if [[ "$*" == *"mr add"* || "$*" == *"modrinth add"* ]]; then
  for ((i = 1; i <= $#; i++)); do
    if [[ "${!i}" == "--project-id" ]]; then
      next=$((i + 1))
      mod_id="${!next}"
    fi
    # Short form: mr add <id>
    if [[ "${!i}" == "mr" ]]; then
      next=$((i + 1))
      if [[ "${!next}" == "add" ]]; then
        id_idx=$((i + 2))
        # Only use positional id if no --project-id flag found
        : "${mod_id:=${!id_idx}}"
      fi
    fi
  done
  if [[ -n "$mod_id" ]]; then
    mkdir -p mods
    cat > "mods/$mod_id.pw.toml" <<MODRINTH
name = "$mod_id"
filename = "$mod_id.jar"
side = "both"
MODRINTH
  fi
fi

if [[ "$*" == *"cf add"* || "$*" == *"curseforge add"* ]]; then
  cf_mod=""
  for ((i = 1; i <= $#; i++)); do
    if [[ "${!i}" == "--addon-id" ]]; then
      next=$((i + 1))
      cf_mod="${!next}"
    fi
    # Short form: cf add <name>
    if [[ "${!i}" == "cf" ]]; then
      next=$((i + 1))
      if [[ "${!next}" == "add" ]]; then
        id_idx=$((i + 2))
        : "${cf_mod:=${!id_idx}}"
      fi
    fi
  done
  if [[ -n "$cf_mod" ]]; then
    mkdir -p mods
    cat > "mods/$cf_mod.pw.toml" <<CURSEFORGE
name = "$cf_mod"
filename = "$cf_mod.jar"
side = "both"
CURSEFORGE
  fi
fi
"#,
                    );
                }

                if name == "packwiz" && stdout.contains("Exported") {
                    code.push_str(
                        r#"
# Create a real zip mrpack artifact if 'mr export' is requested.
# Native archive extraction requires a valid zip file.
if [[ "$*" == *"mr export"* ]]; then
  OUTPUT_FILE=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      -o)
        OUTPUT_FILE="$2"
        shift 2
        ;;
      *)
        shift
        ;;
    esac
  done

  if [[ -n "$OUTPUT_FILE" ]]; then
    mkdir -p "$(dirname "$OUTPUT_FILE")"
    python3 -c "
import zipfile, os, sys
with zipfile.ZipFile(sys.argv[1], 'w') as z:
    z.writestr('overrides/config/generated.txt', 'override=true')
" "$OUTPUT_FILE"
  fi
fi
"#,
                    );
                }

                if name == "java" {
                    code.push_str(
                        r##"
JAR_FILE=""
INSTALL_DIR=""
IS_FABRIC_INSTALLER=false
IS_QUILT_INSTALLER=false
IS_NEOFORGE_INSTALLER=false
IS_FORGE_INSTALLER=false
SIDE="unknown"

# First pass: identify the jar file
for arg in "$@"; do
  if [[ "$arg" == *"fabric-installer"* && "$arg" == *.jar ]]; then
    IS_FABRIC_INSTALLER=true
    JAR_FILE="$arg"
  elif [[ "$arg" == *"quilt-installer"* && "$arg" == *.jar ]]; then
    IS_QUILT_INSTALLER=true
    JAR_FILE="$arg"
  elif [[ "$arg" == *"neoforge"*"installer"* && "$arg" == *.jar ]]; then
    IS_NEOFORGE_INSTALLER=true
    JAR_FILE="$arg"
  elif [[ "$arg" == *"forge"*"installer"* && "$arg" == *.jar ]]; then
    IS_FORGE_INSTALLER=true
    JAR_FILE="$arg"
  fi
done

if $IS_FABRIC_INSTALLER; then
  # Parse Fabric installer flags (single dash): -dir <path>
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -dir) INSTALL_DIR="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  if [[ -n "$INSTALL_DIR" ]]; then
    mkdir -p "$INSTALL_DIR/libraries"
    echo 'mock fabric launcher' > "$INSTALL_DIR/fabric-server-launch.jar"
    echo 'mock vanilla server' > "$INSTALL_DIR/server.jar"
    echo 'serverJar=server.jar' > "$INSTALL_DIR/fabric-server-launcher.properties"
  fi
elif $IS_QUILT_INSTALLER; then
  # Parse Quilt installer flags (double dash): --install-dir=<path>
  for arg in "$@"; do
    case "$arg" in
      --install-dir=*) INSTALL_DIR="${arg#--install-dir=}" ;;
    esac
  done
  if [[ -n "$INSTALL_DIR" ]]; then
    mkdir -p "$INSTALL_DIR/libraries"
    echo 'mock quilt launcher' > "$INSTALL_DIR/quilt-server-launch.jar"
    echo 'mock vanilla server' > "$INSTALL_DIR/server.jar"
  fi
elif $IS_NEOFORGE_INSTALLER || $IS_FORGE_INSTALLER; then
  # Parse NeoForge/Forge installer flags: --install-server or --installServer <path>
  for arg in "$@"; do
    if [[ "$arg" != -* && "$arg" != *.jar && -d "$arg" ]]; then
      INSTALL_DIR="$arg"
    fi
  done
  if [[ -n "$INSTALL_DIR" ]]; then
    mkdir -p "$INSTALL_DIR/libraries"
    printf '#!/bin/bash\n' > "$INSTALL_DIR/run.sh"
    printf '@echo off\n' > "$INSTALL_DIR/run.bat"
    printf '' > "$INSTALL_DIR/user_jvm_args.txt"
  fi
else
  # Default: packwiz installer behavior (for -s server/both)
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -s) SIDE="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  mkdir -p mods
  cat > "mods/${SIDE}-installed.txt" <<JAVAINSTALL
installed=$SIDE
JAVAINSTALL
fi
"##,
                    );
                }

                if !stdout.is_empty() {
                    let escaped = Self::escape_shell_single_quote(stdout);
                    code.push_str(&format!("echo '{}'\n", escaped));
                }
                if !stderr.is_empty() {
                    let escaped = Self::escape_shell_single_quote(stderr);
                    code.push_str(&format!("echo '{}' >&2\n", escaped));
                }
                code.push_str("exit 0");
                code
            }
            MockBehavior::Conditional { rules } => {
                let mut code = String::new();
                for rule in rules {
                    let pattern = rule.args_pattern.join(" ");
                    // Escape shell-sensitive characters in the pattern
                    let escaped = pattern
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('$', "\\$")
                        .replace('`', "\\`");
                    code.push_str(&format!("if [ \"$*\" = \"{}\" ]; then\n", escaped));
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
printf '%s' "{}" >> "{}"
for arg in "$@"; do
  printf '\x1f%s' "$arg" >> "{}"
done
printf '\n' >> "{}"

# Execute behavior
{}
"#,
            name, log_path_str, name, log_path_str, log_path_str, log_path_str, behavior_code
        );

        Ok(script)
    }

    /// Generate behavior code for conditional rules
    fn generate_behavior_code(&self, behavior: &MockBehavior) -> String {
        match behavior {
            MockBehavior::AlwaysSucceed => "exit 0".to_string(),
            MockBehavior::AlwaysFail { error } => {
                let escaped = Self::escape_shell_single_quote(error);
                format!("echo '{}' >&2; exit 1", escaped)
            }
            MockBehavior::SucceedWithOutput { stdout, stderr } => {
                let mut code = String::new();
                if !stdout.is_empty() {
                    let escaped = Self::escape_shell_single_quote(stdout);
                    code.push_str(&format!("echo '{}'; ", escaped));
                }
                if !stderr.is_empty() {
                    let escaped = Self::escape_shell_single_quote(stderr);
                    code.push_str(&format!("echo '{}' >&2; ", escaped));
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
        let expected_args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        Ok(self
            .get_mock_invocations(executable_name)?
            .iter()
            .any(|call| call.args == expected_args))
    }

    /// Get all structured invocations made to a mock executable.
    pub fn get_mock_invocations(&self, executable_name: &str) -> Result<Vec<MockInvocation>> {
        let log_content = self.get_mock_log(executable_name)?;
        let calls = log_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut fields = line.split(MOCK_CALL_SEPARATOR);
                let executable = fields.next().unwrap_or_default().to_string();
                let args = fields.map(str::to_string).collect();
                MockInvocation { executable, args }
            })
            .filter(|call| call.executable == executable_name)
            .collect();
        Ok(calls)
    }

    /// Assert that at least one logged call contains the expected argument sequence.
    pub fn assert_mock_call_contains_args(
        &self,
        executable_name: &str,
        args: &[&str],
    ) -> Result<()> {
        let calls = self.get_mock_invocations(executable_name)?;
        if calls.iter().any(|call| call.contains_args(args)) {
            return Ok(());
        }

        Err(anyhow::anyhow!(
            "Expected `{}` to be called with args {:?}, recorded calls: {:?}",
            executable_name,
            args,
            calls
        ))
    }

    /// Get all calls made to a mock executable
    pub fn get_mock_calls(&self, executable_name: &str) -> Result<Vec<String>> {
        Ok(self
            .get_mock_invocations(executable_name)?
            .into_iter()
            .map(|call| call.render())
            .collect())
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
    interactive_provider: Option<empack_lib::application::session_mocks::MockInteractiveProvider>,
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
            interactive_provider: None,
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

    /// Enable non-interactive mode (--yes flag)
    pub fn with_yes_flag(mut self) -> Self {
        self.app_config.yes = true;
        self
    }

    /// Enable dry-run mode (--dry-run flag)
    pub fn with_dry_run_flag(mut self) -> Self {
        self.app_config.dry_run = true;
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

    /// Allow the mock network provider to construct an inert HTTP client for
    /// paths that require a client handle before using mocked resolvers.
    pub fn with_mock_http_client(mut self) -> Self {
        self.network_provider.enable_http_client();
        self
    }

    /// Configure the interactive provider with custom responses
    pub fn with_interactive_provider(
        mut self,
        interactive_provider: empack_lib::application::session_mocks::MockInteractiveProvider,
    ) -> Self {
        self.interactive_provider = Some(interactive_provider);
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

    /// Pre-populate the packwiz JAR cache so builds skip real downloads.
    ///
    /// Writes mock `packwiz-installer-bootstrap.jar` and `packwiz-installer.jar`
    /// into the cache directory that `cache_root()` resolves to during tests.
    /// Must be called after the builder is configured but before `build()`,
    /// because `build()` sets `EMPACK_CACHE_DIR`.
    pub fn with_pre_cached_jars(self) -> Result<Self> {
        let jar_cache = self.test_env.root_path.join("cache").join("jars");
        std::fs::create_dir_all(&jar_cache)?;
        std::fs::write(
            jar_cache.join("packwiz-installer-bootstrap.jar"),
            "mock-bootstrap-jar",
        )?;
        std::fs::write(
            jar_cache.join("packwiz-installer.jar"),
            "mock-installer-jar",
        )?;
        Ok(self)
    }

    /// Build the hermetic session with all configured providers
    pub fn build(self) -> Result<(HermeticCommandSession, TestEnvironment)> {
        use empack_lib::application::session_mocks::MockInteractiveProvider;

        // Set up cross-platform cache directory for test isolation
        // This prevents tests from reading real cached API responses
        let cache_dir = self.test_env.root_path.join("cache");
        std::fs::create_dir_all(&cache_dir)?;

        // Set platform-appropriate cache environment variables to isolate tests
        // SAFETY: This is safe in test environments where we control execution
        // Tests run sequentially or in isolated processes, so no concurrent modification
        let path_env = self.test_env.get_path_env();
        unsafe {
            // Override empack's cache_root() to use the hermetic temp directory.
            // This works on all platforms (unlike XDG_CACHE_HOME which macOS ignores).
            std::env::set_var("EMPACK_CACHE_DIR", &cache_dir);

            // Unix-like systems use XDG_CACHE_HOME
            #[cfg(unix)]
            std::env::set_var("XDG_CACHE_HOME", &cache_dir);

            // Ensure all command paths, including direct std::process::Command
            // calls inside live providers, resolve to the hermetic mock tools.
            std::env::set_var("PATH", &path_env);
        }

        // Use provided interactive provider or create default one with yes_mode from config
        let interactive_provider = self
            .interactive_provider
            .unwrap_or_else(|| MockInteractiveProvider::new().with_yes_mode(self.app_config.yes));

        // Create session with coordinated mock providers
        let session = CommandSession::new_with_providers(
            LiveFileSystemProvider,
            self.network_provider,
            LiveProcessProvider::new(),
            LiveConfigProvider::new(self.app_config),
            interactive_provider,
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

    #[cfg(unix)]
    #[test]
    fn test_mock_executable_logging() {
        let mut env = TestEnvironment::new().expect("Failed to create test environment");

        env.add_mock_executable("test-cmd", MockBehavior::AlwaysSucceed)
            .expect("Failed to add mock executable");

        let output = std::process::Command::new(env.bin_path.join("test-cmd"))
            .args(["alpha", "beta"])
            .current_dir(&env.work_path)
            .env("PATH", env.get_path_env())
            .output()
            .expect("Failed to execute mock command");

        assert!(output.status.success());
        assert!(
            env.verify_mock_call("test-cmd", &["alpha", "beta"])
                .expect("Failed to verify mock call")
        );
        assert_eq!(
            env.get_mock_calls("test-cmd")
                .expect("Failed to read mock calls"),
            vec!["test-cmd alpha beta".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_mock_executable_logging_preserves_argument_boundaries() {
        let mut env = TestEnvironment::new().expect("Failed to create test environment");

        env.add_mock_executable("test-cmd", MockBehavior::AlwaysSucceed)
            .expect("Failed to add mock executable");

        let output = std::process::Command::new(env.bin_path.join("test-cmd"))
            .args(["alpha", "two words", "gamma"])
            .current_dir(&env.work_path)
            .env("PATH", env.get_path_env())
            .output()
            .expect("Failed to execute mock command");

        assert!(output.status.success());
        assert_eq!(
            env.get_mock_invocations("test-cmd")
                .expect("Failed to read mock invocations"),
            vec![MockInvocation {
                executable: "test-cmd".to_string(),
                args: vec![
                    "alpha".to_string(),
                    "two words".to_string(),
                    "gamma".to_string(),
                ],
            }]
        );
        env.assert_mock_call_contains_args("test-cmd", &["two words", "gamma"])
            .expect("Failed to assert mock call args");
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
