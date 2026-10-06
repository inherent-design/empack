//! Declared library runtimes for Fabric and Quilt, including historical Fabric shading.
use super::*;
use crate::engine::{acquisition::TransferError, content::verify_stream, layout::CollisionIndex};
use empack_core::digest::ExpectedDigest;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibraryKind {
    Fabric,
    Quilt,
}
impl LibraryKind {
    fn from_loader(loader: LoaderKind) -> Result<Self> {
        match loader {
            LoaderKind::Fabric => Ok(Self::Fabric),
            LoaderKind::Quilt => Ok(Self::Quilt),
            _ => anyhow::bail!("Loader does not use a supported library runtime"),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
        }
    }
    fn coordinate(self) -> (&'static str, &'static str) {
        match self {
            Self::Fabric => ("net.fabricmc", "fabric-loader"),
            Self::Quilt => ("org.quiltmc", "quilt-loader"),
        }
    }
    fn catalog(self) -> &'static str {
        match self {
            Self::Fabric => "https://meta.fabricmc.net/v2/versions/loader/",
            Self::Quilt => "https://meta.quiltmc.org/v3/versions/loader/",
        }
    }
}
#[derive(Clone)]
pub(super) struct LauncherContract {
    kind: LibraryKind,
    launch_main: String,
    declared_main: Option<String>,
    shaded: bool,
}
pub struct LibraryServerPlan {
    runtime: RuntimeResolution,
    metadata: ContentId,
    launcher: LauncherContract,
    libraries: Vec<LibraryPlan>,
}
#[derive(Clone)]
struct LibraryPlan {
    name: String,
    path: PortableRelPath,
    url: String,
    expected: ExpectedContent,
    digest_documents: Vec<ContentId>,
}
#[derive(Debug, Clone)]
pub struct LibraryFileEvidence {
    pub name: String,
    pub path: PortableRelPath,
    pub expected: ExpectedContent,
    pub actual: ContentId,
    pub digest_documents: Vec<ContentId>,
}
#[derive(Debug, Clone)]
pub struct LibraryRuntimeEvidence {
    pub metadata: ContentId,
    pub family: LoaderKind,
    pub shaded: bool,
    pub libraries: Vec<LibraryFileEvidence>,
    pub launcher: ContentId,
    pub main_class: String,
}

