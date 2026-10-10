//! Native project and archive fixtures for executable tests.

pub mod restricted;

use anyhow::Result;
use std::path::{Path, PathBuf};

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
    pub empack_lock: PathBuf,
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
        use empack_core::{distribution::Recipe, model::*};
        use empack_lib::engine::{
            documents::DocumentCodec,
            initialize::{InitializeCandidate, default_templates},
        };
        use std::collections::BTreeMap;
        let loader = match self.loader.as_str() {
            "none" | "vanilla" => LoaderKind::Vanilla,
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            _ => anyhow::bail!("Unknown fixture loader"),
        };
        let runtime = RuntimeResolution {
            minecraft: GameVersion::parse(&self.minecraft_version)?,
            loader,
            loader_version: (loader != LoaderKind::Vanilla)
                .then(|| LoaderVersion::parse(&self.loader_version))
                .transpose()?,
        };
        let candidate = InitializeCandidate::new(
            ProjectIntent {
                source_excludes: Vec::new(),
                metadata: PackMetadata {
                    name: self.pack_name.clone(),
                    author: Some(self.author.clone()),
                    version: self.version.clone(),
                    description: None,
                },
                runtime: RuntimeIntent {
                    minecraft: runtime.minecraft.clone(),
                    acceptable_versions: vec![],
                    loader,
                    loader_version: runtime.loader_version.clone(),
                },
                roots: BTreeMap::new(),
                layout: BTreeMap::new(),
                extensions: BTreeMap::new(),
                distribution: DistributionIntent {
                    native: None,
                    recipes: NonEmpty::new(vec![Recipe::MODRINTH])?,
                    archive: DistributionArchive::Zip,
                },
            },
            runtime,
            default_templates(),
        )?;
        let pack_dir = workdir.join("pack");
        std::fs::create_dir_all(&pack_dir)?;
        let empack_yml = workdir.join("empack.yml");
        let empack_lock = workdir.join("empack.lock");
        std::fs::write(
            &empack_yml,
            DocumentCodec.encode_intent(candidate.project().intent())?,
        )?;
        std::fs::write(
            &empack_lock,
            DocumentCodec.encode_lock(candidate.project())?,
        )?;
        for (name, bytes) in candidate.templates() {
            let target = workdir.join("templates").join(name.as_str());
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::write(target, bytes)?;
        }
        Ok(WorkflowProjectPaths {
            root: workdir.into(),
            empack_yml,
            empack_lock,
            pack_dir,
            dist_dir: workdir.join("dist"),
        })
    }

    pub fn dist_dir(&self, workdir: &Path) -> PathBuf {
        workdir.join("dist")
    }

    pub fn artifact_file_name(&self, artifact: WorkflowArtifact) -> String {
        match artifact {
            WorkflowArtifact::Mrpack => format!("{}-{}.mrpack", self.pack_name, self.version),
            WorkflowArtifact::Client => {
                format!("{}-{}-client.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::Server => {
                format!("{}-{}-server.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::ClientFull => {
                format!("{}-{}-client-full.zip", self.pack_name, self.version)
            }
            WorkflowArtifact::ServerFull => {
                format!("{}-{}-server-full.zip", self.pack_name, self.version)
            }
        }
    }

    pub fn artifact_path(&self, workdir: &Path, artifact: WorkflowArtifact) -> PathBuf {
        self.dist_dir(workdir)
            .join(self.artifact_file_name(artifact))
    }
}

/// Write explicit fixture members without exercising the application's archive implementation.
pub fn write_zip(path: &Path, members: &[(&str, &[u8])]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut archive = zip::ZipWriter::new(std::fs::File::create(path)?);
    for (name, bytes) in members {
        archive.start_file(*name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(bytes)?;
    }
    archive.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_project_fixture_writes_expected_layout() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let fixture = WorkflowProjectFixture::new("workflow-fixture-pack");

        let paths = fixture.write_to(temp_dir.path()).unwrap();

        assert!(paths.empack_yml.exists());
        assert!(paths.empack_lock.exists());
        let intent = empack_lib::engine::documents::DocumentCodec
            .decode_intent(&std::fs::read(paths.empack_yml).unwrap(), "fixture")
            .unwrap();
        let project = empack_lib::engine::documents::DocumentCodec
            .decode_lock(
                &std::fs::read(paths.empack_lock).unwrap(),
                &intent,
                "fixture",
            )
            .unwrap();
        assert_eq!(project.intent().metadata.name, "workflow-fixture-pack");
        assert_eq!(project.lock().runtime.minecraft.as_str(), "1.21.1");
    }

    #[test]
    fn test_workflow_project_fixture_artifact_paths_are_deterministic() {
        let fixture = WorkflowProjectFixture::new("workflow-fixture-pack");
        let root = PathBuf::from("/tmp/workflow-fixture-pack");

        assert_eq!(
            fixture.artifact_path(&root, WorkflowArtifact::Mrpack),
            root.join("dist").join("workflow-fixture-pack-1.0.0.mrpack")
        );
        assert_eq!(
            fixture.artifact_path(&root, WorkflowArtifact::ServerFull),
            root.join("dist")
                .join("workflow-fixture-pack-1.0.0-server-full.zip")
        );
    }
}
