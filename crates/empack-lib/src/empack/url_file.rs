//! Provider-free files retain their source integrity and environment contracts.
use super::config::DependencyStatus;
use super::content::{SideEnv, SideRequirement};
use crate::application::session::{FileSystemProvider, Session};
use crate::primitives::ProjectType;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlDependencyRecord {
    pub status: DependencyStatus,
    pub title: String,
    #[serde(rename = "type")]
    pub project_type: ProjectType,
    /// Destination relative to the pack root, independent of URL basenames.
    pub destination: String,
    pub downloads: Vec<String>,
    pub hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub env: SideEnv,
}

/// Packwiz supports one requirement for every supported side.
/// Mixed required/optional sides have no lossless representation.
pub fn requirements(env: &SideEnv) -> Result<(&'static str, bool)> {
    use SideRequirement::*;
    let client = if env.client == Unknown {
        Required
    } else {
        env.client.clone()
    };
    let server = if env.server == Unknown {
        Required
    } else {
        env.server.clone()
    };
    match (client, server) {
        (Unsupported, Unsupported) => anyhow::bail!("File is unsupported on both environments"),
        (Required, Optional) | (Optional, Required) => {
            anyhow::bail!("Mixed required/optional environments cannot be represented by packwiz")
        }
        (Optional, Unsupported) => Ok(("client", true)),
        (Unsupported, Optional) => Ok(("server", true)),
        (Required, Unsupported) => Ok(("client", false)),
        (Unsupported, Required) => Ok(("server", false)),
        (Optional, Optional) => Ok(("both", true)),
        _ => Ok(("both", false)),
    }
}

