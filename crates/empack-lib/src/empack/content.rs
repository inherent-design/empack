use std::path::PathBuf;

use anyhow::Context;
use thiserror::Error;

use crate::application::session::NetworkProvider;
use crate::Result;

// ---------------------------------------------------------------------------
// SF-1: UrlKind classifier
// ---------------------------------------------------------------------------

/// Classification result for a user-supplied URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlKind {
    ModrinthModpack {
        slug: String,
        version: Option<String>,
    },
    ModrinthProject {
        slug: String,
    },
    CurseForgeModpack {
        slug: String,
    },
    CurseForgeProject {
        slug: String,
    },
    DirectDownload {
        url: String,
        extension: String,
    },
}

/// Errors produced by [`classify_url`].
#[derive(Debug, Error)]
pub enum UrlClassifyError {
    #[error("unrecognized URL: {0}")]
    Unrecognized(String),
}

/// Classify a URL into a [`UrlKind`] variant.
///
/// Matching is substring-based against known platform path segments.
/// The returned `extension` on `DirectDownload` omits the leading dot.
pub fn classify_url(url: &str) -> std::result::Result<UrlKind, UrlClassifyError> {
    if url.contains("modrinth.com/modpack/") {
        let slug_and_version = extract_path_after_segment(url, "modrinth.com/modpack/");
        let (slug, version) = split_slug_version(&slug_and_version);
        return Ok(UrlKind::ModrinthModpack { slug, version });
    }

    if url.contains("modrinth.com/mod/") {
        let slug = extract_after_segment(url, "modrinth.com/mod/");
        return Ok(UrlKind::ModrinthProject { slug });
    }

    if url.contains("modrinth.com/plugin/") {
        let slug = extract_after_segment(url, "modrinth.com/plugin/");
        return Ok(UrlKind::ModrinthProject { slug });
    }

    if url.contains("curseforge.com/minecraft/modpacks/") {
        let slug = extract_after_segment(url, "curseforge.com/minecraft/modpacks/");
        return Ok(UrlKind::CurseForgeModpack { slug });
    }

    if url.contains("curseforge.com/minecraft/mc-mods/") {
        let slug = extract_after_segment(url, "curseforge.com/minecraft/mc-mods/");
        return Ok(UrlKind::CurseForgeProject { slug });
    }

    if let Some(ext) = url_extension(url) {
        match ext.as_str() {
            "jar" | "zip" => {
                return Ok(UrlKind::DirectDownload {
                    url: url.to_string(),
                    extension: ext,
                });
            }
            _ => {}
        }
    }

    Err(UrlClassifyError::Unrecognized(url.to_string()))
}

fn extract_after_segment(url: &str, segment: &str) -> String {
    url.find(segment)
        .map(|idx| url[idx + segment.len()..].to_string())
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .split('#')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn extract_path_after_segment(url: &str, segment: &str) -> String {
    url.find(segment)
        .map(|idx| {
            url[idx + segment.len()..]
                .split('?')
                .next()
                .unwrap_or_default()
                .split('#')
                .next()
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string()
        })
        .unwrap_or_default()
}

fn split_slug_version(segment: &str) -> (String, Option<String>) {
    let clean = segment.split('?').next().unwrap_or(segment);
    let parts: Vec<&str> = clean.splitn(3, '/').collect();
    match parts.as_slice() {
        [slug, "version", version] if !version.is_empty() => {
            (slug.to_string(), Some(version.to_string()))
        }
        [slug, _other] => (slug.to_string(), None),
        _ => (segment.to_string(), None),
    }
}

fn url_extension(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let filename = path.rsplit('/').next().unwrap_or(path);
    let dot_pos = filename.rfind('.')?;
    let ext = filename.get(dot_pos + 1..)?;
    if ext.is_empty() { None } else { Some(ext.to_lowercase()) }
}

// ---------------------------------------------------------------------------
// SF-2: JarResolver
// ---------------------------------------------------------------------------

/// Request payload for [`JarResolver::identify`].
pub struct JarIdentifyRequest {
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

/// Identity resolved from a JAR file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JarIdentity {
    Modrinth {
        project_id: String,
        version_id: String,
        title: String,
    },
    CurseForge {
        project_id: u64,
        file_id: u64,
        title: String,
    },
    Unidentified,
}

/// Trait for identifying JAR files by their content hashes.
#[allow(async_fn_in_trait)]
pub trait JarResolver {
    fn identify(
        &self,
        request: JarIdentifyRequest,
    ) -> impl std::future::Future<Output = Result<JarIdentity>>;
}

/// Resolver that queries the Modrinth version-file endpoint and the
/// CurseForge fingerprint endpoint to identify a JAR.
pub struct ApiJarResolver<'a> {
    pub modrinth: &'a dyn NetworkProvider,
    pub curseforge: &'a dyn NetworkProvider,
    pub curseforge_api_key: Option<&'a str>,
}