impl LibraryServerPlan {
    pub async fn resolve(
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        runtime: RuntimeResolution,
        limits: TransferLimits,
    ) -> Result<Self> {
        let kind = LibraryKind::from_loader(runtime.loader)?;
        let loader = runtime
            .loader_version
            .as_ref()
            .context("Library runtime requires an exact loader version")?;
        let mut url = reqwest::Url::parse(kind.catalog())?;
        url.path_segments_mut()
            .expect("static base")
            .pop_if_empty()
            .push(runtime.minecraft.as_str())
            .push(loader.as_str())
            .push("server")
            .push("json");
        let metadata = metadata_document(transport, scope, url.into(), 16 << 20, limits).await?;
        let worker =
            scope.spawn_blocking(metadata_work(), ResourceRequest::default(), move |_| {
                Self::from_metadata(runtime, &metadata)
            })?;
        let mut plan = scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts()
            .0;
        for library in &mut plan.libraries {
            if library.expected.digests.is_some() {
                continue;
            }
            let mut declared = None;
            for algorithm in ["sha256", "sha1", "md5"] {
                let document = match metadata_document(
                    transport,
                    scope,
                    format!("{}.{}", library.url, algorithm),
                    4096,
                    limits,
                )
                .await
                {
                    Ok(document) => document,
                    Err(error)
                        if error
                            .downcast_ref::<TransferError>()
                            .is_some_and(|error| matches!(error, TransferError::NotFound)) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let id = document.lease().id();
                let worker = scope.spawn_blocking(
                    metadata_work(),
                    ResourceRequest::default(),
                    move |_| -> Result<ExpectedDigest> {
                        let mut bytes = String::new();
                        document.lease().open().read_to_string(&mut bytes)?;
                        // Maven repositories publish either a bare digest or checksum-tool output.
                        let hash = bytes
                            .split_whitespace()
                            .next()
                            .context("Empty library checksum document")?;
                        Ok(ExpectedDigest::parse(algorithm, hash)?)
                    },
                )?;
                declared = Some(
                    scope
                        .accept(worker.wait().await?)?
                        .transpose()?
                        .into_parts()
                        .0,
                );
                library.digest_documents.push(id);
                break;
            }
            library.expected.digests =
                Some(DigestSet::new(vec![declared.context(
                    "Library runtime library has no declared digest",
                )?])?);
        }
        Ok(plan)
    }
    fn from_metadata(runtime: RuntimeResolution, metadata: &AcquiredContent) -> Result<Self> {
        let kind = LibraryKind::from_loader(runtime.loader)?;
        let loader = runtime
            .loader_version
            .as_ref()
            .context("Library runtime requires an exact loader version")?;
        let shaded = kind == LibraryKind::Fabric
            && semver::Version::parse(loader.as_str())? <= semver::Version::new(0, 12, 5);
        let value: Value = serde_json::from_reader(metadata.lease().open())?;
        ensure!(
            value["inheritsFrom"] == runtime.minecraft.as_str()
                && value["id"]
                    == format!(
                        "{}-loader-{}-{}",
                        kind.name(),
                        loader.as_str(),
                        runtime.minecraft.as_str()
                    ),
            "Library runtime metadata differs from selected runtime"
        );
        let launch_main = value["mainClass"]
            .as_str()
            .context("Library runtime metadata lacks server main class")?
            .to_owned();
        class_path(&launch_main)?;
        let declared_main = if kind == LibraryKind::Quilt {
            let main = value["launcherMainClass"]
                .as_str()
                .context("Quilt metadata lacks launcher main class")?
                .to_owned();
            class_path(&main)?;
            Some(main)
        } else {
            None
        };
        for argument in ["game", "jvm"] {
            ensure!(
                value["arguments"][argument].is_null()
                    || value["arguments"][argument]
                        .as_array()
                        .is_some_and(|args| args.is_empty()),
                "Library runtime metadata declares unsupported launch arguments"
            );
        }
        let libraries = value["libraries"]
            .as_array()
            .context("Library runtime metadata lacks libraries")?;
        ensure!(
            !libraries.is_empty() && libraries.len() <= 512,
            "Library runtime library count exceeds limit"
        );
        let mut planned = Vec::new();
        let mut collisions = CollisionIndex::default();
        let mut loader_count = 0;
        let mut intermediary_count = 0;
        for library in libraries {
            ensure!(
                library.get("rules").is_none() && library.get("natives").is_none(),
                "Library runtime server library declares unsupported conditional selection"
            );
            let name = library["name"]
                .as_str()
                .context("Library lacks Maven identity")?;
            let (relative, coordinate) = maven_path(name)?;
            if (coordinate[0], coordinate[1]) == kind.coordinate() {
                ensure!(
                    coordinate[2] == loader.as_str(),
                    "Library runtime library version differs from selected loader"
                );
                loader_count += 1;
            }
            if coordinate[0] == "org.quiltmc" && coordinate[1] == "hashed" {
                ensure!(
                    coordinate[2] == runtime.minecraft.as_str(),
                    "Quilt mappings differ from selected game"
                );
            }
            if coordinate[0] == "net.fabricmc" && coordinate[1] == "intermediary" {
                ensure!(
                    coordinate[2] == runtime.minecraft.as_str(),
                    "Intermediary differs from selected game"
                );
                intermediary_count += 1;
            }
            let mut url = reqwest::Url::parse(
                library["url"]
                    .as_str()
                    .context("Library lacks repository URL")?,
            )?;
            validate_download_url(url.as_str())?;
            ensure!(
                url.query().is_none() && url.path().ends_with('/'),
                "Library repository is not a base URL"
            );
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Library repository cannot contain path segments"))?
                .pop_if_empty()
                .extend(relative.split('/'));
            let path = PortableRelPath::parse(
                &format!("libraries/{relative}"),
                PathSyntax::ArchiveMember,
            )?;
            collisions.insert_file(&path)?;
            let mut digests = Vec::new();
            for algorithm in ["md5", "sha1", "sha256", "sha512"] {
                if let Some(raw) = library.get(algorithm) {
                    digests.push(ExpectedDigest::parse(
                        algorithm,
                        raw.as_str().context("Library digest is not text")?,
                    )?);
                }
            }
            let size = library
                .get("size")
                .map(|size| {
                    size.as_u64()
                        .filter(|size| *size > 0)
                        .context("Library size is not positive")
                })
                .transpose()?;
            planned.push(LibraryPlan {
                name: name.into(),
                path,
                url: url.into(),
                expected: ExpectedContent {
                    digests: if digests.is_empty() {
                        None
                    } else {
                        Some(DigestSet::new(digests)?)
                    },
                    size,
                    accepted_observation: None,
                },
                digest_documents: Vec::new(),
            });
        }
        ensure!(
            loader_count == 1 && intermediary_count == 1,
            "Library runtime metadata must select one loader and intermediary"
        );
        Ok(Self {
            runtime,
            metadata: metadata.lease().id(),
            launcher: LauncherContract {
                kind,
                launch_main,
                declared_main,
                shaded,
            },
            libraries: planned,
        })
    }
    pub async fn acquire(
        self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        transfer: TransferLimits,
        archive: ArchiveLimits,
        policy: SourceEvidencePolicy,
    ) -> Result<PreparedServerRuntime> {
        let vanilla = VanillaServerPlan::resolve(
            transport,
            scope,
            RuntimeResolution {
                minecraft: self.runtime.minecraft.clone(),
                loader: LoaderKind::Vanilla,
                loader_version: None,
            },
            transfer,
        )
        .await?
        .acquire(transport, scope, transfer, archive, policy)
        .await?;
        let maximum = scope.available_scratch_bytes();
        let mut pool = crate::engine::content::ContentPool::owned(scope, maximum).await?;
        let mut libraries = Vec::new();
        for library in &self.libraries {
            let content = transport
                .acquire(
                    scope,
                    DownloadRequest {
                        alternatives: NonEmpty::new(vec![library.url.clone()])?,
                        expected: library.expected.clone(),
                        limits: transfer,
                        evidence: policy,
                        initial: InitialObservation::RequireEvidence,
                    },
                )
                .await?;
            libraries.push(pool.consolidate_owned(scope, content).await?);
        }
        self.finish(vanilla, libraries, scope, archive).await
    }
}
pub(super) async fn metadata_document(
    transport: &HttpAcquisition,
    scope: &mut WorkScope,
    url: String,
    maximum: u64,
    limits: TransferLimits,
) -> Result<AcquiredContent> {
    transport
        .acquire(
            scope,
            DownloadRequest {
                alternatives: NonEmpty::new(vec![url])?,
                expected: ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                limits: TransferLimits {
                    file_bytes: limits.file_bytes.min(maximum),
                    transfer_bytes: limits.transfer_bytes.min(maximum),
                    ..limits
                },
                evidence: SourceEvidencePolicy::Compatibility,
                initial: InitialObservation::Accepted,
            },
        )
        .await
}
fn maven_path(name: &str) -> Result<(String, Vec<&str>)> {
    let parts: Vec<_> = name.split(':').collect();
    ensure!(
        parts.len() == 3
            && parts.iter().all(|part| !part.is_empty()
                && part.len() <= 256
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte)))
            && parts[0].split('.').all(|part| !part.is_empty()),
        "Invalid runtime Maven coordinate"
    );
    Ok((
        format!(
            "{}/{}/{}/{}-{}.jar",
            parts[0].replace('.', "/"),
            parts[1],
            parts[2],
            parts[1],
            parts[2]
        ),
        parts,
    ))
}
fn class_path(name: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name.split('.').all(|part| !part.is_empty()
                && part
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || b"_$".contains(&ch))),
        "Invalid runtime main class"
    );
    Ok(format!("{}.class", name.replace('.', "/")))
}

