use std::collections::HashMap;
use std::io::Write;

use tempfile::NamedTempFile;

use super::*;
#[cfg(feature = "test-utils")]
use crate::application::session::NetworkProvider;
use crate::empack::content::{OverrideCategory, OverrideSide, SideEnv, SideRequirement};
use crate::empack::parsing::ModLoader;
#[cfg(feature = "test-utils")]
use crate::empack::search::ProjectResolverTrait;
#[cfg(feature = "test-utils")]
use crate::networking::rate_budget::{
    FixedWindowBudget, HeaderDrivenBudget, HostBudgetRegistry, RateBudget,
};
use crate::primitives::ProjectPlatform;
#[cfg(feature = "test-utils")]
use reqwest::StatusCode;
#[cfg(feature = "test-utils")]
use reqwest::header::HeaderMap;
#[cfg(feature = "test-utils")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "test-utils")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "test-utils")]
use std::time::{Duration, Instant};

#[cfg(feature = "test-utils")]
#[derive(Default)]
struct RecordingBudget {
    acquire_calls: AtomicU32,
    record_calls: AtomicU32,
    last_status: Mutex<Option<StatusCode>>,
    last_remaining: Mutex<Option<u32>>,
}

#[cfg(feature = "test-utils")]
impl RateBudget for RecordingBudget {
    fn record_response(&self, headers: &HeaderMap, status: StatusCode) {
        self.record_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_status.lock().unwrap() = Some(status);
        *self.last_remaining.lock().unwrap() = headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u32>().ok());
    }

    fn acquire(&self) -> Duration {
        self.acquire_calls.fetch_add(1, Ordering::Relaxed);
        Duration::ZERO
    }

    fn is_exhausted(&self) -> bool {
        false
    }
}

#[cfg(feature = "test-utils")]
struct TestNetworkProvider {
    client: reqwest::Client,
    budgets: HostBudgetRegistry,
}

#[cfg(feature = "test-utils")]
impl TestNetworkProvider {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            budgets: HostBudgetRegistry::empty(),
        }
    }
}

#[cfg(feature = "test-utils")]
impl NetworkProvider for TestNetworkProvider {
    fn http_client(&self) -> Result<reqwest::Client> {
        Ok(self.client.clone())
    }

    fn project_resolver(
        &self,
        _client: reqwest::Client,
        _curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        panic!("project_resolver is not used in import rate-budget tests")
    }

    fn rate_budgets(&self) -> &HostBudgetRegistry {
        &self.budgets
    }
}

#[cfg(feature = "test-utils")]
fn registry_with_budget(host: &str, budget: Arc<dyn RateBudget>) -> HostBudgetRegistry {
    HostBudgetRegistry::with_budgets(HashMap::from([(host.to_string(), budget)]))
}

#[cfg(feature = "test-utils")]
fn test_api_bases(modrinth: &str, curseforge: &str) -> ResolveApiBases {
    ResolveApiBases {
        modrinth: modrinth.to_string(),
        curseforge: curseforge.to_string(),
    }
}

#[cfg(feature = "test-utils")]
fn modrinth_pref(project_id: &str) -> PlatformRef {
    PlatformRef {
        destination_path: format!("mods/{project_id}.jar"),
        platform: ProjectPlatform::Modrinth,
        project_id: project_id.to_string(),
        file_id: None,
        hashes: HashMap::new(),
        download_urls: Vec::new(),
        env: SideEnv {
            client: SideRequirement::Required,
            server: SideRequirement::Required,
        },
        required: true,
        resolved_name: None,
        resolved_slug: None,
        resolved_type: None,
        cf_class_id: None,
    }
}

#[cfg(feature = "test-utils")]
fn curseforge_pref(project_id: &str, file_id: Option<&str>) -> PlatformRef {
    PlatformRef {
        destination_path: format!("mods/{project_id}.jar"),
        platform: ProjectPlatform::CurseForge,
        project_id: project_id.to_string(),
        file_id: file_id.map(str::to_string),
        hashes: HashMap::new(),
        download_urls: Vec::new(),
        env: SideEnv {
            client: SideRequirement::Required,
            server: SideRequirement::Required,
        },
        required: true,
        resolved_name: None,
        resolved_slug: None,
        resolved_type: None,
        cf_class_id: None,
    }
}

#[cfg(feature = "test-utils")]
fn manifest_with_content(content: Vec<ContentEntry>) -> ModpackManifest {
    ModpackManifest {
        identity: PackIdentity {
            name: "Test Pack".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            summary: None,
        },
        target: RuntimeTarget {
            minecraft_version: "1.20.1".to_string(),
            loader: ModLoader::Fabric,
            loader_version: "0.16.0".to_string(),
        },
        content,
        overrides: Vec::new(),
        source_platform: ProjectPlatform::Modrinth,
        archive_path: std::path::PathBuf::from("/tmp/test.mrpack"),
    }
}

// ---------------------------------------------------------------------------
// CurseForge manifest parsing
// ---------------------------------------------------------------------------

const CF_MANIFEST_JSON: &str = r#"{
  "minecraft": {
    "version": "1.20.1",
    "modLoaders": [
      { "id": "fabric-0.16.0", "primary": true }
    ]
  },
  "files": [
    { "projectID": 12345, "fileID": 67890, "required": true },
    { "projectID": 54321, "fileID": 98760, "required": false }
  ],
  "manifestType": "minecraftModpack",
  "overrides": "overrides",
  "name": "TestPack",
  "version": "2.0.0",
  "author": "TestAuthor"
}"#;

