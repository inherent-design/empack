//! Provider-free files retain their source integrity and environment contracts.
use super::config::DependencyStatus;
use super::content::SideEnv;
use crate::application::session::{FileSystemProvider, Session};
use crate::primitives::ProjectType;
use anyhow::{Context, Result, ensure};
use empack_core::digest::{
    ContentId, DigestAlgorithm, DigestSet, ExpectedDigest, IntegrityEvidence,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::io::Read;
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
    use empack_core::requirements::{ChoiceKey, Environments, OptionalChoice};
    // This DTO has no explicit choice field. Its imported per-file choice is enabled
    // by default. The new codec supplies real choice identities and metadata.
    let semantic = env.to_requirements(OptionalChoice {
        key: ChoiceKey::parse("imported-file")?,
        default_enabled: true,
        description: None,
    });
    let uniform = semantic.uniform()?;
    let side = match uniform.environments {
        Environments::Client => "client",
        Environments::Server => "server",
        Environments::Both => "both",
    };
    Ok((side, uniform.choice.is_some()))
}

struct VerifiedDownload {
    url: String,
    hashes: BTreeMap<String, String>,
    size: u64,
    evidence: IntegrityEvidence,
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
        empack_core::path::InstallDestination::parse(&self.destination)?;
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
        self.expected_digests()?;
        requirements(&self.env)?;
        Ok(())
    }

    fn expected_digests(&self) -> Result<DigestSet> {
        Ok(DigestSet::parse(self.hashes.iter().map(
            |(algorithm, digest)| (algorithm.as_str(), digest.as_str()),
        ))?)
    }

    pub fn metadata(&self, url: &str) -> Result<toml::Value> {
        self.validate()?;
        ensure!(
            self.downloads.iter().any(|alternative| alternative == url),
            "URL is not a declared alternative"
        );
        let (side, optional) = requirements(&self.env)?;
        let algorithm = ["sha512", "sha256", "sha1", "md5"]
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
        Ok(self.verify_download_details(session).await?.url)
    }

    /// Verify project cache bytes or acquire a declared alternative before repairing metadata.
    pub async fn verify_for_project(
        &self,
        session: &dyn Session,
        workdir: &Path,
    ) -> Result<String> {
        self.validate()?;
        Ok(self
            .verify_available(session, workdir, &self.downloads[0])
            .await?
            .url)
    }

    async fn verify_available(
        &self,
        session: &dyn Session,
        workdir: &Path,
        url: &str,
    ) -> Result<VerifiedDownload> {
        self.validate()?;
        let cache = crate::platform::cache::packwiz_download_cache_dir(workdir)?;
        session.process().check_cancelled()?;
        match self.verify_cached(session.filesystem(), &cache, url)? {
            Some(verified) => {
                report_integrity(session, &self.destination, &verified.evidence);
                Ok(verified)
            }
            None => self.verify_download_details(session).await,
        }
    }

    async fn verify_download_details(&self, session: &dyn Session) -> Result<VerifiedDownload> {
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
                self.verify_reader(staged.as_file_mut(), url)
            }
            .await;
            match result {
                Ok(verified) => {
                    report_integrity(session, &self.destination, &verified.evidence);
                    return Ok(verified);
                }
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

    fn verify_reader(
        &self,
        reader: &mut dyn crate::application::session::ReadSeek,
        url: &str,
    ) -> Result<VerifiedDownload> {
        let size = reader.seek(std::io::SeekFrom::End(0))?;
        ensure!(
            size <= super::import::MAX_IMPORT_ARCHIVE_BYTES,
            "Source file exceeds size limit"
        );
        if let Some(expected) = self.size {
            ensure!(size == expected, "Source file size mismatch");
        }
        let expected = self.expected_digests()?;
        reader.rewind()?;
        let mut sha1 = sha1::Sha1::new();
        let mut sha256 = sha2::Sha256::new();
        let mut sha512 = sha2::Sha512::new();
        let mut md5 = self.hashes.contains_key("md5").then(md5::Md5::new);
        let mut buffer = [0; 64 * 1024];
        let mut observed_size = 0u64;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            observed_size += count as u64;
            ensure!(
                observed_size <= size,
                "Source file grew during verification"
            );
            sha1.update(&buffer[..count]);
            sha256.update(&buffer[..count]);
            sha512.update(&buffer[..count]);
            if let Some(md5) = &mut md5 {
                md5.update(&buffer[..count]);
            }
        }
        ensure!(
            observed_size == size,
            "Source file changed size during verification"
        );
        let address: [u8; 32] = sha256.finalize().into();
        let mut actual = vec![
            ExpectedDigest::Sha1(sha1.finalize().into()),
            ExpectedDigest::Sha256(address),
            ExpectedDigest::Sha512(sha512.finalize().into()),
        ];
        if let Some(md5) = md5 {
            actual.push(ExpectedDigest::Md5(md5.finalize().into()));
        }
        expected
            .check(&actual)
            .with_context(|| format!("Source integrity failed for {}", self.destination))?;
        let hashes = actual
            .iter()
            .map(|value| (value.algorithm().name().to_string(), value.hex()))
            .collect();
        Ok(VerifiedDownload {
            url: url.into(),
            hashes,
            size,
            evidence: IntegrityEvidence::MatchedExpected {
                expected,
                actual: ContentId::from_sha256(address),
            },
        })
    }

    /// Read the pinned backend's v2 cache as a lookup hint, never as integrity evidence.
    fn verify_cached(
        &self,
        fs: &dyn FileSystemProvider,
        cache: &Path,
        url: &str,
    ) -> Result<Option<VerifiedDownload>> {
        let index_path = cache.join("index.json");
        fs.validate_output_path(cache, &index_path)?;
        if !fs.exists(&index_path) {
            return Ok(None);
        }
        ensure!(
            fs.is_regular_file(&index_path),
            "Cache index must be a regular file"
        );
        let reader = fs.open_reader(&index_path)?;
        let mut bytes = Vec::new();
        reader.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "Cache index exceeds size limit"
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct CacheIndex {
            version: u32,
            hashes: BTreeMap<String, Vec<String>>,
        }
        let index: CacheIndex = serde_json::from_slice(&bytes)?;
        ensure!(index.version == 2, "Unsupported packwiz cache version");
        // Match the same strongest digest selected for backend metadata.
        let algorithm = ["sha512", "sha256", "sha1", "md5"]
            .into_iter()
            .find(|key| self.hashes.contains_key(*key))
            .context("Missing source digest")?;
        let Some(position) = index.hashes.get(algorithm).and_then(|hashes| {
            hashes
                .iter()
                .position(|hash| hash.eq_ignore_ascii_case(&self.hashes[algorithm]))
        }) else {
            return Ok(None);
        };
        let digest = index
            .hashes
            .get("sha256")
            .and_then(|hashes| hashes.get(position))
            .context("Cache entry has no content address")?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid cache content address"
        );
        let path = cache.join(&digest[..2]).join(&digest[2..]);
        fs.validate_output_path(cache, &path)?;
        if !fs.exists(&path) {
            return Ok(None);
        }
        ensure!(
            fs.is_regular_file(&path),
            "Cached content must be a regular file"
        );
        let verified = self
            .verify_reader(&mut *fs.open_reader(&path)?, url)
            .with_context(|| {
                format!(
                    "Invalid cached content; remove {} and rebuild",
                    path.display()
                )
            })?;
        ensure!(
            verified.hashes["sha256"].eq_ignore_ascii_case(digest),
            "Cache content address mismatch; remove {} and rebuild",
            path.display()
        );
        Ok(Some(verified))
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
            if toml::from_str::<toml::Value>(&fs.read_to_string(&path)?)?
                == self.metadata(verified_url)?
            {
                return Ok(());
            }
        }
        fs.create_dir_all(path.parent().unwrap())?;
        fs.write_atomic(&path, &toml::to_string(&self.metadata(verified_url)?)?)
    }
}