impl ApiJarResolver<'_> {
    /// Attempt to identify a JAR via Modrinth's `/v2/version_file/{sha1}` endpoint.
    async fn query_modrinth(&self, sha1: &str) -> Result<Option<JarIdentity>> {
        let client = self.modrinth.http_client()?;
        let url = format!("https://api.modrinth.com/v2/version_file/{sha1}");

        let response = client.get(&url).send().await?;
        if !response.status().is_success() {
            return Ok(None);
        }

        #[derive(serde::Deserialize)]
        struct ModrinthVersionFile {
            project_id: String,
            version_id: String,
            name: String,
        }

        let body: ModrinthVersionFile = response.json().await?;
        Ok(Some(JarIdentity::Modrinth {
            project_id: body.project_id,
            version_id: body.version_id,
            title: body.name,
        }))
    }

    /// Attempt to identify a JAR via CurseForge's fingerprint endpoint.
    /// POST https://api.curseforge.com/v1/fingerprints with Murmur2 hash.
    async fn query_curseforge(&self, murmur2_hash: u32) -> Result<Option<JarIdentity>> {
        let api_key = match self.curseforge_api_key {
            Some(k) => k,
            None => return Ok(None),
        };

        let client = self.curseforge.http_client()?;

        let response = client
            .post("https://api.curseforge.com/v1/fingerprints")
            .header("x-api-key", api_key)
            .json(&serde_json::json!({ "fingerprints": [murmur2_hash] }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        #[derive(serde::Deserialize)]
        struct DataEnvelope {
            data: FingerprintData,
        }

        #[derive(serde::Deserialize)]
        struct FingerprintData {
            #[serde(rename = "exactMatches", default)]
            exact_matches: Vec<ExactMatch>,
        }

        #[derive(serde::Deserialize)]
        struct ExactMatch {
            id: u64,
            file: ExactMatchFile,
        }

        #[derive(serde::Deserialize)]
        struct ExactMatchFile {
            #[serde(rename = "modId")]
            mod_id: u64,
            #[serde(rename = "displayName")]
            display_name: String,
        }

        let envelope: DataEnvelope = response.json().await?;
        if let Some(match_) = envelope.data.exact_matches.into_iter().next() {
            return Ok(Some(JarIdentity::CurseForge {
                project_id: match_.file.mod_id,
                file_id: match_.id,
                title: match_.file.display_name,
            }));
        }

        Ok(None)
    }
}

impl JarResolver for ApiJarResolver<'_> {
    async fn identify(&self, request: JarIdentifyRequest) -> Result<JarIdentity> {
        let bytes = if request.sha1.is_none() {
            Some(
                std::fs::read(&request.path)
                    .with_context(|| format!("reading JAR file: {:?}", request.path))?,
            )
        } else {
            None
        };

        let sha1 = match request.sha1 {
            Some(h) => h,
            None => compute_sha1_hex(bytes.as_ref().unwrap()),
        };

        if let Some(identity) = self.query_modrinth(&sha1).await? {
            return Ok(identity);
        }

        let bytes = match bytes {
            Some(b) => b,
            None => {
                std::fs::read(&request.path)
                    .with_context(|| format!("reading JAR file: {:?}", request.path))?
            }
        };
        // CurseForge fingerprint API requires whitespace bytes stripped before hashing
        let filtered: Vec<u8> = bytes
            .iter()
            .copied()
            .filter(|&b| b != 9 && b != 10 && b != 13 && b != 32)
            .collect();
        let murmur2_hash = murmur2::murmur2(&filtered, 1);

        if let Some(identity) = self.query_curseforge(murmur2_hash).await? {
            return Ok(identity);
        }

        Ok(JarIdentity::Unidentified)
    }
}

fn compute_sha1_hex(data: &[u8]) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(HEX_CHARS[(b >> 4) as usize] as char);
            out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// SF-3: Type system extensions
// ---------------------------------------------------------------------------

/// Whether content is required, optional, unsupported, or unknown for a side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideRequirement {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

/// Client and server side requirements for a piece of content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideEnv {
    pub client: SideRequirement,
    pub server: SideRequirement,
}

/// Which side an override directory targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideSide {
    Both,
    ClientOnly,
    ServerOnly,
}

/// Semantic category of an override file or directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideCategory {
    Config,
    Script,
    ResourcePack,
    ShaderPack,
    DataPack,
    World,
    ServerConfig,
    ClientConfig,
    ModData,
    Other,
}

#[cfg(test)]
mod tests {
    include!("content.test.rs");
}
