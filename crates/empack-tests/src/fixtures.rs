//! Fixture infrastructure for E2E tests
//!
//! This module provides utilities for loading VCR cassettes containing
//! real API response fixtures captured from Modrinth and CurseForge APIs.

use anyhow::Result;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// VCR cassette structure matching our recorded HTTP interactions
#[derive(Debug, Deserialize)]
pub struct VcrCassette {
    pub name: String,
    pub request: VcrRequest,
    pub response: VcrResponse,
}

#[derive(Debug, Deserialize)]
pub struct VcrRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub query: Value,
    #[serde(default)]
    pub headers: Value,
}

#[derive(Debug, Deserialize)]
pub struct VcrResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Value,
    pub body: Value,
}

/// Resolve a cassette path relative to the empack-tests fixture root.
pub fn cassette_path(relative: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("cassettes")
        .join(relative.as_ref())
}

/// Small workflow project fixture for build/clean/lifecycle tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProjectFixture {
    pub pack_name: String,
    pub author: String,
    pub version: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
}

/// Common, typed paths for a workflow project under test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProjectPaths {
    pub root: PathBuf,
    pub empack_yml: PathBuf,
    pub pack_dir: PathBuf,
    pub pack_toml: PathBuf,
    pub index_toml: PathBuf,
    pub dist_dir: PathBuf,
}

/// Canonical workflow artifacts created under `dist/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowArtifact {
    Mrpack,
    Client,
    Server,
    ClientFull,
    ServerFull,
}

impl WorkflowProjectFixture {
    pub fn new(pack_name: impl Into<String>) -> Self {
        Self {
            pack_name: pack_name.into(),
            author: "Workflow Test".to_string(),
            version: "1.0.0".to_string(),
            minecraft_version: "1.21.1".to_string(),
            loader: "fabric".to_string(),
            loader_version: "0.15.0".to_string(),
        }
    }

    pub fn write_to(&self, workdir: &Path) -> Result<WorkflowProjectPaths> {
        let pack_dir = workdir.join("pack");
        std::fs::create_dir_all(&pack_dir)?;

        let empack_yml = workdir.join("empack.yml");
        let pack_toml = pack_dir.join("pack.toml");
        let index_toml = pack_dir.join("index.toml");
        let dist_dir = workdir.join("dist");

        std::fs::write(
            &empack_yml,
            format!(
                r#"empack:
  dependencies: {{}}
  minecraft_version: "{}"
  loader: {}
  name: "{}"
  author: "{}"
  version: "{}"
"#,
                self.minecraft_version, self.loader, self.pack_name, self.author, self.version
            ),
        )?;

        std::fs::write(
            &pack_toml,
            format!(
                r#"name = "{}"
author = "{}"
version = "{}"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{}"
{} = "{}"
"#,
                self.pack_name,
                self.author,
                self.version,
                self.minecraft_version,
                self.loader,
                self.loader_version
            ),
        )?;

        std::fs::write(
            &index_toml,
            r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#,
        )?;

        Ok(WorkflowProjectPaths {
            root: workdir.to_path_buf(),
            empack_yml,
            pack_dir,
            pack_toml,
            index_toml,
            dist_dir,
        })
    }

    pub fn dist_dir(&self, workdir: &Path) -> PathBuf {
        workdir.join("dist")
    }

    pub fn artifact_file_name(&self, artifact: WorkflowArtifact) -> String {
        match artifact {
            WorkflowArtifact::Mrpack => format!("{}-v{}.mrpack", self.pack_name, self.version),
            WorkflowArtifact::Client => {
                format!("{}-v{}-client.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::Server => {
                format!("{}-v{}-server.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::ClientFull => {
                format!("{}-v{}-client-full.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::ServerFull => {
                format!("{}-v{}-server-full.zip", self.pack_name, self.version)
            }
        }
    }

    pub fn artifact_path(&self, workdir: &Path, artifact: WorkflowArtifact) -> PathBuf {
        self.dist_dir(workdir)
            .join(self.artifact_file_name(artifact))
    }
}

/// Load a VCR cassette and extract the response body as a typed value
pub fn load_vcr_response<T>(cassette_path: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let cassette_content = std::fs::read_to_string(cassette_path)
        .map_err(|e| anyhow::anyhow!("Failed to load VCR cassette '{}': {}", cassette_path, e))?;

    let vcr: VcrCassette = serde_json::from_str(&cassette_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse VCR cassette '{}': {}", cassette_path, e))?;

    serde_json::from_value(vcr.response.body).map_err(|e| {
        anyhow::anyhow!(
            "Failed to deserialize response body from '{}': {}",
            cassette_path,
            e
        )
    })
}

/// Load a VCR cassette's response body as raw JSON string (for mockito)
pub fn load_vcr_body_string(cassette_path: &str) -> Result<String> {
    let cassette_content = std::fs::read_to_string(cassette_path)
        .map_err(|e| anyhow::anyhow!("Failed to load VCR cassette '{}': {}", cassette_path, e))?;

    let vcr: VcrCassette = serde_json::from_str(&cassette_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse VCR cassette '{}': {}", cassette_path, e))?;

    serde_json::to_string(&vcr.response.body).map_err(|e| {
        anyhow::anyhow!(
            "Failed to serialize response body from '{}': {}",
            cassette_path,
            e
        )
    })
}

/// Get the base URL for mockito server
#[cfg(test)]
pub fn mockito_url(server: &mockito::Server) -> String {
    server.url()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_vcr_cassette() {
        let cassette_path = cassette_path("modrinth/search_sodium.json");
        let json: Value = load_vcr_response(cassette_path.to_str().unwrap()).unwrap();
        assert!(json["hits"].is_array());
        assert!(!json["hits"].as_array().unwrap().is_empty());

        // Verify it contains sodium project data
        let hits = json["hits"].as_array().unwrap();
        let first_hit = &hits[0];
        assert_eq!(first_hit["project_id"].as_str().unwrap(), "AANobbMI");
        assert_eq!(first_hit["slug"].as_str().unwrap(), "sodium");
    }

    #[test]
    fn test_load_vcr_body_string() {
        let cassette_path = cassette_path("modrinth/search_sodium.json");
        let body_str = load_vcr_body_string(cassette_path.to_str().unwrap()).unwrap();
        assert!(body_str.contains("sodium"));
        assert!(body_str.contains("AANobbMI"));
    }

    #[test]
    fn test_workflow_project_fixture_writes_expected_layout() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let fixture = WorkflowProjectFixture::new("workflow-fixture-pack");

        let paths = fixture.write_to(temp_dir.path()).unwrap();

        assert!(paths.empack_yml.exists());
        assert!(paths.pack_toml.exists());
        assert!(paths.index_toml.exists());
        assert!(
            std::fs::read_to_string(&paths.pack_toml)
                .unwrap()
                .contains("name = \"workflow-fixture-pack\"")
        );
    }

    #[test]
    fn test_workflow_project_fixture_artifact_paths_are_deterministic() {
        let fixture = WorkflowProjectFixture::new("workflow-fixture-pack");
        let root = PathBuf::from("/tmp/workflow-fixture-pack");

        assert_eq!(
            fixture.artifact_path(&root, WorkflowArtifact::Mrpack),
            root.join("dist")
                .join("workflow-fixture-pack-v1.0.0.mrpack")
        );
        assert_eq!(
            fixture.artifact_path(&root, WorkflowArtifact::ServerFull),
            root.join("dist")
                .join("workflow-fixture-pack-v1.0.0-server-full.zip")
        );
    }
}