/// Verify cached bytes or download alternatives before switching any installed URL.
/// Returned hashes are observations of bytes already checked against source intent.
pub async fn prepare_build(
    session: &dyn Session,
    workdir: &Path,
) -> Result<Vec<UrlDependencyRecord>> {
    let config = session
        .filesystem()
        .config_manager(workdir.to_path_buf())
        .load_empack_config()?;
    let mut prepared = Vec::new();
    for entry in config.empack.dependencies.values() {
        if let super::config::DependencyEntry::Url(record) = entry {
            ensure!(
                record.is_installed(session.filesystem(), workdir)?,
                "Missing URL metadata; run sync"
            );
            let fs = session.filesystem();
            let metadata: toml::Value =
                toml::from_str(&fs.read_to_string(&record.metadata_path(fs, workdir)?)?)?;
            let url = metadata["download"]["url"]
                .as_str()
                .context("Missing installed URL")?;
            let verified = record.verify_available(session, workdir, url).await?;
            prepared.push((record, verified));
        }
    }
    let mut observed = Vec::new();
    for (record, verified) in &prepared {
        record.publish(session.filesystem(), workdir, &verified.url)?;
        let mut export_record = (*record).clone();
        export_record.hashes.extend(verified.hashes.clone());
        export_record.size = Some(verified.size);
        observed.push(export_record);
    }
    if !prepared.is_empty() {
        session.packwiz().run_packwiz_refresh(workdir)?;
    }
    Ok(observed)
}