fn create_cf_zip(manifest_json: &str) -> NamedTempFile {
    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
    zip.start_file::<&str, ()>("manifest.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(manifest_json.as_bytes()).unwrap();
    zip.finish().unwrap();
    tmp
}

#[test]
fn test_parse_curseforge_zip_basic() {
    let tmp = create_cf_zip(CF_MANIFEST_JSON);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();

    assert_eq!(manifest.identity.name, "TestPack");
    assert_eq!(manifest.identity.version, "2.0.0");
    assert_eq!(manifest.identity.author.as_deref(), Some("TestAuthor"));
    assert_eq!(manifest.target.minecraft_version, "1.20.1");
    assert_eq!(manifest.target.loader, ModLoader::Fabric);
    assert_eq!(manifest.target.loader_version, "0.16.0");
    assert_eq!(manifest.source_platform, ProjectPlatform::CurseForge);
}

#[test]
fn test_parse_curseforge_zip_content_entries() {
    let tmp = create_cf_zip(CF_MANIFEST_JSON);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();

    assert_eq!(manifest.content.len(), 2);

    match &manifest.content[0] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.project_id, "12345");
            assert_eq!(pref.file_id.as_deref(), Some("67890"));
            assert!(pref.required);
            assert_eq!(pref.platform, ProjectPlatform::CurseForge);
        }
        _ => panic!("expected PlatformReferenced"),
    }

    match &manifest.content[1] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.project_id, "54321");
            assert!(!pref.required);
        }
        _ => panic!("expected PlatformReferenced"),
    }
}

#[test]
fn test_parse_curseforge_zip_missing_manifest() {
    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
    zip.start_file::<&str, ()>("other.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.finish().unwrap();

    let result = parse_curseforge_zip(tmp.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("manifest.json not found"));
}

#[test]
fn test_parse_curseforge_zip_wrong_type() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [{ "id": "forge-47.2.0", "primary": true }]
        },
        "files": [],
        "manifestType": "resourcePack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);

    let result = parse_curseforge_zip(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expected manifestType"));
}

// ---------------------------------------------------------------------------
// Modrinth manifest parsing
// ---------------------------------------------------------------------------

const MR_MANIFEST_JSON: &str = r#"{
  "dependencies": {
    "minecraft": "1.20.1",
    "fabric-loader": "0.14.0"
  },
  "files": [
    {
      "path": "mods/sodium.jar",
      "downloads": ["https://cdn.modrinth.com/versions/abc123/sodium.jar"],
      "hashes": { "sha1": "deadbeef", "sha512": "feedface" },
      "env": { "client": "required", "server": "required" },
      "fileSize": 1024000
    }
  ],
  "overrides": "overrides",
  "client-overrides": "client-overrides",
  "server-overrides": "server-overrides",
  "name": "ModrinthPack",
  "versionId": "2.5.0",
  "summary": "A test modpack"
}"#;

fn create_mr_zip(manifest_json: &str) -> NamedTempFile {
    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
    zip.start_file::<&str, ()>("modrinth.index.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(manifest_json.as_bytes()).unwrap();
    zip.finish().unwrap();
    tmp
}

#[test]
fn test_parse_modrinth_mrpack_basic() {
    let tmp = create_mr_zip(MR_MANIFEST_JSON);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();

    assert_eq!(manifest.identity.name, "ModrinthPack");
    assert_eq!(manifest.identity.version, "2.5.0");
    assert_eq!(manifest.identity.summary.as_deref(), Some("A test modpack"));
    assert_eq!(manifest.target.minecraft_version, "1.20.1");
    assert_eq!(manifest.target.loader, ModLoader::Fabric);
    assert_eq!(manifest.target.loader_version, "0.14.0");
    assert_eq!(manifest.source_platform, ProjectPlatform::Modrinth);
}

#[test]
fn test_parse_modrinth_mrpack_content_entries() {
    let tmp = create_mr_zip(MR_MANIFEST_JSON);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();

    assert_eq!(manifest.content.len(), 1);

    match &manifest.content[0] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.destination_path, "mods/sodium.jar");
            assert_eq!(
                pref.download_urls,
                vec!["https://cdn.modrinth.com/versions/abc123/sodium.jar"]
            );
            assert_eq!(pref.hashes.get("sha1").unwrap(), "deadbeef");
            assert_eq!(pref.env.client, SideRequirement::Required);
            assert_eq!(pref.env.server, SideRequirement::Required);
            assert_eq!(pref.platform, ProjectPlatform::Modrinth);
        }
        _ => panic!("expected PlatformReferenced"),
    }
}

#[test]
fn test_parse_modrinth_mrpack_embedded_jar() {
    let json = r#"{
      "dependencies": {
        "minecraft": "1.20.1",
        "fabric-loader": "0.14.0"
      },
      "files": [
        {
          "path": "mods/local-mod.jar",
          "hashes": { "sha1": "abcdef" },
          "env": { "client": "required", "server": "unsupported" },
          "fileSize": 512000
        }
      ],
      "overrides": "overrides",
      "name": "EmbeddedPack",
      "versionId": "1.0.0"
    }"#;

    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();

    assert_eq!(manifest.content.len(), 1);

    match &manifest.content[0] {
        ContentEntry::EmbeddedJar(embed) => {
            assert_eq!(embed.source_path, "mods/local-mod.jar");
            assert_eq!(embed.file_size, 512000);
            assert_eq!(embed.env.client, SideRequirement::Required);
            assert_eq!(embed.env.server, SideRequirement::Unsupported);
        }
        _ => panic!("expected EmbeddedJar"),
    }
}