impl UrlDependencyRecord {
    pub fn metadata_relative_path(&self) -> PathBuf {
        let path = Path::new(&self.destination);
        path.with_file_name(format!(
            "{}.pw.toml",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.status == DependencyStatus::Url,
            "URL record must declare status: url"
        );
        ensure!(
            !self.destination.contains('\\'),
            "URL destinations must use forward slashes"
        );
        let root = Path::new("pack");
        super::paths::validate_relative_destination(root, &root.join(&self.destination))?;
        for component in Path::new(&self.destination).components() {
            super::paths::validate_filename(
                component
                    .as_os_str()
                    .to_str()
                    .context("Non-UTF8 URL file path")?,
            )?;
        }
        ensure!(
            !self.destination.ends_with(".pw.toml"),
            "URL content cannot replace packwiz metadata"
        );
        ensure!(
            !self.downloads.is_empty(),
            "URL file requires download alternatives"
        );
        for url in &self.downloads {
            let parsed = reqwest::Url::parse(url)?;
            ensure!(
                matches!(parsed.scheme(), "http" | "https"),
                "URL file requires HTTP or HTTPS"
            );
        }
        let mut supported = false;
        for (algorithm, digest) in &self.hashes {
            let length = match algorithm.as_str() {
                "sha1" => 40,
                "sha256" => 64,
                "sha512" => 128,
                _ => continue,
            };
            ensure!(
                digest.len() == length && digest.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid {algorithm} digest"
            );
            supported = true;
        }
        ensure!(
            supported,
            "URL file requires a SHA-1, SHA-256 or SHA-512 source digest"
        );
        requirements(&self.env)?;
        Ok(())
    }

    pub fn metadata(&self, url: &str) -> Result<toml::Value> {
        self.validate()?;
        ensure!(
            self.downloads.iter().any(|alternative| alternative == url),
            "URL is not a declared alternative"
        );
        let (side, optional) = requirements(&self.env)?;
        let algorithm = ["sha512", "sha256", "sha1"]
            .into_iter()
            .find(|key| self.hashes.contains_key(*key))
            .unwrap();
        let mut value = toml::Table::new();
        value.insert("name".into(), self.title.clone().into());
        value.insert(
            "filename".into(),
            Path::new(&self.destination)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
        );
        value.insert("side".into(), side.into());
        value.insert(
            "download".into(),
            toml::Value::Table(toml::Table::from_iter([
                ("url".into(), url.into()),
                ("hash-format".into(), algorithm.into()),
                ("hash".into(), self.hashes[algorithm].to_lowercase().into()),
            ])),
        );
        if optional {
            value.insert(
                "option".into(),
                toml::Value::Table(toml::Table::from_iter([
                    ("optional".into(), true.into()),
                    ("default".into(), true.into()),
                ])),
            );
        }
        Ok(value.into())
    }

    pub fn metadata_path(&self, fs: &dyn FileSystemProvider, workdir: &Path) -> Result<PathBuf> {
        self.validate()?;
        let pack = workdir.join("pack");
        let destination = pack.join(&self.destination);
        fs.validate_output_path(&pack, &destination)?;
        let metadata = pack.join(self.metadata_relative_path());
        fs.validate_output_path(&pack, &metadata)?;
        ensure!(
            !fs.exists(&metadata) || fs.is_regular_file(&metadata),
            "URL metadata must be a regular file"
        );
        Ok(metadata)
    }

    pub fn is_installed(&self, fs: &dyn FileSystemProvider, workdir: &Path) -> Result<bool> {
        let path = self.metadata_path(fs, workdir)?;
        if !fs.exists(&path) {
            return Ok(false);
        }
        let actual: toml::Value = toml::from_str(&fs.read_to_string(&path)?)?;
        let url = actual
            .get("download")
            .and_then(|v| v.get("url"))
            .and_then(toml::Value::as_str)
            .context("URL metadata has no download URL")?;
        ensure!(
            actual == self.metadata(url)?,
            "URL metadata differs from declared intent: {}",
            path.display()
        );
        Ok(true)
    }

    pub async fn verify_download(&self, session: &dyn Session) -> Result<String> {
        self.validate()?;
        let client = session.network().http_client()?;
        let mut failures = Vec::new();
        for url in &self.downloads {
            session.process().check_cancelled()?;
            let result = async {
                let limit = self
                    .size
                    .unwrap_or(super::import::MAX_IMPORT_ARCHIVE_BYTES)
                    .min(super::import::MAX_IMPORT_ARCHIVE_BYTES);
                let mut staged =
                    crate::networking::download::acquire(&client, session.process(), url, limit)
                        .await?;
                if let Some(size) = self.size {
                    ensure!(
                        staged.as_file().metadata()?.len() == size,
                        "Source file size mismatch"
                    );
                }
                for (algorithm, expected) in &self.hashes {
                    staged.as_file_mut().rewind()?;
                    let actual = match algorithm.as_str() {
                        "sha1" => digest_reader::<sha1::Sha1>(staged.as_file_mut())?,
                        "sha256" => digest_reader::<sha2::Sha256>(staged.as_file_mut())?,
                        "sha512" => digest_reader::<sha2::Sha512>(staged.as_file_mut())?,
                        _ => continue,
                    };
                    ensure!(
                        actual.eq_ignore_ascii_case(expected),
                        "Source {algorithm} digest mismatch for {}",
                        self.destination
                    );
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => return Ok(url.clone()),
                Err(error) => {
                    session.process().check_cancelled()?;
                    failures.push(format!("{error:#}"));
                }
            }
        }
        anyhow::bail!(
            "No verified download for '{}': {}",
            self.destination,
            failures.join("; ")
        )
    }

    pub fn publish(
        &self,
        fs: &dyn FileSystemProvider,
        workdir: &Path,
        verified_url: &str,
    ) -> Result<()> {
        let path = self.metadata_path(fs, workdir)?;
        if fs.exists(&path) {
            ensure!(self.is_installed(fs, workdir)?, "URL metadata collision");
            return Ok(());
        }
        fs.create_dir_all(path.parent().unwrap())?;
        fs.write_atomic(&path, &toml::to_string(&self.metadata(verified_url)?)?)
    }
}

fn digest_reader<D: Digest + Default>(reader: &mut dyn Read) -> Result<String> {
    let mut hasher = D::default();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Preserve backend download/update fields while setting physical-side requirements.
pub fn apply_requirements(metadata: &mut toml::Value, env: &SideEnv) -> Result<()> {
    let (side, optional) = requirements(env)?;
    let table = metadata
        .as_table_mut()
        .context("Invalid installed metadata")?;
    table.insert("side".into(), side.into());
    if optional {
        let options = table
            .entry("option")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .context("Invalid optional-file metadata")?;
        options.insert("optional".into(), true.into());
        options.entry("default").or_insert(true.into());
    } else if let Some(options) = table.get_mut("option").and_then(toml::Value::as_table_mut) {
        options.insert("optional".into(), false.into());
    }
    Ok(())
}

/// Packwiz can return success while omitting failed downloads. Require declared URL entries.
pub fn verify_export(
    fs: &dyn FileSystemProvider,
    archive: &Path,
    records: &[&UrlDependencyRecord],
) -> Result<()> {
    let mut zip = zip::ZipArchive::new(fs.open_reader(archive)?)?;
    let entry = zip.by_name("modrinth.index.json")?;
    ensure!(
        entry.size() <= 16 * 1024 * 1024,
        "Export manifest exceeds size limit"
    );
    let manifest: serde_json::Value = serde_json::from_reader(entry.take(16 * 1024 * 1024 + 1))?;
    let files = manifest["files"]
        .as_array()
        .context("Export has no file manifest")?;
    for record in records {
        let matches: Vec<_> = files
            .iter()
            .filter(|entry| entry["path"].as_str() == Some(&record.destination))
            .collect();
        ensure!(
            matches.len() == 1,
            "Export omitted or duplicated declared URL file: {}",
            record.destination
        );
        let entry = matches[0];
        let (side, optional) = requirements(&record.env)?;
        let required = if optional { "optional" } else { "required" };
        ensure!(
            entry["env"]["client"].as_str()
                == Some(if side == "server" {
                    "unsupported"
                } else {
                    required
                })
                && entry["env"]["server"].as_str()
                    == Some(if side == "client" {
                        "unsupported"
                    } else {
                        required
                    }),
            "Export changed environment requirements for {}",
            record.destination
        );
        if let Some(size) = record.size {
            ensure!(
                entry["fileSize"].as_u64() == Some(size),
                "Export changed declared file size"
            );
        }
        for (algorithm, digest) in &record.hashes {
            if let Some(exported) = entry["hashes"][algorithm].as_str() {
                ensure!(
                    exported.eq_ignore_ascii_case(digest),
                    "Export changed source digest for {}",
                    record.destination
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use crate::application::session_mocks::MockCommandSession;

    fn record(url: String) -> UrlDependencyRecord {
        UrlDependencyRecord {
            status: DependencyStatus::Url,
            title: "Client resources".into(),
            project_type: ProjectType::ResourcePack,
            destination: "resourcepacks/declared.zip".into(),
            downloads: vec![url],
            hashes: BTreeMap::from([(
                "sha256".into(),
                "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5".into(),
            )]),
            size: Some(7),
            env: SideEnv {
                client: SideRequirement::Optional,
                server: SideRequirement::Unsupported,
            },
        }
    }

    #[tokio::test]
    async fn verifies_alternatives_without_replacing_source_digests() {
        let mut server = mockito::Server::new_async().await;
        let _wrong = server
            .mock("GET", "/wrong.bin")
            .with_body("changed")
            .create_async()
            .await;
        let _right = server
            .mock("GET", "/right.bin")
            .with_body("payload")
            .create_async()
            .await;
        let mut record = record(format!("{}/wrong.bin", server.url()));
        let session = MockCommandSession::new();
        let error = record.verify_download(&session).await.unwrap_err();
        assert!(error.to_string().contains("digest mismatch"));
        record.downloads.push(format!("{}/right.bin", server.url()));
        let url = record.verify_download(&session).await.unwrap();
        let root = tempfile::tempdir().unwrap();
        record
            .publish(&LiveFileSystemProvider, root.path(), &url)
            .unwrap();
        assert!(
            record
                .is_installed(&LiveFileSystemProvider, root.path())
                .unwrap()
        );
        let metadata = record.metadata(&url).unwrap();
        assert_eq!(metadata["filename"].as_str(), Some("declared.zip"));
        assert_eq!(metadata["side"].as_str(), Some("client"));
        assert_eq!(metadata["option"]["optional"].as_bool(), Some(true));
        let manager = LiveFileSystemProvider.config_manager(root.path().to_path_buf());
        manager
            .add_dependency_entry(
                "resources",
                super::super::config::DependencyEntry::Url(record.clone()),
            )
            .unwrap();
        assert_eq!(
            manager.find_dependency("resources").unwrap().unwrap().1,
            super::super::config::DependencyEntry::Url(record)
        );
    }

    #[test]
    fn rejects_lossy_requirements_and_untrusted_paths() {
        let mut record = record("https://example.com/file.zip".into());
        record.env.server = SideRequirement::Required;
        assert!(
            record
                .validate()
                .unwrap_err()
                .to_string()
                .contains("Mixed required/optional")
        );
        record.env.server = SideRequirement::Unsupported;
        for path in [
            "../outside",
            "/outside",
            "mods/../outside",
            "mods/bad\\file",
            "mods/x.pw.toml",
        ] {
            record.destination = path.into();
            assert!(record.validate().is_err(), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_destination_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("pack")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("pack/resourcepacks")).unwrap();
        let record = record("https://example.com/file.zip".into());
        assert!(
            record
                .publish(&LiveFileSystemProvider, root.path(), &record.downloads[0])
                .is_err()
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
