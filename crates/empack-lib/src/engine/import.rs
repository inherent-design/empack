//! Archive adapters return declarations and provenance, never project writes or backend commands.
use super::{
    archive_source::ZipContentSource,
    artifacts::ArchiveLimits,
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
    layout::CollisionIndex,
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::FilePermissions,
    model::{
        ContentLayer, Coverage, ExpectedContent, GameVersion, LoaderKind, LoaderVersion,
        ResolvedPin,
    },
    path::{InstallDestination, PathSyntax, PortableRelPath},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

mod acquisition;
pub use acquisition::{
    ImportContentInput, ImportContentKey, ImportContentLimits, ImportContentOutcome,
    ImportContentPlan, ImportInputReason, VerifiedImportContent,
};
mod formats;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Modrinth,
    CurseForge,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportLocation {
    pub member: PortableRelPath,
    /// JSON pointer for manifest records; absent for an archive member itself.
    pub pointer: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedRequirement {
    Required,
    Optional,
    Unsupported,
}
/// Formats do not specify optional defaults or descriptions. Do not invent either during parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRequirements {
    pub client: ImportedRequirement,
    pub server: ImportedRequirement,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedLoader {
    pub kind: LoaderKind,
    pub version: LoaderVersion,
    pub primary: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRuntime {
    pub minecraft: GameVersion,
    /// Empty means vanilla. Multiple declarations remain an explicit selection obligation.
    pub loaders: Vec<ImportedLoader>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
}
/// No Debug: imported locators may contain sensitive input before durable-URL validation.
#[derive(Clone)]
pub enum ImportedAcquisition {
    Downloads(Vec<String>),
    Embedded(PortableRelPath),
}
#[derive(Clone)]
pub struct ImportedFile {
    pub destination: InstallDestination,
    pub layer: ContentLayer,
    pub requirements: ImportedRequirements,
    pub expected: ExpectedContent,
    pub acquisition: ImportedAcquisition,
    /// Known for embedded members; remote formats do not declare portable permissions.
    pub permissions: Option<FilePermissions>,
    pub source: ImportLocation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProvider {
    pub selection: ResolvedPin,
    pub requirements: ImportedRequirements,
    pub source: ImportLocation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    pub location: ImportLocation,
    pub message: String,
}
/// This is input evidence, not a resolved lock or a publication plan.
pub struct ImportedProject {
    pub format: ImportFormat,
    pub metadata: ImportedMetadata,
    pub runtime: ImportedRuntime,
    pub providers: Vec<ImportedProvider>,
    /// Manifest-declared files. Common overrides may intentionally replace these destinations.
    pub files: Vec<ImportedFile>,
    pub overrides: Vec<ImportedFile>,
    pub dependency_coverage: Coverage,
    pub diagnostics: Vec<ImportDiagnostic>,
    /// Entries outside the recognized manifest/content namespaces are retained as evidence.
    pub auxiliary_members: Vec<PortableRelPath>,
    source: AcquiredContent,
}
impl ImportedProject {
    pub fn source_id(&self) -> ContentId {
        self.source.lease().id()
    }
    /// The original immutable archive remains available for later verified member acquisition.
    pub fn archive(&self) -> &AcquiredContent {
        &self.source
    }
    /// Pure layout suggestions with their source records. Conflicting suggestions remain choices;
    /// neither a filename nor the presence of a loader directory changes backend configuration.
    pub fn datapack_layout_proposals(&self) -> BTreeMap<PortableRelPath, Vec<ImportLocation>> {
        let mut proposals = BTreeMap::<PortableRelPath, Vec<ImportLocation>>::new();
        for file in self.files.iter().chain(&self.overrides) {
            let destination = file.destination.relative().as_str();
            for folder in [
                "config/paxi/datapacks",
                "config/openloader/data",
                "datapacks",
            ] {
                if destination.starts_with(&format!("{folder}/")) {
                    proposals
                        .entry(path(folder).expect("static portable folder"))
                        .or_default()
                        .push(file.source.clone());
                }
            }
        }
        proposals
    }
}
#[derive(Clone, Copy)]
pub struct ImportLimits {
    pub archive: ArchiveLimits,
    pub manifest_bytes: u64,
    pub records: usize,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            archive: ArchiveLimits {
                compressed_bytes: 2 << 30,
                ..ArchiveLimits::default()
            },
            manifest_bytes: 16 << 20,
            records: 100_000,
        }
    }
}
/// Work is owned by the caller's operation and has no live-project or HTTP capability.
/// Source acquisition must already have enforced the compressed-input bound while streaming.
pub async fn inspect_import(
    scope: &mut WorkScope,
    source: AcquiredContent,
    limits: ImportLimits,
) -> Result<RetainedOutput<ImportedProject>> {
    ensure!(
        limits.manifest_bytes > 0 && limits.records > 0,
        "Import limits must be positive"
    );
    // Account for parsed strings, archive names and destination indexes as scheduling estimates.
    // The archive reader independently bounds compressed/expanded bytes, entries and depth.
    let memory = limits
        .manifest_bytes
        .checked_mul(16)
        .and_then(|v| v.checked_add((limits.archive.entries as u64).checked_mul(2048)?))
        .context("Import memory estimate overflow")?;
    let retained = ResourceRequest {
        memory_bytes: memory,
        ..Default::default()
    };
    let worker = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            open_files: 4,
            scratch_bytes: limits.manifest_bytes,
            ..retained
        },
        retained,
        move |cancel| inspect(source, limits, &cancel),
    )?;
    scope.accept(worker.wait().await?)?.transpose()
}
fn path(value: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(value, PathSyntax::ArchiveMember)?)
}
fn location(member: &str, pointer: Option<String>) -> Result<ImportLocation> {
    Ok(ImportLocation {
        member: path(member)?,
        pointer,
    })
}
fn inspect(
    source: AcquiredContent,
    limits: ImportLimits,
    cancel: &Cancellation,
) -> Result<ImportedProject> {
    cancel.check()?;
    let mut archive = ZipContentSource::open(&source, limits.archive, cancel)?;
    let members: BTreeMap<_, _> = archive
        .files()
        .map(|(path, entry)| (path.clone(), *entry))
        .collect();
    let names: BTreeSet<_> = members.keys().cloned().collect();
    let has_mr = names.contains(&path("modrinth.index.json")?);
    let has_cf = names.contains(&path("manifest.json")?);
    ensure!(
        !(has_mr && has_cf),
        "Archive contains competing import manifests"
    );
    let (format, manifest) = match (has_mr, has_cf) {
        (true, false) => (ImportFormat::Modrinth, "modrinth.index.json"),
        (false, true) => (ImportFormat::CurseForge, "manifest.json"),
        _ if names.contains(&path("pack.toml")?) => {
            anyhow::bail!("Packwiz import is recognized but not implemented")
        }
        _ => anyhow::bail!("Archive has no supported import manifest"),
    };
    let member = path(manifest)?;
    let size = members
        .get(&member)
        .context("Import manifest disappeared")?
        .bytes;
    ensure!(
        size <= limits.manifest_bytes,
        "Import manifest exceeds byte limit"
    );
    let (content, _) = archive.acquire(
        &member,
        &ExpectedContent {
            digests: None,
            size: Some(size),
            accepted_observation: None,
        },
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        cancel,
    )?;
    let mut bytes = Vec::new();
    content.lease().open().read_to_end(&mut bytes)?;
    cancel.check()?;
    let (mut project, roots) = formats::parse(format, &bytes, source, limits.records, cancel)
        .with_context(|| format!("Invalid import manifest {manifest}"))?;
    let mut used = BTreeSet::from([member]);
    for file in &mut project.files {
        if let ImportedAcquisition::Embedded(member) = &file.acquisition {
            let entry = members.get(member).with_context(|| {
                format!(
                    "{}: declared embedded member is absent",
                    file.source.pointer.as_deref().unwrap_or("/files")
                )
            })?;
            ensure!(
                file.expected.size.is_none_or(|size| size == entry.bytes),
                "Embedded declaration differs from archive size"
            );
            file.permissions = Some(entry.permissions);
            used.insert(member.clone());
        }
    }
    for (member, entry) in archive.files() {
        cancel.check()?;
        for (root, layer) in &roots {
            let Some(destination) = member.as_str().strip_prefix(&format!("{}/", root.as_str()))
            else {
                continue;
            };
            ensure!(
                used.insert(member.clone()),
                "Import content roots overlap or reuse a declared source"
            );
            let destination = InstallDestination::parse(destination)?;
            let requirements = match layer {
                ContentLayer::Common | ContentLayer::CommonOverride => {
                    ImportedRequirements::required()
                }
                ContentLayer::Client => ImportedRequirements {
                    client: ImportedRequirement::Required,
                    server: ImportedRequirement::Unsupported,
                },
                ContentLayer::Server => ImportedRequirements {
                    client: ImportedRequirement::Unsupported,
                    server: ImportedRequirement::Required,
                },
            };
            ensure!(
                project
                    .files
                    .len()
                    .checked_add(project.providers.len())
                    .and_then(|count| count.checked_add(project.overrides.len()))
                    .is_some_and(|count| count < limits.records),
                "Import exceeds record limit"
            );
            project.overrides.push(ImportedFile {
                destination,
                layer: *layer,
                requirements,
                expected: ExpectedContent {
                    digests: None,
                    size: Some(entry.bytes),
                    accepted_observation: None,
                },
                acquisition: ImportedAcquisition::Embedded(member.clone()),
                permissions: Some(entry.permissions),
                source: ImportLocation {
                    member: member.clone(),
                    pointer: None,
                },
            });
        }
    }
    ensure!(
        project
            .files
            .len()
            .checked_add(project.overrides.len())
            .and_then(|v| v.checked_add(project.providers.len()))
            .is_some_and(|total| total <= limits.records),
        "Import exceeds record limit"
    );
    validate_destinations(&project)?;
    project.auxiliary_members = names.difference(&used).cloned().collect();
    cancel.check()?;
    Ok(project)
}
impl ImportedRequirements {
    fn required() -> Self {
        Self {
            client: ImportedRequirement::Required,
            server: ImportedRequirement::Required,
        }
    }
}
fn validate_destinations(project: &ImportedProject) -> Result<()> {
    let mut references = CollisionIndex::default();
    for file in &project.files {
        references
            .insert_file(file.destination.relative())
            .context("Manifest file destination collision")?;
    }
    let mut layers = BTreeMap::<ContentLayer, CollisionIndex>::new();
    let mut all = BTreeSet::new();
    let mut combined = CollisionIndex::default();
    for file in &project.overrides {
        layers
            .entry(file.layer)
            .or_default()
            .insert_file(file.destination.relative())?;
    }
    for file in project.files.iter().chain(&project.overrides) {
        // Exact paths may intentionally replace another layer; aliases/ancestor conflicts may not.
        if all.insert(file.destination.relative().clone()) {
            combined.insert_file(file.destination.relative())?;
        }
    }
    Ok(())
}