#[test]
fn test_parse_modrinth_mrpack_missing_index() {
    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());
    zip.start_file::<&str, ()>("other.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.finish().unwrap();

    let result = parse_modrinth_mrpack(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("modrinth.index.json not found"));
}

// ---------------------------------------------------------------------------
// Loader ID parsing
// ---------------------------------------------------------------------------

#[test]
fn test_parse_from_platform_id_curseforge() {
    assert_eq!(ModLoader::parse_from_platform_id("fabric-0.16.0").unwrap(), ModLoader::Fabric);
    assert_eq!(ModLoader::parse_from_platform_id("forge-47.2.0").unwrap(), ModLoader::Forge);
    assert_eq!(ModLoader::parse_from_platform_id("neoforge-21.1.0").unwrap(), ModLoader::NeoForge);
    assert_eq!(ModLoader::parse_from_platform_id("quilt-0.25.0").unwrap(), ModLoader::Quilt);
}

#[test]
fn test_parse_from_platform_id_modrinth() {
    assert_eq!(ModLoader::parse_from_platform_id("fabric-loader").unwrap(), ModLoader::Fabric);
    assert_eq!(ModLoader::parse_from_platform_id("forge").unwrap(), ModLoader::Forge);
    assert_eq!(ModLoader::parse_from_platform_id("neoforge").unwrap(), ModLoader::NeoForge);
    assert_eq!(ModLoader::parse_from_platform_id("quilt-loader").unwrap(), ModLoader::Quilt);
}

#[test]
fn test_parse_from_platform_id_invalid() {
    assert!(ModLoader::parse_from_platform_id("liteloader-1.0").is_err());
    assert!(ModLoader::parse_from_platform_id("unknown-loader").is_err());
    assert!(ModLoader::parse_from_platform_id("").is_err());
}

// ---------------------------------------------------------------------------
// Override classification
// ---------------------------------------------------------------------------

#[test]
fn test_classify_override() {
    assert_eq!(classify_override("config/sodium-options.json"), OverrideCategory::Config);
    assert_eq!(classify_override("defaultconfigs/something.cfg"), OverrideCategory::Config);
    assert_eq!(classify_override("kubejs/scripts/main.js"), OverrideCategory::Script);
    assert_eq!(classify_override("scripts/zs/example.zs"), OverrideCategory::Script);
    assert_eq!(classify_override("resourcepacks/vanilla.zip"), OverrideCategory::ResourcePack);
    assert_eq!(classify_override("shaderpacks/complementary.zip"), OverrideCategory::ShaderPack);
    assert_eq!(classify_override("datapacks/custom.zip"), OverrideCategory::DataPack);
    assert_eq!(classify_override("data/custom/data.zip"), OverrideCategory::DataPack);
    assert_eq!(classify_override("world/level.dat"), OverrideCategory::World);
    assert_eq!(classify_override("DIM-1/data.dat"), OverrideCategory::World);
    assert_eq!(classify_override("server.properties"), OverrideCategory::ServerConfig);
    assert_eq!(classify_override("server-config/common.yml"), OverrideCategory::ServerConfig);
    assert_eq!(classify_override("options.txt"), OverrideCategory::ClientConfig);
    assert_eq!(classify_override("optionsof.txt"), OverrideCategory::ClientConfig);
    assert_eq!(classify_override("mods/something.jar"), OverrideCategory::ModData);
    assert_eq!(classify_override("unknown/file.txt"), OverrideCategory::Other);
    assert_eq!(classify_override("random.dat"), OverrideCategory::Other);
}

#[test]
fn test_classify_override_backslash_normalized() {
    assert_eq!(classify_override("config\\sodium.json"), OverrideCategory::Config);
    assert_eq!(classify_override("resourcepacks\\pack.zip"), OverrideCategory::ResourcePack);
}

// ---------------------------------------------------------------------------
// CurseForge loader variations
// ---------------------------------------------------------------------------

#[test]
fn test_parse_curseforge_neoforge() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.4",
            "modLoaders": [{ "id": "neoforge-20.4.147", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::NeoForge);
    assert_eq!(manifest.target.loader_version, "20.4.147");
}

#[test]
fn test_parse_curseforge_quilt() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [{ "id": "quilt-0.25.0", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::Quilt);
    assert_eq!(manifest.target.loader_version, "0.25.0");
}

#[test]
fn test_parse_curseforge_no_overrides_field() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [{ "id": "fabric-0.16.0", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert_eq!(manifest.overrides.len(), 0);
}

// ---------------------------------------------------------------------------
// Modrinth loader variations
// ---------------------------------------------------------------------------

#[test]
fn test_parse_modrinth_neoforge() {
    let json = r#"{
        "dependencies": {
            "minecraft": "1.20.4",
            "neoforge": "20.4.147"
        },
        "files": [],
        "overrides": "overrides",
        "name": "NeoForgePack",
        "versionId": "1.0.0"
    }"#;
    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::NeoForge);
    assert_eq!(manifest.target.loader_version, "20.4.147");
}

#[test]
fn test_parse_modrinth_forge() {
    let json = r#"{
        "dependencies": {
            "minecraft": "1.20.1",
            "forge": "47.2.0"
        },
        "files": [],
        "overrides": "overrides",
        "name": "ForgePack",
        "versionId": "1.0.0"
    }"#;
    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::Forge);
    assert_eq!(manifest.target.loader_version, "47.2.0");
}