fn report_integrity(session: &dyn Session, destination: &str, evidence: &IntegrityEvidence) {
    if matches!(evidence, IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == DigestAlgorithm::Md5)
    {
        session.display().status().warning(&format!(
            "{destination}: accepted MD5-only source evidence in compatibility mode; the internal SHA-256 address does not strengthen it"
        ));
    }
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
) -> Result<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(fs.open_reader(archive)?)?;
    let entry = zip.by_name("modrinth.index.json")?;
    ensure!(
        entry.size() <= 16 * 1024 * 1024,
        "Export manifest exceeds size limit"
    );
    let mut manifest: serde_json::Value =
        serde_json::from_reader(entry.take(16 * 1024 * 1024 + 1))?;
    let files = manifest["files"]
        .as_array_mut()
        .context("Export has no file manifest")?;
    for record in records {
        let matches: Vec<_> = files
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry["path"].as_str() == Some(&record.destination)).then_some(index)
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "Export omitted or duplicated declared URL file: {}",
            record.destination
        );
        let entry = &mut files[matches[0]];
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
        let mut matched = false;
        for (algorithm, digest) in &record.hashes {
            if let Some(exported) = entry["hashes"][algorithm].as_str() {
                ensure!(
                    exported.eq_ignore_ascii_case(digest),
                    "Export changed source digest for {}",
                    record.destination
                );
                matched |= matches!(algorithm.as_str(), "sha1" | "sha256" | "sha512");
            }
        }
        ensure!(
            matched,
            "Export lacks a matching source digest for {}",
            record.destination
        );
        let downloads = entry["downloads"]
            .as_array()
            .context("Export lacks downloads")?;
        ensure!(
            !downloads.is_empty()
                && downloads.iter().all(|url| url
                    .as_str()
                    .is_some_and(|url| record.downloads.iter().any(|declared| declared == url))),
            "Export changed declared download URLs"
        );
        entry["downloads"] = serde_json::to_value(&record.downloads)?;
    }
    Ok(serde_json::to_vec(&manifest)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use crate::application::session_mocks::MockCommandSession;
    use crate::empack::content::SideRequirement;

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

    #[test]
    fn weak_source_evidence_is_verified_without_promoting_its_assurance() {
        let mut record = record("https://example.com/file.zip".into());
        record.hashes = BTreeMap::from([("md5".into(), "321c3cf486ed509164edec1e1981fec8".into())]);
        let source_hashes = record.hashes.clone();
        let verified = record
            .verify_reader(&mut std::io::Cursor::new(b"payload"), &record.downloads[0])
            .unwrap();
        let IntegrityEvidence::MatchedExpected { expected, actual } = verified.evidence else {
            panic!("matched source evidence required")
        };
        assert_eq!(expected.strongest(), DigestAlgorithm::Md5);
        assert_eq!(
            ExpectedDigest::Sha256(*actual.bytes()).hex(),
            verified.hashes["sha256"]
        );
        assert_eq!(record.hashes, source_hashes);
        assert_eq!(
            record.metadata(&record.downloads[0]).unwrap()["download"]["hash-format"].as_str(),
            Some("md5")
        );
        record
            .hashes
            .insert("sha256".into(), verified.hashes["sha256"].clone());
        record.hashes.insert("md5".into(), "0".repeat(32));
        assert!(
            record
                .verify_reader(&mut std::io::Cursor::new(b"payload"), &record.downloads[0])
                .is_err()
        );
    }

    #[test]
    fn declared_digest_algorithms_must_all_be_understood() {
        let mut record = record("https://example.com/file.zip".into());
        record.hashes.insert("sha257".into(), "a".repeat(64));
        assert!(
            record.validate().is_err(),
            "unknown declarations must not be discarded"
        );
    }

    #[test]
    fn cached_bytes_are_verified_and_missing_content_is_a_miss() {
        let root = tempfile::tempdir().unwrap();
        let record = record("https://example.com/file.zip".into());
        let digest = &record.hashes["sha256"];
        let index = serde_json::json!({"Version":2,"Hashes":{"sha256":[digest]}});
        std::fs::write(root.path().join("index.json"), index.to_string()).unwrap();
        assert!(
            record
                .verify_cached(&LiveFileSystemProvider, root.path(), &record.downloads[0])
                .unwrap()
                .is_none()
        );
        let path = root.path().join(&digest[..2]).join(&digest[2..]);
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"payload").unwrap();
        let verified = record
            .verify_cached(&LiveFileSystemProvider, root.path(), &record.downloads[0])
            .unwrap()
            .unwrap();
        assert_eq!(verified.size, 7);
        assert_eq!(verified.hashes.len(), 3);
        // Same length, different bytes: the cache index cannot authorize these bytes.
        std::fs::write(&path, b"changed").unwrap();
        let error = record
            .verify_cached(&LiveFileSystemProvider, root.path(), &record.downloads[0])
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("digest mismatch"));
        assert!(error.to_string().contains("remove"));
    }

    #[test]
    fn cached_lookup_rejects_untrusted_addresses_and_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let record = record("https://example.com/file.zip".into());
        let mut sha1_record = record.clone();
        sha1_record.hashes = BTreeMap::from([("sha1".into(), "a".repeat(40))]);
        let index = serde_json::json!({"Version":2,"Hashes":{"sha1":["a".repeat(40)],"sha256":["../outside"]}});
        std::fs::write(root.path().join("index.json"), index.to_string()).unwrap();
        assert!(
            sha1_record
                .verify_cached(&LiveFileSystemProvider, root.path(), &record.downloads[0])
                .is_err()
        );
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            let digest = &record.hashes["sha256"];
            std::fs::write(
                root.path().join("index.json"),
                serde_json::json!({"Version":2,"Hashes":{"sha256":[digest]}}).to_string(),
            )
            .unwrap();
            std::fs::write(outside.path().join(&digest[2..]), b"payload").unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join(&digest[..2])).unwrap();
            assert!(
                record
                    .verify_cached(&LiveFileSystemProvider, root.path(), &record.downloads[0])
                    .is_err()
            );
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
    fn export_requires_a_matching_source_digest() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let record = record("https://example.com/file.zip".into());
        for hashes in [
            serde_json::json!({}),
            serde_json::json!({"md5":"anything"}),
            serde_json::json!({"sha256":"0".repeat(64)}),
        ] {
            let path = root.path().join("export.mrpack");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(
                "modrinth.index.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            let manifest = serde_json::json!({"files":[{"path":record.destination,"fileSize":7,
                "env":{"client":"optional","server":"unsupported"},"hashes":hashes}]});
            zip.write_all(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
            zip.finish().unwrap();
            assert!(
                verify_export(&LiveFileSystemProvider, &path, &[&record]).is_err(),
                "{hashes}"
            );
        }
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
