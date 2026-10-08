//! Read-only official runtime discovery. Catalog observations never replace executable byte proof.
use super::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    model::{ExpectedContent, GameVersion, LoaderKind, LoaderVersion, NonEmpty, RuntimeResolution},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub struct RuntimeCatalogLimits {
    pub transfer: TransferLimits,
    pub entries: usize,
}
impl Default for RuntimeCatalogLimits {
    fn default() -> Self {
        Self {
            transfer: TransferLimits {
                file_bytes: 16 << 20,
                transfer_bytes: 16 << 20,
                ..Default::default()
            },
            entries: 100_000,
        }
    }
}
/// Observed version choices remain bound to the same catalog response while the host decides.
pub struct GameVersions {
    catalog: ContentId,
    latest_release: GameVersion,
    versions: Vec<GameVersion>,
}
impl GameVersions {
    pub fn catalog(&self) -> &ContentId {
        &self.catalog
    }
    pub fn versions(&self) -> &[GameVersion] {
        &self.versions
    }
    pub fn resolve(&self, requested: Option<&GameVersion>) -> Result<GameVersion> {
        let selected = requested.unwrap_or(&self.latest_release);
        ensure!(
            self.versions.contains(selected),
            "Minecraft version is absent from the official catalog"
        );
        Ok(selected.clone())
    }
}
pub struct LoaderVersions {
    catalog: ContentId,
    game: GameVersion,
    loader: LoaderKind,
    versions: Vec<LoaderVersion>,
}
impl LoaderVersions {
    pub fn catalog(&self) -> &ContentId {
        &self.catalog
    }
    pub fn versions(&self) -> &[LoaderVersion] {
        &self.versions
    }
    pub fn resolve(&self, requested: Option<&LoaderVersion>) -> Result<RuntimeResolution> {
        let normalized = requested
            .map(|value| {
                if self.loader == LoaderKind::Forge {
                    LoaderVersion::parse(
                        &crate::empack::versions::canonicalize_forge_loader_version(
                            self.game.as_str(),
                            value.as_str(),
                        ),
                    )
                } else {
                    Ok(value.clone())
                }
            })
            .transpose()?;
        let selected = normalized
            .as_ref()
            .or_else(|| self.versions.first())
            .context("No compatible loader versions are available")?;
        ensure!(
            self.versions.contains(selected),
            "Requested loader version is absent from the compatible catalog"
        );
        Ok(RuntimeResolution {
            minecraft: self.game.clone(),
            loader: self.loader,
            loader_version: Some(selected.clone()),
        })
    }
}
#[derive(Clone)]
struct Endpoints {
    games: String,
    fabric: String,
    quilt: String,
    forge: String,
    neoforge: String,
    legacy_neoforge: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            games: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into(),
            fabric: "https://meta.fabricmc.net/v2/versions/loader/".into(),
            quilt: "https://meta.quiltmc.org/v3/versions/loader/".into(),
            forge: "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json"
                .into(),
            neoforge:
                "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge"
                    .into(),
            legacy_neoforge:
                "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge".into(),
        }
    }
}
/// No project, persistent cache or tool capability is available to discovery.
#[derive(Clone)]
pub struct RuntimeCatalog {
    transport: HttpAcquisition,
    endpoints: Endpoints,
}
impl RuntimeCatalog {
    #[cfg(test)]
    pub(crate) fn for_loopback_tests(base: &str) -> Self {
        Self {
            transport: HttpAcquisition::for_loopback_tests(),
            endpoints: Endpoints {
                games: format!("{base}/games"),
                fabric: format!("{base}/fabric/"),
                quilt: format!("{base}/quilt/"),
                forge: format!("{base}/forge"),
                neoforge: format!("{base}/neoforge"),
                legacy_neoforge: format!("{base}/legacy-neoforge"),
            },
        }
    }
    #[cfg(test)]
    pub(crate) fn with_loader_test_origin(mut self, family: LoaderKind, base: &str) -> Self {
        match family {
            LoaderKind::Fabric => self.endpoints.fabric = format!("{base}/fabric/"),
            LoaderKind::Quilt => self.endpoints.quilt = format!("{base}/quilt/"),
            LoaderKind::Forge => self.endpoints.forge = format!("{base}/forge"),
            LoaderKind::NeoForge => {
                self.endpoints.neoforge = format!("{base}/neoforge");
                self.endpoints.legacy_neoforge = format!("{base}/legacy-neoforge");
            }
            LoaderKind::Vanilla => panic!("Vanilla has no loader endpoint"),
        }
        self
    }
    pub fn new(transport: HttpAcquisition) -> Self {
        Self {
            transport,
            endpoints: Endpoints::default(),
        }
    }
    pub async fn games(
        &self,
        scope: &mut WorkScope,
        limits: RuntimeCatalogLimits,
    ) -> Result<RetainedOutput<GameVersions>> {
        self.read(
            scope,
            self.endpoints.games.clone(),
            limits,
            move |content| parse_games(content, limits.entries),
        )
        .await
    }
    /// Vanilla has no loader catalog. Its exact game selection comes from `GameVersions`.
    pub async fn loaders(
        &self,
        scope: &mut WorkScope,
        game: GameVersion,
        loader: LoaderKind,
        limits: RuntimeCatalogLimits,
    ) -> Result<RetainedOutput<LoaderVersions>> {
        let endpoint = match loader {
            LoaderKind::Vanilla => anyhow::bail!("Vanilla does not have loader versions"),
            LoaderKind::Fabric => &self.endpoints.fabric,
            LoaderKind::Quilt => &self.endpoints.quilt,
            LoaderKind::Forge => &self.endpoints.forge,
            LoaderKind::NeoForge if game.as_str() == "1.20.1" => &self.endpoints.legacy_neoforge,
            LoaderKind::NeoForge => &self.endpoints.neoforge,
        };
        let mut url = reqwest::Url::parse(endpoint)?;
        if matches!(loader, LoaderKind::Fabric | LoaderKind::Quilt) {
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid runtime catalog endpoint"))?
                .pop_if_empty()
                .push(game.as_str());
        }
        self.read(scope, url.to_string(), limits, move |content| {
            parse_loaders(content, game, loader, limits.entries)
        })
        .await
    }
    async fn read<T: Send + 'static>(
        &self,
        scope: &mut WorkScope,
        url: String,
        limits: RuntimeCatalogLimits,
        parse: impl FnOnce(AcquiredContent) -> Result<T> + Send + 'static,
    ) -> Result<RetainedOutput<T>> {
        let content = self
            .transport
            .acquire(
                scope,
                DownloadRequest {
                    alternatives: NonEmpty::new(vec![url])?,
                    expected: ExpectedContent {
                        digests: None,
                        size: None,
                        accepted_observation: None,
                    },
                    limits: limits.transfer,
                    evidence: SourceEvidencePolicy::Compatibility,
                    // This observation identifies the HTTPS response, not any future server JAR.
                    initial: InitialObservation::Accepted,
                },
            )
            .await?;
        let memory = content
            .lease()
            .len()
            .checked_mul(16)
            .and_then(|bytes| bytes.checked_add(64 << 10))
            .context("Runtime catalog memory estimate overflow")?;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: memory,
                open_files: 1,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: memory,
                ..Default::default()
            },
            move |cancel| {
                cancel.check()?;
                let result = parse(content)?;
                cancel.check()?;
                Ok::<_, anyhow::Error>(result)
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }
}
#[derive(Deserialize)]
struct GameManifest {
    latest: Latest,
    versions: Vec<GameEntry>,
}
#[derive(Deserialize)]
struct Latest {
    release: String,
}
#[derive(Deserialize)]
struct GameEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}
fn parse_games(content: AcquiredContent, limit: usize) -> Result<GameVersions> {
    let manifest: GameManifest = serde_json::from_reader(content.lease().open())?;
    ensure!(
        manifest.versions.len() <= limit,
        "Minecraft catalog exceeds entry limit"
    );
    let latest_release = GameVersion::parse(&manifest.latest.release)?;
    let mut seen = BTreeSet::new();
    let mut versions = Vec::new();
    let mut releases = Vec::new();
    for item in manifest.versions {
        let version = GameVersion::parse(&item.id)?;
        ensure!(
            seen.insert(version.clone()),
            "Minecraft catalog repeats a version"
        );
        if item.kind == "release" {
            releases.push(version);
        } else {
            versions.push(version);
        }
    }
    ensure!(
        releases.contains(&latest_release),
        "Minecraft catalog latest release is missing or not a release"
    );
    releases.extend(versions);
    Ok(GameVersions {
        catalog: content.lease().id(),
        latest_release,
        versions: releases,
    })
}
#[derive(Deserialize)]
struct LoaderCombination {
    loader: LoaderEntry,
    intermediary: Intermediary,
}
#[derive(Deserialize)]
struct LoaderEntry {
    version: String,
    stable: Option<bool>,
}
#[derive(Deserialize)]
struct Intermediary {
    version: String,
}
#[derive(Deserialize)]
struct NeoVersions {
    versions: Vec<String>,
}
fn parse_loaders(
    content: AcquiredContent,
    game: GameVersion,
    loader: LoaderKind,
    limit: usize,
) -> Result<LoaderVersions> {
    let choices: Vec<(String, bool)> = match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => {
            let values: Vec<LoaderCombination> = serde_json::from_reader(content.lease().open())?;
            ensure!(values.len() <= limit, "Loader catalog exceeds entry limit");
            values
                .into_iter()
                .map(|value| {
                    ensure!(
                        value.intermediary.version == game.as_str(),
                        "Loader catalog returned a different Minecraft version"
                    );
                    let stable = value
                        .loader
                        .stable
                        .unwrap_or_else(|| !value.loader.version.contains('-'));
                    Ok((value.loader.version, stable))
                })
                .collect::<Result<_>>()?
        }
        LoaderKind::Forge => {
            let values: BTreeMap<String, Vec<String>> =
                serde_json::from_reader(content.lease().open())?;
            ensure!(values.len() <= limit, "Forge catalog exceeds entry limit");
            ensure!(
                values
                    .values()
                    .try_fold(0usize, |sum, versions| sum.checked_add(versions.len()))
                    .is_some_and(|count| count <= limit),
                "Forge catalog exceeds entry limit"
            );
            let all = values.get(game.as_str()).cloned().unwrap_or_default();
            crate::empack::versions::filter_forge_versions_by_minecraft(&all, game.as_str())?
                .into_iter()
                .map(|version| {
                    let stable = !version.contains('-');
                    (version, stable)
                })
                .collect()
        }
        LoaderKind::NeoForge => {
            let values: NeoVersions = serde_json::from_reader(content.lease().open())?;
            ensure!(
                values.versions.len() <= limit,
                "NeoForge catalog exceeds entry limit"
            );
            let matched = if game.as_str() == "1.20.1" {
                crate::empack::versions::filter_forge_versions_by_minecraft(
                    &values.versions,
                    game.as_str(),
                )?
            } else {
                values
                    .versions
                    .into_iter()
                    .filter(|version| neoforge_matches(game.as_str(), version))
                    .collect()
            };
            matched
                .into_iter()
                .map(|version| {
                    let stable = !version.contains('-');
                    (version, stable)
                })
                .collect()
        }
        LoaderKind::Vanilla => anyhow::bail!("Vanilla does not have loader versions"),
    };
    let mut choices = choices;
    choices.sort_unstable_by(|(a, stable_a), (b, stable_b)| {
        stable_b.cmp(stable_a).then_with(|| compare_versions(b, a))
    });
    let mut seen = BTreeSet::new();
    let mut versions = Vec::new();
    for (value, _) in choices {
        let value = LoaderVersion::parse(&value)?;
        ensure!(
            seen.insert(value.clone()),
            "Loader catalog repeats a version"
        );
        versions.push(value);
    }
    Ok(LoaderVersions {
        catalog: content.lease().id(),
        game,
        loader,
        versions,
    })
}
fn neoforge_matches(game: &str, loader: &str) -> bool {
    let (base, suffix) = game.split_once('-').unwrap_or((game, ""));
    let components: Vec<_> = base.split('.').collect();
    if components.len() < 2
        || components.len() > 3
        || components.iter().any(|value| value.parse::<u32>().is_err())
    {
        return false;
    }
    let prefix = if components[0] == "1" {
        if components[1].parse::<u32>().unwrap() < 20 {
            return false;
        }
        format!("{}.{}.", components[1], components.get(2).unwrap_or(&"0"))
    } else {
        format!(
            "{}.{}.{}.",
            components[0],
            components[1],
            components.get(2).unwrap_or(&"0")
        )
    };
    let suffix_matches = match loader.split_once('+') {
        Some((_, declared)) => !suffix.is_empty() && declared == suffix,
        None => suffix.is_empty(),
    };
    loader.starts_with(&prefix) && suffix_matches
}
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // Year-style four-component releases need numeric comparison even with prerelease suffixes.
    let lexical = a.cmp(b);
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        let comparison = if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let n = a.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let m = b.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let left = &a[..n];
            let right = &b[..m];
            let left = &left[left.iter().take_while(|byte| **byte == b'0').count()..];
            let right = &right[right.iter().take_while(|byte| **byte == b'0').count()..];
            a = &a[n..];
            b = &b[m..];
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        } else {
            let ordering = a[0].cmp(&b[0]);
            a = &a[1..];
            b = &b[1..];
            ordering
        };
        if comparison != std::cmp::Ordering::Equal {
            return comparison;
        }
    }
    a.len().cmp(&b.len()).then(lexical)
}

#[cfg(test)]
mod tests;