#[test]
fn test_parse_modrinth_quilt() {
    let json = r#"{
        "dependencies": {
            "minecraft": "1.20.1",
            "quilt-loader": "0.25.0"
        },
        "files": [],
        "overrides": "overrides",
        "name": "QuiltPack",
        "versionId": "1.0.0"
    }"#;
    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::Quilt);
    assert_eq!(manifest.target.loader_version, "0.25.0");
}

// ---------------------------------------------------------------------------
// Manifest type construction
// ---------------------------------------------------------------------------

#[test]
fn test_modpack_manifest_construction() {
    let manifest = ModpackManifest {
        identity: PackIdentity {
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            author: Some("Author".to_string()),
            summary: Some("Summary".to_string()),
        },
        target: RuntimeTarget {
            minecraft_version: "1.20.1".to_string(),
            loader: ModLoader::Fabric,
            loader_version: "0.16.0".to_string(),
        },
        content: vec![ContentEntry::PlatformReferenced(PlatformRef {
            destination_path: "mods/test.pw.toml".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            file_id: Some("v1".to_string()),
            hashes: HashMap::from([("sha1".to_string(), "abc".to_string())]),
            download_urls: vec!["https://example.com/file.jar".to_string()],
            env: SideEnv {
                client: SideRequirement::Required,
                server: SideRequirement::Required,
            },
            required: true,
            resolved_name: Some("Sodium".to_string()),
            resolved_slug: Some("sodium".to_string()),
            resolved_type: Some(crate::primitives::ProjectType::Mod),
            cf_class_id: None,
        })],
        overrides: vec![OverrideEntry {
            source_path: "overrides/config/test.json".to_string(),
            destination_path: "config/test.json".to_string(),
            side: OverrideSide::Both,
            category: OverrideCategory::Config,
        }],
        source_platform: ProjectPlatform::Modrinth,
        archive_path: std::path::PathBuf::from("/tmp/test.mrpack"),
    };

    assert_eq!(manifest.identity.name, "Test");
    assert_eq!(manifest.content.len(), 1);
    assert_eq!(manifest.overrides.len(), 1);
}

// ---------------------------------------------------------------------------
// Modrinth env parsing
// ---------------------------------------------------------------------------

#[test]
fn test_mr_side_requirement() {
    assert_eq!(mr_side_requirement(Some("required")), SideRequirement::Required);
    assert_eq!(mr_side_requirement(Some("optional")), SideRequirement::Optional);
    assert_eq!(mr_side_requirement(Some("unsupported")), SideRequirement::Unsupported);
    assert_eq!(mr_side_requirement(None), SideRequirement::Unknown);
    assert_eq!(mr_side_requirement(Some("")), SideRequirement::Unknown);
}

// ---------------------------------------------------------------------------
// Source detection
// ---------------------------------------------------------------------------

#[test]
fn test_detect_local_source_mrpack() {
    let tmp = NamedTempFile::with_suffix(".mrpack").unwrap();
    let kind = detect_local_source(tmp.path()).unwrap();
    assert_eq!(kind, SourceKind::ModrinthMrpack);
}

#[test]
fn test_detect_local_source_zip() {
    let tmp = NamedTempFile::with_suffix(".zip").unwrap();
    let kind = detect_local_source(tmp.path()).unwrap();
    assert_eq!(kind, SourceKind::CurseForgeZip);
}

#[test]
fn test_detect_local_source_empack_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empack.yml"), "").unwrap();
    let result = detect_local_source(dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already an empack project"));
}

#[test]
fn test_detect_local_source_packwiz_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pack.toml"), "").unwrap();
    let kind = detect_local_source(dir.path()).unwrap();
    assert_eq!(kind, SourceKind::PackwizDirectory);
}

#[test]
fn test_detect_local_source_nonexistent() {
    let result = detect_local_source(std::path::Path::new("/nonexistent/path"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// CurseForge loader parsing edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_parse_cf_loader_multiple_loaders_picks_primary() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [
                { "id": "forge-47.2.0", "primary": false },
                { "id": "fabric-0.16.0", "primary": true }
            ]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::Fabric);
}

#[test]
fn test_parse_cf_loader_no_primary_picks_first() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [
                { "id": "fabric-0.16.0", "primary": false },
                { "id": "forge-47.2.0", "primary": false }
            ]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert_eq!(manifest.target.loader, ModLoader::Fabric);
}

#[test]
fn test_parse_cf_loader_no_loaders_errors() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": []
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let result = parse_curseforge_zip(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("modLoaders"));
}