impl LibraryServerPlan {
    async fn finish(
        self,
        mut vanilla: PreparedServerRuntime,
        libraries: Vec<AcquiredContent>,
        scope: &mut WorkScope,
        limits: ArchiveLimits,
    ) -> Result<PreparedServerRuntime> {
        ensure!(
            vanilla.runtime.loader == LoaderKind::Vanilla
                && vanilla.runtime.minecraft == self.runtime.minecraft,
            "Library runtime base runtime differs from selected Minecraft"
        );
        ensure!(
            libraries.len() == self.libraries.len(),
            "Library runtime library acquisition is incomplete"
        );
        let mut evidence = Vec::new();
        for (plan, content) in self.libraries.iter().zip(&libraries) {
            ensure!(
                plan.expected
                    .size
                    .is_none_or(|size| size == content.lease().len()),
                "Library runtime library size differs from declaration"
            );
            plan.expected
                .digests
                .as_ref()
                .context("Library runtime library lacks source evidence")?
                .check(content.observed_digests().values())?;
            evidence.push(LibraryFileEvidence {
                name: plan.name.clone(),
                path: plan.path.clone(),
                expected: plan.expected.clone(),
                actual: content.lease().id(),
                digest_documents: plan.digest_documents.clone(),
            });
        }
        let maximum = limits
            .compressed_bytes
            .min(scope.available_scratch_bytes() / 2);
        ensure!(
            maximum > 0,
            "No scratch allowance for Fabric launcher preparation"
        );
        let retained = ResourceRequest {
            scratch_bytes: maximum,
            open_files: 1,
            ..ResourceRequest::default()
        };
        let worker_libraries = libraries.clone();
        let paths = self
            .libraries
            .iter()
            .map(|library| library.path.clone())
            .collect::<Vec<_>>();
        let contract = self.launcher.clone();
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 20,
                scratch_bytes: maximum * 2,
                open_files: 5,
            },
            retained,
            move |cancel| {
                launcher::assemble(
                    &paths,
                    &worker_libraries,
                    &contract,
                    limits,
                    maximum,
                    &cancel,
                )
            },
        )?;
        let output = scope.accept(worker.wait().await?)?.transpose()?;
        let mut main = String::new();
        let launcher = AcquiredContent::retain_resources(output.map(|(content, main_class)| {
            main = main_class;
            content
        }))?;
        let launcher_id = launcher.lease().id();
        let game = vanilla
            .files
            .remove(&PortableRelPath::parse(
                "server.jar",
                PathSyntax::ProjectContent,
            )?)
            .context("Vanilla runtime omitted server JAR")?;
        let permissions = game.permissions;
        vanilla.files.insert(
            PortableRelPath::parse("minecraft-server.jar", PathSyntax::ProjectContent)?,
            game,
        );
        for (plan, content) in self.libraries.iter().zip(libraries) {
            vanilla.files.insert(
                plan.path.clone(),
                AcquiredBuildFile {
                    content,
                    permissions,
                },
            );
        }
        vanilla.files.insert(
            PortableRelPath::parse("server.jar", PathSyntax::ProjectContent)?,
            AcquiredBuildFile {
                content: launcher,
                permissions,
            },
        );
        let properties = b"serverJar=minecraft-server.jar\n";
        let properties = verify_stream(
            &mut properties.as_slice(),
            &ExpectedContent {
                digests: None,
                size: Some(properties.len() as u64),
                accepted_observation: None,
            },
            4096,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &scope.cancellation(),
        )?;
        vanilla.files.insert(
            PortableRelPath::parse(
                &format!("{}-server-launcher.properties", self.launcher.kind.name()),
                PathSyntax::ProjectContent,
            )?,
            AcquiredBuildFile {
                content: properties,
                permissions,
            },
        );
        let family = self.runtime.loader;
        vanilla.runtime = self.runtime;
        vanilla.evidence.loader = Some(LoaderRuntimeEvidence::Libraries(LibraryRuntimeEvidence {
            metadata: self.metadata,
            family,
            shaded: self.launcher.shaded,
            libraries: evidence,
            launcher: launcher_id,
            main_class: main,
        }));
        Ok(vanilla)
    }
}
mod launcher;
#[cfg(test)]
mod tests;