#[test]
fn test_parse_cf_loader_no_mc_version_errors() {
    let json = r#"{
        "minecraft": {
            "modLoaders": [{ "id": "fabric-0.16.0", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let result = parse_curseforge_zip(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("minecraft.version"));
}

// ---------------------------------------------------------------------------
// Modrinth missing dependencies
// ---------------------------------------------------------------------------

#[test]
fn test_parse_mr_missing_mc_version_errors() {
    let json = r#"{
        "dependencies": { "fabric-loader": "0.14.0" },
        "files": [],
        "overrides": "overrides",
        "name": "Pack",
        "versionId": "1.0.0"
    }"#;
    let tmp = create_mr_zip(json);
    let result = parse_modrinth_mrpack(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("dependencies.minecraft"));
}

#[test]
fn test_parse_mr_missing_loader_errors() {
    let json = r#"{
        "dependencies": { "minecraft": "1.20.1" },
        "files": [],
        "overrides": "overrides",
        "name": "Pack",
        "versionId": "1.0.0"
    }"#;
    let tmp = create_mr_zip(json);
    let result = parse_modrinth_mrpack(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("dependencies.<loader>"));
}

// ---------------------------------------------------------------------------
// Override entries from archive
// ---------------------------------------------------------------------------

#[test]
fn test_curseforge_override_entries_collected() {
    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());

    // manifest.json
    zip.start_file::<&str, ()>("manifest.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(CF_MANIFEST_JSON.as_bytes()).unwrap();

    // override files
    zip.start_file::<&str, ()>("overrides/config/test.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file::<&str, ()>("overrides/resourcepacks/pack.zip", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"".as_slice()).unwrap();

    zip.finish().unwrap();

    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    // Should have at least 2 override entries
    assert!(manifest.overrides.len() >= 2);

    let config_override = manifest
        .overrides
        .iter()
        .find(|o| o.category == OverrideCategory::Config);
    assert!(config_override.is_some());

    let rp_override = manifest
        .overrides
        .iter()
        .find(|o| o.category == OverrideCategory::ResourcePack);
    assert!(rp_override.is_some());
}

#[test]
fn test_modrinth_side_overrides() {
    let json = r#"{
        "dependencies": {
            "minecraft": "1.20.1",
            "fabric-loader": "0.14.0"
        },
        "files": [],
        "overrides": "overrides",
        "client-overrides": "client-overrides",
        "server-overrides": "server-overrides",
        "name": "Pack",
        "versionId": "1.0.0"
    }"#;

    let tmp = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(tmp.reopen().unwrap());

    zip.start_file::<&str, ()>("modrinth.index.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(json.as_bytes()).unwrap();

    zip.start_file::<&str, ()>("overrides/config/shared.json", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file::<&str, ()>("client-overrides/options.txt", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file::<&str, ()>("server-overrides/server.properties", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();

    zip.finish().unwrap();

    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();

    let shared = manifest
        .overrides
        .iter()
        .find(|o| o.side == OverrideSide::Both);
    assert!(shared.is_some());

    let client = manifest
        .overrides
        .iter()
        .find(|o| o.side == OverrideSide::ClientOnly);
    assert!(client.is_some());

    let server = manifest
        .overrides
        .iter()
        .find(|o| o.side == OverrideSide::ServerOnly);
    assert!(server.is_some());
}

// ---------------------------------------------------------------------------
// Modrinth non-JAR embedded file
// ---------------------------------------------------------------------------

#[test]
fn test_parse_modrinth_non_jar_embedded() {
    let json = r#"{
        "dependencies": {
            "minecraft": "1.20.1",
            "fabric-loader": "0.14.0"
        },
        "files": [
            {
                "path": "resourcepacks/vanilla.zip",
                "downloads": [],
                "hashes": { "sha1": "abc" },
                "env": { "client": "required", "server": "required" },
                "fileSize": 2048
            }
        ],
        "overrides": "overrides",
        "name": "Pack",
        "versionId": "1.0.0"
    }"#;

    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();

    assert_eq!(manifest.content.len(), 1);
    match &manifest.content[0] {
        ContentEntry::EmbeddedJar(embed) => {
            assert_eq!(embed.source_path, "resourcepacks/vanilla.zip");
            assert_eq!(embed.file_size, 2048);
        }
        _ => panic!("expected EmbeddedJar for non-JAR with no downloads"),
    }
}

// ---------------------------------------------------------------------------
// Datapack folder detection
// ---------------------------------------------------------------------------

fn make_test_manifest(
    content: Vec<ContentEntry>,
    overrides: Vec<OverrideEntry>,
) -> ModpackManifest {
    ModpackManifest {
        identity: PackIdentity {
            name: "test".into(),
            version: "1.0".into(),
            author: None,
            summary: None,
        },
        target: RuntimeTarget {
            minecraft_version: "1.20.1".into(),
            loader: ModLoader::Fabric,
            loader_version: "0.14.21".into(),
        },
        content,
        overrides,
        source_platform: ProjectPlatform::Modrinth,
        archive_path: PathBuf::from("/test.mrpack"),
    }
}

fn make_override(destination_path: &str) -> OverrideEntry {
    OverrideEntry {
        source_path: format!("overrides/{}", destination_path),
        destination_path: destination_path.to_string(),
        side: OverrideSide::Both,
        category: classify_override(destination_path),
    }
}

fn make_platform_ref(destination_path: &str) -> ContentEntry {
    ContentEntry::PlatformReferenced(PlatformRef {
        destination_path: destination_path.to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "AABBCCDD".to_string(),
        file_id: Some("v1".to_string()),
        hashes: HashMap::new(),
        download_urls: vec!["https://cdn.modrinth.com/data/AABBCCDD/versions/v1/pack.zip".to_string()],
        env: SideEnv {
            client: SideRequirement::Required,
            server: SideRequirement::Required,
        },
        required: true,
        resolved_name: None,
        resolved_slug: None,
        resolved_type: None,
        cf_class_id: None,
    })
}

#[test]
fn test_detect_datapack_folder_paxi() {
    let manifest = make_test_manifest(
        vec![],
        vec![
            make_override("config/paxi/datapacks/some.zip"),
            make_override("config/sodium-options.json"),
        ],
    );
    assert_eq!(
        detect_datapack_folder(&manifest),
        Some("config/paxi/datapacks".to_string())
    );
}

#[test]
fn test_detect_datapack_folder_openloader() {
    let manifest = make_test_manifest(
        vec![],
        vec![
            make_override("config/openloader/data/pack.zip"),
            make_override("mods/openloader.jar"),
        ],
    );
    assert_eq!(
        detect_datapack_folder(&manifest),
        Some("config/openloader/data".to_string())
    );
}

#[test]
fn test_detect_datapack_folder_root_zips() {
    let manifest = make_test_manifest(
        vec![],
        vec![make_override("datapacks/custom.zip")],
    );
    assert_eq!(
        detect_datapack_folder(&manifest),
        Some("datapacks".to_string())
    );
}

#[test]
fn test_detect_datapack_folder_files_array() {
    let manifest = make_test_manifest(
        vec![make_platform_ref("datapacks/mypack.zip")],
        vec![],
    );
    assert_eq!(
        detect_datapack_folder(&manifest),
        Some("datapacks".to_string())
    );
}

#[test]
fn test_detect_datapack_folder_none() {
    let manifest = make_test_manifest(
        vec![make_platform_ref("mods/sodium.jar")],
        vec![
            make_override("config/sodium-options.json"),
            make_override("mods/local-mod.jar"),
        ],
    );
    assert_eq!(detect_datapack_folder(&manifest), None);
}

#[test]
fn test_detect_datapack_folder_raw_json_ignored() {
    let manifest = make_test_manifest(
        vec![],
        vec![make_override("datapacks/pack/data/minecraft/tags/foo.json")],
    );
    assert_eq!(detect_datapack_folder(&manifest), None);
}

#[test]
fn test_detect_datapack_folder_paxi_over_root() {
    let manifest = make_test_manifest(
        vec![],
        vec![
            make_override("config/paxi/datapacks/loader-pack.zip"),
            make_override("datapacks/root-pack.zip"),
        ],
    );
    assert_eq!(
        detect_datapack_folder(&manifest),
        Some("config/paxi/datapacks".to_string())
    );
}

// ---------------------------------------------------------------------------
// Source detection: additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_detect_local_source_unknown_extension() {
    let tmp = NamedTempFile::with_suffix(".tar.gz").unwrap();
    let result = detect_local_source(tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot detect source type"));
}

#[test]
fn test_detect_local_source_no_extension() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("somefile");
    std::fs::write(&file, "content").unwrap();
    let result = detect_local_source(&file);
    assert!(result.is_err());
}

#[test]
fn test_detect_local_source_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let result = detect_local_source(dir.path());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// CurseForge manifest: author and name defaults
// ---------------------------------------------------------------------------

#[test]
fn test_parse_curseforge_missing_name_defaults() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [{ "id": "fabric-0.16.0", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert!(!manifest.identity.name.is_empty());
}

#[test]
fn test_parse_curseforge_missing_version_defaults() {
    let json = r#"{
        "minecraft": {
            "version": "1.20.1",
            "modLoaders": [{ "id": "fabric-0.16.0", "primary": true }]
        },
        "files": [],
        "manifestType": "minecraftModpack",
        "overrides": "overrides",
        "name": "TestPack"
    }"#;
    let tmp = create_cf_zip(json);
    let manifest = parse_curseforge_zip(tmp.path()).unwrap();
    assert!(!manifest.identity.version.is_empty());
}

// ---------------------------------------------------------------------------
// Modrinth mrpack: edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_parse_modrinth_multiple_downloads_first_used() {
    let json = r#"{
      "dependencies": {
        "minecraft": "1.20.1",
        "fabric-loader": "0.14.0"
      },
      "files": [
        {
          "path": "mods/test.jar",
          "downloads": [
            "https://cdn.modrinth.com/versions/abc/test.jar",
            "https://mirror.example.com/test.jar"
          ],
          "hashes": { "sha1": "abc123" },
          "env": { "client": "required", "server": "optional" },
          "fileSize": 1024
        }
      ],
      "overrides": "overrides",
      "name": "MultiDownload",
      "versionId": "1.0.0"
    }"#;

    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();
    assert_eq!(manifest.content.len(), 1);

    match &manifest.content[0] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.download_urls.len(), 2);
            assert!(pref.download_urls[0].contains("cdn.modrinth.com"));
        }
        _ => panic!("expected PlatformReferenced"),
    }
}

#[test]
fn test_parse_modrinth_optional_env_fields() {
    let json = r#"{
      "dependencies": {
        "minecraft": "1.20.1",
        "fabric-loader": "0.14.0"
      },
      "files": [
        {
          "path": "mods/test.jar",
          "downloads": ["https://cdn.modrinth.com/test.jar"],
          "hashes": { "sha1": "abc" },
          "fileSize": 512
        }
      ],
      "overrides": "overrides",
      "name": "NoEnv",
      "versionId": "1.0.0"
    }"#;

    let tmp = create_mr_zip(json);
    let manifest = parse_modrinth_mrpack(tmp.path()).unwrap();
    assert_eq!(manifest.content.len(), 1);

    match &manifest.content[0] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.env.client, SideRequirement::Unknown);
            assert_eq!(pref.env.server, SideRequirement::Unknown);
        }
        _ => panic!("expected PlatformReferenced"),
    }
}

// ---------------------------------------------------------------------------
// ImportError display
// ---------------------------------------------------------------------------

#[test]
fn test_import_error_display_variants() {
    let errors: Vec<ImportError> = vec![
        ImportError::ArchiveRead("test".to_string()),
        ImportError::CurseForgeManifestMissing,
        ImportError::ModrinthManifestMissing,
        ImportError::ParseFailed("bad".to_string()),
        ImportError::MissingField { field: "name".to_string() },
        ImportError::UnknownLoader("liteloader".to_string()),
        ImportError::AlreadyEmpackProject,
        ImportError::UnrecognizedSource("file.tar".to_string()),
        ImportError::DownloadFailed("timeout".to_string()),
    ];

    for err in &errors {
        let msg = format!("{}", err);
        assert!(!msg.is_empty(), "error display should not be empty");
    }
}

// ---------------------------------------------------------------------------
// SourceKind Debug
// ---------------------------------------------------------------------------

#[test]
fn test_source_kind_debug() {
    let kinds = vec![
        SourceKind::CurseForgeZip,
        SourceKind::ModrinthMrpack,
        SourceKind::PackwizDirectory,
        SourceKind::ModrinthRemote { slug: "test".to_string(), version: None },
        SourceKind::CurseForgeRemote { slug: "test".to_string() },
    ];

    for kind in &kinds {
        let debug = format!("{:?}", kind);
        assert!(!debug.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Override classification: additional patterns
// ---------------------------------------------------------------------------

#[test]
fn test_classify_override_dim_dash_world() {
    assert_eq!(classify_override("DIM-1/data.dat"), OverrideCategory::World);
    assert_eq!(classify_override("dim-2/data.dat"), OverrideCategory::World);
}

#[test]
fn test_classify_override_dim_no_dash_is_other() {
    assert_eq!(classify_override("DIM1/data.dat"), OverrideCategory::Other);
}

#[test]
fn test_classify_override_nested_config() {
    assert_eq!(
        classify_override("defaultconfigs/mycoolmod/settings.toml"),
        OverrideCategory::Config
    );
}

#[test]
fn test_classify_override_options_variants() {
    assert_eq!(classify_override("optionsof.txt"), OverrideCategory::ClientConfig);
    // optionsshaders.txt does not match the optionsof.txt pattern
    assert_eq!(classify_override("optionsshaders.txt"), OverrideCategory::Other);
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_modrinth_project_records_headers_and_metadata() {
    let mut server = mockito::Server::new_async().await;
    let version_mock = server
        .mock("GET", "/v2/version_file/deadbeef")
        .match_query(mockito::Matcher::UrlEncoded(
            "algorithm".to_string(),
            "sha1".to_string(),
        ))
        .with_status(200)
        .with_header("x-ratelimit-remaining", "5")
        .with_header("x-ratelimit-limit", "300")
        .with_header("x-ratelimit-reset", "1")
        .with_body(r#"{"id":"version-123"}"#)
        .create_async()
        .await;
    let project_mock = server
        .mock("GET", "/v2/project/AANobbMI")
        .with_status(200)
        .with_header("x-ratelimit-remaining", "4")
        .with_header("x-ratelimit-limit", "300")
        .with_header("x-ratelimit-reset", "1")
        .with_body(r#"{"title":"Sodium","slug":"sodium","project_type":"mod"}"#)
        .create_async()
        .await;

    let mut pref = modrinth_pref("AANobbMI");
    pref.hashes
        .insert("sha1".to_string(), "deadbeef".to_string());

    let budget = Arc::new(RecordingBudget::default());
    let budget_trait: Arc<dyn RateBudget> = budget.clone();
    let api_bases = test_api_bases(&server.url(), &server.url());
    let client = reqwest::Client::new();
    let mut warnings = Vec::new();

    resolve_modrinth_project_with_client(
        &mut pref,
        &client,
        &api_bases,
        &mut warnings,
        Some(&budget_trait),
    )
    .await;

    assert_eq!(pref.file_id.as_deref(), Some("version-123"));
    assert_eq!(pref.resolved_name.as_deref(), Some("Sodium"));
    assert_eq!(pref.resolved_slug.as_deref(), Some("sodium"));
    assert_eq!(pref.resolved_type, Some(crate::primitives::ProjectType::Mod));
    assert!(warnings.is_empty());
    assert_eq!(budget.acquire_calls.load(Ordering::Relaxed), 2);
    assert_eq!(budget.record_calls.load(Ordering::Relaxed), 2);
    assert_eq!(*budget.last_status.lock().unwrap(), Some(StatusCode::OK));
    assert_eq!(*budget.last_remaining.lock().unwrap(), Some(4));

    version_mock.assert_async().await;
    project_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_curseforge_file_ids_records_budget_and_maps_results() {
    let mut server = mockito::Server::new_async().await;
    let batch_mock = server
        .mock("POST", "/v1/mods/files")
        .match_header("x-api-key", "test-api-key")
        .with_status(200)
        .with_header("x-ratelimit-remaining", "9")
        .with_body(r#"{"data":[{"id":67890,"modId":12345},{"id":98760,"modId":54321}]}"#)
        .create_async()
        .await;

    let provider = TestNetworkProvider::new();
    let budget = Arc::new(RecordingBudget::default());
    let budget_trait: Arc<dyn RateBudget> = budget.clone();
    let api_bases = test_api_bases(&server.url(), &server.url());
    let mut warnings = Vec::new();

    let result = resolve_curseforge_file_ids(
        &[67890, 98760],
        &provider,
        Some("test-api-key"),
        &mut warnings,
        &api_bases,
        Some(&budget_trait),
    )
    .await;

    assert_eq!(result.get(&67890), Some(&12345));
    assert_eq!(result.get(&98760), Some(&54321));
    assert!(warnings.is_empty());
    assert_eq!(budget.acquire_calls.load(Ordering::Relaxed), 1);
    assert_eq!(budget.record_calls.load(Ordering::Relaxed), 1);
    assert_eq!(*budget.last_status.lock().unwrap(), Some(StatusCode::OK));
    assert_eq!(*budget.last_remaining.lock().unwrap(), Some(9));

    batch_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_manifest_shares_curseforge_budget_across_file_lookup_and_project_lookup() {
    let mut server = mockito::Server::new_async().await;
    let batch_mock = server
        .mock("POST", "/v1/mods/files")
        .match_header("x-api-key", "test-api-key")
        .with_status(200)
        .with_body(r#"{"data":[{"id":67890,"modId":12345}]}"#)
        .create_async()
        .await;
    let project_mock = server
        .mock("GET", "/v1/mods/12345")
        .match_header("x-api-key", "test-api-key")
        .with_status(200)
        .with_body(r#"{"data":{"name":"Sodium","slug":"sodium","classId":6}}"#)
        .create_async()
        .await;

    let provider = TestNetworkProvider::new();
    let display = crate::display::LiveDisplayProvider::new();
    let budget: Arc<dyn RateBudget> = Arc::new(FixedWindowBudget::new(1, Duration::from_secs(2)));
    let registry = registry_with_budget("api.curseforge.com", budget);
    let api_bases = test_api_bases(&server.url(), &server.url());
    let manifest = manifest_with_content(vec![ContentEntry::PlatformReferenced(curseforge_pref(
        "",
        Some("67890"),
    ))]);

    let start = Instant::now();
    let resolved = resolve_manifest_with_api_bases(
        manifest,
        &provider,
        &provider,
        Some("test-api-key"),
        &display,
        &registry,
        api_bases,
    )
    .await
    .unwrap();
    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(900));
    assert!(resolved.warnings.is_empty());

    match &resolved.manifest.content[0] {
        ContentEntry::PlatformReferenced(pref) => {
            assert_eq!(pref.project_id, "12345");
            assert_eq!(pref.resolved_name.as_deref(), Some("Sodium"));
            assert_eq!(pref.resolved_slug.as_deref(), Some("sodium"));
            assert_eq!(pref.resolved_type, Some(crate::primitives::ProjectType::Mod));
        }
        _ => panic!("expected platform reference"),
    }

    batch_mock.assert_async().await;
    project_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_resolve_manifest_concurrent_modrinth_requests_share_budget_without_429s() {
    let mut server = mockito::Server::new_async().await;

    let version_mock = server
        .mock("GET", "/v2/version_file/deadbeef")
        .match_query(mockito::Matcher::UrlEncoded(
            "algorithm".to_string(),
            "sha1".to_string(),
        ))
        .with_status(200)
        .with_header("x-ratelimit-remaining", "5")
        .with_header("x-ratelimit-limit", "300")
        .with_header("x-ratelimit-reset", "1")
        .with_body(r#"{"id":"version-1"}"#)
        .create_async()
        .await;

    let mut project_mocks = Vec::new();
    let mut content = Vec::new();
    for index in 0..12 {
        let project_id = format!("project-{index:02}");
        project_mocks.push(
            server
                .mock("GET", format!("/v2/project/{project_id}").as_str())
                .with_status(200)
                .with_header("x-ratelimit-remaining", "5")
                .with_header("x-ratelimit-limit", "300")
                .with_header("x-ratelimit-reset", "1")
                .with_body(
                    serde_json::json!({
                        "title": format!("Project {index:02}"),
                        "slug": format!("project-{index:02}"),
                        "project_type": "mod"
                    })
                    .to_string(),
                )
                .create_async()
                .await,
        );

        let mut pref = modrinth_pref(&project_id);
        if index == 0 {
            pref.hashes
                .insert("sha1".to_string(), "deadbeef".to_string());
        }
        content.push(ContentEntry::PlatformReferenced(pref));
    }

    let provider = TestNetworkProvider::new();
    let display = crate::display::LiveDisplayProvider::new();
    let budget: Arc<dyn RateBudget> = Arc::new(HeaderDrivenBudget::new(300));
    let registry = registry_with_budget("api.modrinth.com", budget);
    let api_bases = test_api_bases(&server.url(), &server.url());
    let manifest = manifest_with_content(content);

    let start = Instant::now();
    let resolved = resolve_manifest_with_api_bases(
        manifest,
        &provider,
        &provider,
        None,
        &display,
        &registry,
        api_bases,
    )
    .await
    .unwrap();
    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(450));
    assert!(resolved.warnings.is_empty());
    assert_eq!(resolved.manifest.content.len(), 12);

    for (index, entry) in resolved.manifest.content.iter().enumerate() {
        match entry {
            ContentEntry::PlatformReferenced(pref) => {
                let expected_name = format!("Project {index:02}");
                let expected_slug = format!("project-{index:02}");
                assert_eq!(pref.resolved_name.as_deref(), Some(expected_name.as_str()));
                assert_eq!(pref.resolved_slug.as_deref(), Some(expected_slug.as_str()));
                assert_eq!(pref.resolved_type, Some(crate::primitives::ProjectType::Mod));
                if index == 0 {
                    assert_eq!(pref.file_id.as_deref(), Some("version-1"));
                }
            }
            _ => panic!("expected platform reference"),
        }
    }

    version_mock.assert_async().await;
    for project_mock in project_mocks {
        project_mock.assert_async().await;
    }
}
